#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    canonical_evidence_ref_from_source, memory_facet_manifest_key,
    scoped_memory_facet_owner_storage_key, CanonicalEntityKey, CanonicalEntityKind,
    CanonicalEntityRef, LongTermMemorySlot, MemoryFacetIndexManifest, QueryFacetInput,
    MEMORY_FACET_POSTING_NAMESPACE,
};
use bm_core::platform::Platform as _;
use bm_core::task_execution::{
    TaskLearningKind, TaskLearningRecord, TaskLearningRoute, TaskPlan, TaskRun, TaskRunKind,
    TaskRunRecord, TaskRunStatus,
};
use bm_sdk::{
    AgentToolDescriptor, AgentToolObservationDigest, AgentToolOutcome, AgentToolRegistrySnapshot,
    AgentToolUsageFeedback, EvidenceBacklink, IngressKind, LongTermMemoryDraft, LongTermMemoryKind,
    LongTermMemoryQuery, LongTermMemorySourceScope, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryEvidenceAuthority, MemoryGovernancePolicyMutation, MemoryGovernanceSelector,
    MemoryGovernanceSuppressionDuration, MemoryGraphEdge, MemoryGraphEdgeKind, MemoryGraphNode,
    MemoryGraphNodeKind, MemoryIdentity, MemoryLongTermControlView, MemoryLongTermListRequest,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest,
    MemoryLongTermTarget, MemoryMaintenanceRequest, MemoryPrivacyClass, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryScope, MemorySemanticJudgmentSource, MemoryStoreHandle,
    MemorySubjectVisibilityPolicy, MemoryTranscriptLifecycleRequest, MemoryWriteCandidate,
    MemoryWriteRequest, ParsedLongTermMemoryExtraction, PressureLevel,
    ProceduralMemoryPromotionInput, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillReuseOutcome,
    RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig, StoreRuntimeBudget,
    TemporalMemoryGraphWriteRequest, TemporalValidity, TranscriptLifecycleTransition,
};

use support::{empty_store_platform, test_runtime_with_scope, StaticHttpClient, StaticLlmClient};

#[test]
fn maintenance_long_term_write_keeps_owner_and_facet_in_one_governed_path() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
        "llm.gateway",
        "chat-a",
    );
    platform
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
    platform
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
                runtime_skill_selected_ids: Vec::new(),
                task_learning_selected_ids: Vec::new(),
                reuse_outcome: RuntimeSkillReuseOutcome::Neutral,
                reuse_outcome_note: String::new(),
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .expect("maintenance");
    let transaction = report.transaction.expect("maintenance transaction");
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
    let maintenance = report.report.expect("maintenance outcome");
    assert_eq!(
        maintenance
            .task_learning_outcome
            .expect("task learning maintenance")
            .canonical_writes,
        1
    );
    assert_eq!(
        platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:owner-default")
            .expect("scoped owner store")
            .count()
            .expect("owner count"),
        1
    );
    let manifest_key = memory_facet_manifest_key("space:owner-default", runtime.subject_id())
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
    store_with_transaction_budget(event_log_max_items, 16)
}

fn store_with_transaction_budget(
    event_log_max_items: usize,
    kv_max_entries: usize,
) -> MemoryStoreHandle {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("store config")
        .with_runtime_store_budget(transaction_budget(event_log_max_items, kv_max_entries));
    MemoryStoreHandle::open_in_memory(config).expect("store platform")
}

fn runtime_with_registry_and_event_budget(
    registry: AgentToolRegistrySnapshot,
    event_log_max_items: usize,
) -> (MemoryStoreHandle, bm_sdk::MemoryRuntime) {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = store_with_event_budget(event_log_max_items);
    let runtime = bm_sdk::MemoryRuntime::builder()
        .identity(MemoryIdentity::new("transaction-agent", "owner-default").expect("identity"))
        .scope(MemoryScope::new("llm.gateway", "chat-a").expect("scope"))
        .profile(profile)
        .store(platform.clone())
        .agent_tool_registry(registry)
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

fn runtime_skill_candidate() -> MemoryWriteCandidate {
    MemoryWriteCandidate {
        candidate_id: "candidate-transaction-skill".to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: MemoryCandidateTarget::ProceduralMemory {
            name: String::new(),
            topic: "transaction_skill".to_string(),
        },
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::RuntimeSkill {
            name: "runtime_skill__transaction_contract".to_string(),
            topic: "transaction_skill".to_string(),
            title: "transaction contract".to_string(),
            summary: "Reject an entire memory write batch when admission fails.".to_string(),
            content:
                "- preflight every mutation\n- commit the batch once\n- report transaction ids"
                    .to_string(),
            citations: vec!["fixture:transaction-contract".to_string()],
        },
        evidence_refs: vec!["chat-a:turn-2".to_string()],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(llm_accept(MemoryCandidateTarget::ProceduralMemory {
            name: String::new(),
            topic: "transaction_skill".to_string(),
        })),
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

fn promotion_input(task_id: &str) -> ProceduralMemoryPromotionInput {
    ProceduralMemoryPromotionInput {
        task_id: task_id.to_string(),
        trigger: "transaction promotion checklist".to_string(),
        procedure: "Promote procedural memory only through the transaction planner.".to_string(),
        constraints: vec!["commit once".to_string()],
        failure_modes: vec!["partial skill without lifecycle".to_string()],
        counterfactual_fix: "plan mutations before store writes".to_string(),
        evidence_refs: vec!["task:first".to_string(), "task:second".to_string()],
        quality_score: 90,
        repeated_evidence_count: 2,
        capability_affinity: vec!["memory".to_string()],
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
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: vec!["fixture:long-term-extraction".to_string()],
        canonical_entities: Vec::new(),
        evidence_count: Some(1),
        observed_at: Some(1_800_000_000),
        last_confirmed_at: Some(1_800_000_000),
        source_revision: Some(1),
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

fn feedback(registry: &AgentToolRegistrySnapshot) -> AgentToolUsageFeedback {
    AgentToolUsageFeedback {
        registry_ref: registry.registry_ref(),
        observations: vec![observation("obs-1"), observation("obs-2")],
        user_visible_result_summary: Some(
            "PDF extraction helped produce release notes from a local artifact.".to_string(),
        ),
        reuse_outcome: RuntimeSkillReuseOutcome::Succeeded,
        operator_note: None,
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
    owner_record_id: &str,
) -> serde_json::Value {
    let docs = facet_index_docs(platform);
    let doc = docs
        .iter()
        .find(|doc| doc.value["owner_record_id"] == owner_record_id)
        .unwrap_or_else(|| panic!("missing facet index for owner {owner_record_id}"));
    let memory_space_id = doc.value["memory_space_id"]
        .as_str()
        .expect("facet memory space");
    let subject_id = doc.value["subject_ids"]
        .as_array()
        .and_then(|subjects| subjects.first())
        .and_then(serde_json::Value::as_str)
        .expect("facet mounted subject");
    assert_eq!(
        doc.key,
        scoped_memory_facet_owner_storage_key(memory_space_id, subject_id, owner_record_id)
            .expect("scoped facet owner key")
    );
    assert_eq!(doc.value["owner_plane"], "long_term");
    assert_eq!(doc.value["status"], "active");
    assert!(doc.value["exact_facets"]
        .as_array()
        .is_some_and(|facets| !facets.is_empty()));
    doc.value.clone()
}

fn assert_no_facet_index_doc_for_owner(platform: &MemoryStoreHandle, owner_record_id: &str) {
    assert!(
        !facet_index_docs(platform)
            .iter()
            .any(|doc| doc.value["owner_record_id"] == owner_record_id),
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
        ProfileId::ServerLinuxDevFull,
        "llm.gateway",
        "chat-a",
    );
    let before_events = platform
        .replay_harness()
        .read_events()
        .expect("events before");
    let before_long_term_count = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
            candidates: vec![long_term_candidate(), runtime_skill_candidate()],
        })
        .expect_err("event budget should reject the whole memory write transaction");

    assert_eq!(err.stage(), "memory_write_transaction_preflight_failed");
    assert_eq!(
        platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:owner-default")
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
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
        "llm.gateway",
        "chat-a",
    );

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![long_term_candidate(), runtime_skill_candidate()],
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

    let long_term_records = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
}

#[test]
fn candidate_to_draft_to_entry_to_exact_entity_posting_to_typed_query_is_reachable() {
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
    let entry = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .list(20)
        .expect("long-term list")
        .into_iter()
        .find(|entry| entry.topic == "typed_entity_candidate")
        .expect("typed entity owner");
    assert_eq!(entry.canonical_entities[0].key, key);

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "unrelated plain text".to_string(),
            limit: 8,
            structured_query_facets: vec![QueryFacetInput::Entity(key)],
            tool_registry_refs: Vec::new(),
        })
        .expect("typed entity recall");

    assert!(recall.facet_index_report.manifest_integrity_verified);
    assert_eq!(
        recall.facet_index_report.exact_facet_candidate_ids,
        vec![entry.id]
    );
}

#[test]
fn content_change_replaces_entities_and_removes_old_exact_posting() {
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
    let owner = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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

    let owner = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
        ProfileId::ServerLinuxDevFull,
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
        platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:owner-default")
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
        ProfileId::ServerLinuxDevFull,
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
        .write(MemoryWriteRequest::Procedural {
            writes: vec![manual_runtime_skill_write(
                "runtime_skill__transaction_manual",
            )],
            source: RuntimeSkillWriteSource::Manual,
        })
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
        ProfileId::ServerLinuxDevFull,
        "llm.gateway",
        "chat-a",
    );

    let report = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![manual_runtime_skill_write(
                "runtime_skill__transaction_manual",
            )],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("procedural write");

    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let transaction = report.transaction.expect("transaction");
    assert_eq!(transaction.operation, "write.procedural");
    assert_eq!(
        transaction.planned_mutations,
        transaction.committed_mutations
    );
    assert!(!transaction.partial_write);
    assert_transaction_events(
        &platform,
        &transaction.transaction_id,
        "write.procedural",
        transaction.event_ids.len(),
    );
}

#[test]
fn procedural_promotion_event_budget_rejects_without_partial_skill() {
    let platform = store_with_event_budget(2);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
        .write(MemoryWriteRequest::ProceduralPromotions {
            promotions: vec![promotion_input("promotion-budget")],
            source: RuntimeSkillWriteSource::TaskLearning,
        })
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
fn procedural_promotion_success_reports_transaction_lineage() {
    let platform = store_with_event_budget(16);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
        "llm.gateway",
        "chat-a",
    );

    let report = runtime
        .write(MemoryWriteRequest::ProceduralPromotions {
            promotions: vec![promotion_input("promotion-success")],
            source: RuntimeSkillWriteSource::TaskLearning,
        })
        .expect("promotion write");

    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let transaction = report.transaction.expect("transaction");
    assert_eq!(transaction.operation, "write.procedural_promotions");
    assert!(!transaction.partial_write);
    assert_transaction_events(
        &platform,
        &transaction.transaction_id,
        "write.procedural_promotions",
        transaction.event_ids.len(),
    );
}

#[test]
fn long_term_extraction_event_budget_rejects_without_partial_memory() {
    let platform = store_with_event_budget(2);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
        "llm.gateway",
        "chat-a",
    );
    let before_events = platform
        .replay_harness()
        .read_events()
        .expect("events before");
    let before_long_term_count = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
        platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:owner-default")
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
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
        "llm.gateway",
        "chat-a",
    );

    let report = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_draft()],
                deletes: Vec::new(),
                skill_writes: vec![manual_runtime_skill_write(
                    "runtime_skill__transaction_extraction",
                )],
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

    let long_term_records = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
}

#[test]
fn long_term_extraction_delete_removes_facet_index_in_same_transaction() {
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
    let owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
    assert!(platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&owner_id)
        .expect("long-term get")
        .is_none());
    assert_no_facet_index_doc_for_owner(&platform, &owner_id);
    let manifest_key = memory_facet_manifest_key(runtime.memory_space_id(), runtime.subject_id())
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
        &["long_term", "memory_facet_indexes", "memory_facet_postings"],
    );
}

#[test]
fn long_term_extraction_plans_delete_and_upsert_against_one_facet_manifest_state() {
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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

    let manifest_key = memory_facet_manifest_key(runtime.memory_space_id(), runtime.subject_id())
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
        platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:owner-default")
            .expect("scoped long-term store")
            .count()
            .expect("owner count"),
        1
    );
}

#[test]
fn transcript_mask_fails_closed_when_facet_source_ref_would_be_redacted() {
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
    let owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
fn agent_tool_feedback_event_budget_rejects_without_partial_experience() {
    let registry = registry();
    let (platform, runtime) = runtime_with_registry_and_event_budget(registry.clone(), 2);
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
        .write(MemoryWriteRequest::AgentToolUsageFeedback {
            feedback: feedback(&registry),
        })
        .expect_err("event budget should reject tool experience and lifecycle together");

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
fn agent_tool_feedback_success_reports_transaction_lineage() {
    let registry = registry();
    let (platform, runtime) = runtime_with_registry_and_event_budget(registry.clone(), 16);

    let report = runtime
        .write(MemoryWriteRequest::AgentToolUsageFeedback {
            feedback: feedback(&registry),
        })
        .expect("agent tool feedback");

    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let transaction = report.transaction.expect("transaction");
    assert_eq!(transaction.operation, "write.agent_tool_usage_feedback");
    assert!(!transaction.partial_write);
    assert_transaction_events(
        &platform,
        &transaction.transaction_id,
        "write.agent_tool_usage_feedback",
        transaction.event_ids.len(),
    );
}

#[test]
fn temporal_memory_graph_write_rejects_missing_backlink_without_partial_graph_state() {
    let (platform, runtime) = runtime_with_registry_and_event_budget(registry(), 8);

    let report = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![graph_node("node:release", "turn:release")],
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
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
    let owners = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
    let seed_platform = support::empty_store_platform(ProfileId::ServerLinuxDevFull);
    let seed_runtime = test_runtime_with_scope(
        seed_platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
        ProfileId::ServerLinuxDevFull,
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
    assert!(platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&record_id)
        .unwrap()
        .is_some());
    assert!(platform
        .replay_harness()
        .scoped_long_term_memory_control_read_store("space:owner-default")
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
fn long_term_control_delete_removes_facet_index_in_same_transaction() {
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
    let owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
    assert!(platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&owner_id)
        .expect("long-term get")
        .is_none());
    assert!(platform
        .replay_harness()
        .scoped_long_term_memory_control_read_store("space:owner-default")
        .expect("scoped long-term control store")
        .get_long_term_control_tombstone(&owner_id)
        .expect("control tombstone")
        .is_some());
    assert_no_facet_index_doc_for_owner(&platform, &owner_id);
    assert_operation_events_include_planes(
        &platform,
        "long_term_control.mutation",
        &[
            "long_term",
            "memory_facet_indexes",
            "memory_facet_postings",
            "long_term_control_tombstone",
            "long_term_control_audit",
        ],
    );
}

#[test]
fn long_term_control_correct_updates_facet_index_revision_in_same_transaction() {
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
    let owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
    let updated = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
            "long_term",
            "memory_facet_indexes",
            "memory_facet_postings",
            "long_term_control_revision",
            "long_term_control_audit",
        ],
    );
}

#[test]
fn long_term_control_supersede_replaces_owner_facet_index_in_same_transaction() {
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
    let old_owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
    assert!(platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&old_owner_id)
        .expect("old long-term get")
        .is_none());
    assert_no_facet_index_doc_for_owner(&platform, &old_owner_id);
    let new_owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
            "long_term",
            "memory_facet_indexes",
            "memory_facet_postings",
            "long_term_control_tombstone",
            "long_term_control_revision",
            "long_term_control_audit",
        ],
    );
}

#[test]
fn long_term_control_change_scope_updates_facet_and_reports_visibility_not_indexed() {
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
    let owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
    assert!(report
        .projection_impact
        .notes
        .contains(&"report_only_subject_visibility_not_indexed".to_string()));
    let updated = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term store")
        .get(&owner_id)
        .expect("long-term get")
        .expect("updated owner");
    assert_eq!(updated.source_revision, Some(1));
    assert_eq!(updated.owner_revision, 2);
    assert_eq!(updated.source_scope, LongTermMemorySourceScope::Chat);
    let facet_doc = assert_facet_index_doc_for_owner(&platform, &owner_id);
    assert_eq!(facet_doc["owner_revision"].as_u64(), Some(2));
    assert_eq!(facet_doc["facet_index_revision"].as_u64(), Some(2));
    assert_operation_events_include_planes(
        &platform,
        "long_term_control.mutation",
        &[
            "long_term",
            "memory_facet_indexes",
            "memory_facet_postings",
            "long_term_control_revision",
            "long_term_control_audit",
        ],
    );
}

#[test]
fn explicit_privacy_transition_updates_owner_facet_and_postings_atomically() {
    let platform = store_with_transaction_budget(128, 128);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
    let owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
    let owner = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
                    .get("owner_record_id")
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
        .system_memory_block
        .contains("SOUL_PRIVATE_TRANSITION_SENTINEL"));
    assert_operation_events_include_planes(
        &platform,
        "long_term_control.mutation",
        &[
            "long_term",
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
        ProfileId::ServerLinuxDevFull,
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
        .scoped_long_term_memory_control_read_store("space:owner-default")
        .expect("scoped long-term control store")
        .list_long_term_governance_policies(10)
        .unwrap()
        .is_empty());
    assert_eq!(
        platform.replay_harness().read_events().unwrap(),
        before_events
    );
}
