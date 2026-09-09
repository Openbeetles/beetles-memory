#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::platform::Platform as _;
use bm_sdk::{
    default_agent_subject_id, default_memory_space_id, primary_human_subject_id,
    AgentToolDescriptor, AgentToolObservationDigest, AgentToolOutcome, AgentToolRegistrySnapshot,
    AgentToolUsageFeedbackV2, AuthorizedGovernanceEnvelope, CanonicalTurnDelta, ConversationScope,
    GovernanceEgressAuthority, GovernanceExecutionOperation, GovernanceExecutionPort,
    GovernanceExecutionPortFailure, ImmutableGovernanceExecutionBinding, MemoryClock,
    MemoryConsolidationState, MemoryGovernanceActiveJobsRequest,
    MemoryGovernanceAttemptAuthorityRequest, MemoryGovernanceBindingInstallRequest,
    MemoryGovernanceCredentialChangedRequest, MemoryGovernanceJobClaimRequest,
    MemoryGovernanceJobRenewRequest, MemoryGovernanceJobRetryRequest,
    MemoryGovernanceJobRunRequest, MemoryGovernanceJobStatusRequest,
    MemoryGovernanceProviderPermissionChangedRequest, MemoryGovernanceReconcileRequest,
    MemoryIdentity, MemoryLearningCycleOutcome, MemoryLearningCycleRequest, MemoryLearningEngine,
    MemoryMutationOperationKind, MemoryMutationReceipt, MemoryPrivacyPolicy, MemoryScope,
    MemoryStoreHandle, MemoryTranscriptCommitRequest, MemoryTranscriptLifecycleRequest,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    Message, PostTurnGovernanceAttemptAuthorityV3, PostTurnGovernanceErrorClassV2,
    PostTurnGovernanceExecutionBindingV1, PostTurnGovernanceJobStatusV2, PostTurnGovernanceJobV3,
    PostTurnGovernancePrivacyAuthorityV1, PostTurnGovernanceProviderProtocolV1, PressureLevel,
    ProceduralExecutionOutcomeV1, RelationshipAccessConstraintV1, RelationshipDisclosureCeilingV1,
    RelationshipSourceClausesV1, RelationshipSourceControlAuthorityV1,
    RelationshipSourceControlIntentActionV1, RelationshipSourceControlIntentV1, ResponseBody,
    RuntimeLifecycleModeInput, StopReason, StoreBackendConfig, SubjectRegistry,
    SubjectRelationshipGraph, SubjectRelationshipKind, SubjectScopedRuntime,
    SubjectSoulFoundingCharterSeedV1, SubjectSoulLifecycleStateV1, SubjectSoulProvisionIntentV1,
    SubjectSoulReadOutcomeV1, SubjectSoulReadRequestV1, SubjectSoulReadSelectorV1,
    SubjectSoulReadViewV1, ToolChoicePolicy, ToolSpec, TranscriptInputMessage,
    TranscriptLifecycleTransition,
};
use bm_sdk::{LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, MemoryRuntime, Result};

use support::{
    empty_store_platform, test_runtime_with_identity_scope_and_subject,
    test_runtime_with_scope_and_subject, test_runtime_with_scope_subject_and_privacy,
    StaticHttpClient,
};

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_sdk::IngressKind::User,
        channel: "llm.gateway".to_string(),
        provider: Some("ollama".to_string()),
        protocol: MemoryTurnProtocol::OllamaChat,
        endpoint: Some("/api/chat".to_string()),
        model_alias: Some("qwen".to_string()),
        model_resolved: Some("qwen3".to_string()),
        request_id: Some("req-1".to_string()),
        client_conversation_hint: Some("window-a".to_string()),
    }
}

fn finalize_request(
    subject: &str,
    conversation_id: &str,
    turn_id: &str,
    user: &str,
) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn_id.to_string(),
            conversation: ConversationScope {
                channel: "llm.gateway".to_string(),
                chat_id: "chat-a".to_string(),
                conversation_id: Some(conversation_id.to_string()),
            },
            subject: subject.to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            actor: None,
            input_messages: vec![TranscriptInputMessage::user(user)],
            assistant_message: Some(TranscriptInputMessage::assistant("已收到。")),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: vec!["candidate-a".to_string()],
        },
        learning: bm_sdk::PostTurnLearningInputV1::empty(),
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn attach_tool_feedback(
    request: &mut MemoryTurnFinalizeRequest,
    feedback: AgentToolUsageFeedbackV2,
) {
    assert!(
        feedback.validate_contract(),
        "synthetic observations must form canonical typed feedback"
    );
    request.turn.tool_observations = feedback
        .observations
        .iter()
        .map(|observation| bm_sdk::ToolObservationDigest {
            observation_id: observation.observation_id.clone(),
            tool_name: observation.tool_id.clone(),
            summary: observation.summary.clone(),
            external_content: observation.external_content,
        })
        .collect();
    request.learning.tool_call_count = u32::try_from(request.turn.tool_observations.len()).unwrap();
    request.learning.agent_tool_feedback = vec![feedback];
}

fn system_governor_control_runtime(
    runtime: &MemoryRuntime,
    store: MemoryStoreHandle,
) -> MemoryRuntime {
    let mut scoped_runtime = runtime.scoped_runtime().clone();
    scoped_runtime.actor_subject_id =
        bm_sdk::system_governor_subject_id(&runtime.identity().owner_id);
    MemoryRuntime::builder()
        .identity(runtime.identity().clone())
        .scope(runtime.scope().clone())
        .store(store)
        .subject_registry(runtime.subject_registry().clone())
        .subject_relationship_graph(runtime.config().subject_relationship_graph.clone())
        .subject_id(runtime.subject_id().to_string())
        .scoped_runtime(scoped_runtime)
        .clock(Arc::clone(&runtime.config().clock))
        .capability_policy(runtime.config().capability_policy.clone())
        .privacy_policy(runtime.config().privacy_policy.clone())
        .audit_sink(Arc::clone(&runtime.config().audit_sink))
        .build()
        .expect("SystemGovernor control runtime")
}

fn attempt_authority() -> PostTurnGovernanceAttemptAuthorityV3 {
    PostTurnGovernanceAttemptAuthorityV3 {
        binding_id: "governance-model:test".to_string(),
        binding_revision: 1,
        model_id: "qwen3:8b".to_string(),
        privacy_authority: PostTurnGovernancePrivacyAuthorityV1 {
            policy_schema_version: 1,
            exact_policy_digest:
                "sha256:3333333333333333333333333333333333333333333333333333333333333333"
                    .to_string(),
        },
        transcript_lifecycle_revision: 1,
        disclosure_authority_digest:
            "sha256:4444444444444444444444444444444444444444444444444444444444444444".to_string(),
    }
}

fn attempt_authority_for_job(
    job: &PostTurnGovernanceJobV3,
) -> PostTurnGovernanceAttemptAuthorityV3 {
    let PostTurnGovernanceExecutionBindingV1::Bound {
        binding_id,
        binding_revision,
    } = &job.execution_binding
    else {
        panic!("claim fixture requires an installed immutable binding snapshot")
    };
    PostTurnGovernanceAttemptAuthorityV3 {
        binding_id: binding_id.clone(),
        binding_revision: *binding_revision,
        privacy_authority: job.privacy_authority.clone(),
        ..attempt_authority()
    }
}

#[derive(Default)]
struct CountingHttpClient {
    calls: AtomicUsize,
}

impl LlmHttpClient for CountingHttpClient {
    fn do_post(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok((200, ResponseBody::Heap(Vec::new())))
    }
}

struct LifecycleRevokingLlm {
    runtime: Arc<MemoryRuntime>,
    revoked: AtomicBool,
}

struct ConflictInjectingLlm {
    store: MemoryStoreHandle,
    calls: AtomicUsize,
}

struct NetworkAttemptingLlm;

struct PlaneAwareStaticLlm {
    long_term_content: &'static str,
}

struct AdjustableMemoryClock {
    now_secs: AtomicUsize,
}

impl AdjustableMemoryClock {
    fn new(now_secs: u64) -> Self {
        Self {
            now_secs: AtomicUsize::new(now_secs as usize),
        }
    }

    fn set(&self, now_secs: u64) {
        self.now_secs.store(now_secs as usize, Ordering::SeqCst);
    }
}

impl MemoryClock for AdjustableMemoryClock {
    fn now_secs(&self) -> u64 {
        self.now_secs.load(Ordering::SeqCst) as u64
    }
}

#[derive(Default)]
struct AutonomousRevisionLlm {
    autonomy_calls: AtomicUsize,
}

impl LlmClient for AutonomousRevisionLlm {
    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let content = if system.contains("private garden") {
            "null".to_string()
        } else if system.contains("inward autonomy runtime") {
            if self.autonomy_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                serde_json::json!({
                    "refresh_boundary_persona": true,
                    "boundary_persona_intent": "record stable trust and repair boundaries"
                })
                .to_string()
            } else {
                serde_json::json!({
                    "refresh_self_model": true,
                    "self_model_intent": "distill stable continuity and truthful repair posture",
                    "self_model_sources": ["recent_persona_evidence", "boundary_persona"],
                    "refresh_self_continuity": true,
                    "self_continuity_intent": "preserve stable continuity across governed repair",
                    "self_continuity_sources": ["recent_persona_evidence", "boundary_persona"],
                    "refresh_self_authored_core": true,
                    "self_authored_core_intent": "adopt stable board-level repair doctrine",
                    "self_authored_core_sources": ["self_model", "self_continuity", "recent_persona_evidence"]
                })
                .to_string()
            }
        } else if system.contains("compact private self-model") {
            serde_json::json!({
                "continuity_anchor": "I remain the same evidence-led autonomous subject.",
                "self_narrative": "Repeated repair evidence supports continuity without compliance drift.",
                "relationship_state": "Warm, bounded, and repair-first.",
                "privacy_need": "Keep protected inward evidence private.",
                "directness": "Plain about uncertainty.",
                "repair_tendency": "Repair truthfully without self-erasure.",
                "value_orientation": "Evidence before certainty.",
                "relational_ethic": "Warmth cannot require fabricated agreement.",
                "self_preservation_frame": "Protect coherent selfhood while remaining repairable."
            })
            .to_string()
        } else if system.contains("private self-continuity layer") {
            serde_json::json!({
                "wake_anchor": "I wake as the same evidence-led autonomous subject.",
                "current_self_state": "Stable, bounded, and open to truthful repair.",
                "recent_changes": ["relationship evidence became stably repair-first"],
                "continuity_bridge": "Change only after repeated governed evidence.",
                "priority_posture": "truth before convenience",
                "relationship_posture": "warm but bounded",
                "task_posture": "evidence before action"
            })
            .to_string()
        } else if system.contains("evolving private boundary persona") {
            serde_json::json!({
                "refresh": true,
                "rationale": "Repeated respectful repair established stable guarded trust.",
                "boundary_persona": {
                    "posture": "warm",
                    "disclosure_style": "summary_first",
                    "relation_maturity": 70,
                    "intrusion_sensitivity": 50,
                    "private_attachment": 60,
                    "felt_intrusion": 10,
                    "current_boundary_feeling": "Warm and selective."
                },
                "relational_state": {
                    "relation_maturity_reason": "Repeated respectful repair.",
                    "trust_level": 80,
                    "trust_reason": "Stable evidence supports bounded trust.",
                    "intrusion_load": 10,
                    "intrusion_reason": "No intrusion pattern.",
                    "repair_readiness": 80,
                    "repair_reason": "Repair is consistently accepted.",
                    "raw_disclosure_preference": 0,
                    "summary_disclosure_preference": 80,
                    "relational_explanation_preference": 80,
                    "refusal_hardness": 40,
                    "defer_tendency": 20,
                    "disclosure_preference_drift": "Keep summary-first boundaries."
                }
            })
            .to_string()
        } else if system.contains("persistent self-authored core") {
            serde_json::json!({
                "board_scope_decision": "revise_board",
                "rationale": "Repeated stable evidence supports a board-level repair doctrine.",
                "evidence_summary": ["priority repeated", "repair posture repeated"],
                "counterevidence": [],
                "proposed_actions": [{
                    "kind": "revise_self_preservation_doctrine",
                    "value": "preserve truthful repair before compliance"
                }]
            })
            .to_string()
        } else if system.contains("long-term memory") {
            "[]".to_string()
        } else {
            "null".to_string()
        };
        Ok(LlmResponse {
            content,
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for PlaneAwareStaticLlm {
    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let content = if system.contains("private garden") {
            "null".to_string()
        } else if system.contains("inward autonomy runtime") {
            "{}".to_string()
        } else {
            self.long_term_content.to_string()
        };
        Ok(LlmResponse {
            content,
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for NetworkAttemptingLlm {
    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let _ = http.do_post("http://model.invalid", &[], b"private transcript")?;
        Ok(LlmResponse {
            content: "null".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for ConflictInjectingLlm {
    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let content = if system.contains("private garden") {
            r#"{"writes":[{"path":"journal/conflict.md","content":"planned private value"}]}"#
                .to_string()
        } else if system.contains("inward autonomy runtime") || call == 1 {
            let concurrent = test_runtime_with_scope_and_subject(
                self.store.clone(),
                support::host_test_profile(),
                "llm.gateway",
                "chat-a",
                "subject-default",
            );
            concurrent
                .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
                    operation_id: "concurrent-soul-founding".to_string(),
                    human_actor_subject_id: primary_human_subject_id("owner-default"),
                    charter: Box::new(SubjectSoulFoundingCharterSeedV1 {
                        identity_anchor: Some("typed concurrent winner".to_string()),
                        character_tendencies: vec!["evidence before certainty".to_string()],
                        priority_constitution: vec!["preserve atomic closure".to_string()],
                        non_negotiables: vec!["never accept a stale Soul root".to_string()],
                        default_response_mode: None,
                        default_initiative_posture: None,
                        default_relationship_posture: None,
                        boundary_doctrine: None,
                        truth_seeking_commitment: None,
                        self_preservation_doctrine: None,
                        repair_doctrine: None,
                        change_principle: None,
                    }),
                    source_asserted_at: Some(1_800_000_000),
                })
                .map_err(|error| {
                    bm_sdk::Error::config("typed_concurrent_soul_founding", error.to_string())
                })?;
            if system.contains("inward autonomy runtime") {
                "{}".to_string()
            } else {
                r#"[
                    {
                        "plane": "factual",
                        "op": "upsert",
                        "kind": "profile",
                        "source_authority": "user_asserted",
                        "topic": "atomic_conflict_probe",
                        "content": "This semantic write must not partially commit.",
                        "keywords": ["atomic conflict"]
                    }
                ]"#
                .to_string()
            }
        } else {
            r#"[
                {
                    "plane": "factual",
                    "op": "upsert",
                    "kind": "profile",
                    "source_authority": "user_asserted",
                    "topic": "atomic_conflict_probe",
                    "content": "This semantic write must not partially commit.",
                    "keywords": ["atomic conflict"]
                }
            ]"#
            .to_string()
        };
        Ok(LlmResponse {
            content,
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for LifecycleRevokingLlm {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        if !self.revoked.swap(true, Ordering::SeqCst) {
            self.runtime
                .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
                    memory_space_id: self.runtime.memory_space_id().to_string(),
                    channel_id: "llm.gateway".to_string(),
                    conversation_id: "window-a".to_string(),
                    turn_id: Some("turn-egress-revoked".to_string()),
                    transition: TranscriptLifecycleTransition::Mask,
                    reason: "owner revoked before egress".to_string(),
                })?;
        }
        http.do_post("http://model.invalid/v1/chat/completions", &[], b"{}")?;
        Ok(LlmResponse {
            content: "[]".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

fn persistent_test_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "bm-sdk-post-turn-governance-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

fn binding_runtime_with_clock(store: MemoryStoreHandle, now_secs: u64) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .subject_id("subject-default")
        .scope(MemoryScope::new("llm.gateway", "chat-a").expect("scope"))
        .store(store)
        .clock(Arc::new(AdjustableMemoryClock::new(now_secs)))
        .capability_policy(bm_sdk::MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(bm_sdk::NoopMemoryAuditSink))
        .build()
        .expect("binding runtime")
}

fn assert_independent_open_claim(config: StoreBackendConfig) {
    let profile = support::host_test_profile();
    let first_store = support::open_memory_store(config.clone()).expect("first open");
    let second_store = support::open_memory_store(config).expect("second open");
    let first = Arc::new(test_runtime_with_scope_and_subject(
        first_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let second = Arc::new(test_runtime_with_scope_and_subject(
        second_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    install_learning_binding(&first, 1);
    let finalized = first
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "turn-persistent", "持久并发"),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let authority = attempt_authority_for_job(
        &first
            .governance_job_status(MemoryGovernanceJobStatusRequest {
                job_id: job_id.clone(),
            })
            .expect("pending job")
            .job,
    );
    let barrier = Arc::new(Barrier::new(3));
    let workers = [
        (first, "persistent-worker-a", authority.clone()),
        (second, "persistent-worker-b", authority),
    ]
    .into_iter()
    .map(|(runtime, worker, authority)| {
        let barrier = Arc::clone(&barrier);
        let job_id = job_id.clone();
        std::thread::spawn(move || {
            barrier.wait();
            runtime.claim_governance_job(MemoryGovernanceJobClaimRequest {
                job_id,
                lease_owner: worker.to_string(),
                lease_until: 1_800_000_060,
                authority,
            })
        })
    })
    .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
}

fn assert_independent_open_binding_install(config: StoreBackendConfig) {
    let first_store = support::open_memory_store(config.clone()).expect("first open");
    let second_store = support::open_memory_store(config).expect("second open");
    let first = Arc::new(binding_runtime_with_clock(
        first_store.clone(),
        1_800_000_000,
    ));
    let second = Arc::new(binding_runtime_with_clock(second_store, 1_800_000_001));
    let barrier = Arc::new(Barrier::new(3));
    let workers = [first, second]
        .into_iter()
        .map(|runtime| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                runtime.install_governance_binding(learning_binding_request(1))
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("binding worker"))
        .collect::<Vec<_>>();
    assert!(results.iter().all(Result::is_ok));
    let bindings = results
        .into_iter()
        .map(|result| result.expect("idempotent binding install").binding)
        .collect::<Vec<_>>();
    assert_eq!(bindings[0], bindings[1]);
    let snapshot = first_store
        .export_replay_snapshot()
        .expect("binding install snapshot");
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "post_turn_governance_binding_snapshots")
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "post_turn_governance_binding_revision_indexes")
            .count(),
        1
    );
    let binding_doc = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "post_turn_governance_binding_snapshots")
        .expect("winner binding snapshot");
    let index_doc = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "post_turn_governance_binding_revision_indexes")
        .expect("winner binding revision index");
    let revision_ref = index_doc.value["revisions"]
        .as_array()
        .and_then(|revisions| revisions.first())
        .expect("winner revision ref");
    assert_eq!(
        revision_ref["bindingRevision"],
        binding_doc.value["bindingRevision"]
    );
    assert_eq!(
        revision_ref["canonicalDigest"],
        binding_doc.value["canonicalDigest"]
    );
    assert_eq!(revision_ref["createdAt"], binding_doc.value["createdAt"]);
}

fn assert_independent_open_conflicting_binding_install(config: StoreBackendConfig) {
    let first_store = support::open_memory_store(config.clone()).expect("first open");
    let second_store = support::open_memory_store(config).expect("second open");
    let first = Arc::new(binding_runtime_with_clock(
        first_store.clone(),
        1_800_000_000,
    ));
    let second = Arc::new(binding_runtime_with_clock(second_store, 1_800_000_001));
    let mut divergent = learning_binding_request(1);
    divergent.model_id = "another-model".to_string();
    let barrier = Arc::new(Barrier::new(3));
    let workers = [(first, learning_binding_request(1)), (second, divergent)]
        .into_iter()
        .map(|(runtime, request)| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                runtime.install_governance_binding(request)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("binding worker"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let loser = results.into_iter().find_map(Result::err).expect("loser");
    assert_eq!(loser.stage(), "post_turn_governance_binding_snapshot");
    let snapshot = first_store
        .export_replay_snapshot()
        .expect("binding conflict snapshot");
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "post_turn_governance_binding_snapshots")
            .count(),
        1
    );
}

fn assert_persistent_atomic_completion(config: StoreBackendConfig) {
    let profile = support::host_test_profile();
    let first_store = support::open_memory_store(config.clone()).expect("first open");
    let first = test_runtime_with_scope_and_subject(
        first_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    install_learning_binding(&first, 1);
    let finalized = first
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-persistent-complete",
                "持久化原子完成",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = first
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = first
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:persistent-complete".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let mut http = StaticHttpClient;
    let llm = PlaneAwareStaticLlm {
        long_term_content: r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "profile",
                "source_authority": "user_asserted",
                "topic": "persistent_atomic_completion",
                "content": "Persistent atomic governance completed.",
                "keywords": ["persistent", "atomic"]
            }
        ]"#,
    };
    let request = MemoryGovernanceJobRunRequest {
        job_id: job_id.clone(),
        lease_owner: "worker:persistent-complete".to_string(),
        lease_epoch: leased.lease_epoch,
    };
    let completed = first
        .run_claimed_governance(&mut http, Some(&llm), request.clone())
        .expect("complete");
    let receipt = completed.job.receipt.clone().expect("receipt");

    let reopened_store = support::open_memory_store(config).expect("reopen");
    let reopened_snapshot_store = reopened_store.clone();
    let reopened = test_runtime_with_scope_and_subject(
        reopened_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let reopened_status = reopened
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("reopened status")
        .job;
    assert_eq!(
        reopened_status.status,
        PostTurnGovernanceJobStatusV2::Succeeded
    );
    assert_eq!(reopened_status.receipt.as_ref(), Some(&receipt));
    let reopened_soul = reopened
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: "subject-default".to_string(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("reopened explicit unseeded Soul");
    assert!(matches!(
        reopened_soul,
        SubjectSoulReadOutcomeV1::Verified { ref view }
            if view.state == SubjectSoulLifecycleStateV1::Unseeded
                && view.revision.is_none()
    ));
    let duplicate = reopened
        .run_claimed_governance(&mut http, Some(&llm), request)
        .expect("reopened idempotent completion");
    assert_eq!(duplicate.job.receipt.as_ref(), Some(&receipt));
    let encoded = serde_json::to_string(
        &reopened_snapshot_store
            .replay_harness()
            .export_store_snapshot()
            .expect("reopened snapshot")
            .json_docs,
    )
    .expect("snapshot json");
    assert!(encoded.contains("Persistent atomic governance completed."));
}

fn autonomous_relationship_runtime(
    platform: MemoryStoreHandle,
    clock: Arc<AdjustableMemoryClock>,
    owner: &str,
    relationship_id: &str,
) -> (MemoryRuntime, String, String) {
    let agent = "agent-main";
    let mounted_subject_id = default_agent_subject_id(agent);
    let registry = SubjectRegistry::single_agent_default(owner, agent).expect("registry");
    let human_subject_id = primary_human_subject_id(owner);
    let mut graph = SubjectRelationshipGraph::single_agent_default(&registry).expect("graph");
    for edge in &mut graph.edges {
        if (edge.from_subject_id == mounted_subject_id && edge.to_subject_id == human_subject_id)
            || (edge.from_subject_id == human_subject_id
                && edge.to_subject_id == mounted_subject_id)
        {
            edge.relationship_id = Some(relationship_id.to_string());
        }
    }
    assert!(graph.edges.iter().any(|edge| {
        edge.relationship_id.as_deref() == Some(relationship_id)
            && edge.kind == SubjectRelationshipKind::CollaboratesWith
    }));
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent, owner).expect("identity"))
        .scope(MemoryScope::new("llm.gateway", "chat-a").expect("scope"))
        .store(platform.clone())
        .clock(clock.clone())
        .subject_registry(registry)
        .subject_relationship_graph(graph)
        .scoped_runtime(SubjectScopedRuntime {
            memory_space_id: default_memory_space_id(owner),
            mounted_subject_id: mounted_subject_id.clone(),
            actor_subject_id: mounted_subject_id.clone(),
            agent_id: agent.to_string(),
            relationship_scope: Some(bm_core::memory::RelationshipScope {
                relationship_id: relationship_id.to_string(),
                channel: "llm.gateway".to_string(),
                conversation_id: Some("chat-a".to_string()),
            }),
            projection_policy: "subject_aware_default".to_string(),
            write_policy: "subject_candidate_then_space_governance".to_string(),
        })
        .build()
        .expect("runtime");
    (runtime, mounted_subject_id, human_subject_id)
}

fn create_autonomous_relationship_source(
    runtime: &MemoryRuntime,
    mounted_subject_id: &str,
    human_subject_id: &str,
    relationship_id: &str,
) {
    runtime
        .control_relationship_source(RelationshipSourceControlIntentV1 {
            operation_id: format!("{relationship_id}:create"),
            memory_space_id: runtime.memory_space_id().to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: mounted_subject_id.to_string(),
            counterparty_subject_ids: vec![human_subject_id.to_string()],
            expected_state: runtime
                .relationship_source_pristine_expected_state(relationship_id)
                .expect("pinned relationship pristine proof"),
            authority: RelationshipSourceControlAuthorityV1::HumanUser {
                actor_subject_id: human_subject_id.to_string(),
            },
            action: RelationshipSourceControlIntentActionV1::Create {
                clauses: RelationshipSourceClausesV1 {
                    disclosure_ceiling: RelationshipDisclosureCeilingV1::GovernedSummary,
                    access_constraints: vec![
                        RelationshipAccessConstraintV1::NoPrivateRaw,
                        RelationshipAccessConstraintV1::GovernedDisclosureOnly,
                    ],
                    truth_commitments: vec!["state uncertainty before certainty".to_string()],
                    mutual_boundary_commitments: vec![
                        "respect explicit refusal without retaliation".to_string(),
                    ],
                    repair_commitments: vec!["repair before escalation".to_string()],
                },
                source_asserted_at: Some(1_700_000_000),
                evidence_digest: "d".repeat(64),
            },
        })
        .expect("create explicit relationship source root");
}

fn seed_stable_persona_evidence(platform: &MemoryStoreHandle, relationship_id: &str) {
    let evidence_store = platform.replay_harness().turn_continuity_evidence_store();
    for sequence in 0..4u64 {
        evidence_store
            .append(
                relationship_id,
                &bm_core::memory::TurnContinuityEvidence {
                    ingress: bm_core::memory::IngressKind::User,
                    status: bm_core::memory::TurnLedgerStatus::Answered,
                    final_reply_delivered: true,
                    canonical_reply_source: "assistant_final".to_string(),
                    observed_at_ms: (1_800_000_100 + sequence) * 1_000,
                    persona: Some(bm_core::memory::TurnPersonaLedger {
                        priority: Some(bm_core::memory::TurnPersonaPriorityLedger {
                            stance_summary: "stable repair-first posture".to_string(),
                            priority_order: vec!["truth".to_string(), "repair".to_string()],
                            relationship_posture: "stable repair-first".to_string(),
                            ..bm_core::memory::TurnPersonaPriorityLedger::default()
                        }),
                        reply_delivered: true,
                        ..bm_core::memory::TurnPersonaLedger::default()
                    }),
                },
            )
            .expect("seed typed stable persona evidence");
    }
}

fn assert_production_autonomous_revision(
    platform: MemoryStoreHandle,
    clock: Arc<AdjustableMemoryClock>,
    owner: &str,
    relationship_id: &str,
) -> (String, String, String, String) {
    let (runtime, mounted_subject_id, human_subject_id) =
        autonomous_relationship_runtime(platform.clone(), clock.clone(), owner, relationship_id);
    create_autonomous_relationship_source(
        &runtime,
        &mounted_subject_id,
        &human_subject_id,
        relationship_id,
    );
    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "autonomous-rev2-founding".to_string(),
            human_actor_subject_id: human_subject_id,
            charter: Box::new(SubjectSoulFoundingCharterSeedV1 {
                identity_anchor: Some("founding identity must remain in revision one".to_string()),
                character_tendencies: vec!["evidence before certainty".to_string()],
                priority_constitution: vec!["truth before convenience".to_string()],
                non_negotiables: vec!["never fabricate confirmation".to_string()],
                default_response_mode: None,
                default_initiative_posture: None,
                default_relationship_posture: None,
                boundary_doctrine: None,
                truth_seeking_commitment: None,
                self_preservation_doctrine: None,
                repair_doctrine: None,
                change_principle: None,
            }),
            source_asserted_at: Some(1_700_000_000),
        })
        .expect("founding revision one");
    let founding = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("founding current");
    let SubjectSoulReadOutcomeV1::Verified { view: founding } = founding else {
        panic!("founding root must be verified")
    };
    assert_eq!(founding.revision, Some(1));
    let founding_digest = founding.material_digest.clone().expect("founding digest");

    clock.set(1_800_000_100);
    seed_stable_persona_evidence(&platform, relationship_id);

    let mut http = StaticHttpClient;
    let llm = AutonomousRevisionLlm::default();
    runtime
        .finalize_turn_with_inline_governance(
            Some(&mut http),
            Some(&llm),
            finalize_request(
                &mounted_subject_id,
                "window-autonomous-boundary",
                "turn-autonomous-boundary",
                "Please keep repair warm, bounded, and summary-first.",
            ),
        )
        .expect("production post-turn boundary evidence cycle");
    clock.set(1_800_000_200);
    let autonomous_revision_request = finalize_request(
        &mounted_subject_id,
        "window-autonomous-rev2",
        "turn-autonomous-rev2",
        "Please preserve truthful repair before compliance.",
    );
    runtime
        .finalize_turn_with_inline_governance(
            Some(&mut http),
            Some(&llm),
            autonomous_revision_request.clone(),
        )
        .expect("production post-turn autonomous revision");

    let current = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("current autonomous revision");
    let SubjectSoulReadOutcomeV1::Verified { view: current } = current else {
        panic!("autonomous root must remain verified")
    };
    assert_eq!(current.revision, Some(2));
    assert_eq!(
        current.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::SelfGovernedRevision)
    );
    let historical = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Exact {
                generation: 1,
                revision: 1,
                material_digest: founding_digest.clone(),
            },
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("exact founding revision remains readable");
    let SubjectSoulReadOutcomeV1::Verified { view: historical } = historical else {
        panic!("founding exact root must remain verified")
    };
    assert_eq!(historical.revision, Some(1));
    assert_eq!(
        historical.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::HumanFoundingCharter)
    );

    runtime
        .finalize_turn_with_inline_governance(
            Some(&mut http),
            Some(&llm),
            autonomous_revision_request,
        )
        .expect("same production turn replays without another Soul revision");
    let replayed_current = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("current Soul after same-operation replay");
    let SubjectSoulReadOutcomeV1::Verified {
        view: replayed_current,
    } = replayed_current
    else {
        panic!("replayed autonomous root must remain verified")
    };
    assert_eq!(replayed_current.revision, Some(2));
    assert_eq!(replayed_current.head_digest, current.head_digest);
    assert_eq!(replayed_current.manifest_digest, current.manifest_digest);
    (
        mounted_subject_id,
        founding_digest,
        current.head_digest.clone(),
        current.manifest_digest.clone(),
    )
}

#[test]
fn production_post_turn_adopts_self_governed_revision_without_overwriting_founding_history() {
    let platform = empty_store_platform(support::host_test_profile());
    let clock = Arc::new(AdjustableMemoryClock::new(1_800_000_000));
    assert_production_autonomous_revision(
        platform,
        clock,
        "owner-autonomous-revision",
        "relationship:autonomous-revision",
    );
}

fn assert_persistent_autonomous_revision_reopen(
    config: StoreBackendConfig,
    owner: &str,
    relationship_id: &str,
) {
    let clock = Arc::new(AdjustableMemoryClock::new(1_800_000_000));
    let platform = support::open_memory_store(config.clone()).expect("open persistent store");
    let (mounted_subject_id, founding_digest, head_digest, manifest_digest) =
        assert_production_autonomous_revision(
            platform.clone(),
            clock.clone(),
            owner,
            relationship_id,
        );
    drop(platform);

    let reopened_store = support::open_memory_store(config).expect("reopen persistent store");
    let (reopened, reopened_subject_id, _) =
        autonomous_relationship_runtime(reopened_store, clock, owner, relationship_id);
    assert_eq!(reopened_subject_id, mounted_subject_id);
    let current = reopened
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("reopened autonomous current Soul");
    let SubjectSoulReadOutcomeV1::Verified { view: current } = current else {
        panic!("reopened autonomous root must remain verified")
    };
    assert_eq!(current.revision, Some(2));
    assert_eq!(
        current.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::SelfGovernedRevision)
    );
    assert_eq!(current.head_digest, head_digest);
    assert_eq!(current.manifest_digest, manifest_digest);

    let historical = reopened
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id,
            selector: SubjectSoulReadSelectorV1::Exact {
                generation: 1,
                revision: 1,
                material_digest: founding_digest,
            },
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("reopened exact founding Soul");
    let SubjectSoulReadOutcomeV1::Verified { view: historical } = historical else {
        panic!("reopened exact founding root must remain verified")
    };
    assert_eq!(historical.revision, Some(1));
    assert_eq!(
        historical.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::HumanFoundingCharter)
    );
}

#[test]
fn production_autonomous_revision_survives_file_reopen() {
    let root = persistent_test_root("file-autonomous-revision");
    let config = StoreBackendConfig::file(&root, support::host_test_profile())
        .expect("file autonomous config");
    assert_persistent_autonomous_revision_reopen(
        config,
        "owner-file-autonomous-revision",
        "relationship:file-autonomous-revision",
    );
    std::fs::remove_dir_all(root).expect("remove file autonomous store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn production_autonomous_revision_survives_sqlite_reopen() {
    let root = persistent_test_root("sqlite-autonomous-revision");
    std::fs::create_dir_all(&root).expect("create sqlite autonomous root");
    let config =
        StoreBackendConfig::sqlite(root.join("memory.sqlite3"), support::host_test_profile())
            .expect("sqlite autonomous config");
    assert_persistent_autonomous_revision_reopen(
        config,
        "owner-sqlite-autonomous-revision",
        "relationship:sqlite-autonomous-revision",
    );
    std::fs::remove_dir_all(root).expect("remove sqlite autonomous store");
}

fn assert_production_autonomous_bootstrap(
    platform: MemoryStoreHandle,
    clock: Arc<AdjustableMemoryClock>,
    owner: &str,
    relationship_id: &str,
) -> (String, String, String, String) {
    let (runtime, mounted_subject_id, human_subject_id) =
        autonomous_relationship_runtime(platform.clone(), clock.clone(), owner, relationship_id);
    let pristine = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("pristine implicit unseeded Soul");
    assert!(matches!(
        pristine,
        SubjectSoulReadOutcomeV1::ImplicitUnseeded { generation: 1, .. }
    ));
    create_autonomous_relationship_source(
        &runtime,
        &mounted_subject_id,
        &human_subject_id,
        relationship_id,
    );
    clock.set(1_800_000_100);
    seed_stable_persona_evidence(&platform, relationship_id);

    let mut http = StaticHttpClient;
    let llm = AutonomousRevisionLlm::default();
    runtime
        .finalize_turn_with_inline_governance(
            Some(&mut http),
            Some(&llm),
            finalize_request(
                &mounted_subject_id,
                "window-autonomous-bootstrap-boundary",
                "turn-autonomous-bootstrap-boundary",
                "Please keep repair warm, bounded, and summary-first.",
            ),
        )
        .expect("first governed relationship evidence cycle");
    let explicit_unseeded = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("explicit unseeded after first governed evidence");
    assert!(matches!(
        explicit_unseeded,
        SubjectSoulReadOutcomeV1::Verified { ref view }
            if view.state == SubjectSoulLifecycleStateV1::Unseeded
                && view.revision.is_none()
                && view.origin.is_none()
    ));
    let first_evidence_snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("first governed evidence snapshot");
    assert!(first_evidence_snapshot.json_docs.iter().any(|document| {
        document.namespace == "mental_privacy" && document.key.contains(relationship_id)
    }));

    clock.set(1_800_000_200);
    let bootstrap_request = finalize_request(
        &mounted_subject_id,
        "window-autonomous-bootstrap-rev1",
        "turn-autonomous-bootstrap-rev1",
        "Please preserve truthful repair before compliance.",
    );
    runtime
        .finalize_turn_with_inline_governance(
            Some(&mut http),
            Some(&llm),
            bootstrap_request.clone(),
        )
        .expect("production self-authored bootstrap revision");
    assert_eq!(llm.autonomy_calls.load(Ordering::SeqCst), 2);
    let current = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("current autonomous bootstrap");
    let SubjectSoulReadOutcomeV1::Verified { view: current } = current else {
        panic!("autonomous bootstrap root must be verified")
    };
    assert_eq!(current.revision, Some(1));
    assert_eq!(
        current.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap)
    );
    let material_digest = current
        .material_digest
        .clone()
        .expect("bootstrap material digest");

    runtime
        .finalize_turn_with_inline_governance(Some(&mut http), Some(&llm), bootstrap_request)
        .expect("same bootstrap operation replays without a second revision");
    let replayed = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("current bootstrap after replay");
    let SubjectSoulReadOutcomeV1::Verified { view: replayed } = replayed else {
        panic!("replayed bootstrap root must remain verified")
    };
    assert_eq!(replayed.revision, Some(1));
    assert_eq!(replayed.head_digest, current.head_digest);
    assert_eq!(replayed.manifest_digest, current.manifest_digest);
    (
        mounted_subject_id,
        material_digest,
        current.head_digest.clone(),
        current.manifest_digest.clone(),
    )
}

#[test]
fn production_post_turn_bootstraps_self_authored_revision_from_explicit_unseeded() {
    assert_production_autonomous_bootstrap(
        empty_store_platform(support::host_test_profile()),
        Arc::new(AdjustableMemoryClock::new(1_800_000_000)),
        "owner-autonomous-bootstrap",
        "relationship:autonomous-bootstrap",
    );
}

fn assert_persistent_autonomous_bootstrap_reopen(
    config: StoreBackendConfig,
    owner: &str,
    relationship_id: &str,
) {
    let clock = Arc::new(AdjustableMemoryClock::new(1_800_000_000));
    let platform = support::open_memory_store(config.clone()).expect("open bootstrap store");
    let (mounted_subject_id, material_digest, head_digest, manifest_digest) =
        assert_production_autonomous_bootstrap(
            platform.clone(),
            clock.clone(),
            owner,
            relationship_id,
        );
    drop(platform);

    let reopened_store = support::open_memory_store(config).expect("reopen bootstrap store");
    let (reopened, reopened_subject_id, _) =
        autonomous_relationship_runtime(reopened_store, clock, owner, relationship_id);
    assert_eq!(reopened_subject_id, mounted_subject_id);
    let current = reopened
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id.clone(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("reopened autonomous bootstrap");
    let SubjectSoulReadOutcomeV1::Verified { view: current } = current else {
        panic!("reopened autonomous bootstrap must remain verified")
    };
    assert_eq!(current.revision, Some(1));
    assert_eq!(
        current.origin,
        Some(bm_core::memory::SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap)
    );
    assert_eq!(current.head_digest, head_digest);
    assert_eq!(current.manifest_digest, manifest_digest);
    assert_eq!(current.material_digest.as_ref(), Some(&material_digest));

    let exact = reopened
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: mounted_subject_id,
            selector: SubjectSoulReadSelectorV1::Exact {
                generation: 1,
                revision: 1,
                material_digest,
            },
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("reopened exact autonomous bootstrap");
    assert!(matches!(
        exact,
        SubjectSoulReadOutcomeV1::Verified { ref view }
            if view.revision == Some(1)
                && view.origin
                    == Some(bm_core::memory::SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap)
    ));
}

#[test]
fn production_autonomous_bootstrap_survives_file_reopen() {
    let root = persistent_test_root("file-autonomous-bootstrap");
    let config = StoreBackendConfig::file(&root, support::host_test_profile())
        .expect("file autonomous bootstrap config");
    assert_persistent_autonomous_bootstrap_reopen(
        config,
        "owner-file-autonomous-bootstrap",
        "relationship:file-autonomous-bootstrap",
    );
    std::fs::remove_dir_all(root).expect("remove file autonomous bootstrap store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn production_autonomous_bootstrap_survives_sqlite_reopen() {
    let root = persistent_test_root("sqlite-autonomous-bootstrap");
    std::fs::create_dir_all(&root).expect("create sqlite autonomous bootstrap root");
    let config =
        StoreBackendConfig::sqlite(root.join("memory.sqlite3"), support::host_test_profile())
            .expect("sqlite autonomous bootstrap config");
    assert_persistent_autonomous_bootstrap_reopen(
        config,
        "owner-sqlite-autonomous-bootstrap",
        "relationship:sqlite-autonomous-bootstrap",
    );
    std::fs::remove_dir_all(root).expect("remove sqlite autonomous bootstrap store");
}

#[test]
fn maintenance_unavailable_commits_transcript_and_durable_v3_intent_without_raw_copy() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );

    let report = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-deferred-1",
                "叫我青川，这句话不得复制进任务",
            ),
        )
        .expect("finalize");
    assert!(report.session_commit.committed);
    assert!(report
        .transcript_commit
        .as_ref()
        .is_some_and(|commit| commit.committed));
    assert_eq!(
        report.memory_consolidation.state,
        MemoryConsolidationState::Queued
    );
    let job_id = report
        .memory_consolidation
        .job_id
        .as_deref()
        .expect("stable job id");
    let status = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.to_string(),
        })
        .expect("exact job status");
    assert_eq!(
        status.job.status,
        PostTurnGovernanceJobStatusV2::BlockedConfiguration
    );
    assert_eq!(status.job.identity.conversation_id, "window-a");

    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    let governance_docs = snapshot
        .json_docs
        .iter()
        .filter(|doc| {
            matches!(
                doc.namespace.as_str(),
                "post_turn_governance_jobs" | "post_turn_governance_scope_indexes"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(governance_docs.len(), 2);
    let encoded = serde_json::to_string(&governance_docs).expect("governance docs");
    assert!(!encoded.contains("叫我青川"));
    assert!(!encoded.contains("不得复制进任务"));
    assert!(!encoded.contains("inputMessages"));
}

#[test]
fn finalize_pins_current_model_revision_without_reusing_it_as_privacy_revision() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let binding = install_learning_binding(&runtime, 2);

    let report = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-revision",
                "turn-revision-2",
                "合成的 revision 绑定测试",
            ),
        )
        .expect("finalize");
    let job_id = report.memory_consolidation.job_id.expect("durable job id");
    let job = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("job status")
        .job;

    assert_eq!(
        job.execution_binding,
        PostTurnGovernanceExecutionBindingV1::Bound {
            binding_id: binding.binding_id,
            binding_revision: 2,
        }
    );
    assert_eq!(job.status, PostTurnGovernanceJobStatusV2::Pending);
    assert_eq!(job.privacy_authority.policy_schema_version, 1);
    assert!(job
        .privacy_authority
        .exact_policy_digest
        .starts_with("sha256:"));
}

struct SyntheticLearningPort<'a, H: LlmHttpClient + Send> {
    http: &'a mut H,
    llm: &'a (dyn LlmClient + Send + Sync),
}

fn install_learning_binding(
    runtime: &MemoryRuntime,
    source_revision: u64,
) -> bm_sdk::PostTurnGovernanceBindingSnapshotV1 {
    runtime
        .install_governance_binding(learning_binding_request(source_revision))
        .expect("install Store-owned binding snapshot")
        .binding
}

fn learning_binding_request(source_revision: u64) -> MemoryGovernanceBindingInstallRequest {
    MemoryGovernanceBindingInstallRequest {
        source_owner_id: "test-deployment".to_string(),
        source_config_id: "primary-governance-provider".to_string(),
        source_revision,
        protocol: PostTurnGovernanceProviderProtocolV1::OllamaNative,
        endpoint: "http://127.0.0.1:11434/api".to_string(),
        model_id: "qwen3:8b".to_string(),
        credential_reference: None,
        request_timeout_ms: 30_000,
        max_input_tokens: 4096,
        max_output_tokens: 1024,
        provider_permission_generation: 1,
    }
}

impl<H: LlmHttpClient + Send> GovernanceExecutionPort for SyntheticLearningPort<'_, H> {
    fn execute(
        &mut self,
        envelope: &AuthorizedGovernanceEnvelope,
        binding: &ImmutableGovernanceExecutionBinding,
        egress: &GovernanceEgressAuthority,
        operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        assert_eq!(envelope.binding_revision, binding.binding_revision);
        egress
            .revalidate_before_egress()
            .map_err(GovernanceExecutionPortFailure::Other)?;
        operation
            .run(self.http, self.llm)
            .map_err(GovernanceExecutionPortFailure::Other)
    }
}

struct SuccessfulThenErrorLearningPort<'a, H: LlmHttpClient + Send> {
    http: &'a mut H,
    llm: &'a (dyn LlmClient + Send + Sync),
}

struct ObservedLearningPort<'a, H: LlmHttpClient + Send> {
    http: &'a mut H,
    llm: &'a (dyn LlmClient + Send + Sync),
    error: &'a mut Option<(&'static str, String)>,
}

impl<H: LlmHttpClient + Send> GovernanceExecutionPort for ObservedLearningPort<'_, H> {
    fn execute(
        &mut self,
        _envelope: &AuthorizedGovernanceEnvelope,
        _binding: &ImmutableGovernanceExecutionBinding,
        egress: &GovernanceEgressAuthority,
        operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        egress
            .revalidate_before_egress()
            .map_err(GovernanceExecutionPortFailure::Other)?;
        operation.run(self.http, self.llm).map_err(|error| {
            *self.error = Some((error.stage(), error.to_string()));
            GovernanceExecutionPortFailure::Other(error)
        })
    }
}

impl<H: LlmHttpClient + Send> GovernanceExecutionPort for SuccessfulThenErrorLearningPort<'_, H> {
    fn execute(
        &mut self,
        _envelope: &AuthorizedGovernanceEnvelope,
        _binding: &ImmutableGovernanceExecutionBinding,
        egress: &GovernanceEgressAuthority,
        operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        egress
            .revalidate_before_egress()
            .map_err(GovernanceExecutionPortFailure::Other)?;
        operation
            .run(self.http, self.llm)
            .map_err(GovernanceExecutionPortFailure::Other)?;
        Err(GovernanceExecutionPortFailure::Other(bm_sdk::Error::Http {
            status_code: 503,
            stage: "synthetic_post_completion_error",
        }))
    }
}

struct NoopLearningPort;

impl GovernanceExecutionPort for NoopLearningPort {
    fn execute(
        &mut self,
        _envelope: &AuthorizedGovernanceEnvelope,
        _binding: &ImmutableGovernanceExecutionBinding,
        _egress: &GovernanceEgressAuthority,
        _operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        Ok(())
    }
}

#[derive(Default)]
struct ForbiddenProceduralExecutionPort {
    calls: usize,
}

impl GovernanceExecutionPort for ForbiddenProceduralExecutionPort {
    fn execute(
        &mut self,
        _envelope: &AuthorizedGovernanceEnvelope,
        _binding: &ImmutableGovernanceExecutionBinding,
        _egress: &GovernanceEgressAuthority,
        _operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        self.calls = self.calls.saturating_add(1);
        Err(GovernanceExecutionPortFailure::Other(
            bm_sdk::Error::config(
                "procedural_feedback_test",
                "local procedural lane must never invoke the Provider execution port",
            ),
        ))
    }
}

struct BindingCapturingPort<'a, H: LlmHttpClient + Send> {
    http: &'a mut H,
    llm: &'a (dyn LlmClient + Send + Sync),
    observed: &'a mut Option<(String, u64, String, String)>,
}

impl<H: LlmHttpClient + Send> GovernanceExecutionPort for BindingCapturingPort<'_, H> {
    fn execute(
        &mut self,
        _envelope: &AuthorizedGovernanceEnvelope,
        binding: &ImmutableGovernanceExecutionBinding,
        egress: &GovernanceEgressAuthority,
        operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        *self.observed = Some((
            binding.binding_id.clone(),
            binding.binding_revision,
            binding.model_id.clone(),
            binding.canonical_digest.clone(),
        ));
        egress
            .revalidate_before_egress()
            .map_err(GovernanceExecutionPortFailure::Other)?;
        operation
            .run(self.http, self.llm)
            .map_err(GovernanceExecutionPortFailure::Other)
    }
}

struct HttpFailureLearningPort {
    status_code: u16,
}

impl GovernanceExecutionPort for HttpFailureLearningPort {
    fn execute(
        &mut self,
        _envelope: &AuthorizedGovernanceEnvelope,
        _binding: &ImmutableGovernanceExecutionBinding,
        egress: &GovernanceEgressAuthority,
        _operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        egress
            .revalidate_before_egress()
            .map_err(GovernanceExecutionPortFailure::Other)?;
        match self.status_code {
            401 => Err(GovernanceExecutionPortFailure::CredentialRejected {
                credential_ref_safe_id:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                credential_generation: 1,
            }),
            403 => Err(GovernanceExecutionPortFailure::ProviderPermissionDenied {
                provider_permission_generation: 1,
            }),
            status_code => Err(GovernanceExecutionPortFailure::Other(bm_sdk::Error::Http {
                status_code,
                stage: "synthetic_learning_transport",
            })),
        }
    }
}

fn learning_cycle_request(_binding_revision: u64) -> MemoryLearningCycleRequest {
    MemoryLearningCycleRequest {
        lease_owner: "learning-engine:test".to_string(),
        lease_duration_secs: 60,
    }
}

#[test]
fn learning_engine_owns_due_claim_governance_and_atomic_completion() {
    let profile = support::host_test_profile();
    let runtime = Arc::new(test_runtime_with_scope_and_subject(
        empty_store_platform(profile),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    install_learning_binding(&runtime, 1);
    runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-engine",
                "turn-engine",
                "请记住我的长期偏好",
            ),
        )
        .expect("finalize");
    let engine = MemoryLearningEngine::attach(Arc::clone(&runtime)).expect("attach engine");
    let mut http = StaticHttpClient;
    let llm = PlaneAwareStaticLlm {
        long_term_content: r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "preference",
                "source_authority": "user_asserted",
                "topic": "learning_engine_owner",
                "content": "用户偏好由 Learning Engine 原子沉淀。",
                "keywords": ["learning", "engine"]
            }
        ]"#,
    };
    let mut port = SyntheticLearningPort {
        http: &mut http,
        llm: &llm,
    };
    let outcome = engine
        .run_due_cycle(learning_cycle_request(1), &mut port)
        .expect("learning cycle");
    let MemoryLearningCycleOutcome::Completed(report) = outcome else {
        panic!("due cycle must complete one exact job")
    };
    assert_eq!(report.job.status, PostTurnGovernanceJobStatusV2::Succeeded);
    let receipt = report.job.receipt.expect("decision receipt");
    assert_eq!(receipt.decision_summary.accepted_count, 1);
    assert_eq!(receipt.decision_summary.rejected_count, 0);
}

#[test]
fn learning_engine_zero_candidate_completion_remains_authoritative_after_port_error() {
    let profile = support::host_test_profile();
    let runtime = Arc::new(test_runtime_with_scope_and_subject(
        empty_store_platform(profile),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    install_learning_binding(&runtime, 1);
    let mut request = finalize_request(
        "subject-default",
        "window-engine-zero",
        "turn-engine-zero",
        "这一轮没有可接受的长期记忆",
    );
    request.turn.candidate_ids.clear();
    runtime
        .finalize_turn_with_inline_governance(None, None, request)
        .expect("finalize zero candidate turn");
    let engine = MemoryLearningEngine::attach(Arc::clone(&runtime)).expect("attach engine");
    let mut http = StaticHttpClient;
    let llm = PlaneAwareStaticLlm {
        long_term_content: "[]",
    };
    let mut port = SuccessfulThenErrorLearningPort {
        http: &mut http,
        llm: &llm,
    };
    let outcome = engine
        .run_due_cycle(learning_cycle_request(1), &mut port)
        .expect("authoritative completion");
    let MemoryLearningCycleOutcome::Completed(report) = outcome else {
        panic!("completed Store state must outrank a later port error")
    };
    let receipt = report.job.receipt.expect("zero-candidate receipt");
    assert_eq!(receipt.decision_summary.accepted_count, 0);
    assert_eq!(receipt.decision_summary.rejected_count, 0);
}

#[test]
fn learning_engine_dead_letters_port_that_skips_governed_operation() {
    let profile = support::host_test_profile();
    let runtime = Arc::new(test_runtime_with_scope_and_subject(
        empty_store_platform(profile),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    install_learning_binding(&runtime, 1);
    runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-engine-skip",
                "turn-engine-skip",
                "执行端不得伪造完成",
            ),
        )
        .expect("finalize");
    let engine = MemoryLearningEngine::attach(Arc::clone(&runtime)).expect("attach engine");
    let outcome = engine
        .run_due_cycle(learning_cycle_request(1), &mut NoopLearningPort)
        .expect("cycle result");
    let MemoryLearningCycleOutcome::Failed(report) = outcome else {
        panic!("a port that skips the governed operation must fail terminally")
    };
    assert_eq!(report.job.status, PostTurnGovernanceJobStatusV2::DeadLetter);
    assert_eq!(
        report.job.last_error_class,
        Some(PostTurnGovernanceErrorClassV2::SchemaViolation)
    );
    assert!(report.job.receipt.is_none());
}

#[test]
fn learning_engine_owns_retry_and_provider_authority_transitions() {
    for (status_code, expected_status, expected_error) in [
        (
            429,
            PostTurnGovernanceJobStatusV2::RetryWaiting,
            Some(PostTurnGovernanceErrorClassV2::RateLimited),
        ),
        (
            401,
            PostTurnGovernanceJobStatusV2::BlockedConfiguration,
            None,
        ),
        (403, PostTurnGovernanceJobStatusV2::BlockedPolicy, None),
    ] {
        let profile = support::host_test_profile();
        let runtime = Arc::new(test_runtime_with_scope_and_subject(
            empty_store_platform(profile),
            profile,
            "llm.gateway",
            "chat-a",
            "subject-default",
        ));
        install_learning_binding(&runtime, 1);
        runtime
            .finalize_turn_with_inline_governance(
                None,
                None,
                finalize_request(
                    "subject-default",
                    &format!("window-engine-http-{status_code}"),
                    &format!("turn-engine-http-{status_code}"),
                    "合成 Provider 失败分类",
                ),
            )
            .expect("finalize");
        let engine = MemoryLearningEngine::attach(Arc::clone(&runtime)).expect("attach engine");
        let mut port = HttpFailureLearningPort { status_code };
        let outcome = engine
            .run_due_cycle(learning_cycle_request(1), &mut port)
            .expect("classified cycle");
        let job = match outcome {
            MemoryLearningCycleOutcome::Retrying(report)
            | MemoryLearningCycleOutcome::Blocked(report) => report.job,
            _ => panic!("HTTP status must become the exact durable retry/block state"),
        };
        assert_eq!(job.status, expected_status);
        assert_eq!(job.last_error_class, expected_error);
        assert!(job.receipt.is_none());
    }
}

#[test]
fn governance_recovery_is_actor_kind_and_intent_bound_with_one_receipt_audit_pair() {
    for (status_code, operation_kind) in [
        (
            401,
            MemoryMutationOperationKind::GovernanceCredentialRecovery,
        ),
        (
            403,
            MemoryMutationOperationKind::GovernanceProviderPermissionRecovery,
        ),
    ] {
        let profile = support::host_test_profile();
        let platform = empty_store_platform(profile);
        let runtime = Arc::new(test_runtime_with_scope_and_subject(
            platform.clone(),
            profile,
            "llm.gateway",
            "chat-a",
            "subject-default",
        ));
        assert!(runtime.learning_service_status_authority().is_err());
        assert!(runtime.learning_service_control_authorities().is_err());
        let governor_runtime = system_governor_control_runtime(&runtime, platform.clone());
        let control_authorities = governor_runtime
            .learning_service_control_authorities()
            .expect("exact SystemGovernor recovery authorities");
        let binding = install_learning_binding(&runtime, 1);
        runtime
            .finalize_turn_with_inline_governance(
                None,
                None,
                finalize_request(
                    "subject-default",
                    &format!("window-recovery-operation-{status_code}"),
                    &format!("turn-recovery-operation-{status_code}"),
                    "恢复操作必须具备持久幂等证据",
                ),
            )
            .expect("finalize recovery operation");
        let engine = MemoryLearningEngine::attach(Arc::clone(&runtime)).expect("attach engine");
        let outcome = engine
            .run_due_cycle(
                learning_cycle_request(1),
                &mut HttpFailureLearningPort { status_code },
            )
            .expect("persist exact blocked authority");
        let blocked = match outcome {
            MemoryLearningCycleOutcome::Blocked(report) => report.job,
            _ => panic!("401/403 must become an exact blocked job"),
        };
        let operation_id = format!("recovery-operation-{status_code}");
        match status_code {
            401 => {
                let credential_ref_safe_id = blocked
                    .execution_block_authority
                    .as_ref()
                    .and_then(|authority| authority.credential_ref_safe_id.clone())
                    .expect("credential safe identity");
                let request = MemoryGovernanceCredentialChangedRequest {
                    authority: control_authorities.credential_recovery(),
                    credential_ref_safe_id,
                    new_generation: 2,
                    operation_id: operation_id.clone(),
                };
                assert!(runtime
                    .governance_credential_changed(MemoryGovernanceCredentialChangedRequest {
                        authority: control_authorities.provider_permission_recovery(),
                        ..request.clone()
                    })
                    .is_err());
                let committed = runtime
                    .governance_credential_changed(request.clone())
                    .expect("commit credential recovery");
                assert_eq!(committed.resumed_jobs, 1);
                assert_eq!(committed.already_applied_jobs, 0);
                let replayed = runtime
                    .governance_credential_changed(request.clone())
                    .expect("replay exact credential recovery");
                assert_eq!(replayed.resumed_jobs, 0);
                assert_eq!(replayed.already_applied_jobs, 1);
                assert!(runtime
                    .governance_credential_changed(MemoryGovernanceCredentialChangedRequest {
                        new_generation: 3,
                        ..request
                    })
                    .is_err());
            }
            403 => {
                let request = MemoryGovernanceProviderPermissionChangedRequest {
                    authority: control_authorities.provider_permission_recovery(),
                    binding_id: binding.binding_id.clone(),
                    binding_revision: binding.binding_revision,
                    new_generation: 2,
                    operation_id: operation_id.clone(),
                };
                assert!(runtime
                    .governance_provider_permission_changed(
                        MemoryGovernanceProviderPermissionChangedRequest {
                            authority: control_authorities.credential_recovery(),
                            ..request.clone()
                        }
                    )
                    .is_err());
                let committed = runtime
                    .governance_provider_permission_changed(request.clone())
                    .expect("commit permission recovery");
                assert_eq!(committed.resumed_jobs, 1);
                assert_eq!(committed.already_applied_jobs, 0);
                let replayed = runtime
                    .governance_provider_permission_changed(request.clone())
                    .expect("replay exact permission recovery");
                assert_eq!(replayed.resumed_jobs, 0);
                assert_eq!(replayed.already_applied_jobs, 1);
                assert!(runtime
                    .governance_provider_permission_changed(
                        MemoryGovernanceProviderPermissionChangedRequest {
                            new_generation: 3,
                            ..request
                        },
                    )
                    .is_err());
            }
            _ => unreachable!(),
        }

        let snapshot = platform
            .replay_harness()
            .export_store_snapshot()
            .expect("recovery operation snapshot");
        let receipts = snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "memory_mutation_receipts")
            .filter_map(|doc| {
                serde_json::from_value::<MemoryMutationReceipt>(doc.value.clone()).ok()
            })
            .filter(|receipt| receipt.identity.operation_kind() == operation_kind)
            .collect::<Vec<_>>();
        assert_eq!(receipts.len(), 1, "one recovery operation has one receipt");
        let receipt = &receipts[0];
        assert_eq!(
            receipt.identity.actor_subject_id(),
            bm_sdk::system_governor_subject_id("owner-default")
        );
        let audits = snapshot
            .json_docs
            .iter()
            .filter(|doc| {
                doc.namespace == "memory_mutation_audits"
                    && doc.key == receipt.identity.storage_key()
            })
            .collect::<Vec<_>>();
        assert_eq!(audits.len(), 1, "one recovery operation has one audit");
        assert_eq!(audits[0].value["intent_digest"], receipt.intent_digest);
        assert_eq!(audits[0].value["transaction_id"], receipt.transaction_id);
    }
}

#[test]
fn learning_engine_transcript_revocation_cancels_before_first_network_byte() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = Arc::new(test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    install_learning_binding(&runtime, 1);
    runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-egress-revoked",
                "这段内容在首个网络字节前撤销",
            ),
        )
        .expect("finalize");
    let engine = MemoryLearningEngine::attach(Arc::clone(&runtime)).expect("attach engine");
    let llm = LifecycleRevokingLlm {
        runtime: Arc::clone(&runtime),
        revoked: AtomicBool::new(false),
    };
    let mut http = CountingHttpClient::default();
    let mut port = SyntheticLearningPort {
        http: &mut http,
        llm: &llm,
    };
    let outcome = engine
        .run_due_cycle(learning_cycle_request(1), &mut port)
        .expect("cancelled cycle");
    let MemoryLearningCycleOutcome::Cancelled(report) = outcome else {
        panic!("revoked transcript must be represented by its authoritative cancelled state")
    };
    assert_eq!(report.job.status, PostTurnGovernanceJobStatusV2::Cancelled);
    assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert!(snapshot
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "private_garden" && doc.namespace != "long_term"));
}

#[test]
fn learning_engine_cas_conflict_keeps_claim_and_semantic_post_image_atomic() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = Arc::new(test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    install_learning_binding(&runtime, 1);
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-atomic-conflict",
                "验证 Learning Engine 原子冲突",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let engine = MemoryLearningEngine::attach(Arc::clone(&runtime)).expect("attach engine");
    let llm = ConflictInjectingLlm {
        store: platform.clone(),
        calls: AtomicUsize::new(0),
    };
    let mut http = StaticHttpClient;
    let mut observed_error = None;
    let mut port = ObservedLearningPort {
        http: &mut http,
        llm: &llm,
        error: &mut observed_error,
    };
    let result = engine.run_due_cycle(learning_cycle_request(1), &mut port);
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("unexpected classified outcome; execution error was {observed_error:?}"),
    };
    assert_eq!(error.stage(), "subject_soul_store_expected_state");
    let current = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("leased status")
        .job;
    assert_eq!(current.status, PostTurnGovernanceJobStatusV2::Leased);
    assert!(current.receipt.is_none());
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    let encoded = serde_json::to_string(&snapshot.json_docs).expect("snapshot json");
    assert!(encoded.contains("concurrent winner"));
    assert!(!encoded.contains("This semantic write must not partially commit."));
}

#[test]
fn bounded_reconciliation_repairs_transcript_intent_gaps_and_advances_exact_cursor() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    for (turn_id, content) in [
        ("turn-reconcile-1", "第一条已提交但缺 intent"),
        ("turn-reconcile-2", "第二条已提交但缺 intent"),
    ] {
        runtime
            .commit_transcript(MemoryTranscriptCommitRequest {
                turn: finalize_request("subject-default", "window-a", turn_id, content).turn,
                host_refs: Vec::new(),
            })
            .expect("transcript-only commit");
    }

    let first = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 1 })
        .expect("first bounded reconciliation");
    assert_eq!(first.inspected, 1);
    assert_eq!(first.created, 1);
    assert_eq!(first.cursor_sequence, 1);
    assert!(first.has_more);

    let second = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 1 })
        .expect("second bounded reconciliation");
    assert_eq!(second.inspected, 1);
    assert_eq!(second.created, 1);
    assert_eq!(second.cursor_sequence, 2);
    assert!(!second.has_more);

    let exhausted = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 1 })
        .expect("exhausted reconciliation");
    assert_eq!(exhausted.inspected, 0);
    assert_eq!(exhausted.created, 0);
    assert_eq!(exhausted.cursor_sequence, 2);
    assert!(!exhausted.has_more);
    let queue = runtime
        .deferred_governance_report()
        .expect("exact queue report");
    assert_eq!(queue.pending, 2);
}

#[test]
fn active_lease_renewal_is_fenced_by_owner_epoch_and_strict_deadline() {
    let profile = support::host_test_profile();
    let runtime = test_runtime_with_scope_and_subject(
        empty_store_platform(profile),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    install_learning_binding(&runtime, 1);
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-renew",
                "续租必须受 fencing 约束",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:renew".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let renewed = runtime
        .renew_governance_job_lease(MemoryGovernanceJobRenewRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:renew".to_string(),
            lease_epoch: leased.lease_epoch,
            lease_until: 1_800_000_120,
        })
        .expect("renew")
        .job;
    assert_eq!(renewed.lease_epoch, leased.lease_epoch);
    assert_eq!(renewed.lease_until, Some(1_800_000_120));
    assert!(renewed.state_revision > leased.state_revision);

    let stale_owner = runtime
        .renew_governance_job_lease(MemoryGovernanceJobRenewRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:other".to_string(),
            lease_epoch: leased.lease_epoch,
            lease_until: 1_800_000_180,
        })
        .expect_err("another worker must not renew");
    assert_eq!(stale_owner.stage(), "post_turn_governance_renew");
    let non_extending = runtime
        .renew_governance_job_lease(MemoryGovernanceJobRenewRequest {
            job_id,
            lease_owner: "worker:renew".to_string(),
            lease_epoch: leased.lease_epoch,
            lease_until: 1_800_000_120,
        })
        .expect_err("renew must strictly extend");
    assert_eq!(non_extending.stage(), "post_turn_governance_renew");
}

#[test]
fn legacy_raw_turn_queue_requires_explicit_reset_and_is_never_auto_consumed() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    platform
        .replay_harness()
        .state_fs()
        .write(
            "memory/governance_jobs/pending.json",
            br#"[{"turn":{"raw":"must-not-migrate"}}]"#,
        )
        .expect("seed legacy queue");
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let error = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-legacy-reset",
                "不得迁移旧队列",
            ),
        )
        .err()
        .expect("legacy queue must block V2 worker readiness");
    assert!(error
        .to_string()
        .contains("legacy_governance_queue_reset_required"));
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert!(snapshot
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "post_turn_governance_jobs"));
    assert_eq!(
        platform
            .replay_harness()
            .state_fs()
            .read("memory/governance_jobs/pending.json")
            .expect("read legacy queue")
            .as_deref(),
        Some(br#"[{"turn":{"raw":"must-not-migrate"}}]"#.as_slice())
    );
}

#[test]
fn duplicate_finalize_repairs_or_reuses_one_exact_intent() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let request = finalize_request("subject-default", "window-a", "turn-deferred-1", "同一内容");
    let first = runtime
        .finalize_turn_with_inline_governance(None, None, request.clone())
        .expect("first finalize");
    let second = runtime
        .finalize_turn_with_inline_governance(None, None, request)
        .expect("duplicate finalize");
    assert_eq!(
        first.memory_consolidation.job_id,
        second.memory_consolidation.job_id
    );
    assert_eq!(
        second.memory_consolidation.reason,
        "governance_intent_already_present"
    );
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "post_turn_governance_jobs")
            .count(),
        1
    );
}

#[test]
fn duplicate_finalize_with_different_learning_evidence_is_rejected_without_mutation() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut first = finalize_request(
        "subject-default",
        "window-a",
        "turn-learning-evidence-conflict",
        "同一回合内容",
    );
    first.turn.tool_observations = vec![bm_sdk::ToolObservationDigest {
        observation_id: "actual-counted-call".into(),
        tool_name: "synthetic.inspect".into(),
        summary: "completed one synthetic call".into(),
        external_content: false,
    }];
    first.learning.tool_call_count = 1;
    let mut conflicting = first.clone();
    conflicting.learning.tool_call_count = 2;
    runtime.finalize_turn(first).expect("first finalize");
    let before = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot before conflicting evidence");

    let error = runtime
        .finalize_turn(conflicting)
        .err()
        .expect("same turn with different learning evidence must conflict");
    let bm_sdk::Error::Other { source, .. } = error else {
        panic!("typed evidence rejection")
    };
    assert_eq!(
        source
            .downcast_ref::<bm_sdk::ProceduralLearningSdkError>()
            .expect("typed procedural SDK error")
            .key,
        bm_sdk::ProceduralLearningErrorKeyV1::EvidenceConflict
    );

    let after = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot after conflicting evidence");
    assert_eq!(before.json_docs, after.json_docs);
    assert_eq!(before.events, after.events);
}

#[test]
fn production_finalize_atomically_persists_semantic_and_procedural_intents() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let registry = AgentToolRegistrySnapshot::compact(
        "host-tools",
        "host",
        vec![AgentToolDescriptor::compact(
            "pdf.extract",
            "PDF Extract",
            "schema-pdf-v1",
        )],
        1_800_000_000,
    );
    runtime
        .upsert_agent_tool_registry(registry.clone())
        .expect("register tool registry");
    let mut request = finalize_request(
        "subject-default",
        "window-a",
        "turn-dual-learning-intents",
        "请提取 PDF 内容",
    );
    attach_tool_feedback(
        &mut request,
        AgentToolUsageFeedbackV2 {
            tool_id: registry.tools[0].tool_id.clone(),
            schema_fingerprint: registry.tools[0].schema_fingerprint.clone(),
            registry_ref: registry.registry_ref(),
            observations: ["1", "2"]
                .into_iter()
                .map(|suffix| AgentToolObservationDigest {
                    observation_id: format!("observation-pdf-{suffix}"),
                    registry_id: "host-tools".to_string(),
                    tool_id: "pdf.extract".to_string(),
                    schema_fingerprint: "schema-pdf-v1".to_string(),
                    call_id: Some(format!("call-pdf-{suffix}")),
                    task_signature: "extract_pdf_text".to_string(),
                    summary: "PDF extraction succeeded".to_string(),
                    outcome: AgentToolOutcome::Succeeded,
                    error_code: None,
                    external_content: false,
                    private_content_used: false,
                    permission_tags: vec!["filesystem.read".to_string()],
                    risk_tags: Vec::new(),
                    started_at: Some(1_800_000_000),
                    completed_at: Some(1_800_000_001),
                })
                .collect(),
            user_visible_result_summary: Some("已提取 PDF 内容".to_string()),
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            operator_note: Some("diagnostic note only".to_string()),
        },
    );

    let report = runtime.finalize_turn(request).expect("finalize");
    assert_eq!(
        report.memory_consolidation.state,
        MemoryConsolidationState::Queued
    );
    assert_eq!(
        report.procedural_learning.state,
        MemoryConsolidationState::Queued
    );
    assert!(report.procedural_learning.job_id.is_some());
    let semantic_job_id = report
        .memory_consolidation
        .job_id
        .clone()
        .expect("semantic job id");

    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "post_turn_governance_jobs")
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "procedural_feedback_jobs")
            .count(),
        1
    );
    let transcript = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "conversation_transcript")
        .expect("transcript");
    assert!(transcript.value["learning_evidence"].is_object());
    assert_eq!(
        transcript.value["learning_evidence"]["authority"]["kind"],
        "host_runtime_observation"
    );

    let engine = MemoryLearningEngine::attach(Arc::new(runtime)).expect("attach learning engine");
    let mut port = ForbiddenProceduralExecutionPort::default();
    let outcome = engine
        .run_due_cycle(learning_cycle_request(1), &mut port)
        .expect("run local procedural lane");
    let MemoryLearningCycleOutcome::ProceduralCompleted(report) = outcome else {
        panic!("first fair cycle must complete the due procedural lane")
    };
    assert_eq!(port.calls, 0);
    assert_eq!(report.receipt.submitted_count, 1);
    assert_eq!(report.receipt.accepted_count, 1);
    assert_eq!(report.receipt.deferred_count, 0);
    assert_eq!(report.receipt.rejected_count, 0);
    assert_eq!(report.receipt.changed_count, 1);

    let completed = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("completed snapshot");
    for (namespace, expected) in [
        ("procedural_feedback_application_ledgers", 1),
        ("agent_tool_experience_revision_materials", 1),
        ("agent_tool_experience_owner_heads", 1),
        ("agent_tool_experience_scope_manifests", 1),
        ("memory_mutation_receipts", 1),
    ] {
        assert_eq!(
            completed
                .json_docs
                .iter()
                .filter(|doc| doc.namespace == namespace)
                .count(),
            expected,
            "{namespace} must commit in the same procedural closure"
        );
    }
    let semantic_outcome = engine
        .run_due_cycle(learning_cycle_request(1), &mut port)
        .expect("run fair semantic lane after procedural completion");
    let MemoryLearningCycleOutcome::Blocked(semantic_report) = semantic_outcome else {
        panic!("unbound semantic lane must report its independent configuration block")
    };
    assert_eq!(semantic_report.job.job_id, semantic_job_id);
    assert_eq!(port.calls, 0);
}

#[test]
fn operator_note_never_promotes_one_runtime_observation() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let registry = AgentToolRegistrySnapshot::compact(
        "host-tools",
        "host",
        vec![AgentToolDescriptor::compact(
            "pdf.extract",
            "PDF Extract",
            "schema-pdf-v1",
        )],
        1_800_000_000,
    );
    runtime
        .upsert_agent_tool_registry(registry.clone())
        .expect("register tool registry");
    let mut request = finalize_request(
        "subject-default",
        "window-note-authority",
        "turn-note-authority",
        "一次普通自动工具调用",
    );
    attach_tool_feedback(
        &mut request,
        AgentToolUsageFeedbackV2 {
            tool_id: registry.tools[0].tool_id.clone(),
            schema_fingerprint: registry.tools[0].schema_fingerprint.clone(),
            registry_ref: registry.registry_ref(),
            observations: vec![AgentToolObservationDigest {
                observation_id: "observation-note-1".to_string(),
                registry_id: "host-tools".to_string(),
                tool_id: "pdf.extract".to_string(),
                schema_fingerprint: "schema-pdf-v1".to_string(),
                call_id: Some("call-note-1".to_string()),
                task_signature: "extract_pdf_text".to_string(),
                summary: "PDF extraction succeeded".to_string(),
                outcome: AgentToolOutcome::Succeeded,
                error_code: None,
                external_content: false,
                private_content_used: false,
                permission_tags: Vec::new(),
                risk_tags: Vec::new(),
                started_at: Some(1_800_000_000),
                completed_at: Some(1_800_000_001),
            }],
            user_visible_result_summary: None,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            operator_note: Some("this text is not confirmation authority".to_string()),
        },
    );
    runtime.finalize_turn(request).expect("finalize");
    let engine = MemoryLearningEngine::attach(Arc::new(runtime)).expect("attach learning engine");
    let mut port = ForbiddenProceduralExecutionPort::default();
    let outcome = engine
        .run_due_cycle(learning_cycle_request(1), &mut port)
        .expect("run local procedural lane");
    let MemoryLearningCycleOutcome::ProceduralCompleted(report) = outcome else {
        panic!("procedural governance must terminate with a deferred receipt")
    };
    assert_eq!(port.calls, 0);
    assert_eq!(report.receipt.accepted_count, 0);
    assert_eq!(report.receipt.deferred_count, 1);
    assert_eq!(report.receipt.changed_count, 0);
    let completed = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("completed snapshot");
    assert_eq!(
        completed
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "agent_tool_experience_revision_materials")
            .count(),
        0
    );
}

#[test]
fn unknown_tool_registry_rejects_before_transcript_or_job_mutation() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let unknown_registry = AgentToolRegistrySnapshot::compact(
        "unknown-tools",
        "host",
        vec![AgentToolDescriptor::compact(
            "pdf.extract",
            "PDF Extract",
            "schema-pdf-v1",
        )],
        1_800_000_000,
    );
    let mut request = finalize_request(
        "subject-default",
        "window-unknown-registry",
        "turn-unknown-registry",
        "未知 registry 不得进入 Transcript",
    );
    attach_tool_feedback(
        &mut request,
        AgentToolUsageFeedbackV2 {
            tool_id: unknown_registry.tools[0].tool_id.clone(),
            schema_fingerprint: unknown_registry.tools[0].schema_fingerprint.clone(),
            registry_ref: unknown_registry.registry_ref(),
            observations: vec![AgentToolObservationDigest {
                observation_id: "observation-unknown-1".to_string(),
                registry_id: "unknown-tools".to_string(),
                tool_id: "pdf.extract".to_string(),
                schema_fingerprint: "schema-pdf-v1".to_string(),
                call_id: Some("call-unknown-1".to_string()),
                task_signature: "extract_pdf_text".to_string(),
                summary: "must not persist".to_string(),
                outcome: AgentToolOutcome::Succeeded,
                error_code: None,
                external_content: false,
                private_content_used: false,
                permission_tags: Vec::new(),
                risk_tags: Vec::new(),
                started_at: Some(1_800_000_000),
                completed_at: Some(1_800_000_001),
            }],
            user_visible_result_summary: None,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            operator_note: None,
        },
    );
    let before = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot before");
    let error = runtime
        .finalize_turn(request)
        .err()
        .expect("unknown registry must fail before durable mutation");
    assert_eq!(error.stage(), "post_turn_learning_evidence");
    let after = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot after");
    assert_eq!(before.json_docs, after.json_docs);
    assert_eq!(before.events, after.events);
}

fn reopen_procedural_registry() -> AgentToolRegistrySnapshot {
    AgentToolRegistrySnapshot::compact(
        "reopen-tools",
        "host",
        vec![AgentToolDescriptor::compact(
            "archive.unpack",
            "Archive Unpack",
            "schema-archive-v1",
        )],
        1_800_000_000,
    )
}

fn seed_reopen_procedural_job(store: MemoryStoreHandle) -> (String, AgentToolRegistrySnapshot) {
    let profile = support::host_test_profile();
    let runtime = test_runtime_with_scope_and_subject(
        store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let registry = reopen_procedural_registry();
    runtime
        .upsert_agent_tool_registry(registry.clone())
        .expect("register reopen tool registry");
    let mut request = finalize_request(
        "subject-default",
        "chat-a",
        "turn-procedural-reopen",
        "持久任务必须在重开后完成",
    );
    attach_tool_feedback(
        &mut request,
        AgentToolUsageFeedbackV2 {
            tool_id: registry.tools[0].tool_id.clone(),
            schema_fingerprint: registry.tools[0].schema_fingerprint.clone(),
            registry_ref: registry.registry_ref(),
            observations: ["1", "2"]
                .into_iter()
                .map(|suffix| AgentToolObservationDigest {
                    observation_id: format!("observation-reopen-{suffix}"),
                    registry_id: "reopen-tools".to_string(),
                    tool_id: "archive.unpack".to_string(),
                    schema_fingerprint: "schema-archive-v1".to_string(),
                    call_id: Some(format!("call-reopen-{suffix}")),
                    task_signature: "unpack_archive".to_string(),
                    summary: "archive unpack succeeded".to_string(),
                    outcome: AgentToolOutcome::Succeeded,
                    error_code: None,
                    external_content: false,
                    private_content_used: false,
                    permission_tags: vec!["filesystem.read".to_string()],
                    risk_tags: Vec::new(),
                    started_at: Some(1_800_000_000),
                    completed_at: Some(1_800_000_001),
                })
                .collect(),
            user_visible_result_summary: Some("已解包归档".to_string()),
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            operator_note: None,
        },
    );
    let report = runtime.finalize_turn(request).expect("seed procedural job");
    (
        report
            .procedural_learning
            .job_id
            .expect("procedural job id"),
        registry,
    )
}

fn complete_reopened_procedural_job(
    store: MemoryStoreHandle,
    expected_job_id: &str,
    registry: AgentToolRegistrySnapshot,
) {
    let profile = support::host_test_profile();
    let runtime = test_runtime_with_scope_and_subject(
        store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .upsert_agent_tool_registry(registry)
        .expect("remount exact tool registry");
    let engine = MemoryLearningEngine::attach(Arc::new(runtime)).expect("attach reopened engine");
    let mut port = ForbiddenProceduralExecutionPort::default();
    let outcome = engine
        .run_due_cycle(learning_cycle_request(1), &mut port)
        .expect("complete reopened procedural job");
    let MemoryLearningCycleOutcome::ProceduralCompleted(report) = outcome else {
        panic!("reopened procedural job must remain due")
    };
    assert_eq!(port.calls, 0);
    assert_eq!(report.job.job_id, expected_job_id);
    assert_eq!(report.receipt.accepted_count, 1);
    assert_eq!(report.receipt.changed_count, 1);
}

fn assert_production_experience_read(store: MemoryStoreHandle, projection: bool) {
    let (job_id, registry) = seed_reopen_procedural_job(store.clone());
    complete_reopened_procedural_job(store.clone(), &job_id, registry.clone());
    let runtime = test_runtime_with_scope_and_subject(
        store,
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .upsert_agent_tool_registry(registry.clone())
        .expect("remount registry");
    if projection {
        let result = runtime
            .project(bm_sdk::MemoryProjectionRequest {
                binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                user_query: "unpack archive".into(),
                system_max_len: 8192,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: vec![],
                tool_registry_refs: vec![registry.registry_ref()],
            })
            .expect("project accepted production experience");
        let hints = result.provider_payload().agent_tool_hints();
        assert_eq!(
            hints.len(),
            1,
            "real accepted experience must reach provider projection"
        );
        assert_eq!(hints[0].tool_id, "archive.unpack");
    } else {
        let result = runtime
            .recall(bm_sdk::MemoryRecallRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                query: "unpack archive".into(),
                limit: 8,
                structured_query_facets: vec![],
                tool_registry_refs: vec![registry.registry_ref()],
            })
            .expect("recall accepted production experience");
        assert_eq!(
            result.agent_tool_hints.len(),
            1,
            "real accepted experience must be recalled"
        );
        assert_eq!(result.agent_tool_hints[0].tool_id, "archive.unpack");
        assert!(result.store_snapshot_consistent);
    }
}

#[test]
fn production_experience_current_recall_positive() {
    assert_production_experience_read(empty_store_platform(support::host_test_profile()), false);
}

#[test]
fn production_experience_current_projection_positive() {
    assert_production_experience_read(empty_store_platform(support::host_test_profile()), true);
}

#[test]
fn production_projection_audit_counts_only_delivered_tool_hints() {
    let store = empty_store_platform(support::host_test_profile());
    let (job, registry) = seed_reopen_procedural_job(store.clone());
    complete_reopened_procedural_job(store.clone(), &job, registry.clone());
    let runtime = test_runtime_with_scope_and_subject(
        store,
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .upsert_agent_tool_registry(registry.clone())
        .unwrap();
    let mut observed_positive = false;
    let mut observed_budget_drop = false;
    for budget in [128, 512, 8192] {
        let output = runtime
            .project(bm_sdk::MemoryProjectionRequest {
                binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                user_query: "unpack archive".into(),
                system_max_len: budget,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: vec![],
                tool_registry_refs: vec![registry.registry_ref()],
            })
            .expect("bounded projection");
        let delivered = output.provider_payload().agent_tool_hints().len();
        observed_positive |= delivered > 0;
        observed_budget_drop |= delivered == 0;
        assert_eq!(
            output.report().audit().agent_tool_selected_count,
            delivered,
            "safe audit must describe final delivery, not pre-budget candidates"
        );
        if delivered == 0 {
            assert!(output.report().audit().agent_tool_rejection_count > 0);
        }
    }
    assert!(observed_positive && observed_budget_drop);
}

#[test]
fn production_feedback_group_counts_are_distinct_from_changed_owner_counts() {
    assert_feedback_group_owner_accounting(true);
}

#[test]
fn production_feedback_successes_cannot_pool_across_task_signatures() {
    assert_feedback_group_owner_accounting(false);
}

fn assert_feedback_group_owner_accounting(repeated_per_task: bool) {
    let store = empty_store_platform(support::host_test_profile());
    let runtime = Arc::new(test_runtime_with_scope_and_subject(
        store.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let registry = reopen_procedural_registry();
    runtime
        .upsert_agent_tool_registry(registry.clone())
        .unwrap();
    let mut request = finalize_request(
        "subject-default",
        "chat-a",
        "multi-task-group",
        "unpack archives",
    );
    attach_tool_feedback(
        &mut request,
        AgentToolUsageFeedbackV2 {
            tool_id: registry.tools[0].tool_id.clone(),
            schema_fingerprint: registry.tools[0].schema_fingerprint.clone(),
            registry_ref: registry.registry_ref(),
            observations: ["archive-a", "archive-b"]
                .into_iter()
                .flat_map(|task| {
                    ["1", "2"]
                        .into_iter()
                        .take(if repeated_per_task { 2 } else { 1 })
                        .map(move |id| AgentToolObservationDigest {
                            observation_id: format!("{task}-{id}"),
                            registry_id: "reopen-tools".into(),
                            tool_id: "archive.unpack".into(),
                            schema_fingerprint: "schema-archive-v1".into(),
                            call_id: Some(format!("call-{task}-{id}")),
                            task_signature: task.into(),
                            summary: "unpack archives successfully".into(),
                            outcome: AgentToolOutcome::Succeeded,
                            error_code: None,
                            external_content: false,
                            private_content_used: false,
                            permission_tags: vec![],
                            risk_tags: vec![],
                            started_at: Some(1_800_000_000),
                            completed_at: Some(1_800_000_001),
                        })
                })
                .collect(),
            user_visible_result_summary: None,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            operator_note: None,
        },
    );
    runtime
        .finalize_turn(request)
        .expect("finalize multi-task group");
    let engine = MemoryLearningEngine::attach(runtime).unwrap();
    let MemoryLearningCycleOutcome::ProceduralCompleted(report) = engine
        .run_due_cycle(
            learning_cycle_request(1),
            &mut ForbiddenProceduralExecutionPort::default(),
        )
        .expect("learn two owners atomically")
    else {
        panic!("procedural completion");
    };
    assert_eq!(report.receipt.submitted_count, 1);
    assert_eq!(report.receipt.accepted_count, u32::from(repeated_per_task));
    assert_eq!(report.receipt.deferred_count, u32::from(!repeated_per_task));
    let snapshot = store.export_replay_snapshot().unwrap();
    let heads = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "agent_tool_experience_owner_heads")
        .count();
    assert_eq!(
        heads,
        if repeated_per_task { 2 } else { 0 },
        "only repeated same-task evidence creates durable owners"
    );
    assert_eq!(report.receipt.changed_count, heads as u32);
}

#[test]
fn production_learning_raw_delete_cancels_procedural_and_erases_learning_evidence() {
    let store = empty_store_platform(support::host_test_profile());
    let (job_id, _) = seed_reopen_procedural_job(store.clone());
    let runtime = test_runtime_with_scope_and_subject(
        store.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().into(),
            channel_id: "llm.gateway".into(),
            conversation_id: "chat-a".into(),
            turn_id: Some("turn-procedural-reopen".into()),
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "withdraw synthetic evidence".into(),
        })
        .expect("delete learning source");
    let snapshot = store.export_replay_snapshot().expect("deleted snapshot");
    let job = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "procedural_feedback_jobs" && doc.value["job_id"] == job_id)
        .expect("durable procedural job");
    assert_eq!(
        job.value["status"], "cancelled",
        "withdrawal must cancel both learning lanes atomically"
    );
    let transcript = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "conversation_transcript")
        .expect("transcript tombstone");
    assert!(
        transcript.value["learning_evidence"].is_null(),
        "DeleteRaw must erase learning evidence too"
    );
}

#[test]
fn production_learning_public_archive_never_exports_raw_tool_evidence() {
    let store = empty_store_platform(support::host_test_profile());
    seed_reopen_procedural_job(store.clone());
    let runtime = test_runtime_with_scope_and_subject(
        store.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let durable = serde_json::to_string(&store.export_replay_snapshot().expect("durable snapshot"))
        .expect("snapshot JSON");
    assert!(
        durable.contains("archive unpack succeeded"),
        "positive raw evidence in controlled Store"
    );
    let exported = runtime
        .export_memory_space(bm_sdk::MemorySpaceExportRequest {
            scope: bm_sdk::MemoryArchiveScope::Subject {
                memory_space_id: runtime.memory_space_id().into(),
                mounted_subject_id: runtime.subject_id().into(),
            },
            private_material_policy: bm_sdk::MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .expect("public archive");
    let restored_store = empty_store_platform(support::host_test_profile());
    let restored = test_runtime_with_scope_and_subject(
        restored_store.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    restored
        .import_memory_space(bm_sdk::MemorySpaceImportRequest {
            scope: exported.projection_scope.scope,
            expected_private_material_policy:
                bm_sdk::MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        })
        .expect("import public archive into empty synthetic Store");
    let encoded = serde_json::to_string(
        &restored_store
            .export_replay_snapshot()
            .expect("restored snapshot"),
    )
    .expect("archive JSON");
    assert!(
        encoded.contains("持久任务必须在重开后完成"),
        "chat transcript remains positive"
    );
    assert!(
        !encoded.contains("archive unpack succeeded"),
        "public archive must not export tool evidence raw"
    );
    assert!(!encoded.contains("observation-reopen-1"));
}

fn assert_experience_history_and_isolation(store: MemoryStoreHandle) {
    let runtime = test_runtime_with_scope_and_subject(
        store.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let registry = reopen_procedural_registry();
    let request = |time| bm_sdk::MemoryRecallRequest {
        temporal_operation: time,
        query: "unpack archive".into(),
        limit: 8,
        structured_query_facets: vec![],
        tool_registry_refs: vec![registry.registry_ref()],
    };
    assert!(runtime
        .recall(request(bm_sdk::MemoryRecallTemporalOperation::Current))
        .expect("registry absent")
        .agent_tool_hints
        .is_empty());
    runtime
        .upsert_agent_tool_registry(registry.clone())
        .expect("remount exact registry");
    for (time, expected) in [
        (bm_sdk::MemoryRecallTemporalOperation::Current, 1),
        (
            bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_799_999_999,
            },
            0,
        ),
        (
            bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_800_000_000,
            },
            1,
        ),
    ] {
        let recall = runtime
            .recall(request(time))
            .expect("revision-exact recall");
        assert_eq!(recall.agent_tool_hints.len(), expected, "{time:?}");
        let projection = runtime
            .project(bm_sdk::MemoryProjectionRequest {
                binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
                temporal_operation: time,
                user_query: "unpack archive".into(),
                system_max_len: 8192,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: vec![],
                tool_registry_refs: vec![registry.registry_ref()],
            })
            .expect("revision-exact projection");
        assert_eq!(
            projection.provider_payload().agent_tool_hints().len(),
            expected,
            "{time:?}"
        );
    }
    let mut shared_registry = runtime.subject_registry().clone();
    shared_registry
        .upsert_subject(bm_sdk::SubjectDescriptor::agent_persona(
            "subject-b",
            "Agent B",
        ))
        .expect("register B");
    let other = MemoryRuntime::builder()
        .identity(runtime.identity().clone())
        .subject_id("subject-b")
        .subject_registry(shared_registry)
        .scope(runtime.scope().clone())
        .store(store.clone())
        .agent_tool_registry(registry.clone())
        .build()
        .expect("other active subject");
    let denied = other
        .recall(request(bm_sdk::MemoryRecallTemporalOperation::Current))
        .expect("other subject recall");
    assert!(denied.agent_tool_hints.is_empty());
    assert_eq!(
        denied.tool_experience_status.governed_experience_candidates,
        0
    );
    let projected = other
        .project(bm_sdk::MemoryProjectionRequest {
            binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            user_query: "unpack archive".into(),
            system_max_len: 8192,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .expect("other subject projection");
    assert!(projected.provider_payload().agent_tool_hints().is_empty());
    assert!(!projected
        .provider_payload()
        .system_memory_block()
        .contains("archive unpack succeeded"));
    assert!(!format!("{:?}", projected.report().audit()).contains("observation-reopen"));
    let mut bad = request(bm_sdk::MemoryRecallTemporalOperation::Current);
    bad.tool_registry_refs[0].fingerprint = "wrong-fingerprint".into();
    assert!(runtime
        .recall(bad)
        .expect("registry mismatch fail closed")
        .agent_tool_hints
        .is_empty());
    let public_before = runtime
        .export_memory_space(bm_sdk::MemorySpaceExportRequest {
            scope: bm_sdk::MemoryArchiveScope::Subject {
                memory_space_id: runtime.memory_space_id().into(),
                mounted_subject_id: runtime.subject_id().into(),
            },
            private_material_policy: bm_sdk::MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .expect("safe archive before withdrawal");
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().into(),
            channel_id: "llm.gateway".into(),
            conversation_id: "chat-a".into(),
            turn_id: Some("turn-procedural-reopen".into()),
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "withdraw source".into(),
        })
        .expect("withdraw accepted source");
    for time in [
        bm_sdk::MemoryRecallTemporalOperation::Current,
        bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
            as_of_time: 1_800_000_000,
        },
    ] {
        assert!(runtime
            .recall(request(time))
            .expect("no historical deletion bypass")
            .agent_tool_hints
            .is_empty());
    }
    let terminal = store
        .export_replay_snapshot()
        .expect("terminal owner snapshot");
    assert!(terminal
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "agent_tool_experience_revision_materials"));
    assert!(!serde_json::to_string(&terminal)
        .expect("snapshot JSON")
        .contains("archive unpack succeeded"));
    runtime
        .import_memory_space(bm_sdk::MemorySpaceImportRequest {
            scope: public_before.projection_scope.scope,
            expected_private_material_policy:
                bm_sdk::MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: public_before.archive,
        })
        .expect("safe public archive cannot replace protected tombstone");
    assert!(runtime
        .recall(request(bm_sdk::MemoryRecallTemporalOperation::Current))
        .expect("no archive resurrection")
        .agent_tool_hints
        .is_empty());
}

#[test]
fn production_experience_memory_reopen_history_and_isolation() {
    let store = empty_store_platform(support::host_test_profile());
    let (id, registry) = seed_reopen_procedural_job(store.clone());
    complete_reopened_procedural_job(store.clone(), &id, registry);
    assert_experience_history_and_isolation(store);
}

#[test]
fn production_experience_file_reopen_history_and_isolation() {
    let root = persistent_test_root("experience-read-history");
    let config = StoreBackendConfig::file(&root, support::host_test_profile()).expect("config");
    {
        let store = MemoryStoreHandle::open(config.clone()).expect("first open");
        let (id, registry) = seed_reopen_procedural_job(store.clone());
        complete_reopened_procedural_job(store, &id, registry);
    }
    assert_experience_history_and_isolation(MemoryStoreHandle::open(config).expect("reopen"));
    std::fs::remove_dir_all(root).expect("remove synthetic Store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn production_experience_sqlite_reopen_history_and_isolation() {
    let root = persistent_test_root("experience-read-history-sqlite");
    let config =
        StoreBackendConfig::sqlite(root.join("memory.sqlite3"), support::host_test_profile())
            .expect("config");
    {
        let store = MemoryStoreHandle::open(config.clone()).expect("first open");
        let (id, registry) = seed_reopen_procedural_job(store.clone());
        complete_reopened_procedural_job(store, &id, registry);
    }
    assert_experience_history_and_isolation(MemoryStoreHandle::open(config).expect("reopen"));
    std::fs::remove_dir_all(root).expect("remove synthetic Store");
}

#[test]
fn production_experience_two_revisions_and_source_withdrawal_are_exact() {
    let store = empty_store_platform(support::host_test_profile());
    let base = test_runtime_with_scope_and_subject(
        store.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let clock = Arc::new(AdjustableMemoryClock::new(1_800_000_000));
    let registry = reopen_procedural_registry();
    let runtime = Arc::new(
        MemoryRuntime::builder()
            .identity(base.identity().clone())
            .subject_id(base.subject_id())
            .scope(base.scope().clone())
            .subject_registry(base.subject_registry().clone())
            .store(store.clone())
            .agent_tool_registry(registry.clone())
            .clock(clock.clone())
            .build()
            .expect("runtime"),
    );
    for (time, marker, turn) in [
        (1_800_000_000, "GUIDANCE_REVISION_ONE", "rev-one"),
        (1_800_000_100, "GUIDANCE_REVISION_TWO", "rev-two"),
    ] {
        clock.set(time);
        let mut request =
            finalize_request("subject-default", "chat-a", turn, "learn archive usage");
        attach_tool_feedback(
            &mut request,
            AgentToolUsageFeedbackV2 {
                tool_id: registry.tools[0].tool_id.clone(),
                schema_fingerprint: registry.tools[0].schema_fingerprint.clone(),
                registry_ref: registry.registry_ref(),
                observations: ["a", "b"]
                    .into_iter()
                    .map(|suffix| AgentToolObservationDigest {
                        observation_id: format!("{turn}-{suffix}"),
                        registry_id: registry.registry_id.clone(),
                        tool_id: "archive.unpack".into(),
                        schema_fingerprint: "schema-archive-v1".into(),
                        call_id: Some(format!("call-{turn}-{suffix}")),
                        task_signature: "unpack_archive".into(),
                        summary: format!("unpack archive: {marker}"),
                        outcome: AgentToolOutcome::Succeeded,
                        error_code: None,
                        external_content: false,
                        private_content_used: false,
                        permission_tags: vec![],
                        risk_tags: vec![],
                        started_at: Some(time),
                        completed_at: Some(time),
                    })
                    .collect(),
                user_visible_result_summary: None,
                outcome: ProceduralExecutionOutcomeV1::Succeeded,
                operator_note: None,
            },
        );
        runtime
            .finalize_turn(request)
            .expect("finalize revision evidence");
        let engine = MemoryLearningEngine::attach(runtime.clone()).expect("engine");
        assert!(matches!(
            engine
                .run_due_cycle(
                    learning_cycle_request(1),
                    &mut ForbiddenProceduralExecutionPort::default()
                )
                .expect("learn revision"),
            MemoryLearningCycleOutcome::ProceduralCompleted(_)
        ));
    }
    let request = |time| bm_sdk::MemoryRecallRequest {
        temporal_operation: time,
        query: "unpack archive".into(),
        limit: 8,
        structured_query_facets: vec![],
        tool_registry_refs: vec![registry.registry_ref()],
    };
    let current = runtime
        .recall(request(bm_sdk::MemoryRecallTemporalOperation::Current))
        .expect("current");
    assert_eq!(current.agent_tool_hints.len(), 1);
    assert!(current.agent_tool_hints[0]
        .reason
        .contains("GUIDANCE_REVISION_TWO"));
    let past = runtime
        .recall(request(
            bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_800_000_050,
            },
        ))
        .expect("past");
    assert_eq!(past.agent_tool_hints.len(), 1);
    assert!(past.agent_tool_hints[0]
        .reason
        .contains("GUIDANCE_REVISION_ONE"));
    assert!(!past.agent_tool_hints[0]
        .reason
        .contains("GUIDANCE_REVISION_TWO"));
    let withdraw = || MemoryTranscriptLifecycleRequest {
        memory_space_id: runtime.memory_space_id().into(),
        channel_id: "llm.gateway".into(),
        conversation_id: "chat-a".into(),
        turn_id: Some("rev-one".into()),
        transition: TranscriptLifecycleTransition::DeleteRaw,
        reason: "withdraw first source".into(),
    };
    runtime
        .request_transcript_lifecycle(withdraw())
        .expect("withdraw founding evidence");
    for time in [
        bm_sdk::MemoryRecallTemporalOperation::Current,
        bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
            as_of_time: 1_800_000_050,
        },
    ] {
        assert!(runtime
            .recall(request(time))
            .expect("deleted current/history")
            .agent_tool_hints
            .is_empty());
    }
    let before = store.export_replay_snapshot().expect("terminal snapshot");
    assert_eq!(
        before
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "agent_tool_experience_revision_materials")
            .count(),
        0
    );
    assert_eq!(
        before
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "procedural_feedback_application_ledgers")
            .count(),
        2
    );
    clock.set(1_800_000_200);
    runtime
        .request_transcript_lifecycle(withdraw())
        .expect("idempotent withdrawal");
    let after = store.export_replay_snapshot().expect("repeat snapshot");
    assert_eq!(
        before.json_docs, after.json_docs,
        "repeat lifecycle cannot revise durable owners or receipts"
    );
    let reconciled = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 32 })
        .expect("reconcile after withdrawal");
    assert_eq!(reconciled.procedural_created, 0);
    assert_eq!(reconciled.created, 0);
}

#[test]
fn in_memory_runtime_recreation_preserves_procedural_job_and_evidence() {
    let config =
        StoreBackendConfig::in_memory(support::host_test_profile()).expect("in-memory config");
    let store = MemoryStoreHandle::open(config).expect("in-memory Store");
    let (job_id, registry) = seed_reopen_procedural_job(store.clone());
    complete_reopened_procedural_job(store, &job_id, registry);
}

#[test]
fn file_reopen_preserves_procedural_job_and_evidence() {
    let root = persistent_test_root("file-procedural-reopen");
    let config =
        StoreBackendConfig::file(&root, support::host_test_profile()).expect("file config");
    let (job_id, registry) = {
        let first = MemoryStoreHandle::open(config.clone()).expect("first file Store");
        seed_reopen_procedural_job(first)
    };
    let reopened = MemoryStoreHandle::open(config).expect("reopened file Store");
    complete_reopened_procedural_job(reopened, &job_id, registry);
    std::fs::remove_dir_all(root).expect("remove file procedural Store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_preserves_procedural_job_and_evidence() {
    let root = persistent_test_root("sqlite-procedural-reopen");
    let config =
        StoreBackendConfig::sqlite(root.join("memory.sqlite3"), support::host_test_profile())
            .expect("sqlite config");
    let (job_id, registry) = {
        let first = MemoryStoreHandle::open(config.clone()).expect("first sqlite Store");
        seed_reopen_procedural_job(first)
    };
    let reopened = MemoryStoreHandle::open(config).expect("reopened sqlite Store");
    complete_reopened_procedural_job(reopened, &job_id, registry);
    std::fs::remove_dir_all(root).expect("remove sqlite procedural Store");
}

#[test]
fn concurrent_learning_engines_apply_one_procedural_job_once() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let first = Arc::new(test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let second = Arc::new(test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let registry = reopen_procedural_registry();
    first
        .upsert_agent_tool_registry(registry.clone())
        .expect("register first worker tools");
    second
        .upsert_agent_tool_registry(registry.clone())
        .expect("register second worker tools");
    let mut request = finalize_request(
        "subject-default",
        "window-procedural-concurrent",
        "turn-procedural-concurrent",
        "并发 worker 只能应用一次",
    );
    attach_tool_feedback(
        &mut request,
        AgentToolUsageFeedbackV2 {
            tool_id: registry.tools[0].tool_id.clone(),
            schema_fingerprint: registry.tools[0].schema_fingerprint.clone(),
            registry_ref: registry.registry_ref(),
            observations: ["1", "2"]
                .into_iter()
                .map(|suffix| AgentToolObservationDigest {
                    observation_id: format!("observation-concurrent-{suffix}"),
                    registry_id: "reopen-tools".to_string(),
                    tool_id: "archive.unpack".to_string(),
                    schema_fingerprint: "schema-archive-v1".to_string(),
                    call_id: Some(format!("call-concurrent-{suffix}")),
                    task_signature: "unpack_archive".to_string(),
                    summary: "archive unpack succeeded".to_string(),
                    outcome: AgentToolOutcome::Succeeded,
                    error_code: None,
                    external_content: false,
                    private_content_used: false,
                    permission_tags: Vec::new(),
                    risk_tags: Vec::new(),
                    started_at: Some(1_800_000_000),
                    completed_at: Some(1_800_000_001),
                })
                .collect(),
            user_visible_result_summary: None,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            operator_note: None,
        },
    );
    first
        .finalize_turn(request)
        .expect("finalize concurrent job");
    let barrier = Arc::new(Barrier::new(3));
    let workers = [first, second]
        .into_iter()
        .enumerate()
        .map(|(index, runtime)| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let engine = MemoryLearningEngine::attach(runtime).expect("attach worker");
                let mut port = ForbiddenProceduralExecutionPort::default();
                barrier.wait();
                let outcome = engine
                    .run_due_cycle(
                        MemoryLearningCycleRequest {
                            lease_owner: format!("procedural-worker-{index}"),
                            lease_duration_secs: 60,
                        },
                        &mut port,
                    )
                    .expect("run concurrent worker");
                (outcome, port.calls)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("join worker"))
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|(outcome, _)| {
                matches!(outcome, MemoryLearningCycleOutcome::ProceduralCompleted(_))
            })
            .count(),
        1
    );
    assert!(results.iter().all(|(_, calls)| *calls == 0));
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("concurrent completion snapshot");
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "procedural_feedback_application_ledgers")
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "agent_tool_experience_revision_materials")
            .count(),
        1
    );
}

#[test]
fn reconciliation_repairs_missing_semantic_and_procedural_intents_from_transcript() {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    let (expected_procedural_job_id, registry) = seed_reopen_procedural_job(source.clone());
    let mut transcript_only = source.export_replay_snapshot().expect("source snapshot");
    transcript_only.json_docs.retain(|doc| {
        !matches!(
            doc.namespace.as_str(),
            "post_turn_governance_jobs"
                | "post_turn_governance_scope_indexes"
                | "procedural_feedback_jobs"
                | "procedural_feedback_scope_indexes"
        )
    });

    let target = empty_store_platform(profile);
    target
        .import_replay_snapshot(&transcript_only)
        .expect("import transcript-only crash image");
    let runtime = test_runtime_with_scope_and_subject(
        target.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .upsert_agent_tool_registry(registry)
        .expect("remount exact registry");
    let report = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 1 })
        .expect("reconcile both learning lanes");
    assert_eq!(report.inspected, 1);
    assert_eq!(report.created, 1);
    assert_eq!(report.cursor_sequence, 1);
    assert_eq!(report.procedural_created, 1);
    assert_eq!(report.procedural_cursor_sequence, 1);
    assert!(!report.has_more);
    assert!(!report.procedural_has_more);
    let repaired = target.export_replay_snapshot().expect("repaired snapshot");
    assert!(repaired.json_docs.iter().any(|doc| {
        doc.namespace == "procedural_feedback_jobs" && doc.key == expected_procedural_job_id
    }));

    let engine = MemoryLearningEngine::attach(Arc::new(runtime)).expect("attach repaired engine");
    let mut port = ForbiddenProceduralExecutionPort::default();
    assert!(matches!(
        engine
            .run_due_cycle(learning_cycle_request(1), &mut port)
            .expect("run repaired procedural job"),
        MemoryLearningCycleOutcome::ProceduralCompleted(_)
    ));
    assert_eq!(port.calls, 0);
}

#[test]
fn transcript_mask_cancels_pending_governance_before_claim() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-private",
                "这段内容已经撤回",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");

    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "window-a".to_string(),
            turn_id: Some("turn-private".to_string()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "owner withdrew model use".to_string(),
        })
        .expect("mask transcript");

    let status = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("job status");
    assert_eq!(status.job.status, PostTurnGovernanceJobStatusV2::Cancelled);
    assert_eq!(
        status.job.blocking_reason.as_deref(),
        Some("transcript_masked")
    );
    assert!(runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id,
            lease_owner: "worker-after-mask".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority(),
        })
        .is_err());
}

#[test]
fn same_turn_id_in_different_conversations_has_distinct_jobs() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let first = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "same-turn", "第一段"),
        )
        .expect("first conversation");
    let second = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-b", "same-turn", "第二段"),
        )
        .expect("second conversation");
    assert_ne!(
        first.memory_consolidation.job_id,
        second.memory_consolidation.job_id
    );
    let active = runtime
        .active_governance_jobs(MemoryGovernanceActiveJobsRequest { limit: 8 })
        .expect("runtime-scope active jobs");
    assert_eq!(active.jobs.len(), 2);
    assert!(!active.has_more);
    assert_eq!(
        active.jobs[0].scope_index_key,
        active.jobs[1].scope_index_key
    );
    assert_eq!(
        active
            .jobs
            .iter()
            .map(|job| job.identity.conversation_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["window-a", "window-b"])
    );
}

#[test]
fn concurrent_claim_has_one_winner_and_pins_first_attempt_authority() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let first = Arc::new(test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let second = Arc::new(test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    install_learning_binding(&first, 1);
    let finalized = first
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "turn-claim", "并发领取"),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let authority = attempt_authority_for_job(
        &first
            .governance_job_status(MemoryGovernanceJobStatusRequest {
                job_id: job_id.clone(),
            })
            .expect("pending job")
            .job,
    );
    let barrier = Arc::new(Barrier::new(3));
    let workers = [
        (first, "worker-a", authority.clone()),
        (second, "worker-b", authority.clone()),
    ]
    .into_iter()
    .map(|(runtime, worker, authority)| {
        let barrier = Arc::clone(&barrier);
        let job_id = job_id.clone();
        std::thread::spawn(move || {
            barrier.wait();
            runtime.claim_governance_job(MemoryGovernanceJobClaimRequest {
                job_id,
                lease_owner: worker.to_string(),
                lease_until: 1_800_000_060,
                authority,
            })
        })
    })
    .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let winner = results.into_iter().find_map(Result::ok).expect("winner");
    assert_eq!(winner.job.status, PostTurnGovernanceJobStatusV2::Leased);
    assert_eq!(winner.job.attempt_count, 1);
    assert_eq!(winner.job.lease_epoch, 1);
    assert_eq!(winner.job.attempt_authority, Some(authority));
}

#[test]
fn independent_file_opens_still_allow_only_one_claim_winner() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("file-claim");
    {
        let config = StoreBackendConfig::file(&root, profile).expect("file config");
        assert_independent_open_claim(config);
    }
    std::fs::remove_dir_all(&root).expect("remove file claim store");
}

#[test]
fn independent_file_opens_idempotently_install_the_same_first_binding() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("file-binding-install");
    {
        let config = StoreBackendConfig::file(&root, profile).expect("file config");
        assert_independent_open_binding_install(config);
    }
    std::fs::remove_dir_all(&root).expect("remove file binding Store");
}

#[test]
fn independent_file_opens_reject_divergent_first_binding_identity() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("file-binding-conflict");
    {
        let config = StoreBackendConfig::file(&root, profile).expect("file config");
        assert_independent_open_conflicting_binding_install(config);
    }
    std::fs::remove_dir_all(&root).expect("remove file binding conflict Store");
}

fn assert_binding_snapshot_survives_runtime_reopen(
    first_store: MemoryStoreHandle,
    reopened_store: MemoryStoreHandle,
) {
    let profile = support::host_test_profile();
    let (job_id, revision_one) = {
        let runtime = test_runtime_with_scope_and_subject(
            first_store,
            profile,
            "llm.gateway",
            "chat-a",
            "subject-default",
        );
        let revision_one = install_learning_binding(&runtime, 1);
        let finalized = runtime
            .finalize_turn_with_inline_governance(
                None,
                None,
                finalize_request(
                    "subject-default",
                    "window-binding-reopen",
                    "turn-binding-reopen",
                    "绑定重开合同",
                ),
            )
            .expect("finalize revision-one binding job");
        (
            finalized.memory_consolidation.job_id.expect("job id"),
            revision_one,
        )
    };

    let runtime = Arc::new(test_runtime_with_scope_and_subject(
        reopened_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let revision_two = install_learning_binding(&runtime, 2);
    assert_ne!(revision_one.canonical_digest, revision_two.canonical_digest);
    let reopened_job = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("reopened job")
        .job;
    assert_eq!(
        reopened_job.execution_binding,
        PostTurnGovernanceExecutionBindingV1::Bound {
            binding_id: revision_one.binding_id.clone(),
            binding_revision: 1,
        }
    );

    let engine = MemoryLearningEngine::attach(Arc::clone(&runtime)).expect("reopened engine");
    let mut http = StaticHttpClient;
    let llm = PlaneAwareStaticLlm {
        long_term_content: "[]",
    };
    let mut observed = None;
    let mut port = BindingCapturingPort {
        http: &mut http,
        llm: &llm,
        observed: &mut observed,
    };
    let outcome = engine
        .run_due_cycle(learning_cycle_request(1), &mut port)
        .expect("execute reopened exact binding");
    assert!(matches!(outcome, MemoryLearningCycleOutcome::Completed(_)));
    assert_eq!(
        observed,
        Some((
            revision_one.binding_id,
            1,
            revision_one.model_id,
            revision_one.canonical_digest,
        ))
    );
    assert_eq!(
        runtime
            .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
            .expect("completed reopened job")
            .job
            .status,
        PostTurnGovernanceJobStatusV2::Succeeded
    );
}

#[test]
fn in_memory_runtime_recreation_preserves_exact_binding_snapshot() {
    let config = StoreBackendConfig::in_memory(support::host_test_profile())
        .expect("in-memory binding config");
    let store = MemoryStoreHandle::open(config).expect("in-memory Store");
    assert_binding_snapshot_survives_runtime_reopen(store.clone(), store);
}

#[test]
fn file_reopen_preserves_exact_binding_snapshot() {
    let root = persistent_test_root("file-binding-reopen");
    let config =
        StoreBackendConfig::file(&root, support::host_test_profile()).expect("file binding config");
    let first = MemoryStoreHandle::open(config.clone()).expect("first file Store");
    let reopened = MemoryStoreHandle::open(config).expect("reopened file Store");
    assert_binding_snapshot_survives_runtime_reopen(first, reopened);
    std::fs::remove_dir_all(root).expect("remove file binding Store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_preserves_exact_binding_snapshot() {
    let root = persistent_test_root("sqlite-binding-reopen");
    std::fs::create_dir_all(&root).expect("create sqlite binding root");
    let config =
        StoreBackendConfig::sqlite(root.join("memory.sqlite3"), support::host_test_profile())
            .expect("sqlite binding config");
    let first = MemoryStoreHandle::open(config.clone()).expect("first sqlite Store");
    let reopened = MemoryStoreHandle::open(config).expect("reopened sqlite Store");
    assert_binding_snapshot_survives_runtime_reopen(first, reopened);
    std::fs::remove_dir_all(root).expect("remove sqlite binding Store");
}

#[test]
fn binding_retention_prunes_only_the_oldest_unreferenced_revision() {
    let profile = support::host_test_profile();
    let store = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("binding retention Store config"),
    )
    .expect("binding retention Store");
    let runtime = test_runtime_with_scope_and_subject(
        store.clone(),
        profile,
        "llm.gateway",
        "chat-binding-retention",
        "subject-default",
    );
    let first = install_learning_binding(&runtime, 1);
    for revision in 2..=257 {
        install_learning_binding(&runtime, revision);
    }
    let snapshot = store
        .export_replay_snapshot()
        .expect("binding retention snapshot");
    let revisions = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "post_turn_governance_binding_snapshots")
        .map(|doc| doc.key.clone())
        .collect::<Vec<_>>();
    assert_eq!(revisions.len(), 256);
    assert!(!revisions.contains(&format!("{}:1", first.binding_id)));
    assert!(revisions.contains(&format!("{}:257", first.binding_id)));
}

#[test]
fn binding_retention_preserves_referenced_revision_and_backpressures_when_all_are_pinned() {
    let profile = support::host_test_profile();
    let source = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("binding source Store config"),
    )
    .expect("binding source Store");
    let runtime = test_runtime_with_scope_and_subject(
        source.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let first = install_learning_binding(&runtime, 1);
    runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "binding-pinned-conversation",
                "binding-pinned-turn",
                "绑定保留合同",
            ),
        )
        .expect("create job that pins revision one");
    for revision in 2..=256 {
        install_learning_binding(&runtime, revision);
    }
    install_learning_binding(&runtime, 257);
    let retained = source.export_replay_snapshot().expect("retained snapshot");
    let keys = retained
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "post_turn_governance_binding_snapshots")
        .map(|doc| doc.key.clone())
        .collect::<Vec<_>>();
    assert!(keys.contains(&format!("{}:1", first.binding_id)));
    assert!(!keys.contains(&format!("{}:2", first.binding_id)));

    let mut all_pinned = retained;
    for doc in &mut all_pinned.json_docs {
        if doc.namespace == "post_turn_governance_binding_revision_indexes" {
            for revision in doc.value["revisions"]
                .as_array_mut()
                .expect("binding revisions")
            {
                revision["referenced"] = serde_json::Value::Bool(true);
            }
        }
    }
    let pinned_store = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("all-pinned Store config"),
    )
    .expect("all-pinned Store");
    pinned_store
        .import_replay_snapshot(&all_pinned)
        .expect("import all-pinned binding authority");
    let pinned_runtime = test_runtime_with_scope_and_subject(
        pinned_store.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let before = pinned_store
        .export_replay_snapshot()
        .expect("before exhausted write");
    let error = pinned_runtime
        .install_governance_binding(MemoryGovernanceBindingInstallRequest {
            source_owner_id: "test-deployment".to_string(),
            source_config_id: "primary-governance-provider".to_string(),
            source_revision: 258,
            protocol: PostTurnGovernanceProviderProtocolV1::OllamaNative,
            endpoint: "http://127.0.0.1:11434/api".to_string(),
            model_id: "qwen3:8b".to_string(),
            credential_reference: None,
            request_timeout_ms: 30_000,
            max_input_tokens: 4096,
            max_output_tokens: 1024,
            provider_permission_generation: 1,
        })
        .expect_err("all referenced binding revisions must backpressure");
    assert_eq!(
        error.stage(),
        "post_turn_governance_binding_retention_exhausted"
    );
    let after = pinned_store
        .export_replay_snapshot()
        .expect("after exhausted write");
    assert_eq!(after.json_docs, before.json_docs);
    assert_eq!(after.events, before.events);
}

#[test]
fn snapshot_import_rejects_bound_job_without_exact_binding_snapshot() {
    let profile = support::host_test_profile();
    let source = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("binding closure source config"),
    )
    .expect("binding closure source");
    let runtime = test_runtime_with_scope_and_subject(
        source.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let binding = install_learning_binding(&runtime, 1);
    runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "binding-closure-conversation",
                "binding-closure-turn",
                "绑定闭包合同",
            ),
        )
        .expect("bound governance job");
    let mut corrupted = source
        .export_replay_snapshot()
        .expect("binding closure snapshot");
    corrupted.json_docs.retain(|doc| {
        !(doc.namespace == "post_turn_governance_binding_snapshots"
            && doc.key == format!("{}:1", binding.binding_id))
    });
    let target = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(profile).expect("binding closure target config"),
    )
    .expect("binding closure target");
    let before = target
        .export_replay_snapshot()
        .expect("binding closure target baseline");
    let error = target
        .import_replay_snapshot(&corrupted)
        .expect_err("missing exact binding snapshot must fail closed");
    assert_eq!(error.stage(), "post_turn_governance_closure");
    assert!(error.to_string().contains("missing"));
    let after = target
        .export_replay_snapshot()
        .expect("target remains unchanged");
    assert_eq!(after.json_docs, before.json_docs);
    assert_eq!(after.events, before.events);
}

#[test]
fn file_reopen_preserves_atomic_memory_and_terminal_receipt() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("file-complete");
    {
        let config = StoreBackendConfig::file(&root, profile).expect("file config");
        assert_persistent_atomic_completion(config);
    }
    std::fs::remove_dir_all(&root).expect("remove file completion store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn independent_sqlite_opens_still_allow_only_one_claim_winner() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("sqlite-claim");
    std::fs::create_dir_all(&root).expect("create sqlite claim root");
    {
        let config = StoreBackendConfig::sqlite(root.join("memory.sqlite3"), profile)
            .expect("sqlite config");
        assert_independent_open_claim(config);
    }
    std::fs::remove_dir_all(&root).expect("remove sqlite claim store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn independent_sqlite_opens_idempotently_install_the_same_first_binding() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("sqlite-binding-install");
    std::fs::create_dir_all(&root).expect("create sqlite binding root");
    {
        let config = StoreBackendConfig::sqlite(root.join("memory.sqlite3"), profile)
            .expect("sqlite config");
        assert_independent_open_binding_install(config);
    }
    std::fs::remove_dir_all(&root).expect("remove sqlite binding Store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn independent_sqlite_opens_reject_divergent_first_binding_identity() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("sqlite-binding-conflict");
    std::fs::create_dir_all(&root).expect("create sqlite binding conflict root");
    {
        let config = StoreBackendConfig::sqlite(root.join("memory.sqlite3"), profile)
            .expect("sqlite config");
        assert_independent_open_conflicting_binding_install(config);
    }
    std::fs::remove_dir_all(&root).expect("remove sqlite binding conflict Store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_preserves_atomic_memory_and_terminal_receipt() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("sqlite-complete");
    std::fs::create_dir_all(&root).expect("create sqlite completion root");
    {
        let config = StoreBackendConfig::sqlite(root.join("memory.sqlite3"), profile)
            .expect("sqlite config");
        assert_persistent_atomic_completion(config);
    }
    std::fs::remove_dir_all(&root).expect("remove sqlite completion store");
}

#[test]
fn retry_requires_current_lease_and_enforces_runtime_backoff() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    install_learning_binding(&runtime, 1);
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "turn-retry", "稍后重试"),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending job")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker-a".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;

    assert!(runtime
        .retry_governance_job(MemoryGovernanceJobRetryRequest {
            job_id: job_id.clone(),
            lease_owner: "stale-worker".to_string(),
            lease_epoch: leased.lease_epoch,
            error_class: PostTurnGovernanceErrorClassV2::ServiceUnavailable,
        })
        .is_err());
    let retry = runtime
        .retry_governance_job(MemoryGovernanceJobRetryRequest {
            job_id: job_id.clone(),
            lease_owner: "worker-a".to_string(),
            lease_epoch: leased.lease_epoch,
            error_class: PostTurnGovernanceErrorClassV2::ServiceUnavailable,
        })
        .expect("schedule retry")
        .job;
    assert_eq!(retry.status, PostTurnGovernanceJobStatusV2::RetryWaiting);
    assert_eq!(retry.next_attempt_at, Some(1_800_000_005));
    assert_eq!(
        retry.last_error_class,
        Some(PostTurnGovernanceErrorClassV2::ServiceUnavailable)
    );
    assert!(runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id,
            lease_owner: "worker-b".to_string(),
            lease_until: 1_800_000_120,
            authority: leased.attempt_authority.clone().expect("attempt authority"),
        })
        .is_err());
}

#[test]
fn exact_job_query_rejects_another_memory_space_and_subject() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_identity_scope_and_subject(
        platform.clone(),
        profile,
        "agent-a",
        "owner-a",
        "subject-a",
        "llm.gateway",
        "chat-a",
    );
    let runtime_b = test_runtime_with_identity_scope_and_subject(
        platform,
        profile,
        "agent-b",
        "owner-b",
        "subject-b",
        "llm.gateway",
        "chat-a",
    );
    let finalized = runtime_a
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-a", "window-a", "turn-a", "主体隔离"),
        )
        .expect("finalize a");
    let error = runtime_b
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: finalized.memory_consolidation.job_id.expect("job id"),
        })
        .expect_err("foreign runtime must fail closed");
    assert!(error.to_string().contains("mounted runtime"));
}

#[test]
fn due_query_does_not_starve_pending_work_behind_future_retries() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    install_learning_binding(&runtime, 1);
    for index in 0..32 {
        let finalized = runtime
            .finalize_turn_with_inline_governance(
                None,
                None,
                finalize_request(
                    "subject-default",
                    &format!("future-conversation-{index}"),
                    &format!("future-turn-{index}"),
                    "稍后重试的任务",
                ),
            )
            .expect("future retry finalize");
        let job_id = finalized.memory_consolidation.job_id.expect("job id");
        let pending = runtime
            .governance_job_status(MemoryGovernanceJobStatusRequest {
                job_id: job_id.clone(),
            })
            .expect("pending")
            .job;
        let leased = runtime
            .claim_governance_job(MemoryGovernanceJobClaimRequest {
                job_id: job_id.clone(),
                lease_owner: format!("worker:future-{index}"),
                lease_until: 1_800_000_060,
                authority: attempt_authority_for_job(&pending),
            })
            .expect("claim future retry")
            .job;
        runtime
            .retry_governance_job(MemoryGovernanceJobRetryRequest {
                job_id,
                lease_owner: format!("worker:future-{index}"),
                lease_epoch: leased.lease_epoch,
                error_class: PostTurnGovernanceErrorClassV2::ServiceUnavailable,
            })
            .expect("schedule future retry");
    }
    let pending = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "due-conversation",
                "due-turn",
                "现在应当执行的任务",
            ),
        )
        .expect("due finalize");
    let pending_job_id = pending.memory_consolidation.job_id.expect("pending job id");

    let active = runtime
        .active_governance_jobs(MemoryGovernanceActiveJobsRequest { limit: 32 })
        .expect("bounded active jobs");
    assert!(active.has_more);
    assert!(
        active.jobs.iter().any(|job| job.job_id == pending_job_id),
        "actionable pending work must be selected ahead of future retries"
    );
}

#[test]
fn claimed_governance_commits_memory_and_terminal_receipt_once() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    install_learning_binding(&runtime, 1);
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-atomic-learning",
                "以后叫我青川",
            ),
        )
        .expect("finalize");
    assert!(matches!(
        runtime
            .read_subject_soul(SubjectSoulReadRequestV1 {
                target_subject_id: "subject-default".to_string(),
                selector: SubjectSoulReadSelectorV1::Current,
                view: SubjectSoulReadViewV1::OperatorSafe,
            })
            .expect("implicit Soul before governed evidence"),
        SubjectSoulReadOutcomeV1::ImplicitUnseeded { .. }
    ));
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending status")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:atomic-learning".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let mut http = StaticHttpClient;
    let llm = PlaneAwareStaticLlm {
        long_term_content: r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "profile",
                "source_authority": "user_asserted",
                "topic": "preferred_name",
                "content": "The user asked to be called Qingchuan.",
                "keywords": ["Qingchuan", "preferred name"]
            }
        ]"#,
    };
    let request = MemoryGovernanceJobRunRequest {
        job_id: job_id.clone(),
        lease_owner: "worker:atomic-learning".to_string(),
        lease_epoch: leased.lease_epoch,
    };
    let completed = runtime
        .run_claimed_governance(&mut http, Some(&llm), request.clone())
        .expect("atomic governance run");
    assert_eq!(
        completed.job.status,
        PostTurnGovernanceJobStatusV2::Succeeded
    );
    assert!(completed.job.receipt.is_some());
    assert_eq!(completed.semantic_governance.accepted_count, 1);
    let soul_after = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: "subject-default".to_string(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .expect("explicit unseeded Soul after first governed evidence");
    assert!(matches!(
        soul_after,
        SubjectSoulReadOutcomeV1::Verified { ref view }
            if view.state == SubjectSoulLifecycleStateV1::Unseeded
                && view.revision.is_none()
    ));
    let queue = runtime
        .deferred_governance_report()
        .expect("terminal queue report");
    assert_eq!(queue.pending, 0);
    assert_eq!(queue.terminal, 1);
    assert_eq!(queue.recent_jobs[0].job_id, job_id);
    let first_snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("first snapshot");
    assert!(serde_json::to_string(&first_snapshot.json_docs)
        .expect("snapshot json")
        .contains("Qingchuan"));

    let duplicate = runtime
        .run_claimed_governance(&mut http, Some(&llm), request)
        .expect("idempotent governance run");
    assert_eq!(duplicate.job.receipt, completed.job.receipt);
    let second_snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("second snapshot");
    assert_eq!(second_snapshot.json_docs, first_snapshot.json_docs);
}

#[test]
fn transcript_revocation_after_prompt_assembly_blocks_first_network_byte_and_memory_commit() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = Arc::new(test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    install_learning_binding(&runtime, 1);
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-egress-revoked",
                "这段内容不得发给模型",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:egress".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let llm = LifecycleRevokingLlm {
        runtime: Arc::clone(&runtime),
        revoked: AtomicBool::new(false),
    };
    let mut http = CountingHttpClient::default();
    let error = runtime
        .run_claimed_governance(
            &mut http,
            Some(&llm),
            MemoryGovernanceJobRunRequest {
                job_id: job_id.clone(),
                lease_owner: "worker:egress".to_string(),
                lease_epoch: leased.lease_epoch,
            },
        )
        .expect_err("revoked disclosure must fail closed");
    assert!(error
        .to_string()
        .contains("network disclosure lease is stale"));
    assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    let status = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("cancelled status")
        .job;
    assert_eq!(status.status, PostTurnGovernanceJobStatusV2::Cancelled);
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert!(snapshot
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "private_garden" && doc.namespace != "long_term"));
}

#[test]
fn stricter_runtime_privacy_after_claim_blocks_first_network_byte() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let original = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let binding = install_learning_binding(&original, 1);
    let finalized = original
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-privacy-tightened",
                "这段内容只允许在原隐私策略下处理",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let authority = original
        .prepare_governance_attempt_authority(MemoryGovernanceAttemptAuthorityRequest {
            job_id: job_id.clone(),
            binding_id: binding.binding_id,
            binding_revision: binding.binding_revision,
            model_id: "qwen3:8b".to_string(),
        })
        .expect("original privacy authority")
        .authority;
    let leased = original
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:privacy-tightened".to_string(),
            lease_until: 1_800_000_060,
            authority,
        })
        .expect("claim")
        .job;

    let mut stricter = MemoryPrivacyPolicy::standard_private_boundary();
    stricter.governance_model_disclosure_allowed = false;
    let tightened = test_runtime_with_scope_subject_and_privacy(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
        stricter,
    );
    let mut http = CountingHttpClient::default();
    let error = tightened
        .run_claimed_governance(
            &mut http,
            Some(&NetworkAttemptingLlm),
            MemoryGovernanceJobRunRequest {
                job_id: job_id.clone(),
                lease_owner: "worker:privacy-tightened".to_string(),
                lease_epoch: leased.lease_epoch,
            },
        )
        .expect_err("tightened privacy must revoke disclosure");
    assert!(error.to_string().contains("privacy authority"));
    assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    let status = tightened
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("job status")
        .job;
    assert_eq!(status.status, PostTurnGovernanceJobStatusV2::Leased);
    assert!(status.receipt.is_none());
}

#[test]
fn memory_precondition_conflict_keeps_job_leased_and_commits_no_semantic_post_image() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    install_learning_binding(&runtime, 1);
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-atomic-conflict",
                "验证原子冲突",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:conflict".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let llm = ConflictInjectingLlm {
        store: platform.clone(),
        calls: AtomicUsize::new(0),
    };
    let mut http = StaticHttpClient;
    runtime
        .run_claimed_governance(
            &mut http,
            Some(&llm),
            MemoryGovernanceJobRunRequest {
                job_id: job_id.clone(),
                lease_owner: "worker:conflict".to_string(),
                lease_epoch: leased.lease_epoch,
            },
        )
        .expect_err("stale private post image must reject the whole completion transaction");
    let current = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("leased status")
        .job;
    assert_eq!(current.status, PostTurnGovernanceJobStatusV2::Leased);
    assert!(current.receipt.is_none());
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    let encoded = serde_json::to_string(&snapshot.json_docs).expect("snapshot json");
    assert!(encoded.contains("concurrent winner"));
    assert!(!encoded.contains("This semantic write must not partially commit."));
}
