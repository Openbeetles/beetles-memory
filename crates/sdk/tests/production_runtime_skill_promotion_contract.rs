#![cfg(feature = "sqlite-store")]

mod support;

use bm_sdk::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

const METHOD: &str = "1. Inspect the archive manifest and validate target paths\n2. Unpack archive into the selected workspace\n3. Verify extracted files against the manifest";

struct Clock(AtomicU64);
impl MemoryClock for Clock {
    fn now_secs(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct NoProvider;
impl GovernanceExecutionPort for NoProvider {
    fn execute(
        &mut self,
        _: &AuthorizedGovernanceEnvelope,
        _: &ImmutableGovernanceExecutionBinding,
        _: &GovernanceEgressAuthority,
        _: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        panic!("deterministic procedural promotion must not invoke a Provider")
    }
}

fn registry() -> AgentToolRegistrySnapshot {
    AgentToolRegistrySnapshot::compact(
        "promotion-tools",
        "host",
        vec![AgentToolDescriptor::compact(
            "archive.unpack",
            "Unpack archive",
            "schema-archive-v1",
        )],
        1_800_000_000,
    )
}

struct TestRuntime {
    memory: Arc<MemoryRuntime>,
    governor: MemoryRuntime,
    capability: MemoryProceduralSubmissionCapability,
    producer_spec: ProceduralProducerSpecV1,
}

impl std::ops::Deref for TestRuntime {
    type Target = MemoryRuntime;
    fn deref(&self) -> &MemoryRuntime {
        &self.memory
    }
}

impl TestRuntime {
    fn new(
        memory: MemoryRuntime,
        store: MemoryStoreHandle,
        tools: AgentToolRegistrySnapshot,
    ) -> Self {
        let mut scoped = memory.scoped_runtime().clone();
        scoped.actor_subject_id = memory
            .subject_registry()
            .system_governor()
            .unwrap()
            .subject_id
            .clone();
        let governor = MemoryRuntime::builder()
            .identity(memory.config().identity.clone())
            .scope(memory.config().scope.clone())
            .subject_registry(memory.subject_registry().clone())
            .subject_relationship_graph(memory.subject_relationship_graph().clone())
            .scoped_runtime(scoped)
            .store(store)
            .clock(memory.config().clock.clone())
            .capability_policy(MemoryCapabilityPolicy::strict_profile())
            .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
            .agent_tool_registry(tools.clone())
            .build()
            .unwrap();
        let registered = governor
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "register-test-executor".into(),
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
                spec: ProceduralProducerSpecV1 {
                    binding_id: "test-executor".into(),
                    scope: ProceduralProducerScopeV1 {
                        memory_space_id: memory.memory_space_id().into(),
                        mounted_subject_id: memory.subject_id().into(),
                        channel_id: memory.config().scope.channel.clone(),
                        chat_id: memory.config().scope.chat_id.clone(),
                    },
                    principal: ProceduralProducerPrincipalV1::LocalCapability {
                        capability_id: "host-executor".into(),
                    },
                    source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
                    claims: ProceduralProducerClaimsV1 {
                        execution_facts: true,
                        method_declarations: true,
                        usage_feedback: true,
                        source_classifications: vec![
                            ProceduralSourceSensitivity::NonPrivate,
                            ProceduralSourceSensitivity::Private,
                            ProceduralSourceSensitivity::Unknown,
                        ],
                    },
                    tools: tools
                        .tools
                        .iter()
                        .map(|tool| ProceduralProducerToolV1 {
                            registry_ref: tools.registry_ref(),
                            tool_id: tool.tool_id.clone(),
                            schema_fingerprint: tool.schema_fingerprint.clone(),
                        })
                        .collect(),
                    source_config_ref: "synthetic-host-executor".into(),
                },
            })
            .unwrap();
        let capability = governor
            .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
            .unwrap();
        Self {
            memory: Arc::new(memory),
            governor,
            capability,
            producer_spec: registered.binding.spec,
        }
    }

    fn submit(&self, input: MemoryTurnFinalizeRequest) -> Result<MemoryTurnFinalizeReport> {
        self.memory
            .finalize_turn_with_procedural_evidence(&self.capability, input)
    }

    fn model_capability(&self) -> MemoryProceduralSubmissionCapability {
        let mut spec = self.producer_spec.clone();
        spec.binding_id = "model-proposals".into();
        spec.principal = ProceduralProducerPrincipalV1::OfficialGovernance {
            service_id: "synthetic-model".into(),
        };
        spec.source_authority = ProceduralProducerSourceAuthorityV1::ModelInferred {
            subject_id: self.subject_id().into(),
        };
        spec.claims.execution_facts = false;
        spec.claims.usage_feedback = false;
        let registered = self
            .governor
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "register-model-proposals".into(),
                spec,
                state: ProceduralProducerStateV1::Active,
                expected_revision: None,
            })
            .unwrap();
        self.governor
            .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
            .unwrap()
    }
}

fn runtime(store: MemoryStoreHandle, clock: Arc<Clock>) -> TestRuntime {
    let memory = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("promotion-agent", "promotion-owner").unwrap())
        .scope(MemoryScope::new("sdk.direct", "promotion-chat").unwrap())
        .store(store.clone())
        .clock(clock)
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .agent_tool_registry(registry())
        .build()
        .unwrap();
    TestRuntime::new(memory, store, registry())
}

fn request(runtime: &MemoryRuntime, turn_id: &str) -> MemoryTurnFinalizeRequest {
    let now = runtime.config().clock.now_secs();
    let facts = ["first-call", "second-call"]
        .into_iter()
        .map(|call| ToolExecutionFactV1 {
            observation_id: format!("{turn_id}-{call}-observation"),
            call_id: format!("{turn_id}-{call}"),
            outcome: ToolExecutionOutcome::Succeeded,
            source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
            started_at: Some(now),
            completed_at: Some(now),
        })
        .collect::<Vec<_>>();
    let canonical_observations = facts
        .iter()
        .map(|observation| ToolObservationDigest {
            observation_id: observation.observation_id.clone(),
            call_id: observation.call_id.clone(),
            tool_name: "archive.unpack".into(),
            summary: "Independent synthetic execution result".into(),
            external_content: false,
        })
        .collect();
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn_id.into(),
            conversation: ConversationScope {
                channel: "sdk.direct".into(),
                chat_id: "promotion-chat".into(),
                conversation_id: Some("promotion-chat".into()),
            },
            subject: runtime.subject_id().into(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: IngressKind::User,
                channel: "sdk.direct".into(),
                provider: None,
                protocol: MemoryTurnProtocol::Native,
                endpoint: None,
                model_alias: None,
                model_resolved: None,
                request_id: None,
                client_conversation_hint: None,
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user("unpack archive")],
            assistant_message: Some(TranscriptInputMessage::assistant("Archive verified")),
            tool_observations: canonical_observations,
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        learning: PostTurnLearningInputV2 {
            tool_call_count: 2,
            agent_tool_feedback: vec![AgentToolUsageFeedbackV3 {
                registry_ref: registry().registry_ref(),
                tool_id: "archive.unpack".into(),
                schema_fingerprint: "schema-archive-v1".into(),
                method_evidence: vec![ToolMethodEvidenceV1 {
                    method_id: format!("method-{turn_id}"),
                    task_signature: "unpack archive".into(),
                    body: METHOD.into(),
                    execution_refs: facts
                        .iter()
                        .map(|fact| fact.observation_id.clone())
                        .collect(),
                    source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
                    external_content: false,
                }],
                execution_facts: facts,
            }],
            ..PostTurnLearningInputV2::empty()
        },
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn project(runtime: &MemoryRuntime, turn_id: &str) -> MemoryProjectionOutput {
    runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Turn {
                turn_id: turn_id.into(),
            },
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "unpack archive".into(),
            system_max_len: 16_384,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: vec![registry().registry_ref()],
        })
        .unwrap()
}

fn skills(runtime: &MemoryRuntime) -> Vec<RuntimeSkillSummary> {
    runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: RuntimeSkillOwningScope::Subject {
                mounted_subject_id: runtime.subject_id().into(),
            },
            query: None,
            include_disabled: true,
            include_retired: true,
            limit: 10,
        })
        .unwrap()
        .skills
}

fn complete_request(
    memory: &TestRuntime,
    input: MemoryTurnFinalizeRequest,
    accepted: u32,
) -> (String, u32) {
    complete_request_using(memory, input, accepted, &memory.capability)
}

fn complete_request_using(
    memory: &TestRuntime,
    input: MemoryTurnFinalizeRequest,
    accepted: u32,
    capability: &MemoryProceduralSubmissionCapability,
) -> (String, u32) {
    let finalized = memory
        .finalize_turn_with_procedural_evidence(capability, input)
        .unwrap();
    let expected_job = finalized.procedural_learning.job_id.unwrap();
    let outcome = MemoryLearningEngine::attach(memory.memory.clone())
        .unwrap()
        .run_due_cycle(
            MemoryLearningCycleRequest {
                lease_owner: "promotion-worker".into(),
                lease_duration_secs: 60,
            },
            &mut NoProvider,
        )
        .unwrap();
    let MemoryLearningCycleOutcome::ProceduralCompleted(report) = outcome else {
        panic!("official procedural completion required: {outcome:?}");
    };
    assert_eq!(report.job.job_id, expected_job);
    assert_eq!(report.receipt.accepted_count, accepted);
    assert_eq!(
        report.receipt.partially_accepted_count,
        u32::from(accepted == 0),
        "these mixed groups retain execution and reject only their method"
    );
    (expected_job, report.receipt.changed_count)
}

fn method_request(
    runtime: &MemoryRuntime,
    turn_id: &str,
    method: &str,
) -> MemoryTurnFinalizeRequest {
    let mut input = request(runtime, turn_id);
    for feedback in &mut input.learning.agent_tool_feedback {
        for declaration in &mut feedback.method_evidence {
            declaration.body = method.into();
        }
    }
    input
}

fn test_directory(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "bm-runtime-promotion-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn two_accepted_production_turns_create_one_runtime_skill_and_project_it_after_reopen() {
    let eligibility = bm_core::skills::govern_runtime_skill_write_shapes(
        &[RuntimeSkillWrite {
            name: String::new(),
            topic: "unpack archive".into(),
            title: "Inspect the archive manifest and validate target paths".into(),
            summary: "Inspect the archive manifest and validate target paths".into(),
            content: METHOD.into(),
            citations: Vec::new(),
            source_chat_id: Some("promotion-chat".into()),
            observed_at: 1_800_000_000,
        }],
        bm_core::skills::RuntimeSkillWriteSource::Extraction,
    );
    assert_eq!(
        eligibility.accepted, 1,
        "actual observation method must pass the unchanged Core structure gate: {eligibility:?}"
    );
    let path = std::env::temp_dir().join(format!(
        "bm-production-runtime-promotion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    let config =
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap();
    let store = support::open_memory_store(config.clone()).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let memory = Arc::new(runtime(store.clone(), clock.clone()));
    assert!(
        skills(&memory).is_empty(),
        "the production Store starts empty"
    );
    for turn_id in ["method-source-one", "method-source-two"] {
        let finalized = memory.submit(request(&memory, turn_id)).unwrap();
        let expected_job = finalized.procedural_learning.job_id.unwrap();
        let engine = MemoryLearningEngine::attach(memory.memory.clone()).unwrap();
        let outcome = engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "promotion-worker".into(),
                    lease_duration_secs: 60,
                },
                &mut NoProvider,
            )
            .unwrap();
        let MemoryLearningCycleOutcome::ProceduralCompleted(report) = outcome else {
            panic!("official procedural worker must complete: {outcome:?}");
        };
        assert_eq!(report.job.job_id, expected_job);
        assert_eq!(report.receipt.accepted_count, 1);
        assert!(
            report.receipt.changed_count > 0,
            "real accepted owner mutation"
        );
        clock.0.fetch_add(2, Ordering::SeqCst);
        let delivered = project(&memory, "next-execution");
        assert!(
            !delivered.provider_payload().agent_tool_hints().is_empty(),
            "each source produces real governed AgentTool experience, not an empty fixture"
        );
        if turn_id == "method-source-one" {
            assert!(
                skills(&memory).is_empty(),
                "one turn is not repeated evidence"
            );
        }
    }
    let promoted = skills(&memory);
    assert_eq!(promoted.len(), 1, "two accepted independent turns must create one Runtime owner without a seed or public create");
    let locator = promoted[0].locator.clone();
    assert_eq!(locator.owner_revision(), 1);
    assert!(promoted[0].enabled);
    let delivered = project(&memory, "use-promoted-method");
    let receipt = delivered.report().selection_receipt().unwrap();
    assert!(receipt
        .runtime_skills
        .iter()
        .any(|selected| selected.locator == locator));
    assert!(delivered
        .provider_payload()
        .system_memory_block()
        .contains("Verify extracted files"));
    complete_request(&memory, request(&memory, "method-source-three"), 1);
    assert_eq!(
        skills(&memory)[0].locator,
        locator,
        "a third source never creates or rewrites revision one"
    );
    clock.0.fetch_add(2, Ordering::SeqCst);
    let use_projection = project(&memory, "promoted-usage");
    let selection = use_projection.report().selection_receipt().unwrap().clone();
    let selected = selection
        .runtime_skills
        .iter()
        .find(|selected| selected.locator == locator)
        .unwrap()
        .clone();
    let mut use_request = request(&memory, "promoted-usage");
    use_request.turn.tool_observations.clear();
    use_request.learning = PostTurnLearningInputV2 {
        selection_receipt: Some(selection),
        runtime_skill_feedback: vec![RuntimeSkillUsageFeedbackV1 {
            locator: selected.locator,
            selected_content_digest: selected.content_digest,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: "execution-promoted-usage".into(),
        }],
        ..PostTurnLearningInputV2::empty()
    };
    assert_eq!(complete_request(&memory, use_request, 1).1, 1);
    let used = skills(&memory).remove(0);
    assert_eq!(used.locator.owner_revision(), locator.owner_revision() + 1);
    assert_eq!(used.validated_success_count, 1);
    drop(memory);
    drop(store);
    let reopened_store = support::open_memory_store(config).unwrap();
    let reopened = runtime(reopened_store.clone(), clock);
    assert_eq!(skills(&reopened)[0].locator, used.locator);
    let reopened_projection = project(&reopened, "use-after-reopen");
    assert!(reopened_projection
        .report()
        .selection_receipt()
        .unwrap()
        .runtime_skills
        .iter()
        .any(|selected| selected.locator == used.locator));
    drop(reopened);
    drop(reopened_store);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_governed_method_source_cannot_register_as_a_usage_witness() {
    use bm_core::memory::*;
    let path = test_directory("governed-usage-authority");
    for backend in ["memory", "file", "sqlite"] {
        let profile = support::host_test_profile();
        let config = match backend {
            "memory" => StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => StoreBackendConfig::file(path.join(backend), profile).unwrap(),
            "sqlite" => StoreBackendConfig::sqlite(path.join("source.db"), profile).unwrap(),
            _ => unreachable!(),
        };
        let mut store = support::open_memory_store(config.clone()).unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let memory = runtime(store.clone(), clock.clone());
        for id in ["witness-positive-a", "witness-positive-b"] {
            complete_request(&memory, request(&memory, id), 1);
            clock.0.fetch_add(2, Ordering::SeqCst);
        }
        assert!(!skills(&memory).is_empty(), "real selectable runtime skill");
        let locator = "synthetic://usage-authority/source";
        let group = canonical_recall_evidence_group(locator);
        memory
            .write(MemoryWriteRequest::GovernedEvidenceDocuments {
                mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                    draft: Box::new(GovernedEvidenceDocumentDraft {
                        memory_space_id: memory.memory_space_id().into(),
                        mounted_subject_id: memory.subject_id().into(),
                        document_id: "usage-method-source".into(),
                        source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
                        source_locator: locator.into(),
                        canonical_evidence_group: group.clone(),
                        evidence_family_group: None,
                        source_revision: 1,
                        content_digest: governed_evidence_document_content_digest(
                            locator,
                            &group,
                            None,
                            METHOD,
                            &[],
                        ),
                        body: METHOD.into(),
                        chunks: vec![],
                        authority: MemoryEvidenceAuthority::WorldObservation,
                        privacy: MemoryPrivacyClass::SharedWithSubject,
                        observed_at: clock.now_secs(),
                    }),
                }],
            })
            .unwrap();
        let mut spec = memory.producer_spec.clone();
        spec.binding_id = "governed-method".into();
        spec.source_authority = ProceduralProducerSourceAuthorityV1::GovernedSource {
            source_revision: GovernedOwnerRevisionRef::try_new(
                GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    "usage-method-source",
                ),
                1,
            )
            .unwrap(),
        };
        spec.claims.execution_facts = false;
        let before = store.export_replay_snapshot().unwrap();
        let denied =
            memory
                .governor
                .control_procedural_producer(MemoryProceduralProducerControlRequest {
                    operation_id: "reject-source-usage".into(),
                    expected_revision: None,
                    state: ProceduralProducerStateV1::Active,
                    spec: spec.clone(),
                });
        assert!(
            denied.is_err(),
            "{backend}: a real document cannot attest skill execution"
        );
        assert_eq!(
            store.export_replay_snapshot().unwrap(),
            before,
            "rejected grant writes nothing"
        );
        spec.claims.usage_feedback = false;
        let registered = memory
            .governor
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "allow-source-method".into(),
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
                spec,
            })
            .unwrap();
        let capability = memory
            .governor
            .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
            .unwrap();
        let mut input = method_request(&memory, "source-method-positive", METHOD);
        input.turn.tool_observations.clear();
        input.learning.tool_call_count = 0;
        input.learning.agent_tool_feedback[0]
            .execution_facts
            .clear();
        input.learning.agent_tool_feedback[0].method_evidence[0]
            .execution_refs
            .clear();
        complete_request_using(&memory, input, 1, &capability);
        // This profile's RuntimeSkill indexed delivery requires SQLite. All
        // backends above exercise real registration and method persistence.
        if backend == "sqlite" {
            let selection = project(&memory, "source-usage-negative")
                .report()
                .selection_receipt()
                .unwrap()
                .clone();
            let selected = selection
                .runtime_skills
                .first()
                .expect("positive selected owner")
                .clone();
            let mut input = request(&memory, "source-usage-negative");
            input.turn.tool_observations.clear();
            input.learning = PostTurnLearningInputV2 {
                selection_receipt: Some(selection),
                runtime_skill_feedback: vec![RuntimeSkillUsageFeedbackV1 {
                    locator: selected.locator,
                    selected_content_digest: selected.content_digest,
                    outcome: ProceduralExecutionOutcomeV1::Succeeded,
                    observation_ref: "not-executed".into(),
                }],
                ..PostTurnLearningInputV2::empty()
            };
            let before_submission = store.export_replay_snapshot().unwrap();
            assert!(memory
                .finalize_turn_with_procedural_evidence(&capability, input)
                .is_err());
            assert!(
                store.export_replay_snapshot().unwrap() == before_submission,
                "denied submission changes nothing"
            );
        }
        let before = store.export_replay_snapshot().unwrap();
        drop(memory);
        if backend != "memory" {
            drop(store);
            store = support::open_memory_store(config).unwrap();
        }
        let after = store.export_replay_snapshot().unwrap();
        assert!(
            after.json_docs == before.json_docs && after.blobs == before.blobs,
            "reopen preserves exact persistent documents and blobs"
        );
        assert!(
            after.events.starts_with(&before.events),
            "reopen retains the original event history"
        );
        assert!(
            after.events[before.events.len()..].iter().all(|event| {
                event.kind == nonproduction_replay_harness::MemoryStoreEventKind::RuntimeLifecycle
                    && event.scope == nonproduction_replay_harness::StoreEventScope::system("open")
                    && event.plane.is_empty()
                    && event.record_key.is_empty()
                    && event.content_hash.is_empty()
            }),
            "reopen may append only Store-open lifecycle events"
        );
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_public_reconciliation_status_rejects_corrupt_subject_root_without_disclosure() {
    const MARKER: &str = "synthetic-private-corrupt-root-48371";
    let path = test_directory("status-corrupt-root");
    let config = StoreBackendConfig::file(&path, support::host_test_profile()).unwrap();
    let store = support::open_memory_store(config.clone()).unwrap();
    let memory = runtime(
        store.clone(),
        Arc::new(Clock(AtomicU64::new(1_800_000_000))),
    );
    let authority = memory.learning_attachment_status_authority().unwrap();
    let request = || MemoryProceduralReconciliationStatusRequest {
        authority: authority.clone(),
    };
    let positive = memory.procedural_reconciliation_status(request()).unwrap();
    assert_eq!(
        positive.read_availability,
        ProceduralLearningReadAvailabilityV1::Ready
    );
    assert!(positive.recovery.is_none());
    let snapshot = store.export_replay_snapshot().unwrap();
    let original = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "procedural_subject_validity_roots")
        .unwrap();
    let mut physical = None;
    for shard in std::fs::read_dir(path.join("kv").join(&original.namespace).join("_v2")).unwrap() {
        for entry in std::fs::read_dir(shard.unwrap().path()).unwrap() {
            let file = entry.unwrap().path();
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
            if value == original.value {
                assert!(physical.replace(file).is_none());
            }
        }
    }
    let physical = physical.expect("exact synthetic root on real File backend");
    let mut corrupt = original.value.clone();
    corrupt["state"] = serde_json::json!({"kind": MARKER});
    let bytes = serde_json::to_vec(&corrupt).unwrap();
    std::fs::write(&physical, &bytes).unwrap();
    let error = memory
        .procedural_reconciliation_status(request())
        .unwrap_err();
    assert!(!error.to_string().contains(MARKER));
    let Error::Other { source, .. } = error else {
        panic!("typed status error required");
    };
    let typed = source.downcast_ref::<ProceduralLearningSdkError>().unwrap();
    assert_eq!(typed.operation, ProceduralLearningSdkOperation::Status);
    assert_eq!(typed.key, ProceduralLearningErrorKeyV1::RepairRequired);
    assert_eq!(
        std::fs::read(&physical).unwrap(),
        bytes,
        "inspection never repairs or rewrites a corrupt root"
    );
    drop(memory);
    drop(store);
    assert!(
        support::open_memory_store(config).is_err(),
        "reopen also fails closed"
    );
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_feedback_owner_capacity_is_not_store_corruption() {
    for evidence_capacity in [true, false] {
        let store = support::open_memory_store(
            StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
        )
        .unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let memory = runtime(store.clone(), clock.clone());
        let limits = memory.runtime_budget().governed_state_budget;
        let per_turn = if evidence_capacity {
            limits.max_agent_tool_experience_evidence_refs_per_owner
                / limits.max_agent_tool_experience_revisions_per_owner
                + 1
        } else {
            0
        };
        let accepted_turns = if evidence_capacity {
            limits.max_agent_tool_experience_evidence_refs_per_owner / per_turn
        } else {
            limits.max_agent_tool_experience_revisions_per_owner
        };
        assert!(accepted_turns > 0);
        for ordinal in 0..=accepted_turns {
            let id = format!("owner-capacity-{ordinal}");
            let mut input = request(&memory, &id);
            let group = &mut input.learning.agent_tool_feedback[0];
            if evidence_capacity {
                let fact = group.execution_facts[0].clone();
                group.method_evidence.clear();
                group.execution_facts = (0..per_turn)
                    .map(|call| ToolExecutionFactV1 {
                        call_id: format!("call-{id}-{call}"),
                        observation_id: format!("observation-{id}-{call}"),
                        ..fact.clone()
                    })
                    .collect();
            } else {
                group.execution_facts.clear();
                group.method_evidence[0].execution_refs.clear();
            }
            input.turn.tool_observations = support::procedural::canonical_observations(
                "archive.unpack",
                &group.execution_facts,
            );
            input.learning.tool_call_count = per_turn as u32;
            let finalized = memory
                .finalize_turn_with_procedural_evidence(&memory.capability, input)
                .unwrap();
            let job_id = finalized.procedural_learning.job_id.unwrap();
            let before = store.export_replay_snapshot().unwrap();
            let outcome = MemoryLearningEngine::attach(memory.memory.clone())
                .unwrap()
                .run_due_cycle(
                    MemoryLearningCycleRequest {
                        lease_owner: "owner-capacity-worker".into(),
                        lease_duration_secs: 60,
                    },
                    &mut NoProvider,
                )
                .unwrap();
            if ordinal < accepted_turns {
                assert!(
                    matches!(outcome, MemoryLearningCycleOutcome::ProceduralCompleted(_)),
                    "positive owner update: {outcome:?}"
                );
            } else {
                let MemoryLearningCycleOutcome::ProceduralRetrying(report) = outcome else {
                    panic!("bounded capacity must be retryable, not corruption: {outcome:?}");
                };
                assert_eq!(report.job.job_id, job_id);
                assert_eq!(
                    report.job.last_error_class,
                    Some(bm_core::memory::ProceduralFeedbackErrorClassV1::BudgetExceeded)
                );
                let after = store.export_replay_snapshot().unwrap();
                let stable = |snapshot: &bm_sdk::nonproduction_replay_harness::StoreSnapshot| {
                    snapshot
                        .json_docs
                        .iter()
                        .filter(|doc| {
                            !(doc.namespace == "procedural_feedback_jobs" && doc.key == job_id
                                || doc.namespace == "procedural_feedback_scope_indexes"
                                    && doc.key == report.job.discovery_root_key)
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                };
                let before_stable = stable(&before);
                let after_stable = stable(&after);
                assert!(
                    before_stable == after_stable,
                    "no partial owner/material/audit write; changed addresses: {:?}",
                    after_stable
                        .iter()
                        .filter(|doc| !before_stable.contains(doc))
                        .map(|doc| (&doc.namespace, &doc.key))
                        .collect::<Vec<_>>()
                );
                assert_eq!(before.blobs, after.blobs);
            }
            clock.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[test]
fn production_promotion_keeps_one_owner_and_its_sources_on_three_backends() {
    let path = test_directory("backends");
    let profile = support::host_test_profile();
    for config in [
        StoreBackendConfig::in_memory(profile).unwrap(),
        StoreBackendConfig::file(path.join("file"), profile).unwrap(),
        StoreBackendConfig::sqlite(path.join("sqlite.db"), profile).unwrap(),
    ] {
        let store = support::open_memory_store(config.clone()).unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let memory = Arc::new(runtime(store.clone(), clock.clone()));
        assert!(skills(&memory).is_empty());
        for turn in ["backend-first", "backend-second"] {
            let (_, changed) = complete_request(&memory, request(&memory, turn), 1);
            assert!(changed > 0);
            clock.0.fetch_add(2, Ordering::SeqCst);
        }
        let before = skills(&memory);
        assert_eq!(
            before.len(),
            1,
            "each backend materializes the same real production owner"
        );
        assert_eq!(before[0].locator.owner_revision(), 1);
        complete_request(&memory, request(&memory, "backend-third"), 1);
        assert_eq!(skills(&memory)[0].locator, before[0].locator);
        if config.backend() != StoreBackendKind::InMemory {
            drop(memory);
            drop(store);
            let reopened = support::open_memory_store(config).unwrap();
            let memory = runtime(reopened.clone(), clock);
            assert_eq!(skills(&memory)[0].locator, before[0].locator);
            assert!(skills(&memory)[0].enabled);
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn pfi2_runtime_usage_requires_its_exact_retained_application_on_reopen() {
    let path = test_directory("usage-proof-reopen");
    let database = path.join("synthetic.db");
    let config = StoreBackendConfig::sqlite(&database, support::host_test_profile()).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let memory = runtime(
        support::open_memory_store(config.clone()).unwrap(),
        clock.clone(),
    );
    for turn in ["usage-proof-first", "usage-proof-second"] {
        complete_request(&memory, request(&memory, turn), 1);
        clock.0.fetch_add(2, Ordering::SeqCst);
    }
    let projection = project(&memory, "usage-proof-observation");
    let receipt = projection.report().selection_receipt().unwrap().clone();
    let selected = receipt
        .runtime_skills
        .first()
        .expect("nonempty genuine promoted skill")
        .clone();
    let mut input = request(&memory, "usage-proof-observation");
    input.turn.tool_observations.clear();
    input.learning = PostTurnLearningInputV2 {
        selection_receipt: Some(receipt),
        runtime_skill_feedback: vec![RuntimeSkillUsageFeedbackV1 {
            locator: selected.locator,
            selected_content_digest: selected.content_digest,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: "synthetic-usage-proof".into(),
        }],
        ..PostTurnLearningInputV2::empty()
    };
    let (usage_job, _) = complete_request(&memory, input, 1);
    assert_eq!(skills(&memory)[0].validated_success_count, 1);
    drop(memory);
    let reopened = runtime(support::open_memory_store(config.clone()).unwrap(), clock);
    assert_eq!(skills(&reopened)[0].validated_success_count, 1);
    drop(reopened);
    let connection = rusqlite::Connection::open(&database).unwrap();
    let (owner_key, original_owner): (String, String) = connection
        .query_row(
            "SELECT key, value_json FROM bm_kv WHERE namespace = 'runtime_skill_records'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let owner: bm_core::skills::RuntimeSkillOwnerRecord =
        serde_json::from_str(&original_owner).unwrap();
    assert_eq!(owner.lifecycle.usage_outcome.succeeded_count, 1);
    let mut lifecycle = owner.lifecycle.clone();
    lifecycle.usage_outcome.succeeded_count = 0;
    lifecycle.usage_outcome.last_outcome = Some(bm_core::skills::RuntimeSkillUsageOutcome::Neutral);
    let forged = bm_core::skills::RuntimeSkillOwnerRecord::build(
        &owner.memory_space_id,
        owner.owning_scope.clone(),
        owner.creation_ref.clone(),
        owner.owner_revision,
        owner.intrinsic_contract.clone(),
        owner.procedural_content.clone(),
        lifecycle,
        owner.privacy_class,
    )
    .unwrap();
    let (manifest_key, original_manifest): (String, String) = connection
        .query_row(
            "SELECT key, value_json FROM bm_kv WHERE namespace = 'runtime_skill_scope_manifests'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let manifest: bm_core::skills::RuntimeSkillScopeManifest =
        serde_json::from_str(&original_manifest).unwrap();
    let forged_manifest = bm_core::skills::RuntimeSkillScopeManifest::build(
        manifest.revision,
        &manifest.memory_space_id,
        manifest.owning_scope,
        [bm_core::skills::RuntimeSkillOwnerBinding::from_record(&forged).unwrap()],
        4,
    )
    .unwrap();
    connection.execute("UPDATE bm_kv SET value_json = ?1 WHERE namespace = 'runtime_skill_records' AND key = ?2",
        rusqlite::params![serde_json::to_string(&forged).unwrap(), owner_key]).unwrap();
    connection.execute("UPDATE bm_kv SET value_json = ?1 WHERE namespace = 'runtime_skill_scope_manifests' AND key = ?2",
        rusqlite::params![serde_json::to_string(&forged_manifest).unwrap(), manifest_key]).unwrap();
    drop(connection);
    assert!(support::open_memory_store(config.clone()).is_err(),
        "resealing a current owner and manifest must not change the outcome of its immutable usage evidence");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute("UPDATE bm_kv SET value_json = ?1 WHERE namespace = 'runtime_skill_records' AND key = ?2",
        rusqlite::params![original_owner, owner_key]).unwrap();
    connection.execute("UPDATE bm_kv SET value_json = ?1 WHERE namespace = 'runtime_skill_scope_manifests' AND key = ?2",
        rusqlite::params![original_manifest, manifest_key]).unwrap();
    assert_eq!(
        connection
            .execute(
                "DELETE FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                rusqlite::params!["procedural_feedback_application_ledgers", usage_job]
            )
            .unwrap(),
        1,
        "remove only the synthetic usage proof, not its promotion sources or owner"
    );
    let before: u64 = connection
        .query_row("SELECT COUNT(*) FROM bm_kv", [], |row| row.get(0))
        .unwrap();
    drop(connection);
    let result = support::open_memory_store(config);
    assert!(result.is_err(), "a retained positive usage summary without its exact application must fail closed on Store-open");
    let connection = rusqlite::Connection::open(&database).unwrap();
    let after: u64 = connection
        .query_row("SELECT COUNT(*) FROM bm_kv", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        after, before,
        "open must not repair corrupt evidence by fabricating a zero summary"
    );
    drop(connection);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn weak_different_method_model_and_private_sources_do_not_create_runtime_skills() {
    let path = test_directory("negative");
    for case in ["weak", "different-method", "model", "private", "external"] {
        let config = StoreBackendConfig::sqlite(
            path.join(format!("{case}.db")),
            support::host_test_profile(),
        )
        .unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let memory = Arc::new(runtime(
            support::open_memory_store(config).unwrap(),
            clock.clone(),
        ));
        // An unrelated valid source is a real nonempty experience control, but
        // cannot satisfy the other case's exact task/authority/method evidence.
        complete_request(&memory, request(&memory, "positive-control"), 1);
        assert!(!project(&memory, "before-negative")
            .provider_payload()
            .agent_tool_hints()
            .is_empty());
        assert!(skills(&memory).is_empty());
        for turn in ["negative-one", "negative-two"] {
            clock.0.fetch_add(2, Ordering::SeqCst);
            let method = if case == "weak" {
                "archive unpack completed successfully"
            } else if case == "different-method" && turn == "negative-one" {
                "1. Read the zip manifest\n2. Extract zip entries into a sandbox\n3. Check each extracted checksum"
            } else {
                METHOD
            };
            let mut input = method_request(&memory, turn, method);
            for feedback in &mut input.learning.agent_tool_feedback {
                for declaration in &mut feedback.method_evidence {
                    declaration.task_signature = "negative-specific-task".into();
                    declaration.source_sensitivity = if case == "private" {
                        ProceduralSourceSensitivity::Private
                    } else {
                        ProceduralSourceSensitivity::NonPrivate
                    };
                    declaration.external_content = case == "external";
                    if case == "model" {
                        declaration.execution_refs.clear();
                    }
                }
                if case == "model" {
                    feedback.execution_facts.clear();
                }
            }
            for observation in &mut input.turn.tool_observations {
                observation.external_content = case == "external";
            }
            input.turn.external_content_used = case == "external";
            if case == "model" {
                complete_request_using(&memory, input, 1, &memory.model_capability());
            } else {
                let partial = matches!(case, "weak" | "private" | "external")
                    || (case == "different-method" && turn == "negative-two");
                complete_request(&memory, input, u32::from(!partial));
            }
        }
        assert!(
            skills(&memory).is_empty(),
            "{case} cannot acquire promotion authority"
        );
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn pfi2_producer_revocation_fences_current_and_historical_delivery_immediately() {
    let path = test_directory("producer-revocation");
    let store = support::open_memory_store(
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let memory = runtime(store, clock.clone());
    for turn in ["first-authorized-source", "second-authorized-source"] {
        complete_request(&memory, request(&memory, turn), 1);
        clock.0.fetch_add(2, Ordering::SeqCst);
    }
    assert!(
        !skills(&memory).is_empty(),
        "the positive fixture must persist a real promoted RuntimeSkill"
    );
    let positive = project(&memory, "before-producer-revoke");
    let receipt = positive.report().selection_receipt().unwrap();
    assert!(!receipt.agent_tool_experiences.is_empty());
    assert!(!receipt.runtime_skills.is_empty());
    assert!(positive
        .provider_payload()
        .system_memory_block()
        .contains("Verify extracted files"));
    let as_of_time = clock.now_secs();
    let historical = memory
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Preview,
            temporal_operation: MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time },
            user_query: "unpack archive".into(),
            system_max_len: 16_384,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![registry().registry_ref()],
        })
        .unwrap();
    assert!(
        !historical.provider_payload().agent_tool_hints().is_empty(),
        "historical disclosure needs a nonempty positive control"
    );
    // Replaying registration retrieves the real exact revision; it must not
    // overwrite a later revocation. No caller-constructed revision reference.
    let registered = memory
        .governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "register-test-executor".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: memory.producer_spec.clone(),
        })
        .unwrap();
    memory
        .governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "revoke-live-producer".into(),
            expected_revision: Some(registered.binding.revision_ref().unwrap()),
            state: ProceduralProducerStateV1::Revoked,
            spec: memory.producer_spec.clone(),
        })
        .unwrap();
    for temporal_operation in [
        MemoryRecallTemporalOperation::Current,
        MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time },
    ] {
        let denied = memory
            .project(MemoryProjectionRequest {
                binding: match temporal_operation {
                    MemoryRecallTemporalOperation::Current => ProceduralProjectionBindingV1::Turn {
                        turn_id: "after-producer-revoke".into(),
                    },
                    MemoryRecallTemporalOperation::HistoricalAsOf { .. } => {
                        ProceduralProjectionBindingV1::Preview
                    }
                },
                temporal_operation,
                user_query: "unpack archive".into(),
                system_max_len: 16_384,
                recent_messages_limit: 4,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: vec![],
                tool_registry_refs: vec![registry().registry_ref()],
            })
            .unwrap();
        assert!(denied.report().selection_receipt().is_none_or(|receipt|
            receipt.agent_tool_experiences.is_empty() && receipt.runtime_skills.is_empty()),
            "revocation must immediately fence all procedural candidates, including retained history");
        assert!(denied.provider_payload().agent_tool_hints().is_empty());
        assert!(!denied
            .provider_payload()
            .system_memory_block()
            .contains("Verify extracted files"));
    }
    drop(memory);
    std::fs::remove_dir_all(path).unwrap();
}

fn revoked_inspection_fixture(
    label: &str,
) -> (TestRuntime, std::path::PathBuf, RuntimeSkillOwnerLocator) {
    let path = test_directory(label);
    let store = support::open_memory_store(
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let memory = runtime(store, clock.clone());
    for turn in ["inspection-source-a", "inspection-source-b"] {
        complete_request(&memory, request(&memory, turn), 1);
        clock.0.fetch_add(2, Ordering::SeqCst);
    }
    let locator = skills(&memory)
        .into_iter()
        .next()
        .expect("nonempty public list positive")
        .locator;
    assert!(memory
        .get_runtime_skill(RuntimeSkillDetailRequest {
            locator: locator.clone()
        })
        .unwrap()
        .procedure_text
        .contains("Verify extracted files"));
    let registered = memory
        .governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "register-test-executor".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: memory.producer_spec.clone(),
        })
        .unwrap();
    memory
        .governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "revoke-inspection-producer".into(),
            expected_revision: Some(registered.binding.revision_ref().unwrap()),
            state: ProceduralProducerStateV1::Revoked,
            spec: memory.producer_spec.clone(),
        })
        .unwrap();
    (memory, path, locator)
}

#[test]
fn pfi2_reconciling_public_skill_inspection_never_returns_old_body_or_statistics() {
    let (memory, path, locator) = revoked_inspection_fixture("revoked-public-detail");
    assert!(
        memory
            .get_runtime_skill(RuntimeSkillDetailRequest { locator })
            .is_err(),
        "public detail must not bypass the current authority fence"
    );
    assert!(
        skills(&memory).is_empty(),
        "public list must not expose an old usage summary"
    );
    drop(memory);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn pfi2_reconciling_public_reports_distinguish_unavailable_from_true_zero() {
    let (memory, path, _) = revoked_inspection_fixture("revoked-public-reports");
    let registry_report =
        serde_json::to_value(memory.agent_tool_registry_report().unwrap()).unwrap();
    assert_eq!(
        registry_report["governed_experiences"],
        serde_json::Value::Null,
        "unread experience count is unavailable, not a factual zero"
    );
    assert_eq!(registry_report["read_availability"], "reconciling");
    let recall = memory
        .recall(MemoryRecallRequest {
            temporal_operation: MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![],
            query: "unpack archive".into(),
            limit: 8,
            tool_registry_refs: vec![registry().registry_ref()],
        })
        .unwrap();
    let status = serde_json::to_value(recall.tool_experience_status).unwrap();
    assert_eq!(
        status["governed_experience_candidates"],
        serde_json::Value::Null
    );
    assert_eq!(status["read_availability"], "reconciling");
    assert_ne!(status["reason"], AGENT_TOOL_NO_EXPERIENCE_REASON);
    drop(memory);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn pfi2_public_skill_detail_exposes_content_not_internal_source_commitments() {
    let path = test_directory("public-detail-commitments");
    let store = support::open_memory_store(
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let memory = runtime(store, clock.clone());
    for turn in ["detail-proof-a", "detail-proof-b"] {
        complete_request(&memory, request(&memory, turn), 1);
        clock.0.fetch_add(2, Ordering::SeqCst);
    }
    let detail = memory
        .get_runtime_skill(RuntimeSkillDetailRequest {
            locator: skills(&memory).remove(0).locator,
        })
        .unwrap();
    assert!(detail.procedure_text.contains("Verify extracted files"));
    let content: serde_json::Value = serde_json::from_str(&detail.raw_content).unwrap();
    assert!(
        content.get("lifecycle").is_none(),
        "public content must not serialize the Store's retained usage directory"
    );
    assert!(
        content.get("intrinsic_contract").is_none(),
        "source commitments are not public method content"
    );
    assert_eq!(content["procedure"], detail.procedure_text);
    assert!(
        detail.citations.is_empty(),
        "internal feedback source job commitments are not public citations"
    );
    drop(memory);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn pfi2_revoked_queued_feedback_is_cancelled_without_transient_retry() {
    let config = StoreBackendConfig::in_memory(support::host_test_profile()).unwrap();
    let memory = runtime(
        support::open_memory_store(config).unwrap(),
        Arc::new(Clock(AtomicU64::new(1_800_000_000))),
    );
    complete_request(&memory, request(&memory, "worker-positive"), 1);
    let finalized = memory
        .finalize_turn_with_procedural_evidence(
            &memory.capability,
            request(&memory, "queued-before-revoke"),
        )
        .unwrap();
    let job_id = finalized.procedural_learning.job_id.unwrap();
    let registered = memory
        .governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "register-test-executor".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: memory.producer_spec.clone(),
        })
        .unwrap();
    memory
        .governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "revoke-queued-producer".into(),
            expected_revision: Some(registered.binding.revision_ref().unwrap()),
            state: ProceduralProducerStateV1::Revoked,
            spec: memory.producer_spec.clone(),
        })
        .unwrap();
    let engine = MemoryLearningEngine::attach(memory.memory.clone()).unwrap();
    let mut cancelled = None;
    for _ in 0..16 {
        match engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "revocation-worker".into(),
                    lease_duration_secs: 60,
                },
                &mut NoProvider,
            )
            .unwrap()
        {
            MemoryLearningCycleOutcome::ProceduralFailed(report) => {
                cancelled = Some(report);
                break;
            }
            MemoryLearningCycleOutcome::ProceduralReconciliation(page) => {
                assert_eq!(page.job.attempt_count, 1);
                assert!(!page.replayed);
                assert!(page.job.checkpoint.is_some());
            }
            MemoryLearningCycleOutcome::Blocked(report) => {
                assert_eq!(
                    report.job.status,
                    bm_sdk::PostTurnGovernanceJobStatusV2::BlockedConfiguration
                );
                assert_eq!(report.reason, "governance_execution_binding_unavailable");
                assert_eq!(
                    report
                        .job
                        .execution_block_authority
                        .as_ref()
                        .unwrap()
                        .typed_block_reason,
                    bm_sdk::PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable
                );
                assert_eq!(report.job.attempt_count, 0);
            }
            outcome => {
                panic!("revoked evidence must terminate, not enter transient retry: {outcome:?}")
            }
        }
    }
    let report = cancelled.expect("bounded reconciliation cannot starve queued cancellation");
    assert_eq!(report.job.job_id, job_id);
    assert_eq!(
        report.job.status,
        bm_core::memory::ProceduralFeedbackJobStatusV1::Cancelled
    );
    assert!(report.job.next_attempt_at.is_none());
    assert!(report.job.lease_owner.is_none());
    assert!(report.job.receipt.is_none());
}

#[test]
fn pfi2_edited_skill_preserves_governed_management_and_projection_after_reconciliation() {
    verify_controlled_skill_revision("edit");
}

#[test]
fn pfi2_retired_skill_remains_inspectable_but_never_projects_after_reconciliation() {
    verify_controlled_skill_revision("retire");
}

fn verify_controlled_skill_revision(control: &str) {
    let path = test_directory(&format!("skill-{control}-read-proof"));
    let config =
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap();
    let store = support::open_memory_store(config.clone()).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let memory = runtime(store.clone(), clock.clone());
    for turn in ["lifecycle-source-a", "lifecycle-source-b"] {
        complete_request(&memory, request(&memory, turn), 1);
        clock.0.fetch_add(2, Ordering::SeqCst);
    }
    let initial = skills(&memory).remove(0);
    assert!(
        project(&memory, "lifecycle-before-control")
            .report()
            .procedural_delivery_reports()
            .iter()
            .any(|item| item.rendered),
        "terminal selection negative requires a real rendered skill before control"
    );
    let initial_detail = memory
        .get_runtime_skill(RuntimeSkillDetailRequest {
            locator: initial.locator.clone(),
        })
        .unwrap();
    let edited_method = "1. Inspect the archive manifest\n2. Validate target paths before unpacking\n3. Verify extracted files and record the validation result";
    let changed = if control == "edit" {
        memory
            .edit_runtime_skill(RuntimeSkillEditRequest {
                locator: initial.locator.clone(),
                title: initial.title.clone(),
                topic: "unpack archive".into(),
                summary: initial_detail.summary_text,
                procedure: edited_method.into(),
                edit_reason: "preserve validation outcome".into(),
                observed_at: clock.now_secs(),
            })
            .unwrap()
    } else {
        memory
            .retire_runtime_skill(RuntimeSkillRetireRequest {
                locator: initial.locator.clone(),
                observed_at: clock.now_secs(),
            })
            .unwrap()
    };
    assert!(changed.changed);
    assert!(memory
        .get_runtime_skill(RuntimeSkillDetailRequest {
            locator: changed.current_locator.clone()
        })
        .is_err());
    let engine = MemoryLearningEngine::attach(memory.memory.clone()).unwrap();
    let mut finished = false;
    for _ in 0..16 {
        match engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "lifecycle-proof-worker".into(),
                    lease_duration_secs: 60,
                },
                &mut NoProvider,
            )
            .unwrap()
        {
            MemoryLearningCycleOutcome::ProceduralReconciliation(page) => {
                if page.completed {
                    finished = true;
                    break;
                }
            }
            MemoryLearningCycleOutcome::Blocked(report) => {
                assert_eq!(report.reason, "governance_execution_binding_unavailable")
            }
            other => panic!("unexpected lifecycle worker result: {other:?}"),
        }
    }
    assert!(finished);
    let verify = |memory: &MemoryRuntime| {
        let detail = memory
            .get_runtime_skill(RuntimeSkillDetailRequest {
                locator: changed.current_locator.clone(),
            })
            .expect("legal control keeps current-source-authorized management readable");
        assert_eq!(detail.summary.owner_id, initial.owner_id);
        // No tool-experience references: retirement of a RuntimeSkill does not
        // retire the independently valid source Method with the same text.
        let projection = memory
            .project(MemoryProjectionRequest {
                binding: ProceduralProjectionBindingV1::Turn {
                    turn_id: "lifecycle-read-proof".into(),
                },
                temporal_operation: MemoryRecallTemporalOperation::Current,
                user_query: "unpack archive".into(),
                system_max_len: 16_384,
                recent_messages_limit: 0,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: vec![],
                tool_registry_refs: vec![],
            })
            .unwrap();
        if control == "edit" {
            assert_eq!(detail.procedure_text, edited_method);
            assert!(projection
                .provider_payload()
                .system_memory_block()
                .contains("record the validation result"));
        } else {
            assert_eq!(detail.summary.status, "retired");
            assert!(projection
                .report()
                .selection_receipt()
                .is_none_or(|receipt| receipt.runtime_skills.is_empty()));
            assert!(projection.report().procedural_delivery_reports().iter().all(|item| !item.selected && !item.rendered),
                "no retired RuntimeSkill body may be delivered; independent source Method text is not this owner");
        }
    };
    verify(&memory);
    drop(engine);
    drop(memory);
    drop(store);
    let reopened = runtime(support::open_memory_store(config).unwrap(), clock);
    verify(&reopened);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn pfi2_skill_availability_changes_reconcile_exact_revisions_without_losing_the_owner() {
    let path = test_directory("skill-control-reconciliation");
    for backend in ["memory", "file", "sqlite"] {
        let profile = support::host_test_profile();
        let config = match backend {
            "memory" => StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => StoreBackendConfig::file(path.join("file"), profile).unwrap(),
            "sqlite" => StoreBackendConfig::sqlite(path.join("sqlite.db"), profile).unwrap(),
            _ => unreachable!(),
        };
        let mut store = support::open_memory_store(config.clone()).unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let memory = runtime(store.clone(), clock.clone());
        for turn in ["control-source-a", "control-source-b"] {
            complete_request(&memory, request(&memory, turn), 1);
            clock.0.fetch_add(2, Ordering::SeqCst);
        }
        let original = skills(&memory).remove(0);
        let mut locator = original.locator.clone();
        for enabled in [false, true] {
            let changed = memory
                .set_runtime_skill_enabled(RuntimeSkillSetEnabledRequest {
                    locator,
                    enabled,
                    observed_at: clock.now_secs(),
                })
                .unwrap();
            assert!(changed.changed);
            let pending = memory
                .list_runtime_skills(RuntimeSkillListRequest {
                    owning_scope: RuntimeSkillOwningScope::Subject {
                        mounted_subject_id: memory.subject_id().into(),
                    },
                    query: None,
                    include_disabled: true,
                    include_retired: true,
                    limit: 8,
                })
                .unwrap();
            assert_eq!(
                pending.read_availability,
                ProceduralLearningReadAvailabilityV1::Reconciling,
                "{backend}: ordinary control must atomically fence the new exact revision"
            );
            assert_eq!(pending.total, None);
            assert!(pending.skills.is_empty());
            let engine = MemoryLearningEngine::attach(memory.memory.clone()).unwrap();
            let mut completed = false;
            for _ in 0..16 {
                let outcome = engine
                    .run_due_cycle(
                        MemoryLearningCycleRequest {
                            lease_owner: "skill-control-worker".into(),
                            lease_duration_secs: 60,
                        },
                        &mut NoProvider,
                    )
                    .unwrap();
                let report = match outcome {
                    MemoryLearningCycleOutcome::ProceduralReconciliation(report) => report,
                    MemoryLearningCycleOutcome::Blocked(report) => {
                        assert_eq!(report.reason, "governance_execution_binding_unavailable");
                        continue;
                    }
                    other => panic!("control must use original durable worker: {other:?}"),
                };
                if report.completed {
                    completed = true;
                    break;
                }
            }
            assert!(completed);
            let current = skills(&memory).remove(0);
            assert_eq!(current.owner_id, original.owner_id);
            assert_eq!(current.enabled, enabled);
            assert_eq!(current.locator, changed.current_locator);
            locator = current.locator;
            clock.0.fetch_add(2, Ordering::SeqCst);
        }
        drop(memory);
        if backend != "memory" {
            drop(store);
            store = support::open_memory_store(config).unwrap();
        }
        let reopened = runtime(store, clock);
        assert_eq!(skills(&reopened).remove(0).locator, locator);
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn pfi2_reopen_rejects_resealed_unpublished_checkpoint() {
    for attack in ["reseal", "missing-page-receipt"] {
        checkpoint_proof_attack(attack);
    }
}

fn checkpoint_proof_attack(attack: &str) {
    use bm_core::memory::{
        ProceduralReconciliationCursorV1, ProceduralReconciliationReadDispositionV1,
    };
    let (memory, path, _) = revoked_inspection_fixture(attack);
    let engine = MemoryLearningEngine::attach(memory.memory.clone()).unwrap();
    let mut pending = None;
    for _ in 0..16 {
        let outcome = engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "checkpoint-proof-worker".into(),
                    lease_duration_secs: 60,
                },
                &mut NoProvider,
            )
            .unwrap();
        let report = match outcome {
            MemoryLearningCycleOutcome::ProceduralReconciliation(report) => report,
            MemoryLearningCycleOutcome::Blocked(report) => {
                assert_eq!(report.reason, "governance_execution_binding_unavailable");
                continue;
            }
            other => panic!("expected real reconciliation page: {other:?}"),
        };
        assert!(
            !report.completed,
            "stop before the actual publication transaction"
        );
        if matches!(
            report.job.checkpoint.as_ref().unwrap().cursor,
            ProceduralReconciliationCursorV1::Complete
        ) {
            pending = Some(report.job);
            break;
        }
    }
    let mut job = pending.expect("real complete checkpoint waiting for publication");
    assert_eq!(
        job.status,
        bm_core::memory::ProceduralFeedbackJobStatusV1::ReadyToContinue
    );
    let checkpoint = job.checkpoint.as_mut().unwrap();
    let proof = checkpoint
        .verified_owners
        .iter_mut()
        .find(|proof| {
            matches!(
                proof.disposition,
                ProceduralReconciliationReadDispositionV1::Withdrawn
            )
        })
        .expect("real denied source positive");
    proof.disposition = ProceduralReconciliationReadDispositionV1::Visible;
    checkpoint.dependencies.clear();
    checkpoint.content_digest = checkpoint.canonical_digest().unwrap();
    job.validate()
        .expect("attack must pass standalone canonical validation");
    let connection = rusqlite::Connection::open(path.join("store.db")).unwrap();
    if attack == "reseal" {
        assert_eq!(connection.execute("UPDATE bm_kv SET value_json = ?1 WHERE namespace = 'procedural_feedback_jobs' AND key = ?2",
            rusqlite::params![serde_json::to_string(&job).unwrap(), &job.job_id]).unwrap(), 1);
    } else {
        let key = job
            .checkpoint_authority
            .as_ref()
            .unwrap()
            .operation
            .storage_key();
        assert_eq!(connection.execute("DELETE FROM bm_kv WHERE namespace IN ('memory_mutation_receipts', 'memory_mutation_audits') AND key = ?1", [&key]).unwrap(), 2);
    }
    let image = || {
        let mut statement = connection
            .prepare("SELECT namespace, key, value_json FROM bm_kv ORDER BY namespace, key")
            .unwrap();
        let docs = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let events: u64 = connection
            .query_row("SELECT COUNT(*) FROM bm_event_log", [], |row| row.get(0))
            .unwrap();
        (docs, events)
    };
    let corrupt_image = image();
    let mut rejected = false;
    for _ in 0..2 {
        let live = engine.run_due_cycle(
            MemoryLearningCycleRequest {
                lease_owner: "cannot-launder-checkpoint".into(),
                lease_duration_secs: 60,
            },
            &mut NoProvider,
        );
        match live {
            Err(_) => {
                rejected = true;
                break;
            }
            Ok(MemoryLearningCycleOutcome::Blocked(report)) => {
                assert_eq!(report.reason, "governance_execution_binding_unavailable")
            }
            other => panic!("{attack}: bad prior page must not be consumed: {other:?}"),
        }
    }
    assert!(
        rejected,
        "the official procedural lane must actually reject the bad page"
    );
    assert_eq!(
        image(),
        corrupt_image,
        "failed live consumption must not mint a new page/job/receipt/event"
    );
    drop(connection);
    drop(engine);
    drop(memory);
    let before = std::fs::read(path.join("store.db")).unwrap();
    let reopened = support::open_memory_store(
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap(),
    );
    assert!(reopened.is_err(), "unpublished checkpoint must be bound to its prior atomic page receipt, not only its own digest");
    assert_eq!(
        std::fs::read(path.join("store.db")).unwrap(),
        before,
        "failed open cannot repair or initialize corrupt proof"
    );
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn pfi2_reconciliation_pages_reopen_without_reauthorizing_withdrawn_history() {
    let path = test_directory("reconciliation-pages");
    let profile = support::host_test_profile();
    for backend in ["memory", "file", "sqlite"] {
        let config = match backend {
            "memory" => StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => StoreBackendConfig::file(path.join("file"), profile).unwrap(),
            "sqlite" => StoreBackendConfig::sqlite(path.join("sqlite.db"), profile).unwrap(),
            _ => unreachable!(),
        };
        let mut store = support::open_memory_store(config.clone()).unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let fixture = runtime(store.clone(), clock.clone());
        for turn in ["reconcile-source-a", "reconcile-source-b"] {
            complete_request(&fixture, request(&fixture, turn), 1);
            clock.0.fetch_add(2, Ordering::SeqCst);
        }
        let positive = project(&fixture, "reconcile-positive");
        assert!(
            !skills(&fixture).is_empty(),
            "{backend}: real persisted promotion is required"
        );
        assert!(
            !positive.provider_payload().agent_tool_hints().is_empty(),
            "{backend}: real tool delivery is required"
        );
        // The existing desktop profile dispatches RuntimeSkill recall through
        // SQLite only. Every backend still commits and reopens its real owners;
        // SQLite additionally proves the public Skill delivery boundary.
        if backend == "sqlite" {
            assert!(!positive
                .report()
                .selection_receipt()
                .unwrap()
                .runtime_skills
                .is_empty());
            assert!(positive
                .provider_payload()
                .system_memory_block()
                .contains("Verify extracted files"));
        }
        let as_of_time = clock.now_secs();
        let registered = fixture
            .governor
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "register-test-executor".into(),
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
                spec: fixture.producer_spec.clone(),
            })
            .unwrap();
        fixture
            .governor
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "reconcile-revoke".into(),
                expected_revision: Some(registered.binding.revision_ref().unwrap()),
                state: ProceduralProducerStateV1::Revoked,
                spec: fixture.producer_spec.clone(),
            })
            .unwrap();
        let mut memory = fixture.memory.clone();
        drop(fixture);
        let mut completed = false;
        let mut pages = Vec::new();
        for _ in 0..16 {
            let engine = MemoryLearningEngine::attach(memory.clone()).unwrap();
            let outcome = engine
                .run_due_cycle(
                    MemoryLearningCycleRequest {
                        lease_owner: "page-worker".into(),
                        lease_duration_secs: 60,
                    },
                    &mut NoProvider,
                )
                .unwrap();
            let MemoryLearningCycleOutcome::ProceduralReconciliation(report) = outcome else {
                panic!("{backend}: authorized reconciliation must commit a durable page, got {outcome:?}");
            };
            assert_eq!(
                report.job.attempt_count, 1,
                "successful continuation is not failure retry"
            );
            assert!(!report.replayed);
            pages.push(report.job.checkpoint.as_ref().unwrap().page_number);
            completed = report.completed;
            drop(engine);
            if backend != "memory" {
                drop(memory);
                drop(store);
                store = support::open_memory_store(config.clone()).unwrap();
                memory = Arc::new(
                    MemoryRuntime::builder()
                        .identity(
                            MemoryIdentity::new("promotion-agent", "promotion-owner").unwrap(),
                        )
                        .scope(MemoryScope::new("sdk.direct", "promotion-chat").unwrap())
                        .store(store.clone())
                        .clock(clock.clone())
                        .capability_policy(MemoryCapabilityPolicy::strict_profile())
                        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
                        .agent_tool_registry(registry())
                        .build()
                        .unwrap(),
                );
            }
            if completed {
                break;
            }
        }
        assert!(
            completed,
            "{backend}: bounded real inventory must reach the complete checkpoint"
        );
        assert!(
            pages.len() > 1,
            "fixture must actually cross a durable page boundary"
        );
        for temporal_operation in [
            MemoryRecallTemporalOperation::Current,
            MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time },
        ] {
            let denied = memory
                .project(MemoryProjectionRequest {
                    binding: ProceduralProjectionBindingV1::Preview,
                    temporal_operation,
                    user_query: "unpack archive".into(),
                    system_max_len: 16_384,
                    recent_messages_limit: 0,
                    pressure: PressureLevel::Normal,
                    mode_input: RuntimeLifecycleModeInput::default(),
                    structured_query_facets: vec![],
                    tool_registry_refs: vec![registry().registry_ref()],
                })
                .unwrap();
            assert!(
                denied.provider_payload().agent_tool_hints().is_empty(),
                "{backend}: Ready is not historical permission"
            );
            assert!(!denied
                .provider_payload()
                .system_memory_block()
                .contains("Verify extracted files"));
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_new_feedback_never_reuses_fully_withdrawn_method_sources() {
    use bm_core::skills::{
        AgentToolExperienceBodyV1, AgentToolExperienceRevisionMaterialV3, AgentToolExperienceStatus,
    };
    let path = test_directory("new-feedback-after-withdrawal");
    for backend in ["memory", "file", "sqlite"] {
        for previously_promoted in [true, false] {
            let profile = support::host_test_profile();
            let label = format!("{backend}-{previously_promoted}");
            let config = match backend {
                "memory" => StoreBackendConfig::in_memory(profile).unwrap(),
                "file" => StoreBackendConfig::file(path.join(&label), profile).unwrap(),
                "sqlite" => {
                    StoreBackendConfig::sqlite(path.join(format!("{label}.db")), profile).unwrap()
                }
                _ => unreachable!(),
            };
            let mut store = support::open_memory_store(config.clone()).unwrap();
            let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
            let memory = runtime(store.clone(), clock.clone());
            complete_request(&memory, request(&memory, "withdrawn-a-first"), 1);
            clock.0.fetch_add(2, Ordering::SeqCst);
            if previously_promoted {
                complete_request(&memory, request(&memory, "withdrawn-a-second"), 1);
                clock.0.fetch_add(2, Ordering::SeqCst);
            }
            assert_eq!(!skills(&memory).is_empty(), previously_promoted);
            assert!(!project(&memory, "before-withdrawal")
                .provider_payload()
                .agent_tool_hints()
                .is_empty());
            let as_of_time = clock.now_secs();
            let original = memory
                .governor
                .control_procedural_producer(MemoryProceduralProducerControlRequest {
                    operation_id: "register-test-executor".into(),
                    expected_revision: None,
                    state: ProceduralProducerStateV1::Active,
                    spec: memory.producer_spec.clone(),
                })
                .unwrap();
            memory
                .governor
                .control_procedural_producer(MemoryProceduralProducerControlRequest {
                    operation_id: "withdraw-all-method-sources".into(),
                    expected_revision: Some(original.binding.revision_ref().unwrap()),
                    state: ProceduralProducerStateV1::Revoked,
                    spec: memory.producer_spec.clone(),
                })
                .unwrap();
            let engine = MemoryLearningEngine::attach(memory.memory.clone()).unwrap();
            let mut completed = false;
            for _ in 0..16 {
                match engine
                    .run_due_cycle(
                        MemoryLearningCycleRequest {
                            lease_owner: "withdraw-all-worker".into(),
                            lease_duration_secs: 60,
                        },
                        &mut NoProvider,
                    )
                    .unwrap()
                {
                    MemoryLearningCycleOutcome::ProceduralReconciliation(report) => {
                        if report.completed {
                            completed = true;
                            break;
                        }
                    }
                    MemoryLearningCycleOutcome::Blocked(report) => {
                        assert_eq!(report.reason, "governance_execution_binding_unavailable");
                    }
                    outcome => panic!("{label}: expected real reconciliation: {outcome:?}"),
                }
            }
            assert!(completed);
            drop(engine);
            assert!(project(&memory, "fully-withdrawn")
                .provider_payload()
                .agent_tool_hints()
                .is_empty());

            let mut spec = memory.producer_spec.clone();
            spec.binding_id = "new-method-declarer".into();
            spec.principal = ProceduralProducerPrincipalV1::LocalCapability {
                capability_id: "new-method-declarer".into(),
            };
            let registered = memory
                .governor
                .control_procedural_producer(MemoryProceduralProducerControlRequest {
                    operation_id: "register-new-method-declarer".into(),
                    expected_revision: None,
                    state: ProceduralProducerStateV1::Active,
                    spec,
                })
                .unwrap();
            let capability = memory
                .governor
                .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
                .unwrap();
            clock.0.fetch_add(2, Ordering::SeqCst);
            let mut declaration = method_request(&memory, "new-b-declaration", METHOD);
            declaration.turn.tool_observations.clear();
            declaration.learning.tool_call_count = 0;
            declaration.learning.agent_tool_feedback[0]
                .execution_facts
                .clear();
            declaration.learning.agent_tool_feedback[0].method_evidence[0]
                .execution_refs
                .clear();
            let (new_job, _) = complete_request_using(&memory, declaration, 1, &capability);
            let current = || {
                store
                    .export_replay_snapshot()
                    .unwrap()
                    .json_docs
                    .into_iter()
                    .filter(|document| {
                        document.namespace == "agent_tool_experience_revision_materials"
                    })
                    .map(|document| {
                        serde_json::from_value::<AgentToolExperienceRevisionMaterialV3>(
                            document.value,
                        )
                        .unwrap()
                    })
                    .filter(|material| {
                        matches!(material.body, AgentToolExperienceBodyV1::Method { .. })
                    })
                    .max_by_key(|material| material.owner_revision)
                    .expect("new accepted method must really exist")
            };
            let material = current();
            assert_eq!(
                material.status,
                AgentToolExperienceStatus::Candidate,
                "{label}: a new declaration cannot borrow withdrawn execution witnesses"
            );
            let AgentToolExperienceBodyV1::Method { sources, .. } = &material.body else {
                unreachable!()
            };
            assert!(!sources.is_empty(), "positive: B was actually accepted");
            assert!(sources.iter().all(|source| {
                source.source_job_id == new_job && source.execution_refs.is_empty()
            }));
            drop(memory);
            if backend != "memory" {
                drop(store);
                store = support::open_memory_store(config).unwrap();
            }
            let reopened = MemoryRuntime::builder()
                .identity(MemoryIdentity::new("promotion-agent", "promotion-owner").unwrap())
                .scope(MemoryScope::new("sdk.direct", "promotion-chat").unwrap())
                .store(store.clone())
                .clock(clock)
                .capability_policy(MemoryCapabilityPolicy::strict_profile())
                .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
                .agent_tool_registry(registry())
                .build()
                .unwrap();
            for temporal_operation in [
                MemoryRecallTemporalOperation::Current,
                MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time },
            ] {
                let denied = reopened
                    .project(MemoryProjectionRequest {
                        binding: ProceduralProjectionBindingV1::Preview,
                        temporal_operation,
                        user_query: "unpack archive".into(),
                        system_max_len: 16_384,
                        recent_messages_limit: 0,
                        pressure: PressureLevel::Normal,
                        mode_input: RuntimeLifecycleModeInput::default(),
                        structured_query_facets: vec![],
                        tool_registry_refs: vec![registry().registry_ref()],
                    })
                    .unwrap();
                assert!(denied.provider_payload().agent_tool_hints().is_empty());
                assert!(!denied
                    .provider_payload()
                    .system_memory_block()
                    .contains("Verify extracted files"));
            }
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_reversible_claim_narrowing_restores_exact_applied_method_sources() {
    use bm_core::skills::{
        AgentToolExperienceBodyV1, AgentToolExperienceRevisionMaterialV3, AgentToolExperienceStatus,
    };
    fn one_call(memory: &MemoryRuntime, id: &str) -> MemoryTurnFinalizeRequest {
        let mut input = request(memory, id);
        input.turn.tool_observations.truncate(1);
        input.learning.tool_call_count = 1;
        input.learning.agent_tool_feedback[0]
            .execution_facts
            .truncate(1);
        input.learning.agent_tool_feedback[0].method_evidence[0]
            .execution_refs
            .truncate(1);
        input
    }
    fn current_method(store: &MemoryStoreHandle) -> AgentToolExperienceRevisionMaterialV3 {
        store
            .export_replay_snapshot()
            .unwrap()
            .json_docs
            .into_iter()
            .filter(|document| document.namespace == "agent_tool_experience_revision_materials")
            .map(|document| {
                serde_json::from_value::<AgentToolExperienceRevisionMaterialV3>(document.value)
                    .unwrap()
            })
            .filter(|material| matches!(material.body, AgentToolExperienceBodyV1::Method { .. }))
            .max_by_key(|material| material.owner_revision)
            .expect("nonempty method owner")
    }
    fn finish_reconciliation(memory: &TestRuntime) {
        let engine = MemoryLearningEngine::attach(memory.memory.clone()).unwrap();
        for _ in 0..16 {
            match engine
                .run_due_cycle(
                    MemoryLearningCycleRequest {
                        lease_owner: "restore-worker".into(),
                        lease_duration_secs: 60,
                    },
                    &mut NoProvider,
                )
                .unwrap()
            {
                MemoryLearningCycleOutcome::ProceduralReconciliation(report) => {
                    if report.completed {
                        return;
                    }
                }
                MemoryLearningCycleOutcome::Blocked(report) => {
                    assert_eq!(report.reason, "governance_execution_binding_unavailable")
                }
                outcome => panic!("restoration must finish its real reconciliation: {outcome:?}"),
            }
        }
        panic!("finite fixture did not finish reconciliation");
    }
    let path = test_directory("restore-method-contributions");
    for backend in ["memory", "file", "sqlite"] {
        let profile = support::host_test_profile();
        let config = match backend {
            "memory" => StoreBackendConfig::in_memory(profile).unwrap(),
            "file" => StoreBackendConfig::file(path.join("file"), profile).unwrap(),
            "sqlite" => StoreBackendConfig::sqlite(path.join("sqlite.db"), profile).unwrap(),
            _ => unreachable!(),
        };
        let mut store = support::open_memory_store(config.clone()).unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let memory = runtime(store.clone(), clock.clone());
        complete_request(&memory, one_call(&memory, "restore-source-a"), 1);
        clock.0.fetch_add(2, Ordering::SeqCst);
        let mut spec_b = memory.producer_spec.clone();
        spec_b.binding_id = "second-executor".into();
        spec_b.principal = ProceduralProducerPrincipalV1::LocalCapability {
            capability_id: "second-executor".into(),
        };
        let grant_b = memory
            .governor
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "register-second-executor".into(),
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
                spec: spec_b,
            })
            .unwrap();
        let cap_b = memory
            .governor
            .procedural_submission_capability(&grant_b.binding.revision_ref().unwrap())
            .unwrap();
        complete_request_using(&memory, one_call(&memory, "restore-source-b"), 1, &cap_b);
        let positive = current_method(&store);
        assert_eq!(positive.status, AgentToolExperienceStatus::Active);
        let AgentToolExperienceBodyV1::Method {
            sources: original_sources,
            ..
        } = &positive.body
        else {
            unreachable!()
        };
        assert_eq!(
            original_sources.len(),
            2,
            "two different real producers support the same method"
        );
        let original = memory
            .governor
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "register-test-executor".into(),
                expected_revision: None,
                state: ProceduralProducerStateV1::Active,
                spec: memory.producer_spec.clone(),
            })
            .unwrap();
        let mut narrowed = memory.producer_spec.clone();
        narrowed.claims.method_declarations = false;
        let narrowed = memory
            .governor
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "narrow-first-method-claim".into(),
                expected_revision: Some(original.binding.revision_ref().unwrap()),
                state: ProceduralProducerStateV1::Active,
                spec: narrowed,
            })
            .unwrap();
        finish_reconciliation(&memory);
        let reduced = current_method(&store);
        assert_eq!(reduced.status, AgentToolExperienceStatus::Candidate);
        assert!(
            matches!(&reduced.body, AgentToolExperienceBodyV1::Method { sources, .. } if sources.len() == 1)
        );
        memory
            .governor
            .control_procedural_producer(MemoryProceduralProducerControlRequest {
                operation_id: "restore-first-method-claim".into(),
                expected_revision: Some(narrowed.binding.revision_ref().unwrap()),
                state: ProceduralProducerStateV1::Active,
                spec: memory.producer_spec.clone(),
            })
            .unwrap();
        finish_reconciliation(&memory);
        let restored = current_method(&store);
        assert_eq!(
            restored.status,
            AgentToolExperienceStatus::Active,
            "{backend}: restoring authority must recompute from the complete applied source set"
        );
        assert!(
            matches!(&restored.body, AgentToolExperienceBodyV1::Method { sources, .. } if sources == original_sources)
        );
        assert_eq!(restored.owner_ref, positive.owner_ref);
        assert!(!project(&memory, "restored-positive")
            .provider_payload()
            .agent_tool_hints()
            .is_empty());
        drop(memory);
        if backend != "memory" {
            drop(store);
            store = support::open_memory_store(config).unwrap();
        }
        assert_eq!(
            current_method(&store),
            restored,
            "{backend}: exact restored revision survives reopen"
        );
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_runtime_usage_authority_recovers_from_zero_after_reopen_without_double_counting() {
    verify_usage_authority_restoration(false, false);
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_retired_runtime_usage_withdraws_and_restores_without_reactivation() {
    verify_usage_authority_restoration(true, false);
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn pfi2_withdrawn_usage_retained_proof_cannot_be_erased_or_replaced_on_reopen() {
    verify_usage_authority_restoration(false, true);
}

#[cfg(feature = "nonproduction-replay-harness")]
fn verify_usage_authority_restoration(retired: bool, attack_retained: bool) {
    fn owner(store: &MemoryStoreHandle) -> bm_core::skills::RuntimeSkillOwnerRecord {
        let mut values = store
            .export_replay_snapshot()
            .unwrap()
            .json_docs
            .into_iter()
            .filter(|document| document.namespace == "runtime_skill_records")
            .map(|document| {
                serde_json::from_value::<bm_core::skills::RuntimeSkillOwnerRecord>(document.value)
                    .unwrap()
            });
        let value = values.next().expect("nonempty promoted owner");
        assert!(values.next().is_none());
        value
    }
    fn reconcile(memory: &TestRuntime) {
        let engine = MemoryLearningEngine::attach(memory.memory.clone()).unwrap();
        for _ in 0..16 {
            match engine
                .run_due_cycle(
                    MemoryLearningCycleRequest {
                        lease_owner: "usage-restoration-worker".into(),
                        lease_duration_secs: 60,
                    },
                    &mut NoProvider,
                )
                .unwrap()
            {
                MemoryLearningCycleOutcome::ProceduralReconciliation(report) => {
                    if report.completed {
                        return;
                    }
                }
                MemoryLearningCycleOutcome::Blocked(report) => {
                    assert_eq!(report.reason, "governance_execution_binding_unavailable")
                }
                outcome => panic!("usage restoration must complete: {outcome:?}"),
            }
        }
        panic!("finite usage restoration did not finish");
    }
    let path = test_directory(if retired {
        "retired-usage-authority-restoration"
    } else {
        "usage-authority-restoration"
    });
    let config =
        StoreBackendConfig::sqlite(path.join("synthetic.db"), support::host_test_profile())
            .unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let mut store = support::open_memory_store(config.clone()).unwrap();
    let memory = runtime(store.clone(), clock.clone());
    for id in ["usage-restore-method-a", "usage-restore-method-b"] {
        complete_request(&memory, request(&memory, id), 1);
        clock.0.fetch_add(2, Ordering::SeqCst);
    }
    for id in ["usage-restore-fact-a", "usage-restore-fact-b"] {
        let projection = project(&memory, id);
        let receipt = projection.report().selection_receipt().unwrap().clone();
        let selected = receipt
            .runtime_skills
            .first()
            .expect("real public selection")
            .clone();
        let mut input = request(&memory, id);
        input.turn.tool_observations.clear();
        input.learning = PostTurnLearningInputV2 {
            selection_receipt: Some(receipt),
            runtime_skill_feedback: vec![RuntimeSkillUsageFeedbackV1 {
                locator: selected.locator,
                selected_content_digest: selected.content_digest,
                outcome: ProceduralExecutionOutcomeV1::Succeeded,
                observation_ref: id.into(),
            }],
            ..PostTurnLearningInputV2::empty()
        };
        complete_request(&memory, input, 1);
        clock.0.fetch_add(2, Ordering::SeqCst);
    }
    if retired {
        memory
            .retire_runtime_skill(RuntimeSkillRetireRequest {
                locator: skills(&memory).remove(0).locator,
                observed_at: clock.now_secs(),
            })
            .unwrap();
        clock.0.fetch_add(2, Ordering::SeqCst);
        reconcile(&memory);
    }
    let positive = owner(&store);
    assert_eq!(positive.lifecycle.usage_outcome.succeeded_count, 2);
    let original = memory
        .governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "register-test-executor".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: memory.producer_spec.clone(),
        })
        .unwrap();
    let mut spec = memory.producer_spec.clone();
    spec.claims.usage_feedback = false;
    let narrowed = memory
        .governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "narrow-usage-claims".into(),
            expected_revision: Some(original.binding.revision_ref().unwrap()),
            state: ProceduralProducerStateV1::Active,
            spec,
        })
        .unwrap();
    reconcile(&memory);
    let withdrawn = owner(&store);
    assert_eq!(withdrawn.lifecycle.usage_outcome.observation_count, 0);
    assert_eq!(
        withdrawn.lifecycle.usage_outcome.retained_contributions,
        positive.lifecycle.usage_outcome.retained_contributions
    );
    if attack_retained {
        assert_withdrawn_retained_proof(&store, &path);
    }
    drop(memory);
    drop(store);
    store = support::open_memory_store(config.clone()).unwrap();
    let memory = runtime(store.clone(), clock.clone());
    memory
        .governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "restore-usage-claims".into(),
            expected_revision: Some(narrowed.binding.revision_ref().unwrap()),
            state: ProceduralProducerStateV1::Active,
            spec: memory.producer_spec.clone(),
        })
        .unwrap();
    reconcile(&memory);
    let restored = owner(&store);
    if retired {
        assert_eq!(
            restored.lifecycle.state,
            bm_core::skills::RuntimeSkillLifecycleState::Retired
        );
        assert_eq!(
            restored.lifecycle.availability,
            bm_core::skills::RuntimeSkillAvailability::Disabled
        );
        assert!(project(&memory, "retired-restored-projection")
            .report()
            .selection_receipt()
            .is_none_or(|receipt| receipt.runtime_skills.is_empty()));
    }
    assert_eq!(
        restored.lifecycle.usage_outcome,
        positive.lifecycle.usage_outcome
    );
    assert_eq!(restored.procedural_content, positive.procedural_content);
    assert_eq!(restored.intrinsic_contract, positive.intrinsic_contract);
    assert_eq!(restored.owner_ref, positive.owner_ref);
    assert_eq!(skills(&memory)[0].validated_success_count, 2);
    drop(memory);
    drop(store);
    let reopened = support::open_memory_store(config).unwrap();
    assert_eq!(owner(&reopened), restored);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[cfg(feature = "nonproduction-replay-harness")]
fn assert_withdrawn_retained_proof(source: &MemoryStoreHandle, root: &std::path::Path) {
    use bm_core::skills::{
        RuntimeSkillOwnerBinding, RuntimeSkillOwnerRecord, RuntimeSkillScopeManifest,
    };
    let snapshot = source.export_replay_snapshot().unwrap();
    let owner_doc = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "runtime_skill_records")
        .unwrap();
    let manifest_doc = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "runtime_skill_scope_manifests")
        .unwrap();
    let owner: RuntimeSkillOwnerRecord = serde_json::from_value(owner_doc.value.clone()).unwrap();
    let manifest: RuntimeSkillScopeManifest =
        serde_json::from_value(manifest_doc.value.clone()).unwrap();
    assert_eq!(owner.lifecycle.usage_outcome.observation_count, 0);
    assert!(owner.lifecycle.usage_outcome.contributions.is_empty());
    assert!(!owner
        .lifecycle
        .usage_outcome
        .retained_contributions
        .is_empty());
    for backend in ["file", "sqlite"] {
        for attack in ["erase", "replace"] {
            let path = root.join(format!("withdrawn-{backend}-{attack}"));
            let config = if backend == "file" {
                StoreBackendConfig::file(&path, support::host_test_profile()).unwrap()
            } else {
                StoreBackendConfig::sqlite(&path, support::host_test_profile()).unwrap()
            };
            // File has no indexed RuntimeSkill delivery. Prepare its proof from
            // the real SQLite execution above via the existing synthetic harness;
            // admission/open still exercise the actual File backend, not a mock.
            let store = support::open_memory_store(config.clone()).unwrap();
            store.import_replay_snapshot(&snapshot).unwrap();
            drop(store);
            let positive = support::open_memory_store(config.clone()).unwrap();
            assert!(positive
                .export_replay_snapshot()
                .unwrap()
                .json_docs
                .iter()
                .any(|doc| doc == owner_doc));
            drop(positive);
            let mut lifecycle = owner.lifecycle.clone();
            if attack == "erase" {
                lifecycle.usage_outcome.retained_contributions.clear();
            } else {
                lifecycle.usage_outcome.retained_contributions[0].content_digest =
                    format!("sha256:{}", "f".repeat(64));
            }
            let forged = RuntimeSkillOwnerRecord::build(
                &owner.memory_space_id,
                owner.owning_scope.clone(),
                owner.creation_ref.clone(),
                owner.owner_revision,
                owner.intrinsic_contract.clone(),
                owner.procedural_content.clone(),
                lifecycle,
                owner.privacy_class,
            )
            .unwrap();
            let forged_manifest = RuntimeSkillScopeManifest::build(
                manifest.revision,
                &manifest.memory_space_id,
                manifest.owning_scope.clone(),
                [RuntimeSkillOwnerBinding::from_record(&forged).unwrap()],
                16,
            )
            .unwrap();
            let replacements = [
                (owner_doc, serde_json::to_value(forged).unwrap()),
                (manifest_doc, serde_json::to_value(forged_manifest).unwrap()),
            ];
            if backend == "sqlite" {
                let connection = rusqlite::Connection::open(&path).unwrap();
                for (doc, value) in &replacements {
                    assert_eq!(connection.execute("UPDATE bm_kv SET value_json = ?1 WHERE namespace = ?2 AND key = ?3",
                        rusqlite::params![serde_json::to_string(value).unwrap(), &doc.namespace, &doc.key]).unwrap(), 1);
                }
            } else {
                for (doc, value) in &replacements {
                    let mut exact = None;
                    for shard in
                        std::fs::read_dir(path.join("kv").join(&doc.namespace).join("_v2")).unwrap()
                    {
                        for file in std::fs::read_dir(shard.unwrap().path()).unwrap() {
                            let file = file.unwrap().path();
                            let before: serde_json::Value =
                                serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
                            if before == doc.value {
                                assert!(exact.replace(file).is_none());
                            }
                        }
                    }
                    std::fs::write(
                        exact.expect("exact synthetic owner document"),
                        serde_json::to_vec(value).unwrap(),
                    )
                    .unwrap();
                }
            }
            fn bytes(path: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
                if path.is_file() {
                    return vec![(path.to_owned(), std::fs::read(path).unwrap())];
                }
                let mut files = Vec::new();
                for entry in std::fs::read_dir(path).unwrap() {
                    files.extend(bytes(&entry.unwrap().path()));
                }
                files.sort_by(|a, b| a.0.cmp(&b.0));
                files
            }
            let before = bytes(&path);
            assert!(
                support::open_memory_store(config).is_err(),
                "{backend}/{attack}: zero active contributions cannot erase retained authority"
            );
            assert_eq!(
                bytes(&path),
                before,
                "failed open cannot rewrite or initialize proof"
            );
        }
    }
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_private_feedback_body_is_absent_before_worker_runs() {
    const PRIVATE_MARKER: &str = "synthetic-private-method-only-7ea341";
    let config = StoreBackendConfig::in_memory(support::host_test_profile()).unwrap();
    let store = support::open_memory_store(config).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let memory = runtime(store.clone(), clock);
    let mut input = request(&memory, "private-intake");
    // The marker belongs only to learning payload, not to independently valid
    // conversation text. No worker has run to conceal an intake-time disclosure.
    for declaration in &mut input.learning.agent_tool_feedback[0].method_evidence {
        declaration.body = format!("1. {PRIVATE_MARKER}\n2. Keep this method private.");
        declaration.source_sensitivity = ProceduralSourceSensitivity::Private;
    }
    assert!(!serde_json::to_string(&input.turn)
        .unwrap()
        .contains(PRIVATE_MARKER));
    let finalized = memory
        .submit(input)
        .expect("legitimate conversation commits");
    assert!(finalized.transcript_commit.unwrap().committed);
    let snapshot = store.export_replay_snapshot().unwrap();
    assert!(snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript"));
    assert!(
        !serde_json::to_string(&snapshot)
            .unwrap()
            .contains(PRIVATE_MARKER),
        "privacy-denied method payload must never enter any durable intake document"
    );
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_finalize_operation_does_not_grant_producer_authority() {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let memory = runtime(
        store.clone(),
        Arc::new(Clock(AtomicU64::new(1_800_000_000))),
    );
    // A correctly scoped ordinary conversation remains usable without learning authority.
    let mut ordinary = request(&memory, "without-feedback");
    ordinary.learning = PostTurnLearningInputV2::with_tool_call_count(2);
    assert!(
        memory
            .finalize_turn(ordinary)
            .unwrap()
            .transcript_commit
            .unwrap()
            .committed
    );
    let before = store.export_replay_snapshot().unwrap();
    assert!(!before.json_docs.is_empty());
    let result = memory.finalize_turn(request(&memory, "unbound-producer"));
    assert!(
        result.is_err(),
        "FinalizeTurn permission must not mint an execution witness"
    );
    assert_eq!(
        store.export_replay_snapshot().unwrap(),
        before,
        "unauthorized evidence must fail before the first write"
    );
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_failed_execution_is_retained_without_inventing_a_method() {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let memory = Arc::new(runtime(
        store.clone(),
        Arc::new(Clock(AtomicU64::new(1_800_000_000))),
    ));
    let mut input = request(&memory, "failed-execution-only");
    for observation in &mut input.learning.agent_tool_feedback[0].execution_facts {
        observation.outcome = ToolExecutionOutcome::Failed;
    }
    input.learning.agent_tool_feedback[0]
        .method_evidence
        .clear();
    let finalized = memory.submit(input).unwrap();
    assert!(finalized.transcript_commit.unwrap().committed);
    let outcome = MemoryLearningEngine::attach(memory.memory.clone())
        .unwrap()
        .run_due_cycle(
            MemoryLearningCycleRequest {
                lease_owner: "execution-statistics-worker".into(),
                lease_duration_secs: 60,
            },
            &mut NoProvider,
        )
        .unwrap();
    assert!(matches!(
        outcome,
        MemoryLearningCycleOutcome::ProceduralCompleted(_)
    ));
    let snapshot = store.export_replay_snapshot().unwrap();
    assert!(snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript"));
    let materials = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "agent_tool_experience_revision_materials")
        .collect::<Vec<_>>();
    assert!(
        !materials.is_empty(),
        "real failed executions must not disappear through a success-only admission filter"
    );
    assert!(
        skills(&memory).is_empty(),
        "execution counts cannot create a method or RuntimeSkill"
    );
}

#[test]
fn withdrawing_an_earlier_promotion_source_retires_current_and_historical_projection() {
    let path = test_directory("withdrawal");
    let config =
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap();
    let store = support::open_memory_store(config.clone()).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let memory = Arc::new(runtime(store.clone(), clock.clone()));
    for turn in ["old-source", "promotion-source"] {
        complete_request(&memory, request(&memory, turn), 1);
        clock.0.fetch_add(2, Ordering::SeqCst);
    }
    let positive = project(&memory, "before-withdrawal");
    assert!(!positive
        .report()
        .selection_receipt()
        .unwrap()
        .runtime_skills
        .is_empty());
    let before = skills(&memory).remove(0);
    let as_of_time = clock.now_secs();
    // Raw archive/import is available only in the dedicated nonproduction
    // harness profile. Production still exercises withdrawal, history and reopen.
    #[cfg(feature = "nonproduction-replay-harness")]
    let archive_scope =
        MemoryArchiveScope::subject(memory.memory_space_id(), memory.subject_id()).unwrap();
    #[cfg(feature = "nonproduction-replay-harness")]
    let archive = memory
        .export_memory_space(MemorySpaceExportRequest {
            scope: archive_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .unwrap()
        .archive;
    clock.0.fetch_add(2, Ordering::SeqCst);
    memory
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: memory.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: "promotion-chat".into(),
            turn_id: Some("old-source".into()),
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "synthetic earlier-source withdrawal".into(),
        })
        .unwrap();
    assert!(
        skills(&memory).is_empty(),
        "withdrawn body and usage are not a public inspection surface"
    );
    assert!(memory
        .get_runtime_skill(RuntimeSkillDetailRequest {
            locator: before.locator.clone()
        })
        .is_err());
    #[cfg(feature = "nonproduction-replay-harness")]
    let retired = {
        let image = store.export_replay_snapshot().unwrap();
        let record: bm_core::skills::RuntimeSkillOwnerRecord = serde_json::from_value(
            image
                .json_docs
                .iter()
                .find(|document| {
                    document.namespace == "runtime_skill_records"
                        && document.value["owner_ref"]["owner_id"] == before.owner_id
                })
                .expect("actual retained owner, not a public read bypass")
                .value
                .clone(),
        )
        .unwrap();
        assert_eq!(
            record.lifecycle.state,
            bm_core::skills::RuntimeSkillLifecycleState::Retired
        );
        assert!(record.owner_revision > before.locator.owner_revision());
        record
    };
    #[cfg(feature = "nonproduction-replay-harness")]
    {
        let terminal = store.export_replay_snapshot().unwrap();
        let error = memory
            .import_memory_space(MemorySpaceImportRequest {
                scope: archive_scope,
                expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
                archive,
            })
            .expect_err("old archive cannot restore a permanently deleted source");
        let bm_sdk::Error::Other { source, .. } = error else {
            panic!("expected typed archive source conflict: {error:?}")
        };
        assert_eq!(
            source.downcast_ref::<MemorySpaceImportConflict>(),
            Some(&MemorySpaceImportConflict::ExistingTranscriptDiffers)
        );
        assert_eq!(store.export_replay_snapshot().unwrap(), terminal);
    }
    assert!(
        skills(&memory).is_empty(),
        "old archive cannot reauthorize withdrawn content"
    );
    for temporal_operation in [
        MemoryRecallTemporalOperation::Current,
        MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time },
    ] {
        let denied = memory
            .project(MemoryProjectionRequest {
                binding: match temporal_operation {
                    MemoryRecallTemporalOperation::Current => ProceduralProjectionBindingV1::Turn {
                        turn_id: "after-withdrawal".into(),
                    },
                    MemoryRecallTemporalOperation::HistoricalAsOf { .. } => {
                        ProceduralProjectionBindingV1::Preview
                    }
                },
                temporal_operation,
                user_query: "unpack archive".into(),
                system_max_len: 16_384,
                recent_messages_limit: 0,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: Vec::new(),
                tool_registry_refs: vec![registry().registry_ref()],
            })
            .unwrap();
        assert!(denied
            .report()
            .selection_receipt()
            .is_none_or(|receipt| receipt.runtime_skills.is_empty()));
        assert!(!denied
            .provider_payload()
            .system_memory_block()
            .contains("Verify extracted files"));
    }
    drop(memory);
    drop(store);
    let reopened = support::open_memory_store(config).unwrap();
    let memory = runtime(reopened.clone(), clock);
    assert!(skills(&memory).is_empty());
    assert!(memory
        .get_runtime_skill(RuntimeSkillDetailRequest {
            locator: before.locator
        })
        .is_err());
    #[cfg(feature = "nonproduction-replay-harness")]
    {
        let image = reopened.export_replay_snapshot().unwrap();
        let persisted = image
            .json_docs
            .iter()
            .find(|document| {
                document.namespace == "runtime_skill_records"
                    && document.key == retired.physical_key
            })
            .unwrap();
        assert_eq!(
            serde_json::from_value::<bm_core::skills::RuntimeSkillOwnerRecord>(
                persisted.value.clone()
            )
            .unwrap(),
            retired
        );
    }
    drop(memory);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn scoped_production_promotions_keep_project_workspace_and_conversation_boundaries() {
    let path = test_directory("scope");
    for scope in [
        AgentToolRegistryScope::Project {
            project_id: "project-a".into(),
        },
        AgentToolRegistryScope::Workspace {
            workspace_id: "workspace-a".into(),
        },
        AgentToolRegistryScope::Conversation {
            conversation_id: "conversation-a".into(),
        },
    ] {
        let mut scoped_registry = registry();
        scoped_registry.scope = scope.clone();
        scoped_registry.fingerprint =
            bm_core::skills::fingerprint_agent_tool_registry(&scoped_registry);
        let label = match scope {
            AgentToolRegistryScope::Project { .. } => "project",
            AgentToolRegistryScope::Workspace { .. } => "workspace",
            _ => "conversation",
        };
        let store = support::open_memory_store(
            StoreBackendConfig::sqlite(
                path.join(format!("{label}.db")),
                support::host_test_profile(),
            )
            .unwrap(),
        )
        .unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
        let build = |matching: bool| {
            let conversation = if matching {
                "conversation-a"
            } else {
                "conversation-b"
            };
            MemoryRuntime::builder()
                .identity(MemoryIdentity::new("promotion-agent", "promotion-owner").unwrap())
                .scope(
                    MemoryScope::new("sdk.direct", "promotion-chat")
                        .unwrap()
                        .with_conversation_id(conversation)
                        .unwrap(),
                )
                .store(store.clone())
                .clock(clock.clone())
                .capability_policy(MemoryCapabilityPolicy::strict_profile())
                .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
                .agent_tool_registry(scoped_registry.clone())
                .procedural_applicability_context(
                    ProceduralApplicabilityContextV1::try_new(
                        Some(if matching { "project-a" } else { "project-b" }.into()),
                        Some(
                            if matching {
                                "workspace-a"
                            } else {
                                "workspace-b"
                            }
                            .into(),
                        ),
                        Some(conversation.into()),
                    )
                    .unwrap(),
                )
                .build()
                .unwrap()
        };
        let memory = Arc::new(TestRuntime::new(
            build(true),
            store.clone(),
            scoped_registry.clone(),
        ));
        for turn in ["scope-first", "scope-second"] {
            let mut input = request(&memory, turn);
            input.turn.conversation.conversation_id = Some("conversation-a".into());
            input.learning.agent_tool_feedback[0].registry_ref = scoped_registry.registry_ref();
            complete_request(&memory, input, 1);
            clock.0.fetch_add(2, Ordering::SeqCst);
        }
        let created = skills(&memory);
        assert_eq!(
            created.len(),
            1,
            "typed {label} owner exists before delivery checks"
        );
        let scoped_project = |runtime: &MemoryRuntime| {
            runtime
                .project(MemoryProjectionRequest {
                    binding: ProceduralProjectionBindingV1::Turn {
                        turn_id: "scope-use".into(),
                    },
                    temporal_operation: MemoryRecallTemporalOperation::Current,
                    user_query: "unpack archive".into(),
                    system_max_len: 16_384,
                    recent_messages_limit: 0,
                    pressure: PressureLevel::Normal,
                    mode_input: RuntimeLifecycleModeInput::default(),
                    structured_query_facets: Vec::new(),
                    tool_registry_refs: vec![scoped_registry.registry_ref()],
                })
                .unwrap()
        };
        let positive = scoped_project(&memory);
        assert!(
            positive
                .report()
                .selection_receipt()
                .unwrap()
                .runtime_skills
                .iter()
                .any(|selection| selection.locator == created[0].locator),
            "real matching {label} context must deliver the production Runtime owner"
        );
        let denied = scoped_project(&build(false));
        assert!(
            denied
                .report()
                .selection_receipt()
                .is_none_or(|receipt| receipt.runtime_skills.is_empty()),
            "wrong {label} cannot receive the owner"
        );
        assert!(!denied
            .provider_payload()
            .system_memory_block()
            .contains("Verify extracted files"));
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn production_sources_from_different_subjects_never_combine_into_a_runtime_skill() {
    let path = test_directory("subjects");
    let store = support::open_memory_store(
        StoreBackendConfig::sqlite(path.join("store.db"), support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let mut subjects =
        SubjectRegistry::single_agent_default("promotion-owner", "promotion-agent").unwrap();
    subjects
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "Agent B",
        ))
        .unwrap();
    let build = |agent: &str, chat: &str| {
        Arc::new(TestRuntime::new(
            MemoryRuntime::builder()
                .identity(MemoryIdentity::new(agent, "promotion-owner").unwrap())
                .scope(MemoryScope::new("sdk.direct", chat).unwrap())
                .subject_registry(subjects.clone())
                .store(store.clone())
                .clock(clock.clone())
                .capability_policy(MemoryCapabilityPolicy::strict_profile())
                .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
                .agent_tool_registry(registry())
                .build()
                .unwrap(),
            store.clone(),
            registry(),
        ))
    };
    let a = build("promotion-agent", "promotion-chat");
    let b = build("agent-b", "other-chat");
    complete_request(&a, request(&a, "a-first"), 1);
    let mut b_input = request(&b, "b-first");
    b_input.turn.conversation.chat_id = "other-chat".into();
    b_input.turn.conversation.conversation_id = Some("other-chat".into());
    complete_request(&b, b_input, 1);
    assert!(skills(&a).is_empty());
    assert!(
        skills(&b).is_empty(),
        "two subjects' independent accepted turns do not form repeated evidence"
    );
    clock.0.fetch_add(2, Ordering::SeqCst);
    complete_request(&a, request(&a, "a-second"), 1);
    assert_eq!(
        skills(&a).len(),
        1,
        "same-subject second source is a nonempty positive control"
    );
    assert!(skills(&b).is_empty());
    assert!(project(&b, "b-use")
        .report()
        .selection_receipt()
        .is_none_or(|receipt| receipt.runtime_skills.is_empty()));
    drop(a);
    drop(b);
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn runtime_builder_rejects_duplicate_project_workspace_conversation_authorities_before_writes() {
    for target in [
        RuntimeSkillApplicabilityTarget::Project {
            project_id: "project-a".into(),
        },
        RuntimeSkillApplicabilityTarget::Workspace {
            workspace_id: "workspace-a".into(),
        },
        RuntimeSkillApplicabilityTarget::Conversation {
            conversation_id: "conversation-a".into(),
        },
    ] {
        let store = support::empty_store_platform(support::host_test_profile());
        let before = store.export_replay_snapshot().unwrap();
        let result = MemoryRuntime::builder()
            .identity(MemoryIdentity::new("promotion-agent", "promotion-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "promotion-chat").unwrap())
            .store(store.clone())
            .runtime_skill_applicability_context(
                RuntimeSkillApplicabilityContext::try_new(vec![target.clone()]).unwrap(),
            )
            .build();
        assert!(result.is_err(), "{target:?} has only the procedural applicability owner, not a second Runtime builder authority");
        assert_eq!(
            store.export_replay_snapshot().unwrap(),
            before,
            "invalid constructor must not initialize signing keys or any owner"
        );
        let valid = MemoryRuntime::builder()
            .identity(MemoryIdentity::new("promotion-agent", "promotion-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "promotion-chat").unwrap())
            .store(store)
            .runtime_skill_applicability_context(
                RuntimeSkillApplicabilityContext::try_new(vec![
                    RuntimeSkillApplicabilityTarget::User {
                        user_ref: "human-owner".into(),
                    },
                    RuntimeSkillApplicabilityTarget::Organization {
                        organization_id: "organization-a".into(),
                    },
                    RuntimeSkillApplicabilityTarget::Device {
                        device_id: "device-a".into(),
                    },
                ])
                .unwrap(),
            )
            .build();
        assert!(
            valid.is_ok(),
            "remaining typed Runtime applicability remains usable"
        );
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn file_and_sqlite_reopen_reject_forged_runtime_promotion_source_commitments() {
    use bm_core::skills::{
        RuntimeSkillOwnerBinding, RuntimeSkillOwnerRecord, RuntimeSkillScopeManifest,
    };
    let root = test_directory("source-open");
    let profile = support::host_test_profile();
    for backend in ["file", "sqlite"] {
        for corruption in [
            "unknown-source",
            "other-subject-source",
            "generic-source-kind",
        ] {
            let case = root.join(format!("{backend}-{corruption}"));
            std::fs::create_dir_all(&case).unwrap();
            let store_path = if backend == "file" {
                case.join("store")
            } else {
                case.join("store.db")
            };
            let config = if backend == "file" {
                StoreBackendConfig::file(&store_path, profile).unwrap()
            } else {
                StoreBackendConfig::sqlite(&store_path, profile).unwrap()
            };
            let store = support::open_memory_store(config.clone()).unwrap();
            let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
            let memory = Arc::new(runtime(store.clone(), clock.clone()));
            for turn in ["open-first", "open-second"] {
                complete_request(&memory, request(&memory, turn), 1);
                clock.0.fetch_add(2, Ordering::SeqCst);
            }
            assert_eq!(
                skills(&memory).len(),
                1,
                "real production promotion positive"
            );
            let mut subjects =
                SubjectRegistry::single_agent_default("promotion-owner", "promotion-agent")
                    .unwrap();
            subjects
                .upsert_subject(SubjectDescriptor::agent_persona(
                    default_agent_subject_id("agent-b"),
                    "Agent B",
                ))
                .unwrap();
            let other = Arc::new(TestRuntime::new(
                MemoryRuntime::builder()
                    .identity(MemoryIdentity::new("agent-b", "promotion-owner").unwrap())
                    .scope(MemoryScope::new("sdk.direct", "other-chat").unwrap())
                    .subject_registry(subjects)
                    .store(store.clone())
                    .clock(clock.clone())
                    .capability_policy(MemoryCapabilityPolicy::strict_profile())
                    .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
                    .agent_tool_registry(registry())
                    .build()
                    .unwrap(),
                store.clone(),
                registry(),
            ));
            let mut other_input = request(&other, "other-subject-source");
            other_input.turn.conversation.chat_id = "other-chat".into();
            other_input.turn.conversation.conversation_id = Some("other-chat".into());
            let other_job_id = complete_request(&other, other_input, 1).0;
            let snapshot = store.export_replay_snapshot().unwrap();
            let owner_doc = snapshot
                .json_docs
                .iter()
                .find(|doc| doc.namespace == "runtime_skill_records")
                .unwrap();
            let owner: RuntimeSkillOwnerRecord =
                serde_json::from_value(owner_doc.value.clone()).unwrap();
            let manifest_doc = snapshot
                .json_docs
                .iter()
                .find(|doc| {
                    doc.namespace == "runtime_skill_scope_manifests"
                        && doc.value["memory_space_id"] == owner.memory_space_id
                        && doc.value["owning_scope"]
                            == serde_json::to_value(&owner.owning_scope).unwrap()
                })
                .unwrap();
            let manifest: RuntimeSkillScopeManifest =
                serde_json::from_value(manifest_doc.value.clone()).unwrap();
            let mut intrinsic = owner.intrinsic_contract.clone();
            match corruption {
                "unknown-source" => {
                    intrinsic.evidence_bindings[0].safe_ref =
                        format!("procedural_feedback_job:sha256:{}", "f".repeat(64))
                }
                "other-subject-source" => {
                    let other_job = snapshot
                        .json_docs
                        .iter()
                        .find(|doc| {
                            doc.namespace == "procedural_feedback_jobs" && doc.key == other_job_id
                        })
                        .unwrap();
                    intrinsic.evidence_bindings[0].safe_ref = other_job_id;
                    let other_job: bm_core::memory::ProceduralFeedbackJobV2 =
                        serde_json::from_value(other_job.value.clone()).unwrap();
                    intrinsic.evidence_bindings[0].source_digest = other_job
                        .feedback_source()
                        .unwrap()
                        .learning_evidence_digest
                        .clone();
                }
                _ => {
                    intrinsic.evidence_bindings[0].kind = RuntimeSkillEvidenceKind::GovernedEvidence
                }
            }
            let forged = RuntimeSkillOwnerRecord::build(
                &owner.memory_space_id,
                owner.owning_scope.clone(),
                owner.creation_ref.clone(),
                owner.owner_revision,
                intrinsic,
                owner.procedural_content.clone(),
                owner.lifecycle.clone(),
                owner.privacy_class,
            )
            .unwrap();
            let forged_manifest = RuntimeSkillScopeManifest::build(
                manifest.revision,
                &manifest.memory_space_id,
                manifest.owning_scope.clone(),
                vec![RuntimeSkillOwnerBinding::from_record(&forged).unwrap()],
                memory
                    .runtime_budget()
                    .governed_state_budget
                    .max_retained_runtime_skill_owners_per_scope,
            )
            .unwrap();
            let replacements = [
                (
                    owner_doc.namespace.clone(),
                    owner_doc.key.clone(),
                    owner_doc.value.clone(),
                    serde_json::to_value(forged).unwrap(),
                ),
                (
                    manifest_doc.namespace.clone(),
                    manifest_doc.key.clone(),
                    manifest_doc.value.clone(),
                    serde_json::to_value(forged_manifest).unwrap(),
                ),
            ];
            drop(other);
            drop(memory);
            drop(store);
            let positive_reopen = support::open_memory_store(config.clone()).unwrap();
            assert!(positive_reopen
                .export_replay_snapshot()
                .unwrap()
                .json_docs
                .iter()
                .any(|doc| doc.namespace == "runtime_skill_records"));
            drop(positive_reopen);
            // Deliberate corruption of only this synthetic backend's two exact
            // current-owner documents. Historical ledger/MOR/source rows stay intact.
            if backend == "sqlite" {
                let connection = rusqlite::Connection::open(&store_path).unwrap();
                for (namespace, key, _, after) in &replacements {
                    assert_eq!(connection.execute("UPDATE bm_kv SET value_json = ?1 WHERE namespace = ?2 AND key = ?3",
                        rusqlite::params![serde_json::to_string(after).unwrap(), namespace, key]).unwrap(), 1);
                }
            } else {
                for (namespace, _, before, after) in &replacements {
                    let mut exact_path = None;
                    for shard in
                        std::fs::read_dir(store_path.join("kv").join(namespace).join("_v2"))
                            .unwrap()
                    {
                        for entry in std::fs::read_dir(shard.unwrap().path()).unwrap() {
                            let path = entry.unwrap().path();
                            let value: serde_json::Value =
                                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                            if value == *before {
                                assert!(
                                    exact_path.replace(path).is_none(),
                                    "one exact physical owner address"
                                );
                            }
                        }
                    }
                    std::fs::write(
                        exact_path.expect("validated exact synthetic document"),
                        serde_json::to_vec(after).unwrap(),
                    )
                    .unwrap();
                }
            }
            let result = support::open_memory_store(config);
            assert!(result.is_err(), "{backend} open must reject {corruption} even with canonical current owner/manifest digests and intact historical ledgers");
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
