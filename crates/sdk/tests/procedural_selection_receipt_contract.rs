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

fn runtime(store: MemoryStoreHandle, clock: Arc<Clock>, agent: &str) -> MemoryRuntime {
    let mut subjects = SubjectRegistry::single_agent_default("receipt-owner", "agent-a").unwrap();
    subjects
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "Agent B",
        ))
        .unwrap();
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent, "receipt-owner").unwrap())
        .scope(MemoryScope::new("sdk.direct", "receipt-chat").unwrap())
        .subject_registry(subjects)
        .store(store)
        .clock(clock)
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .agent_tool_registry(registry())
        .build()
        .unwrap()
}

fn request(
    runtime: &MemoryRuntime,
    turn: &str,
    receipt: Option<ProceduralSelectionReceiptV1>,
) -> MemoryTurnFinalizeRequest {
    let now = runtime.config().clock.now_secs();
    let observations = ["a", "b"]
        .into_iter()
        .map(|suffix| AgentToolObservationDigest {
            observation_id: format!("observation-{turn}-{suffix}"),
            registry_id: "receipt-tools".into(),
            tool_id: "archive.unpack".into(),
            schema_fingerprint: "schema-archive-v1".into(),
            call_id: Some(format!("call-{turn}-{suffix}")),
            task_signature: "unpack_archive".into(),
            summary: "archive unpack completed successfully".into(),
            outcome: AgentToolOutcome::Succeeded,
            error_code: None,
            external_content: false,
            private_content_used: false,
            permission_tags: vec![],
            risk_tags: vec![],
            started_at: Some(now),
            completed_at: Some(now),
        })
        .collect::<Vec<_>>();
    let canonical = observations
        .iter()
        .map(|observation| ToolObservationDigest {
            observation_id: observation.observation_id.clone(),
            tool_name: observation.tool_id.clone(),
            summary: observation.summary.clone(),
            external_content: observation.external_content,
        })
        .collect();
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
        learning: PostTurnLearningInputV1 {
            tool_call_count: 2,
            selection_receipt: receipt,
            runtime_skill_feedback: vec![],
            agent_skill_feedback: vec![],
            task_learning_feedback: vec![],
            agent_tool_feedback: vec![AgentToolUsageFeedbackV2 {
                registry_ref: registry().registry_ref(),
                tool_id: "archive.unpack".into(),
                schema_fingerprint: "schema-archive-v1".into(),
                observations,
                outcome: ProceduralExecutionOutcomeV1::Succeeded,
                user_visible_result_summary: Some("archive unpack completed successfully".into()),
                operator_note: None,
            }],
            authority: ProceduralFeedbackAuthorityInputV1::HostRuntimeObservation,
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

fn complete(runtime: Arc<MemoryRuntime>, expected_job: &str) {
    complete_counts(runtime, expected_job, 1, 1);
}

fn complete_counts(runtime: Arc<MemoryRuntime>, expected_job: &str, accepted: u32, changed: u32) {
    let engine = MemoryLearningEngine::attach(runtime).unwrap();
    let outcome = engine
        .run_due_cycle(
            MemoryLearningCycleRequest {
                lease_owner: "receipt-worker".into(),
                lease_duration_secs: 60,
            },
            &mut NoProvider,
        )
        .unwrap();
    let MemoryLearningCycleOutcome::ProceduralCompleted(report) = outcome else {
        panic!("the official engine must complete the procedural job; safe state: {outcome:?}");
    };
    assert_eq!(report.job.job_id, expected_job);
    assert_eq!(report.receipt.accepted_count, accepted);
    assert_eq!(report.receipt.changed_count, changed);
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
        let report = runtime.finalize_turn(input).unwrap();
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
    assert!(!current_skill(&runtime, true).enabled);
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
fn inferred_success_is_not_validated_runtime_skill_success() {
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
    input.learning.authority = ProceduralFeedbackAuthorityInputV1::ModelInferred;
    input
        .learning
        .runtime_skill_feedback
        .push(RuntimeSkillUsageFeedbackV1 {
            locator: selected.locator,
            selected_content_digest: selected.content_digest,
            outcome: ProceduralExecutionOutcomeV1::Succeeded,
            observation_ref: "model-inferred-execution".into(),
        });
    let result = runtime.finalize_turn(input).unwrap();
    complete_counts(
        runtime.clone(),
        &result.procedural_learning.job_id.unwrap(),
        1,
        0,
    );
    assert_eq!(current_skill(&runtime, false), before);
    drop(runtime);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn inferred_tool_success_is_durable_evidence_without_promoting_experience() {
    let store = memory_store();
    let runtime = Arc::new(runtime(store.clone(), clock(), "agent-a"));
    let mut input = request(&runtime, "model-tool", None);
    input.learning.authority = ProceduralFeedbackAuthorityInputV1::ModelInferred;
    let result = runtime.finalize_turn(input).unwrap();
    complete_counts(
        runtime.clone(),
        &result.procedural_learning.job_id.unwrap(),
        1,
        0,
    );
    let snapshot = store.export_replay_snapshot().unwrap();
    let evidence = &snapshot
        .json_docs
        .iter()
        .find(|doc| {
            doc.namespace == "conversation_transcript" && doc.value["turn_id"] == "model-tool"
        })
        .unwrap()
        .value["learning_evidence"];
    assert_eq!(evidence["authority"]["kind"], "model_inferred");
    assert!(!evidence["agent_tool_feedback"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "agent_tool_experience_owner_heads"));
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
    let result = runtime.finalize_turn(input).unwrap();
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
    let result = runtime.finalize_turn(input).unwrap();
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
    unauthorized.learning.authority = ProceduralFeedbackAuthorityInputV1::HumanConfirmed {
        operation_id: "confirmation-false".into(),
    };
    let before = store.export_replay_snapshot().unwrap();
    assert!(agent.finalize_turn(unauthorized).is_err());
    assert_eq!(
        before.json_docs,
        store.export_replay_snapshot().unwrap().json_docs
    );
    let mut scoped = agent.scoped_runtime().clone();
    scoped.actor_subject_id = primary_human_subject_id("receipt-owner");
    let human = Arc::new(
        MemoryRuntime::builder()
            .identity(agent.identity().clone())
            .scope(agent.scope().clone())
            .subject_registry(agent.subject_registry().clone())
            .scoped_runtime(scoped)
            .store(store.clone())
            .clock(time)
            .capability_policy(MemoryCapabilityPolicy::strict_profile())
            .agent_tool_registry(registry())
            .build()
            .unwrap(),
    );
    let mut input = request(&human, "real-human", None);
    input.turn.tool_observations.truncate(1);
    input.learning.tool_call_count = 1;
    input.learning.agent_tool_feedback[0]
        .observations
        .truncate(1);
    input.learning.authority = ProceduralFeedbackAuthorityInputV1::HumanConfirmed {
        operation_id: "confirmation-real".into(),
    };
    let result = human.finalize_turn(input).unwrap();
    complete_counts(
        human.clone(),
        &result.procedural_learning.job_id.unwrap(),
        1,
        1,
    );
    let snapshot = store.export_replay_snapshot().unwrap();
    let evidence = &snapshot
        .json_docs
        .iter()
        .find(|doc| {
            doc.namespace == "conversation_transcript" && doc.value["turn_id"] == "real-human"
        })
        .unwrap()
        .value["learning_evidence"];
    let typed: PostTurnLearningEvidenceV1 = serde_json::from_value(evidence.clone()).unwrap();
    assert!(typed.validate_contract());
    assert!(matches!(
        typed.authority,
        ProceduralFeedbackAuthorityV1::HumanConfirmed { .. }
    ));
    let mut note = request(&agent, "note-only", None);
    note.turn.tool_observations.truncate(1);
    note.learning.tool_call_count = 1;
    note.learning.agent_tool_feedback[0]
        .observations
        .truncate(1);
    note.learning.agent_tool_feedback[0].operator_note =
        Some("User confirmed this should count".into());
    let result = agent.finalize_turn(note).unwrap();
    complete_counts(
        Arc::new(agent),
        &result.procedural_learning.job_id.unwrap(),
        0,
        0,
    );
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
    let runtime = Arc::new(
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
    );
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
    let result = runtime.finalize_turn(input).unwrap();
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
    let result = runtime
        .finalize_turn(request(&runtime, "seed", None))
        .unwrap();
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
    let report = runtime.finalize_turn(input.clone()).unwrap();
    let job = report.procedural_learning.job_id.unwrap();
    complete(runtime.clone(), &job);
    let before = store.export_replay_snapshot().unwrap();
    time.0.fetch_add(86_401, Ordering::SeqCst);
    let replay = runtime.finalize_turn(input).unwrap();
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
        assert!(a.finalize_turn(request(&a, "guard", Some(forged))).is_err());
    }
    assert!(b
        .finalize_turn(request(&b, "guard", Some(receipt.clone())))
        .is_err());
    time.0.store(receipt.expires_at, Ordering::SeqCst);
    assert!(a
        .finalize_turn(request(&a, "guard", Some(receipt)))
        .is_err());
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
            .finalize_turn(request(&runtime, "reopen-use", Some(receipt.clone())))
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
