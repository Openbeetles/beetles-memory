#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use bm_core::memory::{
    canonical_evidence_ref_from_source, governed_memory_recall_candidate_id,
    memory_facet_manifest_key, primary_human_subject_id, scoped_memory_facet_owner_storage_key,
    CanonicalEntityKey, CanonicalEntityKind, CanonicalEntityRef, GovernedMemoryOwnerPlane,
    GovernedMemoryOwnerRef, LongTermMemoryControlRevision, LongTermMemoryHeadManifest,
    LongTermMemorySlot, LongTermMemoryTombstone, LongTermMemoryVersionMaterial,
    MemoryFacetIndexManifest, QueryFacetInput, LONG_TERM_CONTROL_REVISION_NAMESPACE,
    LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE, MEMORY_FACET_POSTING_NAMESPACE,
};
use bm_core::platform::Platform as _;
use bm_core::task_execution::{
    TaskLearningKind, TaskLearningRecord, TaskLearningRoute, TaskPlan, TaskRun, TaskRunKind,
    TaskRunRecord, TaskRunStatus,
};
use bm_sdk::{
    default_agent_subject_id, AgentToolDescriptor, AgentToolObservationDigest, AgentToolOutcome,
    AgentToolRegistrySnapshot, AgentToolUsageFeedbackV2, CanonicalTurnDelta, ConversationScope,
    EvidenceBacklink, IngressKind, LongTermMemoryDraft, LongTermMemoryKind,
    LongTermMemoryProvenance, LongTermMemoryQuery, LongTermMemorySourceScope,
    MemoryCandidateContent, MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment,
    MemoryCandidateTarget, MemoryClock, MemoryEvidenceAuthority, MemoryGovernancePolicyMutation,
    MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration, MemoryGraphEdge,
    MemoryGraphEdgeKind, MemoryGraphNode, MemoryGraphNodeKind, MemoryIdentity,
    MemoryLongTermControlView, MemoryLongTermListRequest, MemoryLongTermMutation,
    MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest, MemoryLongTermTarget,
    MemoryMaintenanceRequest, MemoryMutationExecution, MemoryPrivacyClass, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryScope, MemorySemanticJudgmentSource, MemoryStoreHandle,
    MemorySubjectVisibilityPolicy, MemoryTranscriptLifecycleRequest, MemoryTurnDeliveryStatus,
    MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource, MemoryWriteCandidate,
    MemoryWriteRequest, ParsedLongTermMemoryExtraction, PostTurnLearningInputV1, PressureLevel,
    ProceduralExecutionOutcomeV1, RuntimeLifecycleModeInput, RuntimeSkillWrite, StoreBackendConfig,
    StoreRuntimeBudget, SubjectDescriptor, SubjectRegistry, TemporalMemoryGraphNodeOwnerRef,
    TemporalMemoryGraphWriteRequest, TemporalValidity, ToolObservationDigest,
    TranscriptInputMessage, TranscriptLifecycleTransition,
};

use support::{empty_store_platform, test_runtime_with_scope, StaticHttpClient, StaticLlmClient};

struct AdjustableTransactionClock {
    now_secs: AtomicU64,
}

impl AdjustableTransactionClock {
    fn new(now_secs: u64) -> Self {
        Self {
            now_secs: AtomicU64::new(now_secs),
        }
    }
}

impl MemoryClock for AdjustableTransactionClock {
    fn now_secs(&self) -> u64 {
        self.now_secs.load(Ordering::SeqCst)
    }
}

struct AdvancingTransactionClock(AtomicU64);

impl MemoryClock for AdvancingTransactionClock {
    fn now_secs(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

#[test]
fn operation_write_uses_one_commit_timestamp_when_runtime_clock_advances() {
    let (platform, runtime) = runtime_with_registry_event_budget_and_clock(
        registry(),
        256,
        Arc::new(AdvancingTransactionClock(AtomicU64::new(1_800_000_000))),
    );
    let committed = runtime
        .write_operation(
            "advancing-clock-write",
            long_term_write_request("advancing_clock"),
        )
        .expect("clock advancement cannot split one transaction timestamp");
    let MemoryMutationExecution::Committed { report, receipt } = committed else {
        panic!("first write commits")
    };
    assert_eq!(report.changed, 1);
    let events = platform.replay_harness().read_events().unwrap();
    let transaction_events = events
        .iter()
        .filter(|event| event.payload.get("transaction_id") == Some(&receipt.transaction_id))
        .collect::<Vec<_>>();
    assert!(!transaction_events.is_empty());
    assert!(transaction_events
        .iter()
        .all(|event| event.timestamp_unix_secs == receipt.committed_at_unix_secs));
    assert!(matches!(
        runtime
            .write_operation(
                "advancing-clock-write",
                long_term_write_request("advancing_clock")
            )
            .unwrap(),
        MemoryMutationExecution::Replayed { .. }
    ));
}

#[test]
fn maintenance_long_term_write_keeps_owner_and_facet_in_one_governed_path() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    runtime
        .replay_harness()
        .task_run_store()
        .upsert(&TaskRunRecord {
            run: TaskRun {
                run_id: "maintenance-run".to_string(),
                kind: TaskRunKind,
                source_channel: "llm.gateway".to_string(),
                source_chat_id: "chat-a".to_string(),
                user_request: "record durable release fact".to_string(),
                title: "release fact".to_string(),
                status: TaskRunStatus::Completed,
                current_step_id: String::new(),
                planner_reason: String::new(),
                final_summary: "The release requires a verified artifact manifest.".to_string(),
                failure_reason: String::new(),
                plan_revision: 1,
                created_at: 1_799_999_900,
                updated_at: 1_800_000_000,
                finished_at: 1_800_000_000,
            },
            plan: TaskPlan {
                goal: "record release fact".to_string(),
                completion_definition: "fact is governed".to_string(),
                risk_notes: Vec::new(),
                ordered_steps: Vec::new(),
            },
        })
        .expect("seed terminal run");
    runtime
        .replay_harness()
        .task_learning_store()
        .upsert(&TaskLearningRecord {
            learning_id: "maintenance-fact".to_string(),
            source_channel: "llm.gateway".to_string(),
            source_chat_id: "chat-a".to_string(),
            run_id: "maintenance-run".to_string(),
            step_id: String::new(),
            kind: TaskLearningKind::DurableFact,
            route: TaskLearningRoute::Pending,
            run_status: TaskRunStatus::Completed,
            topic: "release_artifact_manifest".to_string(),
            summary: "Release artifact manifests must be verified.".to_string(),
            content: "The release requires a verified artifact manifest before publishing."
                .to_string(),
            memory_kind: Some(LongTermMemoryKind::Fact),
            review_summary: String::new(),
            source_artifact_ids: Vec::new(),
            provenance: "task-learning-contract".to_string(),
            archive_note_name: String::new(),
            route_detail: String::new(),
            candidate_state: None,
            candidate_state_updated_at: 0,
            last_failure_reason: String::new(),
            observed_at: 1_800_000_000,
        })
        .expect("seed pending durable fact");
    let seeded = platform
        .export_replay_snapshot()
        .expect("snapshot seeded task learning");
    assert!(
        seeded
            .json_docs
            .iter()
            .any(|doc| doc.namespace == "task_learning_by_chat_indexes"),
        "task learning seed must include its typed chat index"
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response("maintenance summary");

    let report = runtime
        .maintain(
            &mut http,
            &llm,
            MemoryMaintenanceRequest {
                ingress: IngressKind::User,
                user_content: "record the durable release fact".to_string(),
                reply_content: "recorded".to_string(),
                tool_calls: 0,
                external_content_used: false,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .expect("maintenance");
    let maintenance_detail = report
        .report
        .as_ref()
        .map(|outcome| format!("{:?}", outcome.task_learning_outcome));
    let transaction = report
        .transaction
        .as_ref()
        .unwrap_or_else(|| panic!("maintenance transaction missing: {maintenance_detail:?}"));
    assert_eq!(transaction.operation, "maintain");
    assert_eq!(
        transaction.planned_mutations,
        transaction.committed_mutations
    );
    assert!(!transaction.partial_write);
    assert_transaction_events(
        &platform,
        &transaction.transaction_id,
        "maintain",
        transaction.event_ids.len(),
    );
    let maintenance = report.report.as_ref().expect("maintenance outcome");
    assert_eq!(
        maintenance
            .task_learning_outcome
            .as_ref()
            .expect("task learning maintenance")
            .canonical_writes,
        1
    );
    assert_eq!(
        runtime
            .replay_harness()
            .memory_space_long_term_memory_read_store("space:owner-default")
            .expect("scoped owner store")
            .count()
            .expect("owner count"),
        1
    );
    let manifest_key = memory_facet_manifest_key("space:owner-default", runtime.memory_space_id())
        .expect("manifest key");
    assert_eq!(
        platform
            .replay_harness()
            .read_json_docs_by_keys(
                MEMORY_FACET_POSTING_NAMESPACE,
                std::slice::from_ref(&manifest_key),
            )
            .expect("manifest read")
            .len(),
        1
    );
}

fn transaction_budget(event_log_max_items: usize, kv_max_entries: usize) -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        metric_source_max_items: 1,
        event_log_max_items,
        kv_max_entries,
        blob_max_bytes: 4096,
        snapshot_max_bytes: 131_072,
        logical_namespace_max_bytes: 128,
        logical_key_max_bytes: 1024,
        event_record_key_max_bytes: 1024,
        export_max_bytes: 131_072,
        import_max_bytes: 131_072,
    }
}

fn store_with_event_budget(event_log_max_items: usize) -> MemoryStoreHandle {
    store_with_transaction_budget(event_log_max_items, 256)
}

fn store_with_transaction_budget(
    event_log_max_items: usize,
    kv_max_entries: usize,
) -> MemoryStoreHandle {
    let config = StoreBackendConfig::in_memory(support::host_test_profile())
        .expect("store config")
        .try_with_nonproduction_store_budget_limit(transaction_budget(
            event_log_max_items,
            kv_max_entries,
        ))
        .expect("transaction budget must be a valid semantic contraction");
    support::open_memory_store(config).expect("store platform")
}

fn runtime_with_registry_and_event_budget(
    registry: AgentToolRegistrySnapshot,
    event_log_max_items: usize,
) -> (MemoryStoreHandle, bm_sdk::MemoryRuntime) {
    let platform = store_with_event_budget(event_log_max_items);
    let runtime = bm_sdk::MemoryRuntime::builder()
        .identity(MemoryIdentity::new("transaction-agent", "owner-default").expect("identity"))
        .scope(MemoryScope::new("llm.gateway", "chat-a").expect("scope"))
        .store(platform.clone())
        .agent_tool_registry(registry)
        .build()
        .expect("runtime");
    (platform, runtime)
}

fn runtime_with_registry_event_budget_and_clock(
    registry: AgentToolRegistrySnapshot,
    event_log_max_items: usize,
    clock: Arc<dyn MemoryClock>,
) -> (MemoryStoreHandle, bm_sdk::MemoryRuntime) {
    let platform = store_with_event_budget(event_log_max_items);
    let runtime = bm_sdk::MemoryRuntime::builder()
        .identity(MemoryIdentity::new("transaction-agent", "owner-default").expect("identity"))
        .scope(MemoryScope::new("llm.gateway", "chat-a").expect("scope"))
        .store(platform.clone())
        .agent_tool_registry(registry)
        .clock(clock)
        .build()
        .expect("runtime");
    (platform, runtime)
}

fn llm_accept(target: MemoryCandidateTarget) -> MemoryCandidateSemanticJudgment {
    MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision: MemoryCandidateSemanticDecision::Accept,
        governed_target: Some(target),
        reason: "llm_semantic_judgment".to_string(),
    }
}

fn llm_reject() -> MemoryCandidateSemanticJudgment {
    MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision: MemoryCandidateSemanticDecision::Reject,
        governed_target: None,
        reason: "llm_rejected".to_string(),
    }
}

fn long_term_candidate() -> MemoryWriteCandidate {
    MemoryWriteCandidate {
        candidate_id: "candidate-transaction-profile".to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Profile,
            topic: "transaction_profile".to_string(),
        },
        long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "transaction_profile".to_string(),
            body: "The user expects memory writes to be atomic.".to_string(),
            keywords: vec!["transaction".to_string(), "atomic".to_string()],
        },
        evidence_refs: vec!["chat-a:turn-1".to_string()],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(llm_accept(MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Profile,
            topic: "transaction_profile".to_string(),
        })),
    }
}

fn secondary_long_term_candidate() -> MemoryWriteCandidate {
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Project,
        topic: "transaction_project".to_string(),
    };
    MemoryWriteCandidate {
        candidate_id: "candidate-transaction-project".to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: target.clone(),
        long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "transaction_project".to_string(),
            body: "The project requires preflight before committing its memory batch.".to_string(),
            keywords: vec!["transaction".to_string(), "preflight".to_string()],
        },
        evidence_refs: vec!["chat-a:turn-2".to_string()],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(llm_accept(target)),
    }
}

fn typed_entity_candidate(
    body: &str,
    key: CanonicalEntityKey,
    alias: &str,
    source_ref: &str,
) -> MemoryWriteCandidate {
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Project,
        topic: "typed_entity_merge".to_string(),
    };
    MemoryWriteCandidate {
        candidate_id: format!("candidate-{source_ref}"),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: target.clone(),
        long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "typed_entity_merge".to_string(),
            body: body.to_string(),
            keywords: vec!["typed".to_string(), "entity".to_string()],
        },
        evidence_refs: vec![source_ref.to_string()],
        canonical_entities: vec![CanonicalEntityRef {
            key,
            display_label: None,
            aliases: vec![alias.to_string()],
            evidence_refs: vec![
                canonical_evidence_ref_from_source(source_ref).expect("canonical evidence")
            ],
        }],
        semantic_judgment: Some(llm_accept(target)),
    }
}

fn manual_runtime_skill_write(name: &str) -> RuntimeSkillWrite {
    RuntimeSkillWrite {
        name: name.to_string(),
        topic: "transaction_skill".to_string(),
        title: "transaction skill".to_string(),
        summary: "Memory write transaction contract.".to_string(),
        content: "- plan first\n- commit once\n- reject whole batch on admission failure"
            .to_string(),
        citations: vec!["fixture:transaction-contract".to_string()],
        source_chat_id: Some("chat-a".to_string()),
        observed_at: 1_800_000_000,
    }
}

fn extraction_draft() -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind: LongTermMemoryKind::Profile,
        privacy: bm_sdk::MemoryPrivacyClass::SharedWithSubject,
        topic: "transaction_extraction".to_string(),
        content: "Long-term extraction must commit atomically with refs and lifecycle.".to_string(),
        keywords: vec!["transaction".to_string(), "extraction".to_string()],
        source_chat_id: Some("chat-a".to_string()),
        source_type: None,
        source_scope: None,
        subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
        provenance: LongTermMemoryProvenance::new(MemoryEvidenceAuthority::UserAsserted),
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: vec!["fixture:long-term-extraction".to_string()],
        canonical_entities: Vec::new(),
        evidence_count: Some(1),
        observed_at: Some(1_800_000_000),
        source_revision: Some(1),
    }
}

fn secondary_extraction_draft() -> LongTermMemoryDraft {
    let mut draft = extraction_draft();
    draft.kind = LongTermMemoryKind::Project;
    draft.topic = "transaction_extraction_project".to_string();
    draft.content = "A second extraction owner must share the same atomic transaction.".to_string();
    draft.keywords = vec!["transaction".to_string(), "project".to_string()];
    draft.supporting_citations = vec!["fixture:long-term-extraction-project".to_string()];
    draft.source_revision = Some(2);
    draft
}

fn long_term_write_request(topic: &str) -> MemoryWriteRequest {
    let mut draft = extraction_draft();
    draft.topic = topic.to_string();
    draft.content = format!("Durable operation intent for {topic}.");
    MemoryWriteRequest::LongTermExtraction {
        extraction: ParsedLongTermMemoryExtraction {
            upserts: vec![draft],
            deletes: Vec::new(),
            skill_writes: Vec::new(),
        },
    }
}

fn registry() -> AgentToolRegistrySnapshot {
    let mut tool = AgentToolDescriptor::compact("pdf.extract", "Extract PDF text", "schema-pdf-v1");
    tool.permission_tags = vec!["filesystem.read".to_string()];
    tool.risk_tags = vec!["external_content".to_string()];
    AgentToolRegistrySnapshot::compact("host-tools", "host", vec![tool], 1_800_000_000)
}

fn graph_node(id: &str, evidence_ref: &str) -> MemoryGraphNode {
    MemoryGraphNode {
        node_id: id.to_string(),
        kind: MemoryGraphNodeKind::Task,
        label: format!("Graph node {id}"),
        evidence_refs: vec![evidence_ref.to_string()],
    }
}

fn long_term_graph_owner(node_id: &str, owner_id: &str) -> TemporalMemoryGraphNodeOwnerRef {
    TemporalMemoryGraphNodeOwnerRef {
        node_id: node_id.to_string(),
        owner_ref: GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::LongTerm,
            owner_id.to_string(),
        ),
    }
}

fn graph_edge(id: &str, from: &str, to: &str, evidence_ref: &str) -> MemoryGraphEdge {
    MemoryGraphEdge {
        edge_id: id.to_string(),
        kind: MemoryGraphEdgeKind::Supports,
        from_node_id: from.to_string(),
        to_node_id: to.to_string(),
        validity: TemporalValidity {
            valid_from: 1_800_000_000,
            valid_until: None,
            observed_at: 1_800_000_000,
            superseded_by: None,
        },
        evidence_refs: vec![evidence_ref.to_string()],
    }
}

fn observation(observation_id: &str) -> AgentToolObservationDigest {
    AgentToolObservationDigest {
        observation_id: observation_id.to_string(),
        registry_id: "host-tools".to_string(),
        tool_id: "pdf.extract".to_string(),
        schema_fingerprint: "schema-pdf-v1".to_string(),
        call_id: Some(format!("call-{observation_id}")),
        task_signature: "extract_pdf_text_for_release_notes".to_string(),
        summary: "PDF extraction produced usable release note text.".to_string(),
        outcome: AgentToolOutcome::Succeeded,
        error_code: None,
        external_content: true,
        private_content_used: false,
        permission_tags: vec!["filesystem.read".to_string()],
        risk_tags: vec!["external_content".to_string()],
        started_at: Some(1_800_000_010),
        completed_at: Some(1_800_000_011),
    }
}

fn feedback(registry: &AgentToolRegistrySnapshot) -> AgentToolUsageFeedbackV2 {
    AgentToolUsageFeedbackV2 {
        registry_ref: registry.registry_ref(),
        tool_id: "pdf.extract".to_string(),
        schema_fingerprint: "schema-pdf-v1".to_string(),
        observations: vec![observation("obs-1"), observation("obs-2")],
        user_visible_result_summary: Some(
            "PDF extraction helped produce release notes from a local artifact.".to_string(),
        ),
        outcome: ProceduralExecutionOutcomeV1::Succeeded,
        operator_note: None,
    }
}

fn feedback_finalize_request(
    runtime: &bm_sdk::MemoryRuntime,
    turn_id: &str,
    feedback: AgentToolUsageFeedbackV2,
) -> MemoryTurnFinalizeRequest {
    let tool_call_count = feedback.observations.len() as u32;
    let tool_observations = feedback
        .observations
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
            turn_id: turn_id.to_string(),
            conversation: ConversationScope {
                channel: runtime.scope().channel.clone(),
                chat_id: runtime.scope().chat_id.clone(),
                conversation_id: Some(runtime.scope().conversation_id_or_chat_id().to_string()),
            },
            subject: runtime.subject_id().to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: IngressKind::User,
                channel: runtime.scope().channel.clone(),
                provider: None,
                protocol: MemoryTurnProtocol::Native,
                endpoint: None,
                model_alias: None,
                model_resolved: None,
                request_id: Some(format!("request-{turn_id}")),
                client_conversation_hint: None,
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user("synthetic tool execution")],
            assistant_message: Some(TranscriptInputMessage::assistant("synthetic result")),
            tool_observations,
            external_content_used: true,
            candidate_ids: Vec::new(),
        },
        learning: PostTurnLearningInputV1 {
            tool_call_count,
            selection_receipt: None,
            runtime_skill_feedback: Vec::new(),
            agent_skill_feedback: Vec::new(),
            task_learning_feedback: Vec::new(),
            agent_tool_feedback: vec![feedback],
            authority: bm_sdk::ProceduralFeedbackAuthorityInputV1::HostRuntimeObservation,
        },
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn assert_transaction_events(
    platform: &MemoryStoreHandle,
    transaction_id: &str,
    operation: &str,
    expected_count: usize,
) {
    let events = platform.replay_harness().read_events().expect("events");
    let transaction_events = events
        .iter()
        .filter(|event| {
            event.payload.get("transaction_id").map(String::as_str) == Some(transaction_id)
        })
        .collect::<Vec<_>>();
    assert_eq!(transaction_events.len(), expected_count);
    assert!(transaction_events
        .iter()
        .all(|event| { event.payload.get("operation").map(String::as_str) == Some(operation) }));
}

fn store_fingerprints(platform: &MemoryStoreHandle) -> (String, String) {
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("export store snapshot");
    (snapshot.state_fingerprint(), snapshot.event_fingerprint())
}

fn facet_index_docs(
    platform: &MemoryStoreHandle,
) -> Vec<bm_sdk::nonproduction_replay_harness::StoreSnapshotJsonDoc> {
    platform
        .replay_harness()
        .read_json_namespace("memory_facet_indexes")
        .expect("facet index namespace")
}

fn assert_facet_index_doc_for_owner(
    platform: &MemoryStoreHandle,
    owner_id: &str,
) -> serde_json::Value {
    let docs = facet_index_docs(platform);
    let doc = docs
        .iter()
        .find(|doc| doc.value["owner_ref"]["owner_id"] == owner_id)
        .unwrap_or_else(|| panic!("missing facet index for owner {owner_id}"));
    let memory_space_id = doc.value["memory_space_id"]
        .as_str()
        .expect("facet memory space");
    let subject_id = doc.value["subject_ids"]
        .as_array()
        .and_then(|subjects| subjects.first())
        .and_then(serde_json::Value::as_str)
        .expect("facet mounted subject");
    let owner_ref = GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner_id);
    assert_eq!(
        doc.key,
        scoped_memory_facet_owner_storage_key(memory_space_id, subject_id, &owner_ref)
            .expect("scoped facet owner key")
    );
    assert_eq!(doc.value["owner_ref"]["owner_plane"], "long_term");
    assert!(doc.value.get("owner_record_id").is_none());
    assert!(doc.value.get("owner_plane").is_none());
    assert_eq!(doc.value["status"], "active");
    assert!(doc.value["exact_facets"]
        .as_array()
        .is_some_and(|facets| !facets.is_empty()));
    doc.value.clone()
}

fn assert_no_facet_index_doc_for_owner(platform: &MemoryStoreHandle, owner_id: &str) {
    assert!(
        !facet_index_docs(platform)
            .iter()
            .any(|doc| doc.value["owner_ref"]["owner_id"] == owner_id),
        "facet index must be deleted with the owner record"
    );
}

fn assert_operation_events_include_planes(
    platform: &MemoryStoreHandle,
    operation: &str,
    expected_planes: &[&str],
) {
    let events = platform.replay_harness().read_events().expect("events");
    let operation_events = events
        .iter()
        .filter(|event| event.payload.get("operation").map(String::as_str) == Some(operation))
        .collect::<Vec<_>>();
    assert!(
        !operation_events.is_empty(),
        "missing events for {operation}"
    );
    let transaction_id = operation_events[0]
        .payload
        .get("transaction_id")
        .expect("transaction id");
    assert!(operation_events
        .iter()
        .all(|event| event.payload.get("transaction_id") == Some(transaction_id)));
    for expected_plane in expected_planes {
        assert!(
            operation_events
                .iter()
                .any(|event| event.plane == *expected_plane),
            "missing plane {expected_plane} for {operation}"
        );
    }
}

fn assert_transaction_events_include_planes(
    platform: &MemoryStoreHandle,
    transaction_id: &str,
    operation: &str,
    expected_planes: &[&str],
) {
    let events = platform.replay_harness().read_events().expect("events");
    let transaction_events = events
        .iter()
        .filter(|event| {
            event.payload.get("transaction_id").map(String::as_str) == Some(transaction_id)
        })
        .collect::<Vec<_>>();
    assert!(
        !transaction_events.is_empty(),
        "missing events for transaction {transaction_id}"
    );
    assert!(transaction_events
        .iter()
        .all(|event| { event.payload.get("operation").map(String::as_str) == Some(operation) }));
    for expected_plane in expected_planes {
        assert!(
            transaction_events
                .iter()
                .any(|event| event.plane == *expected_plane),
            "missing plane {expected_plane} for transaction {transaction_id}"
        );
    }
}

#[test]
fn candidate_write_event_budget_rejects_without_partial_memory() {
    let platform = store_with_event_budget(2);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let before_events = platform
        .replay_harness()
        .read_events()
        .expect("events before");
    let before_long_term_count = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .count()
        .expect("long-term before");
    let before_skill_names = platform
        .replay_harness()
        .skill_storage()
        .list_names()
        .expect("skills before");

    let err = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![long_term_candidate(), secondary_long_term_candidate()],
        })
        .expect_err("event budget should reject the whole memory write transaction");

    assert_eq!(err.stage(), "memory_write_transaction_preflight_failed");
    assert_eq!(
        runtime
            .replay_harness()
            .memory_space_long_term_memory_read_store("space:owner-default")
            .expect("scoped long-term store")
            .count()
            .unwrap(),
        before_long_term_count
    );
    assert_eq!(
        platform
            .replay_harness()
            .skill_storage()
            .list_names()
            .unwrap(),
        before_skill_names
    );
    assert_eq!(
        platform.replay_harness().read_events().unwrap(),
        before_events
    );
}

#[test]
fn candidate_write_success_reports_transaction_lineage() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![long_term_candidate(), secondary_long_term_candidate()],
        })
        .expect("candidate write");

    assert!(report.accepted);
    assert_eq!(report.changed, 2);
    let transaction = report.transaction.expect("transaction report");
    assert_eq!(transaction.operation, "write.candidates");
    assert_eq!(transaction.changed_count, report.changed);
    assert_eq!(
        transaction.planned_mutations,
        transaction.committed_mutations
    );
    assert!(!transaction.partial_write);
    assert!(!transaction.event_ids.is_empty());

    let events = platform.replay_harness().read_events().expect("events");
    let transaction_events = events
        .iter()
        .filter(|event| {
            event.payload.get("transaction_id").map(String::as_str)
                == Some(transaction.transaction_id.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(transaction_events.len(), transaction.event_ids.len());
    assert!(transaction_events.iter().all(|event| {
        event.payload.get("operation").map(String::as_str) == Some("write.candidates")
    }));

    let long_term_records = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list");
    let owner_id = long_term_records
        .iter()
        .find(|entry| entry.topic == "transaction_profile")
        .expect("accepted long-term entry")
        .id
        .clone();
    let facet_doc = assert_facet_index_doc_for_owner(&platform, &owner_id);
    assert_eq!(facet_doc["memory_space_id"], "space:owner-default");
    assert!(facet_doc["subject_ids"]
        .as_array()
        .is_some_and(|subjects| !subjects.is_empty()));
    assert!(facet_doc["owner_revision"].as_u64().unwrap_or(0) > 0);
    let project_owner_id = long_term_records
        .iter()
        .find(|entry| entry.topic == "transaction_project")
        .expect("accepted project entry")
        .id
        .clone();
    assert_facet_index_doc_for_owner(&platform, &project_owner_id);
}

#[test]
fn candidate_to_draft_to_entry_to_exact_entity_posting_to_typed_query_is_reachable() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let source_ref = "turn:candidate-typed-entity";
    let key = CanonicalEntityKey {
        kind: CanonicalEntityKind::Repository,
        canonical_id: "agent-memory".to_string(),
    };
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Project,
        topic: "typed_entity_candidate".to_string(),
    };
    let candidate = MemoryWriteCandidate {
        candidate_id: "candidate-typed-entity".to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: target.clone(),
        long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "typed_entity_candidate".to_string(),
            body: "The repository is governed by an exact canonical entity key.".to_string(),
            keywords: vec!["typed".to_string(), "entity".to_string()],
        },
        evidence_refs: vec![source_ref.to_string()],
        canonical_entities: vec![CanonicalEntityRef {
            key: key.clone(),
            display_label: Some("Agent Memory".to_string()),
            aliases: vec!["memory repo".to_string()],
            evidence_refs: vec![
                canonical_evidence_ref_from_source(source_ref).expect("canonical evidence")
            ],
        }],
        semantic_judgment: Some(llm_accept(target)),
    };

    runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![candidate],
        })
        .expect("typed entity candidate write");
    let entry = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "typed_entity_candidate")
        .expect("typed entity owner");
    assert_eq!(entry.canonical_entities[0].key, key);

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: "unrelated plain text".to_string(),
            limit: 8,
            structured_query_facets: vec![QueryFacetInput::Entity(key)],
            tool_registry_refs: Vec::new(),
        })
        .expect("typed entity recall");

    assert!(recall.facet_index_report.manifest_integrity_verified);
    assert_eq!(
        recall.facet_index_report.exact_facet_candidate_ids,
        vec![governed_memory_recall_candidate_id(
            &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, entry.id)
        )]
    );
}

#[test]
fn content_change_replaces_entities_and_removes_old_exact_posting() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let old_key = CanonicalEntityKey {
        kind: CanonicalEntityKind::Project,
        canonical_id: "old-project".to_string(),
    };
    let new_key = CanonicalEntityKey {
        kind: CanonicalEntityKind::Product,
        canonical_id: "new-product".to_string(),
    };
    runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![typed_entity_candidate(
                "The governed entity payload is version one.",
                old_key.clone(),
                "old alias",
                "turn:entity-replace-1",
            )],
        })
        .expect("seed old entity");
    runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![typed_entity_candidate(
                "The governed entity payload is version two.",
                new_key.clone(),
                "new alias",
                "turn:entity-replace-2",
            )],
        })
        .expect("replace entity payload");

    let old_recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: "unrelated".to_string(),
            limit: 8,
            structured_query_facets: vec![QueryFacetInput::Entity(old_key)],
            tool_registry_refs: Vec::new(),
        })
        .expect("old entity recall");
    assert!(old_recall
        .facet_index_report
        .exact_facet_candidate_ids
        .is_empty());

    let new_recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: "unrelated".to_string(),
            limit: 8,
            structured_query_facets: vec![QueryFacetInput::Entity(new_key.clone())],
            tool_registry_refs: Vec::new(),
        })
        .expect("new entity recall");
    assert_eq!(
        new_recall
            .facet_index_report
            .exact_facet_candidate_ids
            .len(),
        1
    );
    let owner = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "typed_entity_merge")
        .expect("replaced owner");
    assert_eq!(owner.canonical_entities.len(), 1);
    assert_eq!(owner.canonical_entities[0].key, new_key);
}

#[test]
fn same_content_candidate_reinforcement_unions_entity_aliases_and_evidence() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let key = CanonicalEntityKey {
        kind: CanonicalEntityKind::Repository,
        canonical_id: "agent-memory".to_string(),
    };
    let body = "The same governed entity payload is reinforced.";
    for candidate in [
        typed_entity_candidate(body, key.clone(), "memory repo", "turn:entity-union-1"),
        typed_entity_candidate(body, key.clone(), "bm repo", "turn:entity-union-2"),
    ] {
        runtime
            .write(MemoryWriteRequest::Candidates {
                candidates: vec![candidate],
            })
            .expect("reinforce entity payload");
    }

    let owner = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "typed_entity_merge")
        .expect("reinforced owner");
    assert_eq!(owner.owner_revision, 2);
    assert_eq!(owner.canonical_entities.len(), 1);
    assert_eq!(
        owner.canonical_entities[0].aliases,
        vec!["memory repo", "bm repo"]
    );
    assert_eq!(owner.canonical_entities[0].evidence_refs.len(), 2);
}

#[test]
fn rejected_candidate_does_not_write_recallable_facet_index() {
    let platform = store_with_event_budget(16);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let mut candidate = long_term_candidate();
    candidate.semantic_judgment = Some(llm_reject());

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![candidate],
        })
        .expect("rejected candidate write");

    assert!(!report.accepted);
    assert_eq!(report.changed, 0);
    assert_eq!(
        runtime
            .replay_harness()
            .memory_space_long_term_memory_read_store("space:owner-default")
            .expect("scoped long-term store")
            .count()
            .unwrap(),
        0,
        "rejected candidate must not write long-term owner records"
    );
    assert!(
        facet_index_docs(&platform).is_empty(),
        "rejected candidate must not write recallable facet index docs"
    );
}

#[test]
fn procedural_write_event_budget_rejects_without_partial_skill() {
    let platform = store_with_event_budget(2);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let before_events = platform
        .replay_harness()
        .read_events()
        .expect("events before");
    let before_skill_names = platform
        .replay_harness()
        .skill_storage()
        .list_names()
        .expect("skills before");

    let err = runtime
        .seed_runtime_skills_for_replay(
            vec![support::governed_runtime_skill_write(
                manual_runtime_skill_write("runtime_skill__transaction_manual"),
            )],
            support::runtime_skill_subject_scope(),
        )
        .expect_err("event budget should reject skill write and lifecycle together");

    assert_eq!(err.stage(), "memory_write_transaction_preflight_failed");
    assert_eq!(
        platform
            .replay_harness()
            .skill_storage()
            .list_names()
            .unwrap(),
        before_skill_names
    );
    assert_eq!(
        platform.replay_harness().read_events().unwrap(),
        before_events
    );
}

#[test]
fn procedural_write_success_reports_transaction_lineage() {
    let platform = store_with_event_budget(16);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );

    let report = runtime
        .seed_runtime_skills_for_replay(
            vec![support::governed_runtime_skill_write(
                manual_runtime_skill_write("runtime_skill__transaction_manual"),
            )],
            support::runtime_skill_subject_scope(),
        )
        .expect("procedural write");

    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let transaction = report.transaction.expect("transaction");
    assert_eq!(transaction.operation, "replay.seed_runtime_skills");
    assert_eq!(
        transaction.planned_mutations,
        transaction.committed_mutations
    );
    assert!(!transaction.partial_write);
    assert_transaction_events(
        &platform,
        &transaction.transaction_id,
        "replay.seed_runtime_skills",
        transaction.event_ids.len(),
    );
}

#[test]
fn durable_write_identity_binds_the_exact_scoped_actor_on_one_shared_store() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime_a = support::test_runtime_with_delegated_actor(
        platform.clone(),
        support::host_test_profile(),
        "agent-main",
        "agent:delegated-a",
        "chat-a",
    );
    let runtime_b = support::test_runtime_with_delegated_actor(
        platform,
        support::host_test_profile(),
        "agent-main",
        "agent:delegated-b",
        "chat-a",
    );
    let request = long_term_write_request;

    let first = runtime_a
        .write_operation("same-caller-operation-id", request("actor_bound_a"))
        .expect("actor A commit");
    let second = runtime_b
        .write_operation("same-caller-operation-id", request("actor_bound_b"))
        .expect("actor B commit must not collide with actor A");
    let (
        MemoryMutationExecution::Committed {
            receipt: first_receipt,
            ..
        },
        MemoryMutationExecution::Committed {
            receipt: second_receipt,
            ..
        },
    ) = (first, second)
    else {
        panic!("both actor-scoped operations must commit")
    };
    assert_ne!(
        first_receipt.identity.storage_key(),
        second_receipt.identity.storage_key()
    );
    assert_eq!(
        first_receipt.identity.actor_subject_id(),
        "agent:delegated-a"
    );
    assert_eq!(
        second_receipt.identity.actor_subject_id(),
        "agent:delegated-b"
    );
    assert!(matches!(
        runtime_a
            .write_operation("same-caller-operation-id", request("actor_bound_a"),)
            .expect("actor A replay"),
        MemoryMutationExecution::Replayed { .. }
    ));
    assert!(matches!(
        runtime_b
            .write_operation("same-caller-operation-id", request("actor_bound_b"),)
            .expect("actor B replay"),
        MemoryMutationExecution::Replayed { .. }
    ));
}

#[test]
fn operation_aware_long_term_write_replays_and_rejects_intent_collision() {
    let platform = store_with_event_budget(32);
    let runtime = test_runtime_with_scope(
        platform,
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let request = long_term_write_request;

    let first = runtime
        .write_operation("sdk-write-operation-1", request("operation_aware"))
        .expect("first operation write");
    let MemoryMutationExecution::Committed { report, receipt } = first else {
        panic!("first operation must commit")
    };
    assert_eq!(report.changed, 1);
    assert_eq!(receipt.changed_count, 1);

    let replay = runtime
        .write_operation("sdk-write-operation-1", request("operation_aware"))
        .expect("same operation replay");
    assert!(matches!(replay, MemoryMutationExecution::Replayed { .. }));

    let collision = runtime
        .write_operation("sdk-write-operation-1", request("different_intent"))
        .expect_err("same operation identity with another request must conflict");
    assert_eq!(collision.class(), Some(bm_sdk::ErrorClass::Conflict));

    let noop = runtime
        .write_operation(
            "sdk-write-operation-noop",
            MemoryWriteRequest::LongTermExtraction {
                extraction: ParsedLongTermMemoryExtraction {
                    upserts: Vec::new(),
                    deletes: Vec::new(),
                    skill_writes: Vec::new(),
                },
            },
        )
        .expect("zero-effect operation write");
    let MemoryMutationExecution::Committed { report, receipt } = noop else {
        panic!("zero-effect operation must still commit its receipt")
    };
    assert_eq!(report.changed, 0);
    assert_eq!(receipt.changed_count, 0);

    let zero_effect_collision = runtime
        .write_operation(
            "sdk-write-operation-noop",
            request("must_not_escape_noop_collision"),
        )
        .expect_err("zero-effect receipt must reserve the operation identity");
    assert_eq!(
        zero_effect_collision.class(),
        Some(bm_sdk::ErrorClass::Conflict)
    );
}

#[test]
fn operation_aware_long_term_extraction_commits_noop_replays_and_rejects_collisions() {
    let registry = registry();
    let clock = Arc::new(AdjustableTransactionClock::new(1_800_000_000));
    let (platform, runtime) =
        runtime_with_registry_event_budget_and_clock(registry.clone(), 256, clock.clone());

    let extraction_request = |topic: &str| {
        let mut draft = extraction_draft();
        draft.topic = topic.to_string();
        MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![draft],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        }
    };
    let extraction = runtime
        .write_operation(
            "sdk-write-extraction-changed",
            extraction_request("operation_extraction"),
        )
        .expect("operation-aware extraction");
    let MemoryMutationExecution::Committed { report, receipt } = extraction else {
        panic!("extraction must commit")
    };
    assert_eq!(report.changed, 1);
    assert_eq!(receipt.changed_count, 1);
    let before_replay = store_fingerprints(&platform);
    assert!(matches!(
        runtime
            .write_operation(
                "sdk-write-extraction-changed",
                extraction_request("operation_extraction")
            )
            .expect("extraction replay"),
        MemoryMutationExecution::Replayed { .. }
    ));
    assert_eq!(store_fingerprints(&platform), before_replay);
    let collision = runtime
        .write_operation(
            "sdk-write-extraction-changed",
            extraction_request("operation_extraction_collision"),
        )
        .expect_err("extraction intent collision");
    assert_eq!(collision.class(), Some(bm_sdk::ErrorClass::Conflict));
    let extraction_noop = runtime
        .write_operation(
            "sdk-write-extraction-noop",
            MemoryWriteRequest::LongTermExtraction {
                extraction: ParsedLongTermMemoryExtraction {
                    upserts: Vec::new(),
                    deletes: Vec::new(),
                    skill_writes: Vec::new(),
                },
            },
        )
        .expect("operation-aware extraction noop");
    let MemoryMutationExecution::Committed { report, receipt } = extraction_noop else {
        panic!("extraction noop must commit its receipt")
    };
    assert_eq!(report.changed, 0);
    assert_eq!(receipt.changed_count, 0);
}

#[test]
fn operation_aware_long_term_write_replays_after_file_store_reopen() {
    let root = std::env::temp_dir().join(format!(
        "bm-sdk-write-operation-reopen-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let request = || long_term_write_request("operation_file_reopen");

    {
        let platform = support::open_memory_store(
            StoreBackendConfig::file(&root, support::host_test_profile())
                .expect("file store config"),
        )
        .expect("open first file store");
        let runtime = test_runtime_with_scope(
            platform,
            support::host_test_profile(),
            "llm.gateway",
            "chat-a",
        );
        assert!(matches!(
            runtime
                .write_operation("sdk-write-operation-file", request())
                .expect("first durable operation"),
            MemoryMutationExecution::Committed { .. }
        ));
    }

    {
        let platform = support::open_memory_store(
            StoreBackendConfig::file(&root, support::host_test_profile())
                .expect("reopen file store config"),
        )
        .expect("reopen file store");
        let runtime = test_runtime_with_scope(
            platform,
            support::host_test_profile(),
            "llm.gateway",
            "chat-a",
        );
        assert!(matches!(
            runtime
                .write_operation("sdk-write-operation-file", request())
                .expect("durable replay"),
            MemoryMutationExecution::Replayed { .. }
        ));
    }

    std::fs::remove_dir_all(&root).expect("remove synthetic file store");
}

#[test]
fn replay_skill_event_budget_rejects_without_partial_skill() {
    let platform = store_with_event_budget(2);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let before_events = platform
        .replay_harness()
        .read_events()
        .expect("events before");
    let before_skill_names = platform
        .replay_harness()
        .skill_storage()
        .list_names()
        .expect("skills before");

    let err = runtime
        .seed_runtime_skills_for_replay(
            vec![support::governed_runtime_skill_write(
                manual_runtime_skill_write("runtime_skill__promotion_budget"),
            )],
            support::runtime_skill_subject_scope(),
        )
        .expect_err("event budget should reject promotion and lifecycle together");

    assert_eq!(err.stage(), "memory_write_transaction_preflight_failed");
    assert_eq!(
        platform
            .replay_harness()
            .skill_storage()
            .list_names()
            .unwrap(),
        before_skill_names
    );
    assert_eq!(
        platform.replay_harness().read_events().unwrap(),
        before_events
    );
}

#[test]
fn replay_skill_success_reports_transaction_lineage() {
    let platform = store_with_event_budget(16);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );

    let report = runtime
        .seed_runtime_skills_for_replay(
            vec![support::governed_runtime_skill_write(
                manual_runtime_skill_write("runtime_skill__promotion_success"),
            )],
            support::runtime_skill_subject_scope(),
        )
        .expect("promotion write");

    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let transaction = report.transaction.expect("transaction");
    assert_eq!(transaction.operation, "replay.seed_runtime_skills");
    assert!(!transaction.partial_write);
    assert_transaction_events(
        &platform,
        &transaction.transaction_id,
        "replay.seed_runtime_skills",
        transaction.event_ids.len(),
    );
}

#[test]
fn long_term_extraction_event_budget_rejects_without_partial_memory() {
    let platform = store_with_event_budget(2);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let before_events = platform
        .replay_harness()
        .read_events()
        .expect("events before");
    let before_long_term_count = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .count()
        .expect("long-term before");

    let err = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect_err("event budget should reject extraction and lifecycle together");

    assert_eq!(err.stage(), "memory_write_transaction_preflight_failed");
    assert_eq!(
        runtime
            .replay_harness()
            .memory_space_long_term_memory_read_store("space:owner-default")
            .expect("scoped long-term store")
            .count()
            .unwrap(),
        before_long_term_count
    );
    assert_eq!(
        platform.replay_harness().read_events().unwrap(),
        before_events
    );
}

#[test]
fn long_term_extraction_success_reports_transaction_lineage() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );

    let report = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft(), secondary_extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("extraction write");

    assert!(report.accepted);
    assert_eq!(report.changed, 2);
    let transaction = report.transaction.expect("transaction");
    assert_eq!(transaction.operation, "write.long_term_extraction");
    assert!(!transaction.partial_write);
    assert_transaction_events(
        &platform,
        &transaction.transaction_id,
        "write.long_term_extraction",
        transaction.event_ids.len(),
    );

    let long_term_records = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list");
    let owner_id = long_term_records
        .iter()
        .find(|entry| entry.topic == "transaction_extraction")
        .expect("accepted extraction entry")
        .id
        .clone();
    let facet_doc = assert_facet_index_doc_for_owner(&platform, &owner_id);
    assert_eq!(facet_doc["owner_revision"], 1);
    let project_owner_id = long_term_records
        .iter()
        .find(|entry| entry.topic == "transaction_extraction_project")
        .expect("accepted second extraction entry")
        .id
        .clone();
    assert_facet_index_doc_for_owner(&platform, &project_owner_id);
}

#[test]
fn long_term_extraction_delete_removes_facet_index_in_same_transaction() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed extraction");
    let owner_id = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "transaction_extraction")
        .expect("accepted extraction entry")
        .id;
    assert_facet_index_doc_for_owner(&platform, &owner_id);

    let report = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: Vec::new(),
                deletes: vec![LongTermMemorySlot {
                    kind: LongTermMemoryKind::Profile,
                    topic: "transaction_extraction".to_string(),
                }],
                skill_writes: Vec::new(),
            },
        })
        .expect("delete extraction");

    assert!(report.accepted);
    let transaction = report.transaction.expect("transaction");
    assert_eq!(transaction.operation, "write.long_term_extraction");
    assert!(runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&owner_id)
        .expect("long-term get")
        .is_none());
    assert_no_facet_index_doc_for_owner(&platform, &owner_id);
    let manifest_key =
        memory_facet_manifest_key(runtime.memory_space_id(), runtime.memory_space_id())
            .expect("facet manifest key");
    assert!(platform
        .replay_harness()
        .read_json_docs_by_keys(
            MEMORY_FACET_POSTING_NAMESPACE,
            std::slice::from_ref(&manifest_key),
        )
        .expect("read deleted facet manifest")
        .is_empty());
    assert!(platform
        .replay_harness()
        .read_json_namespace(MEMORY_FACET_POSTING_NAMESPACE)
        .expect("facet posting namespace")
        .is_empty());
    assert_transaction_events_include_planes(
        &platform,
        &transaction.transaction_id,
        "write.long_term_extraction",
        &[
            "long_term_version_materials",
            "long_term_head_manifests",
            "long_term_version_scope_manifests",
            "memory_facet_indexes",
            "memory_facet_postings",
        ],
    );
}

#[test]
fn long_term_extraction_delete_binds_the_scoped_human_actor_to_the_tombstone() {
    let profile = support::host_test_profile();
    let platform = store_with_transaction_budget(128, 256);
    let actor_subject_id = primary_human_subject_id("owner-default");
    let runtime = support::test_runtime_with_delegated_actor(
        platform.clone(),
        profile,
        "agent-owner",
        &actor_subject_id,
        "chat-human-delete",
    );

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed human-attributed extraction");
    let mut refreshed = extraction_draft();
    refreshed.content = "A refreshed owner must preserve the scoped human actor.".to_string();
    refreshed.source_revision = Some(2);
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![refreshed],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("refresh with scoped human actor");
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: Vec::new(),
                deletes: vec![LongTermMemorySlot {
                    kind: LongTermMemoryKind::Profile,
                    topic: "transaction_extraction".to_string(),
                }],
                skill_writes: Vec::new(),
            },
        })
        .expect("delete with scoped human actor");

    let tombstones = platform
        .replay_harness()
        .read_json_namespace(LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE)
        .expect("read tombstones");
    assert_eq!(tombstones.len(), 1);
    let tombstone = serde_json::from_value::<LongTermMemoryTombstone>(tombstones[0].value.clone())
        .expect("typed tombstone");
    assert_eq!(
        tombstone.actor_subject_id.as_deref(),
        Some(actor_subject_id.as_str())
    );
    assert_eq!(tombstone.factual_owner_id, runtime.memory_space_id());
    assert_ne!(tombstone.factual_owner_id, actor_subject_id);

    let revisions = platform
        .replay_harness()
        .read_json_namespace(LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .expect("read transition revisions")
        .into_iter()
        .map(|doc| {
            serde_json::from_value::<LongTermMemoryControlRevision>(doc.value)
                .expect("typed transition revision")
        })
        .collect::<Vec<_>>();
    assert_eq!(revisions.len(), 2);
    assert!(revisions
        .iter()
        .all(|revision| revision.actor_subject_id.as_deref() == Some(actor_subject_id.as_str())));
}

#[test]
fn long_term_extraction_plans_delete_and_upsert_against_one_facet_manifest_state() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed extraction");

    let mut replacement = extraction_draft();
    replacement.topic = "transaction_extraction_replacement".to_string();
    replacement.content =
        "Replacement facet owner must share the same transaction plan.".to_string();
    replacement.supporting_citations = vec!["fixture:replacement-extraction".to_string()];
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![replacement],
                deletes: vec![LongTermMemorySlot {
                    kind: LongTermMemoryKind::Profile,
                    topic: "transaction_extraction".to_string(),
                }],
                skill_writes: Vec::new(),
            },
        })
        .expect("replace extraction in one transaction");

    let manifest_key =
        memory_facet_manifest_key(runtime.memory_space_id(), runtime.memory_space_id())
            .expect("facet manifest key");
    let manifest = platform
        .replay_harness()
        .read_json_docs_by_keys(
            MEMORY_FACET_POSTING_NAMESPACE,
            std::slice::from_ref(&manifest_key),
        )
        .expect("read facet manifest")
        .into_iter()
        .next()
        .map(|doc| serde_json::from_value::<MemoryFacetIndexManifest>(doc.value))
        .transpose()
        .expect("decode facet manifest")
        .expect("facet manifest");
    assert_eq!(manifest.owner_doc_count, 1);
    assert_eq!(
        platform
            .replay_harness()
            .read_json_namespace("memory_facet_indexes")
            .expect("facet owner namespace")
            .len(),
        1
    );
    assert_eq!(
        runtime
            .replay_harness()
            .memory_space_long_term_memory_read_store("space:owner-default")
            .expect("scoped long-term store")
            .count()
            .expect("owner count"),
        1
    );
}

#[test]
fn transcript_mask_fails_closed_when_facet_source_ref_would_be_redacted() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let turn_id = "turn-redact";
    let mut draft = extraction_draft();
    draft.supporting_citations = vec![format!(
        "transcript:{}/{}/{}#turn={}",
        runtime.memory_space_id(),
        "llm.gateway",
        "chat-a",
        turn_id
    )];

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![draft],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed transcript-backed extraction");
    let owner_id = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "transaction_extraction")
        .expect("accepted extraction entry")
        .id;
    assert_facet_index_doc_for_owner(&platform, &owner_id);

    let err = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "chat-a".to_string(),
            turn_id: Some(turn_id.to_string()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "mask_must_update_facet_source_ref".to_string(),
        })
        .expect_err("facet source impact must fail closed until redaction is supported");

    assert_eq!(err.stage(), "transcript_lifecycle_facet_preflight");
    assert_facet_index_doc_for_owner(&platform, &owner_id);
}

#[test]
fn finalize_feedback_event_budget_rejects_without_partial_turn_or_job() {
    let registry = registry();
    let (platform, runtime) = runtime_with_registry_and_event_budget(registry.clone(), 2);
    let before_events = platform
        .replay_harness()
        .read_events()
        .expect("events before");
    let before = store_fingerprints(&platform);

    let err = match runtime.finalize_turn(feedback_finalize_request(
        &runtime,
        "feedback-budget-reject",
        feedback(&registry),
    )) {
        Ok(_) => panic!("event budget should reject turn and procedural job together"),
        Err(error) => error,
    };

    assert_eq!(err.stage(), "memory_write_transaction_preflight_failed");
    assert_eq!(store_fingerprints(&platform), before);
    assert_eq!(
        platform.replay_harness().read_events().unwrap(),
        before_events
    );
}

#[test]
fn finalize_feedback_success_commits_turn_and_durable_job() {
    let registry = registry();
    let (platform, runtime) = runtime_with_registry_and_event_budget(registry.clone(), 128);

    let report = runtime
        .finalize_turn(feedback_finalize_request(
            &runtime,
            "feedback-success",
            feedback(&registry),
        ))
        .expect("finalize tool feedback");

    assert!(report.session_commit.committed);
    assert!(report.transcript_commit.is_some());
    assert!(report.procedural_learning.job_id.is_some());
    let events = platform.replay_harness().read_events().expect("events");
    for job_id in [
        report.procedural_learning.job_id.as_deref().unwrap(),
        report.memory_consolidation.job_id.as_deref().unwrap(),
    ] {
        assert!(events.iter().any(|event| event.record_key == job_id
            && event.plane == "procedural_feedback"
            && event.payload.get("operation").map(String::as_str)
                == Some("post_turn.learning.enqueue")));
    }
}

#[test]
fn temporal_memory_graph_write_rejects_missing_backlink_without_partial_graph_state() {
    let (platform, runtime) = runtime_with_registry_and_event_budget(registry(), 8);

    let report = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![graph_node("node:release", "turn:release")],
            node_owners: vec![long_term_graph_owner("node:release", "node:release")],
            edges: Vec::new(),
            backlinks: Vec::new(),
        })
        .expect("graph write report");

    assert!(!report.accepted);
    assert!(report.transaction.is_none());
    assert_eq!(report.node_count, 1);
    assert_eq!(report.backlink_count, 0);
    assert!(report
        .gate_failures
        .contains(&"missing_evidence_backlink:turn:release".to_string()));

    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert!(!snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace.starts_with("memory_graph_")));
}

#[test]
fn temporal_memory_graph_write_success_reports_transaction_lineage() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let mut release_owner = extraction_draft();
    release_owner.topic = "graph_release_owner".to_string();
    release_owner.content = "Governed release owner anchors the graph transaction.".to_string();
    release_owner.supporting_citations = vec!["turn:release".to_string()];
    let mut verify_owner = extraction_draft();
    verify_owner.topic = "graph_verify_owner".to_string();
    verify_owner.content = "Governed verify owner closes the graph transaction.".to_string();
    verify_owner.supporting_citations = vec!["turn:release".to_string()];
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![release_owner, verify_owner],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed graph owners");
    let owners = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(usize::MAX)
        .expect("graph owners");
    let release_id = owners
        .iter()
        .find(|entry| entry.topic == "graph_release_owner")
        .expect("release owner")
        .id
        .clone();
    let verify_id = owners
        .iter()
        .find(|entry| entry.topic == "graph_verify_owner")
        .expect("verify owner")
        .id
        .clone();

    let report = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(&release_id, "turn:release"),
                graph_node(&verify_id, "turn:release"),
            ],
            node_owners: vec![
                long_term_graph_owner(&release_id, &release_id),
                long_term_graph_owner(&verify_id, &verify_id),
            ],
            edges: vec![graph_edge(
                "edge:release:verify",
                &release_id,
                &verify_id,
                "turn:release",
            )],
            backlinks: vec![EvidenceBacklink {
                source_kind: "conversation_transcript".to_string(),
                source_id: "turn:release".to_string(),
                fingerprint: "fp-release".to_string(),
            }],
        })
        .expect("graph write report");

    assert!(report.accepted);
    assert!(report.gate_failures.is_empty());
    assert_eq!(report.node_count, 2);
    assert_eq!(report.edge_count, 1);
    assert_eq!(report.backlink_count, 1);
    assert_eq!(report.index_count, 2);
    assert_eq!(report.manifest_generation, Some(1));
    assert!(report.graph_revision.is_some());
    let transaction = report.transaction.as_ref().expect("transaction");
    assert_eq!(transaction.operation, "memory_graph.write");
    assert_eq!(
        transaction.planned_mutations,
        transaction.committed_mutations
    );
    assert!(!transaction.partial_write);

    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    for namespace in [
        "memory_graph_nodes",
        "memory_graph_edges",
        "memory_graph_backlinks",
        "memory_graph_indexes",
        "memory_graph_revisions",
        "memory_graph_manifests",
        "memory_graph_node_memberships",
        "memory_graph_edge_memberships",
        "memory_graph_backlink_memberships",
    ] {
        assert!(
            snapshot
                .json_docs
                .iter()
                .any(|doc| doc.namespace == namespace),
            "missing {namespace}"
        );
    }
}

#[test]
fn long_term_control_event_budget_rejects_without_partial_tombstone() {
    let seed_platform = support::empty_store_platform(support::host_test_profile());
    let seed_runtime = test_runtime_with_scope(
        seed_platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    seed_runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed governed long-term");
    let mut seed_snapshot = seed_platform
        .replay_harness()
        .export_store_snapshot()
        .expect("seed snapshot");
    seed_snapshot.events.clear();
    let platform = store_with_event_budget(3);
    platform
        .replay_harness()
        .import_store_snapshot(&seed_snapshot)
        .expect("import governed seed");
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let record_id = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list")
        .records[0]
        .record
        .id
        .clone();
    let before_events = platform
        .replay_harness()
        .read_events()
        .expect("events before");

    let err = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
            },
            reason: "delete_must_be_transactional".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect_err("event budget should reject control mutation as one transaction");

    assert_eq!(err.stage(), "memory_write_transaction_preflight_failed");
    assert!(runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&record_id)
        .unwrap()
        .is_some());
    assert!(runtime
        .replay_harness()
        .memory_space_long_term_memory_control_read_store("space:owner-default")
        .expect("scoped long-term control store")
        .get_long_term_control_tombstone(&record_id)
        .unwrap()
        .is_none());
    assert_eq!(
        platform.replay_harness().read_events().unwrap(),
        before_events
    );
}

#[test]
fn subject_visibility_event_budget_rejects_without_partial_owner_or_indexes() {
    let seed_platform = support::empty_store_platform(support::host_test_profile());
    let subject_a = default_agent_subject_id("agent-a");
    let subject_b = default_agent_subject_id("agent-b");
    let mut registry =
        SubjectRegistry::single_agent_default("owner-default", "agent-a").expect("registry");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(&subject_b, "Agent B"))
        .expect("agent-b subject");
    let seed_runtime_a = support::test_runtime_with_subject_registry(
        seed_platform.clone(),
        "agent-a",
        &subject_a,
        "chat-a",
        registry.clone(),
    );
    let seed_runtime_b = support::test_runtime_with_subject_registry(
        seed_platform.clone(),
        "agent-b",
        &subject_b,
        "chat-b",
        registry.clone(),
    );
    seed_runtime_a
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed governed long-term");
    let seeded_owner = seed_platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(10)
        .expect("seeded owner list")
        .into_iter()
        .next()
        .expect("seeded owner");
    for runtime in [&seed_runtime_a, &seed_runtime_b] {
        let graph_write = runtime
            .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
                operation: "memory_graph.write".to_string(),
                nodes: vec![graph_node(&seeded_owner.id, "turn:visibility-budget")],
                node_owners: vec![long_term_graph_owner(&seeded_owner.id, &seeded_owner.id)],
                edges: Vec::new(),
                backlinks: vec![EvidenceBacklink {
                    source_kind: "long_term_memory".to_string(),
                    source_id: "turn:visibility-budget".to_string(),
                    fingerprint: "fp:visibility-budget".to_string(),
                }],
            })
            .expect("seed per-subject graph closure");
        assert!(graph_write.accepted, "{graph_write:#?}");
    }
    let mut seed_snapshot = seed_platform
        .replay_harness()
        .export_store_snapshot()
        .expect("seed snapshot");
    seed_snapshot.events.clear();
    let platform = store_with_event_budget(3);
    platform
        .replay_harness()
        .import_store_snapshot(&seed_snapshot)
        .expect("import governed seed");
    let runtime = support::test_runtime_with_subject_registry(
        platform.clone(),
        "agent-a",
        &subject_a,
        "chat-a",
        registry,
    );
    let owner = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list")
        .records[0]
        .record
        .clone();
    let before = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("before snapshot");

    let error = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(owner.id.clone()),
                source_scope: LongTermMemorySourceScope::Chat,
                subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![subject_a]),
            },
            reason: "visibility closure must be transactional".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect_err("event budget rejects the complete visibility transaction");

    assert_eq!(error.stage(), "memory_write_transaction_preflight_failed");
    let after = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("after snapshot");
    assert_eq!(after.json_docs, before.json_docs);
    assert_eq!(after.events, before.events);
    let unchanged = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&owner.id)
        .expect("owner get")
        .expect("owner unchanged");
    assert_eq!(unchanged.owner_revision, owner.owner_revision);
    assert_eq!(
        unchanged.subject_visibility,
        MemorySubjectVisibilityPolicy::AllSubjects
    );
}

#[test]
fn long_term_control_delete_removes_facet_index_in_same_transaction() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed extraction");
    let owner_id = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "transaction_extraction")
        .expect("accepted extraction entry")
        .id;
    assert_facet_index_doc_for_owner(&platform, &owner_id);

    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
            },
            reason: "delete_must_update_facet_index".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("control delete");

    assert!(report.accepted);
    assert_eq!(report.operation, "delete");
    assert_eq!(report.affected_records.len(), 1);
    assert_eq!(report.affected_records[0].record_id, owner_id);
    assert!(runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&owner_id)
        .expect("long-term get")
        .is_none());
    assert!(runtime
        .replay_harness()
        .memory_space_long_term_memory_control_read_store("space:owner-default")
        .expect("scoped long-term control store")
        .get_long_term_control_tombstone(&owner_id)
        .expect("control tombstone")
        .is_some());
    assert_no_facet_index_doc_for_owner(&platform, &owner_id);
    assert_operation_events_include_planes(
        &platform,
        "long_term_control.mutation",
        &[
            "long_term_version_materials",
            "long_term_head_manifests",
            "long_term_version_scope_manifests",
            "memory_facet_indexes",
            "memory_facet_postings",
            "long_term_control_tombstone",
            "long_term_control_audit",
        ],
    );
}

#[test]
fn long_term_control_correct_updates_facet_index_revision_in_same_transaction() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed extraction");
    let owner_id = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "transaction_extraction")
        .expect("accepted extraction entry")
        .id;
    let before_facet = assert_facet_index_doc_for_owner(&platform, &owner_id);
    assert_eq!(before_facet["owner_revision"].as_u64(), Some(1));
    assert_eq!(before_facet["facet_index_revision"].as_u64(), Some(1));

    let mut replacement = extraction_draft();
    replacement.content =
        "Corrected long-term extraction must update the governed facet index.".to_string();
    replacement.keywords.push("corrected".to_string());

    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                replacement,
            },
            reason: "correct_must_update_facet_index".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("control correct");

    assert!(report.accepted);
    assert_eq!(report.operation, "correct");
    assert_eq!(report.affected_records.len(), 1);
    assert_eq!(report.affected_records[0].record_id, owner_id);
    assert_eq!(report.affected_records[0].new_owner_revision, Some(2));
    assert_eq!(report.affected_records[0].new_source_revision, Some(1));
    let updated = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&owner_id)
        .expect("long-term get")
        .expect("updated long-term owner");
    assert_eq!(updated.source_revision, Some(1));
    assert_eq!(updated.owner_revision, 2);
    assert!(updated.content.contains("Corrected"));

    let facet_doc = assert_facet_index_doc_for_owner(&platform, &owner_id);
    assert_eq!(facet_doc["owner_revision"].as_u64(), Some(2));
    assert_eq!(facet_doc["facet_index_revision"].as_u64(), Some(2));
    assert_operation_events_include_planes(
        &platform,
        "long_term_control.mutation",
        &[
            "long_term_version_materials",
            "long_term_head_manifests",
            "long_term_version_scope_manifests",
            "memory_facet_indexes",
            "memory_facet_postings",
            "long_term_control_revision",
            "long_term_control_audit",
        ],
    );
}

#[test]
fn long_term_control_supersede_replaces_owner_facet_index_in_same_transaction() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed extraction");
    let old_owner_id = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "transaction_extraction")
        .expect("accepted extraction entry")
        .id;
    assert_facet_index_doc_for_owner(&platform, &old_owner_id);

    let mut replacement = extraction_draft();
    replacement.topic = "transaction_extraction_superseded".to_string();
    replacement.content =
        "Superseded owner record must receive a new active facet index.".to_string();

    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Supersede {
                target: MemoryLongTermTarget::RecordId(old_owner_id.clone()),
                replacement,
            },
            reason: "supersede_must_replace_facet_index".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("control supersede");

    assert!(report.accepted);
    assert_eq!(report.operation, "supersede");
    assert!(runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&old_owner_id)
        .expect("old long-term get")
        .is_none());
    assert_no_facet_index_doc_for_owner(&platform, &old_owner_id);
    let new_owner_id = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "transaction_extraction_superseded")
        .expect("new owner entry")
        .id;
    assert_facet_index_doc_for_owner(&platform, &new_owner_id);
    assert_operation_events_include_planes(
        &platform,
        "long_term_control.mutation",
        &[
            "long_term_version_materials",
            "long_term_head_manifests",
            "long_term_version_scope_manifests",
            "memory_facet_indexes",
            "memory_facet_postings",
            "long_term_control_tombstone",
            "long_term_control_revision",
            "long_term_control_audit",
        ],
    );
}

#[test]
fn restricted_supersede_persists_successor_policy_material_and_facet_exactly() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed extraction");
    let predecessor = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "transaction_extraction")
        .expect("predecessor owner");
    let restricted =
        MemorySubjectVisibilityPolicy::OnlySubjects(vec![runtime.subject_id().to_string()]);
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(predecessor.id.clone()),
                source_scope: predecessor.source_scope,
                subject_visibility: restricted.clone(),
            },
            reason: "restrict predecessor before supersede".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("restrict predecessor");
    let mut replacement = extraction_draft();
    replacement.topic = "transaction_restricted_successor".to_string();
    replacement.content = "Restricted successor keeps the exact policy.".to_string();
    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Supersede {
                target: MemoryLongTermTarget::RecordId(predecessor.id.clone()),
                replacement,
            },
            reason: "supersede without widening visibility".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("supersede restricted predecessor");
    assert!(report.accepted, "{report:#?}");
    assert_eq!(report.projection_impact.subject_visibility, restricted);
    let successor = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "transaction_restricted_successor")
        .expect("successor owner");
    assert_eq!(successor.owner_revision, 1);
    assert_eq!(successor.subject_visibility, restricted);
    let facet = assert_facet_index_doc_for_owner(&platform, &successor.id);
    assert_eq!(facet["owner_revision"].as_u64(), Some(1));
    assert_eq!(facet["facet_index_revision"].as_u64(), Some(1));
    let successor_material = platform
        .replay_harness()
        .read_json_namespace("long_term_version_materials")
        .expect("version materials")
        .into_iter()
        .map(|document| {
            serde_json::from_value::<LongTermMemoryVersionMaterial>(document.value)
                .expect("typed version material")
        })
        .find(|material| {
            material.owner_ref.owner_id == successor.id && material.owner_revision == 1
        })
        .expect("successor material");
    assert_eq!(successor_material.subject_visibility, restricted);
}

#[test]
fn long_term_control_change_scope_persists_visibility_with_owner_and_facet_revision() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed extraction");
    let owner_id = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "transaction_extraction")
        .expect("accepted extraction entry")
        .id;
    assert_facet_index_doc_for_owner(&platform, &owner_id);

    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                source_scope: LongTermMemorySourceScope::Chat,
                subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![runtime
                    .subject_id()
                    .to_string()]),
            },
            reason: "change_scope_must_update_facet_index".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("control change scope");

    assert!(report.accepted);
    assert_eq!(report.operation, "change_scope");
    assert!(!report
        .projection_impact
        .notes
        .contains(&"report_only_subject_visibility_not_indexed".to_string()));
    let updated = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&owner_id)
        .expect("long-term get")
        .expect("updated owner");
    assert_eq!(updated.source_revision, Some(1));
    assert_eq!(updated.owner_revision, 2);
    assert_eq!(updated.source_scope, LongTermMemorySourceScope::Chat);
    assert_eq!(
        updated.subject_visibility,
        MemorySubjectVisibilityPolicy::OnlySubjects(vec![runtime.subject_id().to_string()])
    );
    let facet_doc = assert_facet_index_doc_for_owner(&platform, &owner_id);
    assert_eq!(facet_doc["owner_revision"].as_u64(), Some(2));
    assert_eq!(facet_doc["facet_index_revision"].as_u64(), Some(2));
    let successor_material = platform
        .replay_harness()
        .read_json_namespace("long_term_version_materials")
        .expect("version materials")
        .into_iter()
        .map(|document| {
            serde_json::from_value::<LongTermMemoryVersionMaterial>(document.value)
                .expect("typed version material")
        })
        .find(|material| material.owner_ref.owner_id == owner_id && material.owner_revision == 2)
        .expect("visibility successor material");
    assert_eq!(successor_material.owner_revision, updated.owner_revision);
    assert_eq!(
        successor_material.subject_visibility,
        updated.subject_visibility
    );
    let head = platform
        .replay_harness()
        .read_json_namespace("long_term_head_manifests")
        .expect("head manifests")
        .into_iter()
        .map(|document| {
            serde_json::from_value::<LongTermMemoryHeadManifest>(document.value)
                .expect("typed head manifest")
        })
        .find(|head| head.owner_ref.owner_id == owner_id)
        .expect("visibility owner head");
    assert_eq!(head.current_revision, updated.owner_revision);
    let control = platform
        .replay_harness()
        .read_json_namespace(LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .expect("control revisions")
        .into_iter()
        .map(|document| {
            serde_json::from_value::<LongTermMemoryControlRevision>(document.value)
                .expect("typed control revision")
        })
        .find(|revision| {
            revision.operation == bm_core::memory::LongTermControlOperation::ChangeScope
                && revision
                    .transition
                    .successor
                    .as_ref()
                    .is_some_and(|successor| {
                        successor.owner_ref.owner_id == owner_id
                            && successor.owner_revision == updated.owner_revision
                    })
        })
        .expect("visibility control revision");
    assert_eq!(
        control.successor_material_digest.as_deref(),
        Some(successor_material.content_digest.as_str())
    );
    assert_operation_events_include_planes(
        &platform,
        "long_term_control.mutation",
        &[
            "long_term_version_materials",
            "long_term_head_manifests",
            "long_term_version_scope_manifests",
            "memory_facet_indexes",
            "memory_facet_postings",
            "long_term_control_revision",
            "long_term_control_audit",
        ],
    );
}

#[test]
fn explicit_privacy_transition_updates_owner_facet_and_postings_atomically() {
    let platform = store_with_transaction_budget(128, 256);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let mut draft = extraction_draft();
    draft.content =
        "SOUL_PRIVATE_TRANSITION_SENTINEL must leave every public delivery surface.".to_string();
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![draft],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed extraction");
    let owner_id = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "transaction_extraction")
        .expect("accepted extraction entry")
        .id;
    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![MemoryGraphNode {
                node_id: owner_id.clone(),
                kind: MemoryGraphNodeKind::MemoryRecord,
                label: "Transaction extraction owner graph node".to_string(),
                evidence_refs: vec!["fixture:long-term-extraction".to_string()],
            }],
            node_owners: vec![long_term_graph_owner(&owner_id, &owner_id)],
            edges: Vec::new(),
            backlinks: vec![EvidenceBacklink {
                source_kind: "conversation_transcript".to_string(),
                source_id: "fixture:long-term-extraction".to_string(),
                fingerprint: "fp-soul-private-transition".to_string(),
            }],
        })
        .expect("seed owner-backed graph");
    assert!(
        graph_write.accepted,
        "owner-backed graph rejected: {:?}",
        graph_write.gate_failures
    );
    assert!(platform
        .replay_harness()
        .read_json_namespace("memory_graph_nodes")
        .expect("owner graph nodes before privacy transition")
        .iter()
        .any(|doc| doc.value["node_id"].as_str() == Some(owner_id.as_str())));

    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangePrivacy {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                privacy: MemoryPrivacyClass::SoulPrivate,
            },
            reason: "owner explicitly moved record behind soul privacy".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("change privacy");

    assert!(report.accepted);
    assert_eq!(report.operation, "change_privacy");
    let owner = runtime
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&owner_id)
        .expect("owner read")
        .expect("owner exists");
    assert_eq!(owner.privacy, MemoryPrivacyClass::SoulPrivate);
    assert_eq!(owner.source_revision, Some(1));
    assert_eq!(owner.owner_revision, 2);
    let facet_doc = assert_facet_index_doc_for_owner(&platform, &owner_id);
    assert_eq!(facet_doc["privacy"].as_str(), Some("soul_private"));
    assert_eq!(facet_doc["owner_revision"].as_u64(), Some(2));
    assert!(platform
        .replay_harness()
        .read_json_namespace("memory_facet_postings")
        .expect("posting docs")
        .into_iter()
        .all(|doc| doc
            .value
            .get("owner_versions")
            .and_then(serde_json::Value::as_array)
            .is_none_or(|owners| owners.iter().all(|owner| {
                owner
                    .get("owner_ref")
                    .and_then(|owner_ref| owner_ref.get("owner_id"))
                    .and_then(serde_json::Value::as_str)
                    != Some(&owner_id)
            }))));
    let graph_docs = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("graph closure snapshot")
        .json_docs
        .into_iter()
        .filter(|doc| doc.namespace.starts_with("memory_graph_"))
        .collect::<Vec<_>>();
    assert!(graph_docs
        .iter()
        .all(|doc| !serde_json::to_string(&doc.value)
            .expect("serialize graph doc")
            .contains(&owner_id)));

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "transaction extraction privacy".to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .expect("private transition recall");
    let delivered_working = format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
        recall.working.long_term_memory_text,
        recall.working.shared_factual_plane,
        recall.working.shared_factual_report.candidates,
        recall.working.continuity_capsule_text,
        recall.working.continuity_capsules,
        recall.working.archive_evidence_text,
        recall.working.archive_hits,
        recall.working.selected_archive_hits,
        recall.working.runtime_skill_text,
        recall.working.runtime_skill_report.candidates,
        recall.working.task_recall_text,
        recall
            .working
            .task_recall_report
            .as_ref()
            .map(|report| &report.candidates),
    );
    assert!(!delivered_working.contains("SOUL_PRIVATE_TRANSITION_SENTINEL"));
    assert!(!format!("{:?}", recall.compact_graph).contains("SOUL_PRIVATE_TRANSITION_SENTINEL"));
    assert!(!format!("{:?}", recall.delivery_report).contains("SOUL_PRIVATE_TRANSITION_SENTINEL"));
    let projection = runtime
        .project(MemoryProjectionRequest {
            binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "What is the transaction extraction privacy policy?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("private transition projection");
    assert!(!projection
        .provider_payload()
        .system_memory_block()
        .contains("SOUL_PRIVATE_TRANSITION_SENTINEL"));
    assert_operation_events_include_planes(
        &platform,
        "long_term_control.mutation",
        &[
            "long_term_version_materials",
            "long_term_head_manifests",
            "long_term_version_scope_manifests",
            "memory_facet_indexes",
            "memory_facet_postings",
            "memory_graph",
            "long_term_control_revision",
            "long_term_control_audit",
        ],
    );
}

#[test]
fn long_term_policy_event_budget_rejects_without_partial_policy() {
    let platform = store_with_event_budget(2);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let before_events = platform
        .replay_harness()
        .read_events()
        .expect("events before");

    let err = runtime
        .mutate_memory_governance_policy(MemoryLongTermPolicyRequest {
            operation: MemoryGovernancePolicyMutation::Suppress {
                selector: MemoryGovernanceSelector {
                    memory_space_id: Some(runtime.memory_space_id().to_string()),
                    subject_id: Some(runtime.subject_id().to_string()),
                    kind: Some(LongTermMemoryKind::Preference),
                    topic_pattern: Some("temporary-*".to_string()),
                    source_chat_id: None,
                    source_scope: None,
                },
                duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
            },
            reason: "policy_must_be_transactional".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect_err("event budget should reject policy mutation as one transaction");

    assert_eq!(err.stage(), "memory_write_transaction_preflight_failed");
    assert!(platform
        .replay_harness()
        .memory_space_long_term_memory_control_read_store("space:owner-default")
        .expect("scoped long-term control store")
        .list_long_term_governance_policies(10)
        .unwrap()
        .is_empty());
    assert_eq!(
        platform.replay_harness().read_events().unwrap(),
        before_events
    );
}
