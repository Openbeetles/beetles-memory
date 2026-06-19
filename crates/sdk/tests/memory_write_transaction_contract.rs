mod support;

use bm_core::platform::Platform as _;
use bm_sdk::{
    AgentToolDescriptor, AgentToolObservationDigest, AgentToolOutcome, AgentToolRegistrySnapshot,
    AgentToolUsageFeedback, EvidenceBacklink, LongTermMemoryDraft, LongTermMemoryKind,
    LongTermMemoryQuery, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryGovernancePolicyMutation, MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration,
    MemoryGraphEdge, MemoryGraphEdgeKind, MemoryGraphNode, MemoryGraphNodeKind, MemoryIdentity,
    MemoryLongTermControlView, MemoryLongTermListRequest, MemoryLongTermMutation,
    MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest, MemoryLongTermTarget,
    MemoryPrivacyClass, MemoryScope, MemorySemanticJudgmentSource, MemoryWriteCandidate,
    MemoryWriteRequest, ParsedLongTermMemoryExtraction, ProceduralMemoryPromotionInput, ProfileId,
    RuntimeLifecycleModeInput, RuntimeSkillReuseOutcome, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig, StorePlatform, StoreRuntimeBudget,
    TemporalMemoryGraphWriteRequest, TemporalValidity,
};

use support::test_runtime_with_scope;

fn transaction_budget(event_log_max_items: usize, kv_max_entries: usize) -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        event_log_max_items,
        kv_max_entries,
        blob_max_bytes: 4096,
        snapshot_max_bytes: 16_384,
        logical_namespace_max_bytes: 128,
        logical_key_max_bytes: 1024,
        event_record_key_max_bytes: 1024,
        export_max_bytes: 16_384,
        import_max_bytes: 16_384,
    }
}

fn store_with_event_budget(event_log_max_items: usize) -> StorePlatform {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("store config")
        .with_runtime_store_budget(transaction_budget(event_log_max_items, 16));
    StorePlatform::open_in_memory(config).expect("store platform")
}

fn runtime_with_registry_and_event_budget(
    registry: AgentToolRegistrySnapshot,
    event_log_max_items: usize,
) -> (StorePlatform, bm_sdk::MemoryRuntime) {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = store_with_event_budget(event_log_max_items);
    let runtime = bm_sdk::MemoryRuntime::builder()
        .identity(MemoryIdentity::new("transaction-agent", "owner-default").expect("identity"))
        .scope(MemoryScope::new("llm.gateway", "chat-a").expect("scope"))
        .profile(profile)
        .store_platform(platform.clone())
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
        semantic_judgment: Some(llm_accept(MemoryCandidateTarget::ProceduralMemory {
            name: String::new(),
            topic: "transaction_skill".to_string(),
        })),
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
    platform: &StorePlatform,
    transaction_id: &str,
    operation: &str,
    expected_count: usize,
) {
    let events = platform.read_events().expect("events");
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

#[test]
fn candidate_write_event_budget_rejects_without_partial_memory() {
    let platform = store_with_event_budget(2);
    let runtime = test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
        "llm.gateway",
        "chat-a",
    );
    let before_events = platform.read_events().expect("events before");
    let before_long_term_count = platform
        .long_term_memory_store()
        .count()
        .expect("long-term before");
    let before_skill_names = platform
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
        platform.long_term_memory_store().count().unwrap(),
        before_long_term_count
    );
    assert_eq!(
        platform.skill_storage().list_names().unwrap(),
        before_skill_names
    );
    assert_eq!(platform.read_events().unwrap(), before_events);
}

#[test]
fn candidate_write_success_reports_transaction_lineage() {
    let platform = store_with_event_budget(16);
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

    let events = platform.read_events().expect("events");
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
    let before_events = platform.read_events().expect("events before");
    let before_skill_names = platform
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
        platform.skill_storage().list_names().unwrap(),
        before_skill_names
    );
    assert_eq!(platform.read_events().unwrap(), before_events);
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
    let before_events = platform.read_events().expect("events before");
    let before_skill_names = platform
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
        platform.skill_storage().list_names().unwrap(),
        before_skill_names
    );
    assert_eq!(platform.read_events().unwrap(), before_events);
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
    let before_events = platform.read_events().expect("events before");
    let before_long_term_count = platform
        .long_term_memory_store()
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
        platform.long_term_memory_store().count().unwrap(),
        before_long_term_count
    );
    assert_eq!(platform.read_events().unwrap(), before_events);
}

#[test]
fn long_term_extraction_success_reports_transaction_lineage() {
    let platform = store_with_event_budget(16);
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
}

#[test]
fn agent_tool_feedback_event_budget_rejects_without_partial_experience() {
    let registry = registry();
    let (platform, runtime) = runtime_with_registry_and_event_budget(registry.clone(), 2);
    let before_events = platform.read_events().expect("events before");
    let before_skill_names = platform
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
        platform.skill_storage().list_names().unwrap(),
        before_skill_names
    );
    assert_eq!(platform.read_events().unwrap(), before_events);
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

    let snapshot = platform.export_store_snapshot().expect("snapshot");
    assert!(!snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace.starts_with("memory_graph_")));
}

#[test]
fn temporal_memory_graph_write_success_reports_transaction_lineage() {
    let (platform, runtime) = runtime_with_registry_and_event_budget(registry(), 12);

    let report = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node("node:release", "turn:release"),
                graph_node("node:verify", "turn:release"),
            ],
            edges: vec![graph_edge(
                "edge:release:verify",
                "node:release",
                "node:verify",
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
    assert!(report.index_revision.is_some());
    let transaction = report.transaction.as_ref().expect("transaction");
    assert_eq!(transaction.operation, "memory_graph.write");
    assert_eq!(
        transaction.planned_mutations,
        transaction.committed_mutations
    );
    assert!(!transaction.partial_write);

    let snapshot = platform.export_store_snapshot().expect("snapshot");
    for namespace in [
        "memory_graph_nodes",
        "memory_graph_edges",
        "memory_graph_backlinks",
        "memory_graph_indexes",
        "memory_graph_revisions",
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
    let platform = store_with_event_budget(3);
    platform
        .long_term_memory_store()
        .upsert_many(&[extraction_draft()], 1_800_000_000)
        .expect("seed long-term");
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
    let before_events = platform.read_events().expect("events before");

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
        .long_term_memory_store()
        .get(&record_id)
        .unwrap()
        .is_some());
    assert!(platform
        .long_term_memory_control_store()
        .get_long_term_control_tombstone(&record_id)
        .unwrap()
        .is_none());
    assert_eq!(platform.read_events().unwrap(), before_events);
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
    let before_events = platform.read_events().expect("events before");

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
        .long_term_memory_control_store()
        .list_long_term_governance_policies(10)
        .unwrap()
        .is_empty());
    assert_eq!(platform.read_events().unwrap(), before_events);
}
