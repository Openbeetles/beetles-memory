#![allow(dead_code)]

use bm_sdk::{
    build_temporal_memory_graph_from_evidence, build_vault_migration_preflight,
    compile_edge_memory_budget_report, plan_memory_autopilot_for_profile,
    promote_task_experience_to_procedure, rerank_recall_with_temporal_graph, MemoryAutopilotInput,
    MemoryCapabilityCatalog, MemoryCapabilityPolicy, MemoryGraphEvidence, MemoryGraphNodeKind,
    MemoryIdentity, MemoryProfile, MemoryRuntime, MemoryRuntimeSystemKind, MemoryScope,
    MemoryWriteRequest, PostReplyMemoryMaintenanceContext, PrivateMaterialRedactionReport,
    ProceduralMemoryPromotionInput, ProceduralMemoryPromotionPolicy, ProfileId,
    PromptMemoryContextParams, PromptParticipationPlan, StoreBackendConfig, StorePlatform,
    VaultManifest,
};

fn prompt_context_contract_is_sdk_importable<'a>(
    params: PromptMemoryContextParams<'a>,
) -> PromptMemoryContextParams<'a> {
    params
}

fn post_reply_context_contract_is_sdk_importable<'a>(
    ctx: PostReplyMemoryMaintenanceContext<'a>,
) -> PostReplyMemoryMaintenanceContext<'a> {
    ctx
}

fn sdk_runtime_contract_types_are_importable(
    _runtime: Option<MemoryRuntime>,
    _catalog: Option<MemoryCapabilityCatalog>,
    _policy: MemoryCapabilityPolicy,
    _identity: MemoryIdentity,
    _scope: MemoryScope,
    _write: Option<MemoryWriteRequest>,
) {
}

#[test]
fn profile_and_system_kind_aliases_are_unambiguous() {
    let runtime_kind: MemoryRuntimeSystemKind = MemoryProfile::Embedded.memory_system_kind();
    assert_eq!(runtime_kind, MemoryRuntimeSystemKind::EspCompact);
    assert_eq!(runtime_kind.memory_profile(), MemoryProfile::Embedded);
}

#[test]
fn sdk_runtime_uses_store_platform_as_public_store_entry() {
    let store = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();

    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").unwrap())
        .subject_id("subject-default")
        .scope(MemoryScope::new("local", "chat-1").unwrap())
        .profile(ProfileId::ServerLinuxDevFull)
        .store_platform(store)
        .build()
        .unwrap();

    assert_eq!(runtime.identity().agent_id, "agent-main");
    assert_eq!(runtime.subject_id(), "subject-default");
    assert_eq!(runtime.scope().chat_id, "chat-1");
    assert_eq!(
        runtime.capabilities().profile,
        ProfileId::ServerLinuxDevFull
    );
}

#[test]
fn prompt_participation_plan_is_available_from_sdk() {
    let plan = PromptParticipationPlan::embedded_first_turn_default();
    assert!(plan.load_l1_constitutional);
    assert!(!plan.load_l2_governed_recall);
}

#[test]
fn next_gen_builders_are_sdk_public_without_adapter_ownership() {
    let graph = build_temporal_memory_graph_from_evidence(vec![MemoryGraphEvidence {
        node_id: "fact:current".to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: "Current fact".to_string(),
        source_kind: "turn_ledger".to_string(),
        source_id: "turn:1".to_string(),
        fingerprint: "fp-1".to_string(),
        observed_at: 1,
        supports: Vec::new(),
        supersedes: None,
    }]);
    let rerank =
        rerank_recall_with_temporal_graph("current", vec!["fact:current".to_string()], &graph);
    assert_eq!(rerank.selected_ids, vec!["fact:current"]);
    assert!(graph.gate.high_confidence_projection_allowed);
    assert_eq!(graph.compact_graph.nodes.len(), 1);

    let promotion = promote_task_experience_to_procedure(
        ProceduralMemoryPromotionInput {
            task_id: "task-1".to_string(),
            trigger: "repeatable task".to_string(),
            procedure: "Use the proven path.".to_string(),
            constraints: vec!["stay in SDK".to_string()],
            failure_modes: vec!["adapter shortcut".to_string()],
            counterfactual_fix: "route through runtime".to_string(),
            evidence_refs: vec!["task:1".to_string(), "task:2".to_string()],
            quality_score: 80,
            repeated_evidence_count: 2,
            capability_affinity: vec!["sdk".to_string()],
        },
        ProceduralMemoryPromotionPolicy::default(),
    );
    assert!(promotion.promoted);

    let autopilot = plan_memory_autopilot_for_profile(MemoryAutopilotInput {
        profile: ProfileId::EspEmbeddedSdk,
        pressure: "critical".to_string(),
        recovery_safe_mode: true,
        pending_stale_items: 1,
        pending_conflicts: 1,
        privacy_risk_count: 0,
    });
    assert_eq!(autopilot.mutation_policy, "proposal_only");

    let budget = compile_edge_memory_budget_report(ProfileId::EspStandaloneMemory, 1, 2, 3, 4, 5);
    assert_eq!(budget.profile, ProfileId::EspStandaloneMemory);

    let preflight = build_vault_migration_preflight(
        VaultManifest {
            identity_id: "owner-default".to_string(),
            profile: ProfileId::ServerLinuxDevFull,
            store_backend: "file".to_string(),
            snapshot_fingerprint: "state".to_string(),
            event_fingerprint: "event".to_string(),
            privacy_policy_fingerprint: "privacy".to_string(),
        },
        ProfileId::DesktopMacosEmbeddedSdk,
        PrivateMaterialRedactionReport {
            surface: "export_preview".to_string(),
            checked_refs: Vec::new(),
            redacted_refs: Vec::new(),
            raw_private_leak_count: 0,
        },
        "schema",
        "schema",
    );
    assert!(preflight.passed);
}
