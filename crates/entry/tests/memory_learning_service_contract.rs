mod support;

use std::sync::Arc;
#[cfg(feature = "nonproduction-replay-harness")]
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bm_entry::{
    GovernanceBindingSource, GovernanceCredentialRequest, GovernanceCredentialResolver,
    GovernanceProviderBinding, MemoryLearningAttachmentStatusRequest, MemoryLearningService,
    MemoryLearningServiceStatusRequest, ResolvedGovernanceCredential,
};
use bm_sdk::{
    default_agent_subject_id, CanonicalTurnDelta, ConversationScope, MemoryIdentity, MemoryRuntime,
    MemoryScope, MemoryStoreHandle, MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest,
    MemoryTurnProtocol, MemoryTurnSource, PostTurnGovernanceJobStatusV2, PressureLevel,
    RuntimeLifecycleModeInput, StoreBackendConfig, SubjectDescriptor, SubjectRegistry,
    SubjectRelationshipGraph, SubjectScopedRuntime, TranscriptInputMessage,
};

struct NoBindingSource;

impl GovernanceBindingSource for NoBindingSource {
    fn current_binding(&self) -> bm_sdk::Result<Option<GovernanceProviderBinding>> {
        Ok(None)
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
struct MutableBindingSource {
    binding: Mutex<GovernanceProviderBinding>,
}

#[cfg(feature = "nonproduction-replay-harness")]
impl MutableBindingSource {
    fn new() -> Self {
        Self {
            binding: Mutex::new(GovernanceProviderBinding {
                source_owner_id: "host-config".to_string(),
                source_config_id: "primary-provider".to_string(),
                source_revision: 1,
                protocol: bm_entry::EntryGovernanceModelProtocol::OllamaNative,
                endpoint: "http://127.0.0.1:11434".to_string(),
                model_id: "synthetic-governance-model".to_string(),
                credential_reference: None,
                request_timeout_ms: 5_000,
                max_input_tokens: 4_096,
                max_output_tokens: 1_024,
                provider_permission_generation: 1,
            }),
        }
    }

    fn set_revision(&self, revision: u64) {
        self.binding
            .lock()
            .expect("binding source lock")
            .source_revision = revision;
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
impl GovernanceBindingSource for MutableBindingSource {
    fn current_binding(&self) -> bm_sdk::Result<Option<GovernanceProviderBinding>> {
        Ok(Some(
            self.binding.lock().expect("binding source lock").clone(),
        ))
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
struct BlockingBindingSource {
    inner: MutableBindingSource,
    block_next: std::sync::atomic::AtomicBool,
    entered: (Mutex<bool>, std::sync::Condvar),
    released: (Mutex<bool>, std::sync::Condvar),
}

#[cfg(feature = "nonproduction-replay-harness")]
impl BlockingBindingSource {
    fn new() -> Self {
        Self {
            inner: MutableBindingSource::new(),
            block_next: std::sync::atomic::AtomicBool::new(false),
            entered: (Mutex::new(false), std::sync::Condvar::new()),
            released: (Mutex::new(false), std::sync::Condvar::new()),
        }
    }

    fn block_next_install(&self, revision: u64) {
        self.inner.set_revision(revision);
        *self.entered.0.lock().expect("entered lock") = false;
        *self.released.0.lock().expect("released lock") = false;
        self.block_next
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn wait_until_install_is_blocked(&self) {
        let mut entered = self.entered.0.lock().expect("entered lock");
        while !*entered {
            entered = self.entered.1.wait(entered).expect("entered wait");
        }
    }

    fn release_install(&self) {
        *self.released.0.lock().expect("released lock") = true;
        self.released.1.notify_all();
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
impl GovernanceBindingSource for BlockingBindingSource {
    fn current_binding(&self) -> bm_sdk::Result<Option<GovernanceProviderBinding>> {
        if self
            .block_next
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            *self.entered.0.lock().expect("entered lock") = true;
            self.entered.1.notify_all();
            let mut released = self.released.0.lock().expect("released lock");
            while !*released {
                released = self.released.1.wait(released).expect("released wait");
            }
        }
        self.inner.current_binding()
    }
}

struct UnusedCredentialResolver;

impl GovernanceCredentialResolver for UnusedCredentialResolver {
    fn resolve(
        &self,
        _request: &GovernanceCredentialRequest,
    ) -> bm_sdk::Result<ResolvedGovernanceCredential> {
        Err(bm_sdk::Error::config(
            "memory_learning_service_contract",
            "credential resolver must not run without a binding",
        ))
    }
}

fn two_agent_registry() -> SubjectRegistry {
    let mut registry =
        SubjectRegistry::single_agent_default("owner-shared", "agent-a").expect("registry");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "Agent B",
        ))
        .expect("agent-b");
    registry
}

fn runtime_for_subject(
    store: MemoryStoreHandle,
    registry: SubjectRegistry,
    agent_id: &str,
) -> Arc<MemoryRuntime> {
    Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new(agent_id, "owner-shared").expect("identity"))
            .scope(MemoryScope::new("desktop.embedded", "shared-chat").expect("scope"))
            .store(store)
            .subject_registry(registry)
            .subject_id(default_agent_subject_id(agent_id))
            .build()
            .expect("runtime"),
    )
}

fn runtime_for_subject_and_actor(
    store: MemoryStoreHandle,
    registry: SubjectRegistry,
    agent_id: &str,
    actor_subject_id: &str,
) -> Arc<MemoryRuntime> {
    let mounted_subject_id = default_agent_subject_id(agent_id);
    let graph =
        SubjectRelationshipGraph::single_agent_default_for_subject(&registry, &mounted_subject_id)
            .expect("relationship graph");
    Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new(agent_id, "owner-shared").expect("identity"))
            .scope(MemoryScope::new("desktop.embedded", "shared-chat").expect("scope"))
            .store(store)
            .subject_registry(registry.clone())
            .subject_relationship_graph(graph)
            .subject_id(mounted_subject_id.clone())
            .scoped_runtime(SubjectScopedRuntime {
                memory_space_id: registry.memory_space_id,
                mounted_subject_id,
                actor_subject_id: actor_subject_id.to_string(),
                agent_id: agent_id.to_string(),
                relationship_scope: None,
                projection_policy: "subject_aware_default".to_string(),
                write_policy: "subject_candidate_then_space_governance".to_string(),
            })
            .build()
            .expect("runtime"),
    )
}

fn finalize_request() -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: "embedded-turn-1".to_string(),
            conversation: ConversationScope {
                channel: "desktop.embedded".to_string(),
                chat_id: "shared-chat".to_string(),
                conversation_id: Some("conversation-shared".to_string()),
            },
            subject: default_agent_subject_id("agent-b"),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: bm_sdk::IngressKind::User,
                channel: "desktop.embedded".to_string(),
                provider: None,
                protocol: MemoryTurnProtocol::Native,
                endpoint: None,
                model_alias: None,
                model_resolved: None,
                request_id: Some("embedded-request-1".to_string()),
                client_conversation_hint: Some("conversation-shared".to_string()),
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user("合成的多主体记忆输入")],
            assistant_message: Some(TranscriptInputMessage::assistant("合成回复")),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        tool_calls: 0,
        runtime_skill_selected_ids: Vec::new(),
        task_learning_selected_ids: Vec::new(),
        reuse_outcome_note: String::new(),
        tool_usage_feedback: None,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

#[test]
fn embedded_service_attaches_existing_multi_subject_runtimes_with_one_store_authority() {
    let profile = support::host_production_profile();
    let store = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("in-memory Store config"),
    )
    .expect("Store");
    let registry = two_agent_registry();
    let runtime_a = runtime_for_subject(store.clone(), registry.clone(), "agent-a");
    let runtime_b = runtime_for_subject(store.clone(), registry.clone(), "agent-b");
    assert!(runtime_a.learning_service_status_authority().is_err());
    assert!(runtime_a.learning_service_control_authorities().is_err());
    let governor_runtime = runtime_for_subject_and_actor(
        store.clone(),
        registry.clone(),
        "agent-a",
        &bm_sdk::system_governor_subject_id("owner-shared"),
    );
    let service_authority = governor_runtime
        .learning_service_status_authority()
        .expect("SystemGovernor status authority");
    let control_authorities_a = governor_runtime
        .learning_service_control_authorities()
        .expect("SystemGovernor control authorities");
    let governor_runtime_b = runtime_for_subject_and_actor(
        store.clone(),
        registry.clone(),
        "agent-b",
        &bm_sdk::system_governor_subject_id("owner-shared"),
    );
    let control_authorities_b = governor_runtime_b
        .learning_service_control_authorities()
        .expect("subject-b SystemGovernor control authorities");
    let attachment_a_authority = runtime_a
        .learning_attachment_status_authority()
        .expect("agent-a status authority");
    let attachment_b_authority = runtime_b
        .learning_attachment_status_authority()
        .expect("agent-b status authority");
    let (service, attachment_a) = MemoryLearningService::builder(Arc::clone(&runtime_a))
        .control_authorities(control_authorities_a)
        .binding_source(Arc::new(NoBindingSource))
        .credential_resolver(Arc::new(UnusedCredentialResolver))
        .start()
        .expect("official learning service");
    let attachment_b = service
        .attach_runtime(Arc::clone(&runtime_b), control_authorities_b.clone())
        .expect("same Store and registry attachment");
    assert_eq!(
        service
            .status(MemoryLearningServiceStatusRequest {
                authority: service_authority.clone()
            })
            .expect("service status")
            .attachment_count,
        2
    );
    assert_eq!(
        attachment_a
            .status(MemoryLearningAttachmentStatusRequest {
                authority: attachment_a_authority.clone()
            })
            .expect("attachment-a status")
            .mounted_subject_id,
        default_agent_subject_id("agent-a")
    );
    assert_eq!(
        attachment_b
            .status(MemoryLearningAttachmentStatusRequest {
                authority: attachment_b_authority.clone()
            })
            .expect("attachment-b status")
            .mounted_subject_id,
        default_agent_subject_id("agent-b")
    );

    let different_registry = SubjectRegistry::single_agent_default("owner-shared", "agent-a")
        .expect("different registry");
    let wrong_registry = runtime_for_subject(store.clone(), different_registry, "agent-a");
    assert!(service
        .attach_runtime(wrong_registry, control_authorities_b.clone())
        .is_err());
    let different_store = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("different Store config"),
    )
    .expect("different Store");
    let wrong_store = runtime_for_subject(different_store, registry, "agent-b");
    assert!(service
        .attach_runtime(wrong_store, control_authorities_b)
        .is_err());

    let finalized = runtime_b
        .finalize_turn(finalize_request())
        .expect("finalize through existing embedded runtime");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let job = runtime_b
            .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest {
                job_id: job_id.clone(),
            })
            .expect("job status")
            .job;
        if job.status == PostTurnGovernanceJobStatusV2::BlockedConfiguration {
            assert!(job.receipt.is_none());
            break;
        }
        assert!(Instant::now() < deadline, "learning service did not wake");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(runtime_a
        .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .is_err());
    let report_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if attachment_b
            .status(MemoryLearningAttachmentStatusRequest {
                authority: attachment_b_authority.clone(),
            })
            .expect("attachment-b status")
            .last_job_id
            .as_deref()
            == Some(job_id.as_str())
        {
            break;
        }
        assert!(
            Instant::now() < report_deadline,
            "subject-b attachment did not observe its own job"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(attachment_a
        .status(MemoryLearningAttachmentStatusRequest {
            authority: attachment_a_authority.clone()
        })
        .expect("attachment-a status")
        .last_job_id
        .is_none());
    assert!(attachment_a
        .status(MemoryLearningAttachmentStatusRequest {
            authority: attachment_b_authority,
        })
        .is_err());
    service
        .shutdown(Instant::now() + Duration::from_secs(2))
        .expect("bounded shutdown");
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn rejected_start_and_attach_leave_every_candidate_store_unchanged() {
    let profile = support::host_production_profile();
    let store = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("in-memory Store config"),
    )
    .expect("Store");
    let registry = two_agent_registry();
    let runtime_a = runtime_for_subject(store.clone(), registry.clone(), "agent-a");
    let runtime_b = runtime_for_subject(store.clone(), registry.clone(), "agent-b");
    let governor_a = runtime_for_subject_and_actor(
        store.clone(),
        registry.clone(),
        "agent-a",
        &bm_sdk::system_governor_subject_id("owner-shared"),
    );
    let governor_b = runtime_for_subject_and_actor(
        store.clone(),
        registry.clone(),
        "agent-b",
        &bm_sdk::system_governor_subject_id("owner-shared"),
    );
    let controls_a = governor_a
        .learning_service_control_authorities()
        .expect("agent-a governor controls");
    let controls_b = governor_b
        .learning_service_control_authorities()
        .expect("agent-b governor controls");
    let source = Arc::new(MutableBindingSource::new());

    let before_wrong_start = store
        .export_replay_snapshot()
        .expect("snapshot before start");
    assert!(MemoryLearningService::builder(Arc::clone(&runtime_a))
        .control_authorities(controls_b.clone())
        .binding_source(source.clone())
        .credential_resolver(Arc::new(UnusedCredentialResolver))
        .start()
        .is_err());
    assert_eq!(
        store
            .export_replay_snapshot()
            .expect("snapshot after start"),
        before_wrong_start,
        "wrong start control authority must have zero durable effect"
    );

    let (service, _attachment_a) = MemoryLearningService::builder(Arc::clone(&runtime_a))
        .control_authorities(controls_a.clone())
        .binding_source(source.clone())
        .credential_resolver(Arc::new(UnusedCredentialResolver))
        .worker_limits(bm_entry::MemoryLearningWorkerLimits {
            max_attachments: 1,
            ..bm_entry::MemoryLearningWorkerLimits::default()
        })
        .start()
        .expect("valid service");

    source.set_revision(2);
    let before_wrong_control = store
        .export_replay_snapshot()
        .expect("snapshot before wrong control");
    assert!(service
        .attach_runtime(Arc::clone(&runtime_b), controls_a)
        .is_err());
    assert_eq!(
        store
            .export_replay_snapshot()
            .expect("snapshot after wrong control"),
        before_wrong_control,
        "wrong attach control authority must have zero durable effect"
    );

    let wrong_registry =
        SubjectRegistry::single_agent_default("owner-shared", "agent-b").expect("wrong registry");
    let wrong_registry_runtime =
        runtime_for_subject(store.clone(), wrong_registry.clone(), "agent-b");
    let wrong_registry_governor = runtime_for_subject_and_actor(
        store.clone(),
        wrong_registry,
        "agent-b",
        &bm_sdk::system_governor_subject_id("owner-shared"),
    );
    let wrong_registry_controls = wrong_registry_governor
        .learning_service_control_authorities()
        .expect("wrong-registry controls");
    source.set_revision(3);
    let before_wrong_registry = store
        .export_replay_snapshot()
        .expect("snapshot before wrong registry");
    assert!(service
        .attach_runtime(wrong_registry_runtime, wrong_registry_controls)
        .is_err());
    assert_eq!(
        store
            .export_replay_snapshot()
            .expect("snapshot after wrong registry"),
        before_wrong_registry,
        "wrong registry must have zero durable effect"
    );

    let other_store = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("other in-memory Store config"),
    )
    .expect("other Store");
    let other_runtime = runtime_for_subject(other_store.clone(), registry.clone(), "agent-b");
    let other_governor = runtime_for_subject_and_actor(
        other_store.clone(),
        registry.clone(),
        "agent-b",
        &bm_sdk::system_governor_subject_id("owner-shared"),
    );
    let other_controls = other_governor
        .learning_service_control_authorities()
        .expect("other-store controls");
    source.set_revision(4);
    let before_service_store = store
        .export_replay_snapshot()
        .expect("service store before wrong Store");
    let before_other_store = other_store
        .export_replay_snapshot()
        .expect("other Store before wrong Store");
    assert!(service
        .attach_runtime(other_runtime, other_controls)
        .is_err());
    assert_eq!(
        store
            .export_replay_snapshot()
            .expect("service Store after wrong Store"),
        before_service_store,
        "wrong Store must not mutate the service Store"
    );
    assert_eq!(
        other_store
            .export_replay_snapshot()
            .expect("other Store after wrong Store"),
        before_other_store,
        "wrong Store must not mutate the candidate Store"
    );

    source.set_revision(5);
    let before_capacity = store
        .export_replay_snapshot()
        .expect("snapshot before capacity rejection");
    assert!(service.attach_runtime(runtime_b, controls_b).is_err());
    assert_eq!(
        store
            .export_replay_snapshot()
            .expect("snapshot after capacity rejection"),
        before_capacity,
        "capacity exhaustion must have zero durable effect"
    );

    service
        .shutdown(Instant::now() + Duration::from_secs(2))
        .expect("bounded shutdown");
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn attach_and_shutdown_share_one_linearized_admission_boundary() {
    let profile = support::host_production_profile();
    let store = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("in-memory Store config"),
    )
    .expect("Store");
    let registry = two_agent_registry();
    let runtime_a = runtime_for_subject(store.clone(), registry.clone(), "agent-a");
    let runtime_b = runtime_for_subject(store.clone(), registry.clone(), "agent-b");
    let governor_a = runtime_for_subject_and_actor(
        store.clone(),
        registry.clone(),
        "agent-a",
        &bm_sdk::system_governor_subject_id("owner-shared"),
    );
    let governor_b = runtime_for_subject_and_actor(
        store.clone(),
        registry,
        "agent-b",
        &bm_sdk::system_governor_subject_id("owner-shared"),
    );
    let controls_a = governor_a
        .learning_service_control_authorities()
        .expect("agent-a governor controls");
    let controls_b = governor_b
        .learning_service_control_authorities()
        .expect("agent-b governor controls");
    let source = Arc::new(BlockingBindingSource::new());
    let (service, _attachment_a) = MemoryLearningService::builder(runtime_a)
        .control_authorities(controls_a)
        .binding_source(source.clone())
        .credential_resolver(Arc::new(UnusedCredentialResolver))
        .worker_limits(bm_entry::MemoryLearningWorkerLimits {
            max_attachments: 2,
            ..bm_entry::MemoryLearningWorkerLimits::default()
        })
        .start()
        .expect("valid service");

    source.block_next_install(2);
    let attach_service = service.clone();
    let attach_runtime = Arc::clone(&runtime_b);
    let attach_thread =
        std::thread::spawn(move || attach_service.attach_runtime(attach_runtime, controls_b));
    source.wait_until_install_is_blocked();

    let shutdown_service = service.clone();
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = shutdown_service.shutdown(Instant::now() + Duration::from_secs(2));
        shutdown_tx.send(result).expect("shutdown result receiver");
    });
    assert!(matches!(
        shutdown_rx.recv_timeout(Duration::from_millis(100)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));

    source.release_install();
    let _attachment_b = attach_thread
        .join()
        .expect("attach thread")
        .expect("attach linearized before shutdown");
    shutdown_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("shutdown completion")
        .expect("bounded shutdown");

    source.inner.set_revision(3);
    let before_rejected_attach = store
        .export_replay_snapshot()
        .expect("snapshot before stopped attach");
    assert!(service
        .attach_runtime(
            runtime_b,
            governor_b
                .learning_service_control_authorities()
                .expect("post-shutdown controls")
        )
        .is_err());
    assert_eq!(
        store
            .export_replay_snapshot()
            .expect("snapshot after stopped attach"),
        before_rejected_attach,
        "attach rejected after shutdown must have zero durable effect"
    );
}

#[test]
fn dropping_the_last_service_handle_stops_the_worker_with_live_attachment_handles() {
    let profile = support::host_production_profile();
    let store = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("in-memory Store config"),
    )
    .expect("Store");
    let registry = two_agent_registry();
    let runtime = runtime_for_subject(store.clone(), registry.clone(), "agent-a");
    let governor_runtime = runtime_for_subject_and_actor(
        store,
        registry,
        "agent-a",
        &bm_sdk::system_governor_subject_id("owner-shared"),
    );
    let control_authorities = governor_runtime
        .learning_service_control_authorities()
        .expect("SystemGovernor control authorities");
    let (service, attachment) = MemoryLearningService::builder(runtime)
        .control_authorities(control_authorities)
        .binding_source(Arc::new(NoBindingSource))
        .credential_resolver(Arc::new(UnusedCredentialResolver))
        .start()
        .expect("official learning service");
    drop(service);
    assert!(
        attachment.wake().is_err(),
        "last service handle drop must immediately fence new work"
    );
}
