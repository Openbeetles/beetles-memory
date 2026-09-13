#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::platform::Platform as _;
use bm_sdk::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

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
        panic!("procedural learning must not call a Provider")
    }
}

fn registry() -> AgentToolRegistrySnapshot {
    AgentToolRegistrySnapshot::compact(
        "receipt-tools",
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
    store: MemoryStoreHandle,
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
        binding: &str,
        source: ProceduralProducerSourceAuthorityV1,
    ) -> Self {
        let governor = support::procedural::governor(&memory, store.clone());
        let mut spec = support::procedural::runtime_observer_spec(&memory, binding);
        spec.source_authority = source;
        let capability = support::procedural::register_and_issue(
            &governor,
            spec,
            &format!("register-{binding}"),
        );
        Self {
            memory: Arc::new(memory),
            governor,
            capability,
            store,
        }
    }
    fn submit(&self, input: MemoryTurnFinalizeRequest) -> Result<MemoryTurnFinalizeReport> {
        self.memory
            .finalize_turn_with_procedural_evidence(&self.capability, input)
    }
    fn model_capability(&self) -> MemoryProceduralSubmissionCapability {
        let mut spec = support::procedural::runtime_observer_spec(&self.memory, "receipt-model");
        spec.source_authority = ProceduralProducerSourceAuthorityV1::ModelInferred {
            subject_id: self.subject_id().into(),
        };
        spec.claims.execution_facts = false;
        spec.claims.usage_feedback = false;
        support::procedural::register_and_issue(&self.governor, spec, "register-receipt-model")
    }
}

fn runtime(store: MemoryStoreHandle, clock: Arc<Clock>, agent: &str) -> TestRuntime {
    let mut subjects = SubjectRegistry::single_agent_default("receipt-owner", "agent-a").unwrap();
    subjects
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "Agent B",
        ))
        .unwrap();
    let memory = MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent, "receipt-owner").unwrap())
        .scope(MemoryScope::new("sdk.direct", "receipt-chat").unwrap())
        .subject_registry(subjects)
        .store(store.clone())
        .clock(clock)
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .agent_tool_registry(registry())
        .build()
        .unwrap();
    TestRuntime::new(
        memory,
        store,
        "receipt-executor",
        ProceduralProducerSourceAuthorityV1::RuntimeObservation,
    )
}

fn request(
    runtime: &MemoryRuntime,
    turn: &str,
    receipt: Option<ProceduralSelectionReceiptV1>,
) -> MemoryTurnFinalizeRequest {
    let now = runtime.config().clock.now_secs();
    let facts = ["a", "b"]
        .into_iter()
        .map(|suffix| ToolExecutionFactV1 {
            observation_id: format!("observation-{turn}-{suffix}"),
            call_id: format!("call-{turn}-{suffix}"),
            outcome: ToolExecutionOutcome::Succeeded,
            source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
            started_at: Some(now),
            completed_at: Some(now),
        })
        .collect::<Vec<_>>();
    let canonical = support::procedural::canonical_observations("archive.unpack", &facts);
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn.into(),
            conversation: ConversationScope {
                channel: "sdk.direct".into(),
                chat_id: "receipt-chat".into(),
                conversation_id: Some("receipt-chat".into()),
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
            assistant_message: Some(TranscriptInputMessage::assistant(
                "archive unpack completed successfully",
            )),
            tool_observations: canonical,
            external_content_used: false,
            candidate_ids: vec![],
        },
        learning: PostTurnLearningInputV2 {
            tool_call_count: 2,
            selection_receipt: receipt,
            runtime_skill_feedback: vec![],
            agent_skill_feedback: vec![],
            task_learning_feedback: vec![],
            agent_tool_feedback: vec![AgentToolUsageFeedbackV3 {
                registry_ref: registry().registry_ref(), tool_id: "archive.unpack".into(), schema_fingerprint: "schema-archive-v1".into(),
                method_evidence: vec![ToolMethodEvidenceV1 { method_id: format!("method-{turn}"), task_signature: "unpack_archive".into(),
                    body: "1. Inspect the archive manifest\n2. Unpack archive into the selected workspace\n3. Verify extracted files".into(),
                    execution_refs: facts.iter().map(|fact| fact.observation_id.clone()).collect(),
                    source_sensitivity: ProceduralSourceSensitivity::NonPrivate, external_content: false,
                }], execution_facts: facts,
            }],
            human_confirmation_operation_id: None,
        },
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn project(
    runtime: &MemoryRuntime,
    binding: ProceduralProjectionBindingV1,
    budget: usize,
) -> MemoryProjectionOutput {
    runtime
        .project(MemoryProjectionRequest {
            binding,
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "unpack archive".into(),
            system_max_len: budget,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![registry().registry_ref()],
        })
        .unwrap()
}

fn complete(runtime: Arc<TestRuntime>, expected_job: &str) {
    let report = complete_job(runtime, expected_job);
    assert_eq!(report.receipt.accepted_count, 1);
    assert!(report.receipt.changed_count > 0);
}
fn complete_counts(runtime: Arc<TestRuntime>, expected_job: &str, accepted: u32, changed: u32) {
    let report = complete_job(runtime, expected_job);
    assert_eq!(report.receipt.accepted_count, accepted);
    assert_eq!(report.receipt.changed_count, changed);
}
fn complete_job(
    runtime: Arc<TestRuntime>,
    expected_job: &str,
) -> MemoryProceduralLearningRunReport {
    let engine = MemoryLearningEngine::attach(runtime.memory.clone()).unwrap();
    for _ in 0..16 {
        match engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "receipt-worker".into(),
                    lease_duration_secs: 60,
                },
                &mut NoProvider,
            )
            .unwrap()
        {
            MemoryLearningCycleOutcome::ProceduralCompleted(report) => {
                assert_eq!(report.job.job_id, expected_job);
                let snapshot = runtime.store.export_replay_snapshot().unwrap();
                let ledger: bm_core::memory::ProceduralFeedbackApplicationLedgerV2 =
                    serde_json::from_value(
                        snapshot
                            .json_docs
                            .iter()
                            .find(|doc| {
                                doc.namespace == "procedural_feedback_application_ledgers"
                                    && doc.value["job_id"] == expected_job
                            })
                            .unwrap()
                            .value
                            .clone(),
                    )
                    .unwrap();
                assert_eq!(
                    report.receipt.changed_count as usize,
                    ledger.applied_owner_bindings.len(),
                    "receipt must describe the exact real application"
                );
                return report;
            }
            MemoryLearningCycleOutcome::Blocked(report) => {
                assert_eq!(report.reason, "governance_execution_binding_unavailable")
            }
            other => panic!("expected real procedural completion: {other:?}"),
        }
    }
    panic!("bounded official worker did not complete");
}

fn empty_feedback_request(
    runtime: &MemoryRuntime,
    turn: &str,
    receipt: ProceduralSelectionReceiptV1,
) -> MemoryTurnFinalizeRequest {
    let mut input = request(runtime, turn, Some(receipt));
    input.turn.tool_observations.clear();
    input.learning.tool_call_count = 0;
    input.learning.agent_tool_feedback.clear();
    input
}

fn seed_skill(runtime: &MemoryRuntime) {
    runtime.seed_runtime_skills_for_replay(vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
        name: "runtime_skill__unpack_archive".into(), topic: "unpack archive".into(), title: "Unpack archive safely".into(),
        summary: "Unpack archive with validation".into(),
        content: "1. Inspect the archive manifest\n2. Unpack archive into the selected workspace\n3. Verify extracted files".into(),
        citations: vec!["synthetic governed fixture".into()], source_chat_id: Some("receipt-chat".into()),
        observed_at: runtime.config().clock.now_secs(),
    })], RuntimeSkillOwningScope::Subject { mounted_subject_id: runtime.subject_id().into() }).unwrap();
}

fn current_skill(runtime: &MemoryRuntime, retired: bool) -> RuntimeSkillSummary {
    let report = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: RuntimeSkillOwningScope::Subject {
                mounted_subject_id: runtime.subject_id().into(),
            },
            query: None,
            include_disabled: true,
            include_retired: retired,
            limit: 20,
        })
        .unwrap();
    assert_eq!(report.skills.len(), 1);
    report.skills.into_iter().next().unwrap()
}

#[test]
fn in_memory_runtime_skill_transport_is_explicitly_unavailable_without_false_selection() {
    assert_unavailable_runtime_skill_transport(memory_store());
}

fn assert_unavailable_runtime_skill_transport(store: MemoryStoreHandle) {
    let runtime = runtime(store, clock(), "agent-a");
    seed_skill(&runtime);
    let _positive_persisted_owner = current_skill(&runtime, false);
    assert_eq!(
        runtime
            .capabilities()
            .governed_state
            .runtime_skill_recall_transport,
        RuntimeSkillRecallTransport::Unavailable
    );
    let output = project(
        &runtime,
        ProceduralProjectionBindingV1::Turn {
            turn_id: "unavailable-skill".into(),
        },
        16_384,
    );
    assert!(output
        .report()
        .selection_receipt()
        .is_none_or(|receipt| receipt.runtime_skills.is_empty()));
}

#[cfg(feature = "sqlite-store")]
fn assert_runtime_skill_usage_lifecycle(config: Option<StoreBackendConfig>) {
    let mut store = config
        .as_ref()
        .map(|config| support::open_memory_store(config.clone()).unwrap())
        .unwrap_or_else(memory_store);
    let time = clock();
    let mut runtime = Arc::new(runtime(store.clone(), time.clone(), "agent-a"));
    seed_skill(&runtime);
    let original = current_skill(&runtime, false);
    let original_body = runtime
        .get_runtime_skill(RuntimeSkillDetailRequest {
            locator: original.locator.clone(),
        })
        .unwrap();
    for (turn, outcome) in [
        ("skill-success", ProceduralExecutionOutcomeV1::Succeeded),
        ("skill-failed", ProceduralExecutionOutcomeV1::Failed),
        ("skill-mismatch", ProceduralExecutionOutcomeV1::Mismatch),
    ] {
        time.0.fetch_add(2, Ordering::SeqCst);
        let output = project(
            &runtime,
            ProceduralProjectionBindingV1::Turn {
                turn_id: turn.into(),
            },
            16_384,
        );
        let receipt = output.report().selection_receipt().unwrap().clone();
        assert_eq!(
            receipt.runtime_skills.len(),
            1,
            "actual delivered runtime skill must be selected; safe delivery reports: {:?}",
            output.report().procedural_delivery_reports()
        );
        let selected = receipt.runtime_skills[0].clone();
        let mut input = empty_feedback_request(&runtime, turn, receipt);
        input
            .learning
            .runtime_skill_feedback
            .push(RuntimeSkillUsageFeedbackV1 {
                locator: selected.locator,
                selected_content_digest: selected.content_digest,
                outcome,
                observation_ref: format!("execution-{turn}"),
            });
        let report = runtime.submit(input).unwrap();
        complete_counts(
            runtime.clone(),
            &report.procedural_learning.job_id.unwrap(),
            1,
            1,
        );
    }
    if let Some(config) = config {
        drop(runtime);
        drop(store);
        store = support::open_memory_store(config).unwrap();
        runtime = Arc::new(self::runtime(store.clone(), time.clone(), "agent-a"));
    }
    let latest = current_skill(&runtime, false);
    assert_eq!(
        latest.locator.owner_revision(),
        original.locator.owner_revision() + 3
    );
    assert_eq!(latest.validated_success_count, 1);
    assert_eq!(
        latest.mismatch_count, 1,
        "infrastructure Failed is not method Mismatch"
    );
    let detail = runtime
        .get_runtime_skill(RuntimeSkillDetailRequest {
            locator: latest.locator,
        })
        .unwrap();
    assert_eq!(detail.procedure_text, original_body.procedure_text);
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().into(),
            channel_id: "sdk.direct".into(),
            conversation_id: "receipt-chat".into(),
            turn_id: Some("skill-success".into()),
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "synthetic privacy withdrawal".into(),
        })
        .unwrap();
    let withdrawn = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: RuntimeSkillOwningScope::Subject {
                mounted_subject_id: runtime.subject_id().into(),
            },
            query: None,
            include_disabled: true,
            include_retired: true,
            limit: 20,
        })
        .unwrap();
    assert!(
        withdrawn.skills.is_empty(),
        "withdrawal must close the public body path immediately"
    );
    assert_eq!(
        withdrawn.runtime_skills, None,
        "unavailable is not a fabricated zero count"
    );
    let denied = project(
        &runtime,
        ProceduralProjectionBindingV1::Turn {
            turn_id: "skill-after-delete".into(),
        },
        16_384,
    );
    assert!(denied
        .report()
        .selection_receipt()
        .is_none_or(|receipt| receipt.runtime_skills.is_empty()));
    let snapshot = store.export_replay_snapshot().unwrap();
    assert!(snapshot.json_docs.iter().any(|doc| doc.namespace
        == "procedural_feedback_application_ledgers"
        && doc.value["applied_owner_bindings"]
            .as_array()
            .is_some_and(|bindings| bindings
                .iter()
                .any(|binding| binding["kind"] == "runtime_skill"))));
}

#[cfg(feature = "sqlite-store")]
#[test]
fn inferred_runtime_skill_usage_is_rejected_before_intake() {
    let path = root("inferred-runtime");
    std::fs::create_dir_all(&path).unwrap();
    let store = support::open_memory_store(
        StoreBackendConfig::sqlite(path.join("store.sqlite3"), support::host_test_profile())
            .unwrap(),
    )
    .unwrap();
    let time = clock();
    let runtime = Arc::new(runtime(store, time, "agent-a"));
    seed_skill(&runtime);
    let before = current_skill(&runtime, false);
    let output = project(
        &runtime,
        ProceduralProjectionBindingV1::Turn {
            turn_id: "inferred-skill".into(),
        },
        16_384,
    );
    let receipt = output.report().selection_receipt().unwrap().clone();
    let selected = receipt
        .runtime_skills
        .first()
        .expect("nonempty positive selection")
        .clone();
    let mut input = empty_feedback_request(&runtime, "inferred-skill", receipt);
    let model_capability = runtime.model_capability();
    input
        .learning
        .runtime_skill_feedback
        .push(RuntimeSkillUsageFeedbackV1 {
            locator: selected.locator,
            selected_content_digest: selected.content_digest,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: "model-inferred-execution".into(),
        });
    let snapshot_before = runtime.store.export_replay_snapshot().unwrap();
    let rejected = runtime
        .finalize_turn_with_procedural_evidence(&model_capability, input)
        .err()
        .expect("a model cannot witness RuntimeSkill usage, even with a real selection");
    let Error::Other { source, .. } = rejected else {
        panic!("typed authority rejection required");
    };
    let safe = source.downcast_ref::<ProceduralLearningSdkError>().unwrap();
    assert_eq!(
        safe.key,
        ProceduralLearningErrorKeyV1::ProducerAuthorityDenied
    );
    assert_eq!(
        safe.disposition,
        ProceduralLearningSdkErrorDisposition::AuthorityRejected
    );
    assert_eq!(
        runtime.store.export_replay_snapshot().unwrap(),
        snapshot_before
    );
    assert_eq!(current_skill(&runtime, false), before);
    drop(runtime);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn inferred_tool_execution_is_rejected_but_model_method_provenance_is_preserved() {
    let store = memory_store();
    let runtime = Arc::new(runtime(store.clone(), clock(), "agent-a"));
    let model = runtime.model_capability();
    let before = store.export_replay_snapshot().unwrap();
    let invalid = runtime
        .finalize_turn_with_procedural_evidence(&model, request(&runtime, "model-execution", None))
        .err()
        .expect("model cannot witness execution");
    let Error::Other { source, .. } = invalid else {
        panic!("typed rejection required")
    };
    assert_eq!(
        source
            .downcast_ref::<ProceduralLearningSdkError>()
            .unwrap()
            .key,
        ProceduralLearningErrorKeyV1::ProducerAuthorityDenied
    );
    assert_eq!(store.export_replay_snapshot().unwrap(), before);
    let mut proposal = request(&runtime, "model-method", None);
    proposal.turn.tool_observations.clear();
    proposal.learning.tool_call_count = 0;
    proposal.learning.agent_tool_feedback[0]
        .execution_facts
        .clear();
    proposal.learning.agent_tool_feedback[0].method_evidence[0]
        .execution_refs
        .clear();
    let result = runtime
        .finalize_turn_with_procedural_evidence(&model, proposal)
        .unwrap();
    complete(runtime.clone(), &result.procedural_learning.job_id.unwrap());
    let snapshot = store.export_replay_snapshot().unwrap();
    let evidence: PostTurnLearningEvidenceV2 = serde_json::from_value(
        snapshot
            .json_docs
            .iter()
            .find(|doc| {
                doc.namespace == "conversation_transcript" && doc.value["turn_id"] == "model-method"
            })
            .unwrap()
            .value["learning_evidence"]
            .clone(),
    )
    .unwrap();
    assert!(evidence.authority.is_model_inferred());
    assert!(!evidence.authority.is_human_confirmation());
    assert!(evidence.agent_tool_feedback[0].execution_facts.is_empty());
    assert!(!evidence.agent_tool_feedback[0].method_evidence.is_empty());
    assert!(
        project(&runtime, ProceduralProjectionBindingV1::Preview, 16_384)
            .provider_payload()
            .agent_tool_hints()
            .is_empty()
    );
}

#[cfg(feature = "sqlite-store")]
#[test]
fn shared_program_usage_records_evidence_without_subject_owner_mutation() {
    let path = root("shared-runtime");
    std::fs::create_dir_all(&path).unwrap();
    let store = support::open_memory_store(
        StoreBackendConfig::sqlite(path.join("store.sqlite3"), support::host_test_profile())
            .unwrap(),
    )
    .unwrap();
    let runtime = Arc::new(runtime(store.clone(), clock(), "agent-a"));
    let mut seed = support::governed_runtime_skill_write(RuntimeSkillWrite {
        name: "runtime_skill__shared_archive".into(),
        topic: "unpack archive".into(),
        title: "Shared archive procedure".into(),
        summary: "Unpack archive safely".into(),
        content: "1. Inspect archive manifest\n2. Unpack archive\n3. Verify extracted files".into(),
        citations: vec!["synthetic governed program procedure".into()],
        source_chat_id: None,
        observed_at: 1_800_000_000,
    });
    seed.privacy_class = MemoryPrivacyClass::PublicRuntime;
    runtime
        .seed_runtime_skills_for_replay(vec![seed], RuntimeSkillOwningScope::SharedProgram)
        .unwrap();
    let before = store
        .export_replay_snapshot()
        .unwrap()
        .json_docs
        .into_iter()
        .filter(|doc| {
            matches!(
                doc.namespace.as_str(),
                "runtime_skill_records" | "runtime_skill_scope_manifests"
            )
        })
        .collect::<Vec<_>>();
    let output = project(
        &runtime,
        ProceduralProjectionBindingV1::Turn {
            turn_id: "shared-use".into(),
        },
        16_384,
    );
    let receipt = output.report().selection_receipt().unwrap().clone();
    assert!(
        !receipt.runtime_skills.is_empty(),
        "public shared procedure must actually be delivered: {:?}",
        output.report().procedural_delivery_reports()
    );
    let selected = receipt.runtime_skills[0].clone();
    assert_eq!(
        selected.locator.owning_scope(),
        &RuntimeSkillOwningScope::SharedProgram
    );
    let mut input = empty_feedback_request(&runtime, "shared-use", receipt);
    input
        .learning
        .runtime_skill_feedback
        .push(RuntimeSkillUsageFeedbackV1 {
            locator: selected.locator,
            selected_content_digest: selected.content_digest,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: "shared-execution".into(),
        });
    let result = runtime.submit(input).unwrap();
    complete_counts(
        runtime.clone(),
        &result.procedural_learning.job_id.unwrap(),
        1,
        0,
    );
    let after = store
        .export_replay_snapshot()
        .unwrap()
        .json_docs
        .into_iter()
        .filter(|doc| {
            matches!(
                doc.namespace.as_str(),
                "runtime_skill_records" | "runtime_skill_scope_manifests"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        after, before,
        "subject feedback cannot advance or privatize the shared program owner"
    );
    drop(runtime);
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}

#[cfg(feature = "sqlite-store")]
#[test]
fn mixed_subject_and_shared_runtime_feedback_mutates_only_the_subject_owner() {
    let path = root("mixed-runtime");
    std::fs::create_dir_all(&path).unwrap();
    let store = support::open_memory_store(
        StoreBackendConfig::sqlite(path.join("store.sqlite3"), support::host_test_profile())
            .unwrap(),
    )
    .unwrap();
    let runtime = Arc::new(runtime(store.clone(), clock(), "agent-a"));
    seed_skill(&runtime);
    let before_subject = current_skill(&runtime, false);
    let mut shared = support::governed_runtime_skill_write(RuntimeSkillWrite {
        name: "runtime_skill__shared_archive".into(),
        topic: "unpack archive".into(),
        title: "Shared archive procedure".into(),
        summary: "Unpack archive safely".into(),
        content: "1. Inspect archive manifest\n2. Unpack archive\n3. Verify extracted files".into(),
        citations: vec!["synthetic shared procedure".into()],
        source_chat_id: None,
        observed_at: 1_800_000_000,
    });
    shared.privacy_class = MemoryPrivacyClass::PublicRuntime;
    runtime
        .seed_runtime_skills_for_replay(vec![shared], RuntimeSkillOwningScope::SharedProgram)
        .unwrap();
    let before_shared = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: RuntimeSkillOwningScope::SharedProgram,
            query: None,
            include_disabled: true,
            include_retired: true,
            limit: 20,
        })
        .unwrap();
    assert_eq!(before_shared.skills.len(), 1);
    let output = project(
        &runtime,
        ProceduralProjectionBindingV1::Turn {
            turn_id: "mixed-use".into(),
        },
        16_384,
    );
    let receipt = output.report().selection_receipt().unwrap().clone();
    assert_eq!(
        receipt.runtime_skills.len(),
        2,
        "both independent owners must actually be delivered: {:?}",
        output.report().procedural_delivery_reports()
    );
    assert!(receipt.runtime_skills.iter().any(
        |selection| selection.locator.owning_scope() == &RuntimeSkillOwningScope::SharedProgram
    ));
    assert!(receipt.runtime_skills.iter().any(|selection| matches!(
        selection.locator.owning_scope(),
        RuntimeSkillOwningScope::Subject { .. }
    )));
    let selections = receipt.runtime_skills.clone();
    let mut input = empty_feedback_request(&runtime, "mixed-use", receipt);
    input.learning.runtime_skill_feedback = selections
        .into_iter()
        .enumerate()
        .map(|(index, selected)| RuntimeSkillUsageFeedbackV1 {
            locator: selected.locator,
            selected_content_digest: selected.content_digest,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: format!("mixed-execution-{index}"),
        })
        .collect();
    let result = runtime.submit(input).unwrap();
    complete_counts(
        runtime.clone(),
        &result.procedural_learning.job_id.unwrap(),
        2,
        1,
    );
    assert_eq!(
        current_skill(&runtime, false).locator.owner_revision(),
        before_subject.locator.owner_revision() + 1
    );
    let after_shared = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: RuntimeSkillOwningScope::SharedProgram,
            query: None,
            include_disabled: true,
            include_retired: true,
            limit: 20,
        })
        .unwrap();
    assert_eq!(after_shared, before_shared);
    drop(runtime);
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn human_confirmation_uses_registry_actor_and_note_cannot_substitute() {
    let store = memory_store();
    let time = clock();
    let agent = runtime(store.clone(), time.clone(), "agent-a");
    let mut unauthorized = request(&agent, "false-human", None);
    unauthorized.learning.human_confirmation_operation_id = Some("confirmation-false".into());
    let before = store.export_replay_snapshot().unwrap();
    assert!(agent.submit(unauthorized).is_err());
    assert_eq!(before, store.export_replay_snapshot().unwrap());
    let mut scoped = agent.scoped_runtime().clone();
    let human_id = primary_human_subject_id("receipt-owner");
    scoped.actor_subject_id = human_id.clone();
    let memory = MemoryRuntime::builder()
        .identity(agent.identity().clone())
        .scope(agent.scope().clone())
        .subject_registry(agent.subject_registry().clone())
        .scoped_runtime(scoped)
        .store(store.clone())
        .clock(time)
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .agent_tool_registry(registry())
        .build()
        .unwrap();
    let human = Arc::new(TestRuntime::new(
        memory,
        store.clone(),
        "human-confirmation",
        ProceduralProducerSourceAuthorityV1::HumanUser {
            subject_id: human_id.clone(),
        },
    ));
    let single = |runtime: &MemoryRuntime, id: &str| {
        let mut input = request(runtime, id, None);
        input.turn.tool_observations.truncate(1);
        input.learning.tool_call_count = 1;
        input.learning.agent_tool_feedback[0]
            .execution_facts
            .truncate(1);
        input.learning.agent_tool_feedback[0].method_evidence[0]
            .execution_refs
            .truncate(1);
        input
    };
    let mut input = single(&human, "real-human");
    input.learning.human_confirmation_operation_id = Some("confirmation-real".into());
    let result = human.submit(input).unwrap();
    complete(human.clone(), &result.procedural_learning.job_id.unwrap());
    let snapshot = store.export_replay_snapshot().unwrap();
    let typed: PostTurnLearningEvidenceV2 = serde_json::from_value(
        snapshot
            .json_docs
            .iter()
            .find(|doc| {
                doc.namespace == "conversation_transcript" && doc.value["turn_id"] == "real-human"
            })
            .unwrap()
            .value["learning_evidence"]
            .clone(),
    )
    .unwrap();
    assert!(typed.validate_contract());
    assert!(
        matches!(typed.authority, ProceduralFeedbackAuthorityV2::Producer {
        source_authority: ProceduralProducerSourceAuthorityV1::HumanUser { subject_id },
        confirmation: Some(ProceduralHumanConfirmationV1 { actor_subject_id, operation_id, .. }), ..
    } if subject_id == human_id && actor_subject_id == human_id && operation_id == "confirmation-real")
    );
    let note = single(&agent, "note-only");
    let mut wire = serde_json::to_value(&note.learning).unwrap();
    wire["agent_tool_feedback"][0]["operator_note"] = "User confirmed this should count".into();
    assert!(serde_json::from_value::<PostTurnLearningInputV2>(wire).is_err());
    let result = agent.submit(note).unwrap();
    complete(Arc::new(agent), &result.procedural_learning.job_id.unwrap());
    let snapshot = store.export_replay_snapshot().unwrap();
    let ordinary: PostTurnLearningEvidenceV2 = serde_json::from_value(
        snapshot
            .json_docs
            .iter()
            .find(|doc| {
                doc.namespace == "conversation_transcript" && doc.value["turn_id"] == "note-only"
            })
            .unwrap()
            .value["learning_evidence"]
            .clone(),
    )
    .unwrap();
    assert!(!ordinary.authority.is_human_confirmation());
}

#[test]
fn standard_package_and_task_evidence_are_consumed_without_rewriting_their_owners() {
    use bm_core::task_execution::{
        TaskLearningKind, TaskLearningRecord, TaskLearningRoute, TaskPlan, TaskRun, TaskRunKind,
        TaskRunRecord, TaskRunStatus,
    };
    let root = root("standard-task");
    std::fs::create_dir_all(&root).unwrap();
    let skill_file = root.join("SKILL.md");
    let skill_text = "---\nname: unpack-archive\ndescription: Unpack archive and verify files.\n---\n# Unpack archive\n1. Inspect archive manifest\n2. Unpack archive\n3. Verify extracted files\n";
    std::fs::write(&skill_file, skill_text).unwrap();
    let store = memory_store();
    let time = clock();
    let runtime = Arc::new(TestRuntime::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("agent-a", "receipt-owner").unwrap())
            .scope(MemoryScope::new("sdk.direct", "receipt-chat").unwrap())
            .store(store.clone())
            .clock(time)
            .capability_policy(MemoryCapabilityPolicy::strict_profile())
            .add_agent_skill_dir(AgentSkillDirConfig::read_only(&root, "receipt-pack"))
            .agent_tool_registry(registry())
            .build()
            .unwrap(),
        store.clone(),
        "package-executor",
        ProceduralProducerSourceAuthorityV1::RuntimeObservation,
    ));
    for (id, status) in [
        ("previous-archive-run", TaskRunStatus::Completed),
        ("current-archive-run", TaskRunStatus::Running),
    ] {
        runtime
            .replay_harness()
            .task_run_store()
            .upsert(&TaskRunRecord {
                run: TaskRun {
                    run_id: id.into(),
                    kind: TaskRunKind,
                    source_channel: "sdk.direct".into(),
                    source_chat_id: "receipt-chat".into(),
                    user_request: "unpack archive".into(),
                    title: "unpack archive".into(),
                    status,
                    current_step_id: String::new(),
                    planner_reason: String::new(),
                    final_summary: "archive verified".into(),
                    failure_reason: String::new(),
                    plan_revision: 1,
                    created_at: 1_799_999_990,
                    updated_at: 1_800_000_000,
                    finished_at: if status == TaskRunStatus::Completed {
                        1_800_000_000
                    } else {
                        0
                    },
                },
                plan: TaskPlan {
                    goal: "unpack archive".into(),
                    completion_definition: "verify extracted files".into(),
                    risk_notes: vec![],
                    ordered_steps: vec![],
                },
            })
            .unwrap();
    }
    let learning = TaskLearningRecord {
        learning_id: "archive-learning".into(),
        source_channel: "sdk.direct".into(),
        source_chat_id: "receipt-chat".into(),
        run_id: "previous-archive-run".into(),
        step_id: String::new(),
        kind: TaskLearningKind::ReusableProcedure,
        route: TaskLearningRoute::RuntimeSkill,
        run_status: TaskRunStatus::Completed,
        topic: "unpack archive".into(),
        summary: "Unpack archive after validating the manifest".into(),
        content: "Inspect archive manifest, unpack archive, verify extracted files.".into(),
        memory_kind: None,
        review_summary: "Synthetic execution verified".into(),
        source_artifact_ids: vec![],
        provenance: "synthetic typed task evidence".into(),
        archive_note_name: String::new(),
        route_detail: "existing governed procedure evidence".into(),
        candidate_state: None,
        candidate_state_updated_at: 0,
        last_failure_reason: String::new(),
        observed_at: 1_800_000_000,
    };
    runtime
        .replay_harness()
        .task_learning_store()
        .upsert(&learning)
        .unwrap();
    let output = project(
        &runtime,
        ProceduralProjectionBindingV1::Turn {
            turn_id: "package-task-use".into(),
        },
        16_384,
    );
    let receipt = output.report().selection_receipt().unwrap().clone();
    let package = receipt
        .standard_agent_skills
        .first()
        .expect("nonempty Standard package delivery")
        .clone();
    let task = receipt
        .task_learnings
        .first()
        .expect("nonempty Task delivery")
        .clone();
    assert_eq!(
        task.learning_digest,
        learning.canonical_content_digest().unwrap()
    );
    let mut input = empty_feedback_request(&runtime, "package-task-use", receipt);
    input
        .learning
        .agent_skill_feedback
        .push(AgentSkillUsageFeedbackV1 {
            package_id: package.package_id,
            package_fingerprint: package.package_fingerprint,
            package_binding: package.package_binding,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: "package-use".into(),
        });
    input
        .learning
        .task_learning_feedback
        .push(TaskLearningUsageFeedbackV1 {
            learning_id: task.learning_id,
            learning_digest: task.learning_digest,
            outcome: ProceduralExecutionOutcomeV1::Mismatch,
        });
    let result = runtime.submit(input).unwrap();
    complete_counts(
        runtime.clone(),
        &result.procedural_learning.job_id.unwrap(),
        2,
        0,
    );
    assert_eq!(std::fs::read_to_string(skill_file).unwrap(), skill_text);
    assert_eq!(
        runtime
            .replay_harness()
            .task_learning_store()
            .get("archive-learning")
            .unwrap()
            .unwrap(),
        learning,
        "reuse Mismatch cannot rewrite the original Task review or source outcome"
    );
    assert!(
        runtime
            .list_runtime_skills(RuntimeSkillListRequest {
                owning_scope: RuntimeSkillOwningScope::Subject {
                    mounted_subject_id: runtime.subject_id().into()
                },
                query: None,
                include_disabled: true,
                include_retired: true,
                limit: 20,
            })
            .unwrap()
            .skills
            .is_empty(),
        "package usage cannot copy host skill content into a new runtime owner"
    );
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}

fn seed(store: MemoryStoreHandle, clock: Arc<Clock>) {
    let runtime = Arc::new(runtime(store, clock, "agent-a"));
    let result = runtime.submit(request(&runtime, "seed", None)).unwrap();
    complete(runtime, &result.procedural_learning.job_id.unwrap());
}

fn clock() -> Arc<Clock> {
    Arc::new(Clock(AtomicU64::new(1_800_000_000)))
}
fn memory_store() -> MemoryStoreHandle {
    support::open_memory_store(StoreBackendConfig::in_memory(support::host_test_profile()).unwrap())
        .unwrap()
}

#[test]
fn production_receipt_binds_final_delivery_and_survives_runtime_reopen() {
    let store = memory_store();
    let time = clock();
    seed(store.clone(), time.clone());
    let receipt = {
        let runtime = runtime(store.clone(), time.clone(), "agent-a");
        let output = project(
            &runtime,
            ProceduralProjectionBindingV1::Turn {
                turn_id: "use".into(),
            },
            16_384,
        );
        assert!(!output.provider_payload().agent_tool_hints().is_empty());
        let receipt = output.report().selection_receipt().unwrap().clone();
        assert_eq!(
            receipt.agent_tool_experiences.len(),
            output.provider_payload().agent_tool_hints().len()
        );
        receipt
    };
    let runtime = Arc::new(runtime(store.clone(), time.clone(), "agent-a"));
    let input = request(&runtime, "use", Some(receipt.clone()));
    let report = runtime.submit(input.clone()).unwrap();
    let job = report.procedural_learning.job_id.unwrap();
    complete(runtime.clone(), &job);
    let before = store.export_replay_snapshot().unwrap();
    time.0.fetch_add(86_401, Ordering::SeqCst);
    let replay = runtime.submit(input).unwrap();
    assert_eq!(
        replay.procedural_learning.job_id.as_deref(),
        Some(job.as_str())
    );
    let after = store.export_replay_snapshot().unwrap();
    assert_eq!(before.json_docs, after.json_docs);
    assert_eq!(before.events, after.events);
    let transcript = after
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "conversation_transcript" && doc.value["turn_id"] == "use")
        .unwrap();
    assert_eq!(
        transcript.value["learning_evidence"]["selection_receipt"],
        serde_json::to_value(receipt).unwrap()
    );
}

#[test]
fn preview_history_cross_subject_and_budget_drops_cannot_mint_false_exposure() {
    let store = memory_store();
    let time = clock();
    seed(store.clone(), time.clone());
    let a = runtime(store.clone(), time.clone(), "agent-a");
    let preview = project(&a, ProceduralProjectionBindingV1::Preview, 16_384);
    assert!(!preview.provider_payload().agent_tool_hints().is_empty());
    assert!(preview.report().selection_receipt().is_none());
    let historical = a.project(MemoryProjectionRequest {
        binding: ProceduralProjectionBindingV1::Turn {
            turn_id: "history-use".into(),
        },
        temporal_operation: MemoryRecallTemporalOperation::HistoricalAsOf {
            as_of_time: 1_800_000_000,
        },
        user_query: "unpack archive".into(),
        system_max_len: 16_384,
        recent_messages_limit: 4,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
        structured_query_facets: vec![],
        tool_registry_refs: vec![registry().registry_ref()],
    });
    assert!(historical.is_err());
    let b = runtime(store, time, "agent-b");
    let denied = project(
        &b,
        ProceduralProjectionBindingV1::Turn {
            turn_id: "other".into(),
        },
        16_384,
    );
    assert!(denied.provider_payload().agent_tool_hints().is_empty());
    assert!(denied
        .report()
        .selection_receipt()
        .is_none_or(|receipt| receipt.agent_tool_experiences.is_empty()));
    let tiny = project(
        &a,
        ProceduralProjectionBindingV1::Turn {
            turn_id: "tiny".into(),
        },
        128,
    );
    assert!(tiny.provider_payload().agent_tool_hints().is_empty());
    assert!(tiny
        .report()
        .selection_receipt()
        .is_none_or(|receipt| receipt.agent_tool_experiences.is_empty()));
}

#[test]
fn forged_scope_tamper_and_expired_first_intake_are_atomic_zero_write() {
    let store = memory_store();
    let time = clock();
    seed(store.clone(), time.clone());
    let a = runtime(store.clone(), time.clone(), "agent-a");
    let output = project(
        &a,
        ProceduralProjectionBindingV1::Turn {
            turn_id: "guard".into(),
        },
        16_384,
    );
    let receipt = output.report().selection_receipt().unwrap().clone();
    assert!(!receipt.agent_tool_experiences.is_empty());
    let b = runtime(store.clone(), time.clone(), "agent-b");
    let before = store.export_replay_snapshot().unwrap();
    let mut altered = receipt.clone();
    altered.delivery_digest = format!("sha256:{}", "f".repeat(64));
    let mut scoped = receipt.clone();
    scoped.identity.chat_id = "other-chat".into();
    for forged in [altered, scoped] {
        assert!(a.submit(request(&a, "guard", Some(forged))).is_err());
    }
    assert!(b
        .submit(request(&b, "guard", Some(receipt.clone())))
        .is_err());
    time.0.store(receipt.expires_at, Ordering::SeqCst);
    assert!(a.submit(request(&a, "guard", Some(receipt))).is_err());
    let after = store.export_replay_snapshot().unwrap();
    assert_eq!(before.json_docs, after.json_docs);
    assert_eq!(before.events, after.events);
}

#[test]
fn esp_real_runtime_rejects_every_feedback_class_before_store_but_accepts_empty_turn() {
    let desktop_store = memory_store();
    let time = clock();
    seed(desktop_store.clone(), time.clone());
    let desktop = runtime(desktop_store.clone(), time.clone(), "agent-a");
    seed_skill(&desktop);
    let skill = current_skill(&desktop, false);
    let snapshot = desktop_store.export_replay_snapshot().unwrap();
    let skill_digest = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "runtime_skill_records")
        .expect("real persisted runtime owner")
        .value["content_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let output = project(
        &desktop,
        ProceduralProjectionBindingV1::Turn {
            turn_id: "esp-denied".into(),
        },
        16_384,
    );
    let receipt = output
        .report()
        .selection_receipt()
        .expect("real desktop-issued receipt")
        .clone();
    assert!(
        !receipt.agent_tool_experiences.is_empty(),
        "real tool delivery signs the desktop receipt"
    );
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(ProfileId::EspEmbeddedSdk).unwrap(),
    )
    .unwrap();
    let esp = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-a", "receipt-owner").unwrap())
        .scope(MemoryScope::new("sdk.direct", "receipt-chat").unwrap())
        .store(store.clone())
        .clock(time)
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .build()
        .unwrap();
    assert!(
        !esp.capabilities()
            .procedural_learning
            .finalize_feedback
            .visible
    );
    let mut variants = Vec::new();
    let mut receipt_only = empty_feedback_request(&esp, "esp-denied", receipt.clone());
    variants.push(receipt_only.clone());
    receipt_only.learning.selection_receipt = None;
    let mut runtime_feedback = receipt_only.clone();
    runtime_feedback
        .learning
        .runtime_skill_feedback
        .push(RuntimeSkillUsageFeedbackV1 {
            locator: skill.locator,
            selected_content_digest: skill_digest,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: "synthetic-runtime-use".into(),
        });
    variants.push(runtime_feedback);
    let mut package_feedback = receipt_only.clone();
    package_feedback
        .learning
        .agent_skill_feedback
        .push(AgentSkillUsageFeedbackV1 {
            package_id: "synthetic-package".into(),
            package_fingerprint: "synthetic-fingerprint".into(),
            package_binding: format!("sha256:{}", "a".repeat(64)),
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: "synthetic-package-use".into(),
        });
    variants.push(package_feedback);
    let mut task_feedback = receipt_only.clone();
    task_feedback
        .learning
        .task_learning_feedback
        .push(TaskLearningUsageFeedbackV1 {
            learning_id: "synthetic-task-learning".into(),
            learning_digest: format!("sha256:{}", "b".repeat(64)),
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
        });
    variants.push(task_feedback);
    variants.push(request(&esp, "esp-denied", None));
    let before = store.export_replay_snapshot().unwrap();
    for input in variants {
        let result = esp.finalize_turn(input);
        let Err(Error::Other { source, .. }) = result else {
            panic!("unsupported feedback needs its typed capability error");
        };
        let typed = source
            .downcast_ref::<ProceduralLearningSdkError>()
            .expect("typed SDK error");
        assert_eq!(
            typed.key,
            ProceduralLearningErrorKeyV1::CapabilityUnavailable
        );
        let after = store.export_replay_snapshot().unwrap();
        assert_eq!(after.json_docs, before.json_docs);
        assert_eq!(after.events, before.events);
    }
    let report = esp
        .finalize_turn(receipt_only)
        .expect("empty single-agent finalize remains available on ESP");
    assert!(report
        .transcript_commit
        .is_some_and(|report| report.committed));
}

fn persistent_reopen(config: StoreBackendConfig) {
    let time = clock();
    let receipt = {
        let store = support::open_memory_store(config.clone()).unwrap();
        seed(store.clone(), time.clone());
        let runtime = runtime(store, time.clone(), "agent-a");
        project(
            &runtime,
            ProceduralProjectionBindingV1::Turn {
                turn_id: "reopen-use".into(),
            },
            16_384,
        )
        .report()
        .selection_receipt()
        .unwrap()
        .clone()
    };
    {
        let store = support::open_memory_store(config.clone()).unwrap();
        let runtime = Arc::new(runtime(store, time.clone(), "agent-a"));
        let result = runtime
            .submit(request(&runtime, "reopen-use", Some(receipt.clone())))
            .unwrap();
        complete(runtime, &result.procedural_learning.job_id.unwrap());
    }
    let store = support::open_memory_store(config).unwrap();
    let snapshot = store.export_replay_snapshot().unwrap();
    let transcript = snapshot
        .json_docs
        .iter()
        .find(|doc| {
            doc.namespace == "conversation_transcript" && doc.value["turn_id"] == "reopen-use"
        })
        .unwrap();
    assert_eq!(
        transcript.value["learning_evidence"]["selection_receipt"],
        serde_json::to_value(receipt).unwrap()
    );
}

fn root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "bm-procedural-receipt-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn file_reopen_keeps_unexpired_selection_receipt_authority() {
    let root = root("file");
    persistent_reopen(StoreBackendConfig::file(&root, support::host_test_profile()).unwrap());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn file_runtime_skill_transport_is_explicitly_unavailable_without_false_selection() {
    let root = root("runtime-file");
    assert_unavailable_runtime_skill_transport(
        support::open_memory_store(
            StoreBackendConfig::file(&root, support::host_test_profile()).unwrap(),
        )
        .unwrap(),
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_keeps_runtime_skill_historical_application_proof_after_owner_advances() {
    let root = root("runtime-sqlite");
    std::fs::create_dir_all(&root).unwrap();
    assert_runtime_skill_usage_lifecycle(Some(
        StoreBackendConfig::sqlite(root.join("store.sqlite3"), support::host_test_profile())
            .unwrap(),
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_keeps_unexpired_selection_receipt_authority() {
    let root = root("sqlite");
    std::fs::create_dir_all(&root).unwrap();
    persistent_reopen(
        StoreBackendConfig::sqlite(root.join("store.sqlite3"), support::host_test_profile())
            .unwrap(),
    );
    std::fs::remove_dir_all(root).unwrap();
}
