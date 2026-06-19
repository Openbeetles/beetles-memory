#![allow(dead_code)]

use bm_sdk::{
    build_temporal_memory_graph_from_evidence, build_vault_migration_preflight,
    compile_edge_memory_budget_report, default_agent_subject_id, default_memory_space_id,
    plan_memory_autopilot_for_profile, promote_task_experience_to_procedure,
    rerank_recall_with_temporal_graph, ActorAttribution, AgentSkillDirConfig,
    AgentSkillDirectoryReport, AgentSkillProjectionAudit, AgentToolDescriptor,
    AgentToolExperienceGovernanceReport, AgentToolHint, AgentToolObservationDigest,
    AgentToolProjectionAudit, AgentToolRegistryRef, AgentToolRegistryReport,
    AgentToolRegistrySnapshot, AgentToolUsageFeedback, ConversationKey, DerivedMemoryPlane,
    DerivedMemoryRef, GraphRecallExpansionBudget, GraphRecallExpansionBudgetReport, HostOpaqueRef,
    HostRefRelation, HostRefVisibility, LLMRuntimeProjectionEnvelope, MemoryAutopilotInput,
    MemoryCapabilityCatalog, MemoryCapabilityPolicy, MemoryGovernancePolicyMutation,
    MemoryGovernancePolicyMutationReport, MemoryGovernanceSelector,
    MemoryGovernanceSuppressionDuration, MemoryGraphEvidence, MemoryGraphNodeKind, MemoryIdentity,
    MemoryLongTermControlView, MemoryLongTermDetailReport, MemoryLongTermDetailRequest,
    MemoryLongTermGovernancePolicy, MemoryLongTermListReport, MemoryLongTermListRequest,
    MemoryLongTermMutation, MemoryLongTermMutationReport, MemoryLongTermMutationRequest,
    MemoryLongTermPolicyRequest, MemoryLongTermSelector, MemoryLongTermTarget, MemoryProfile,
    MemoryRuntime, MemoryRuntimeSystemKind, MemoryScope, MemorySubjectVisibilityPolicy,
    MemoryTranscriptCommitRequest, MemoryTranscriptExportRequest, MemoryTranscriptLifecycleRequest,
    MemoryTranscriptRepairRequest, MemoryTranscriptReplayRequest, MemoryWriteRequest,
    PostReplyMemoryMaintenanceContext, PrivateDisclosureIntegrityReport,
    PrivateMaterialRedactionReport, ProceduralMemoryPromotionInput,
    ProceduralMemoryPromotionPolicy, ProfileId, ProjectedAgentSkillHint, PromptMemoryContextParams,
    PromptParticipationPlan, RedactedTranscriptSlice, SoulLifeProjectionReport, StoreBackendConfig,
    StorePlatform, SubjectKind, SubjectRegistry, SubjectRelationshipGraph, SubjectScopedRuntime,
    TranscriptEvidenceRef, TranscriptLifecycleTransition, TranscriptRedactionReason,
    TranscriptRedactionReportItem, TranscriptRepairIssue, TranscriptRepairIssueKind,
    TranscriptRepairReport, TranscriptReplayAudit, TranscriptReplayView, TranscriptTurnPage,
    TranscriptTurnRecord, VaultManifest, WorkIntegrityReport, AGENT_TOOL_NO_EXPERIENCE_REASON,
    AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH, AGENT_TOOL_REGISTRY_FORBIDDEN_BY_PROFILE,
};

fn prompt_context_contract_is_sdk_importable<'a>(
    params: PromptMemoryContextParams<'a>,
) -> PromptMemoryContextParams<'a> {
    params
}

fn sdk_agent_skill_contract_types_are_importable(
    _dir: Option<AgentSkillDirConfig>,
    _directory: Option<AgentSkillDirectoryReport>,
    _projection: Option<AgentSkillProjectionAudit>,
    _hint: Option<ProjectedAgentSkillHint>,
) {
}

#[allow(clippy::too_many_arguments)]
fn sdk_agent_tool_contract_types_are_importable(
    _descriptor: Option<AgentToolDescriptor>,
    _registry_ref: Option<AgentToolRegistryRef>,
    _registry: Option<AgentToolRegistrySnapshot>,
    _registry_report: Option<AgentToolRegistryReport>,
    _hint: Option<AgentToolHint>,
    _projection: Option<AgentToolProjectionAudit>,
    _observation: Option<AgentToolObservationDigest>,
    _feedback: Option<AgentToolUsageFeedback>,
    _governance: Option<AgentToolExperienceGovernanceReport>,
) {
    assert_eq!(
        AGENT_TOOL_NO_EXPERIENCE_REASON,
        "no_governed_tool_experience"
    );
    assert_eq!(
        AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH,
        "agent_tool_registry_fingerprint_mismatch"
    );
    assert_eq!(
        AGENT_TOOL_REGISTRY_FORBIDDEN_BY_PROFILE,
        "agent_tool_registry_forbidden_by_profile"
    );
}

fn post_reply_context_contract_is_sdk_importable<'a>(
    ctx: PostReplyMemoryMaintenanceContext<'a>,
) -> PostReplyMemoryMaintenanceContext<'a> {
    ctx
}

#[allow(clippy::too_many_arguments)]
fn sdk_runtime_contract_types_are_importable(
    _runtime: Option<MemoryRuntime>,
    _catalog: Option<MemoryCapabilityCatalog>,
    _policy: MemoryCapabilityPolicy,
    _identity: MemoryIdentity,
    _scope: MemoryScope,
    _write: Option<MemoryWriteRequest>,
    _long_term_list: Option<MemoryLongTermListRequest>,
    _long_term_detail: Option<MemoryLongTermDetailRequest>,
    _long_term_mutation: Option<MemoryLongTermMutationRequest>,
    _long_term_policy: Option<MemoryLongTermPolicyRequest>,
    _long_term_list_report: Option<MemoryLongTermListReport>,
    _long_term_detail_report: Option<MemoryLongTermDetailReport>,
    _long_term_mutation_report: Option<MemoryLongTermMutationReport>,
    _long_term_policy_report: Option<MemoryGovernancePolicyMutationReport>,
    _long_term_view: Option<MemoryLongTermControlView>,
    _long_term_target: Option<MemoryLongTermTarget>,
    _long_term_selector: Option<MemoryLongTermSelector>,
    _long_term_mutation_enum: Option<MemoryLongTermMutation>,
    _long_term_policy_enum: Option<MemoryGovernancePolicyMutation>,
    _long_term_policy_selector: Option<MemoryGovernanceSelector>,
    _long_term_policy_duration: Option<MemoryGovernanceSuppressionDuration>,
    _long_term_governance_policy: Option<MemoryLongTermGovernancePolicy>,
    _subject_visibility: Option<MemorySubjectVisibilityPolicy>,
    _registry: Option<SubjectRegistry>,
    _graph: Option<SubjectRelationshipGraph>,
    _scoped_runtime: Option<SubjectScopedRuntime>,
    _kind: Option<SubjectKind>,
) {
}

#[allow(clippy::too_many_arguments)]
fn sdk_transcript_contract_types_are_importable(
    _key: ConversationKey,
    _actor: ActorAttribution,
    _host_ref: HostOpaqueRef,
    _relation: HostRefRelation,
    _visibility: HostRefVisibility,
    _turn: Option<TranscriptTurnRecord>,
    _slice: Option<RedactedTranscriptSlice>,
    _commit: Option<MemoryTranscriptCommitRequest>,
    _replay: MemoryTranscriptReplayRequest,
    _lifecycle: MemoryTranscriptLifecycleRequest,
    _repair: MemoryTranscriptRepairRequest,
    _export: MemoryTranscriptExportRequest,
    _transition: TranscriptLifecycleTransition,
    _redaction_reason: TranscriptRedactionReason,
    _redaction_item: Option<TranscriptRedactionReportItem>,
    _replay_audit: Option<TranscriptReplayAudit>,
    _evidence_ref: Option<TranscriptEvidenceRef>,
    _derived_plane: Option<DerivedMemoryPlane>,
    _derived_ref: Option<DerivedMemoryRef>,
    _turn_page: Option<TranscriptTurnPage>,
    _repair_report: Option<TranscriptRepairReport>,
    _repair_issue: Option<TranscriptRepairIssue>,
    _repair_kind: TranscriptRepairIssueKind,
    _view: TranscriptReplayView,
) {
}

#[test]
fn transcript_replay_export_page_requests_are_public() {
    let replay = MemoryTranscriptReplayRequest {
        memory_space_id: "space".to_string(),
        channel_id: "channel".to_string(),
        conversation_id: "conversation".to_string(),
        limit: 1,
        cursor: Some("1:turn-a".to_string()),
        view: TranscriptReplayView::HostUi,
    };
    let export = MemoryTranscriptExportRequest {
        memory_space_id: replay.memory_space_id.clone(),
        channel_id: replay.channel_id.clone(),
        conversation_id: replay.conversation_id.clone(),
        limit: replay.limit,
        cursor: replay.cursor.clone(),
    };
    let repair = MemoryTranscriptRepairRequest {
        memory_space_id: replay.memory_space_id.clone(),
        channel_id: replay.channel_id.clone(),
        conversation_id: replay.conversation_id.clone(),
    };

    assert_eq!(export.cursor.as_deref(), Some("1:turn-a"));
    assert_eq!(repair.conversation_id, "conversation");
}

fn sdk_projection_report_set_types_are_importable(
    _runtime_projection: Option<LLMRuntimeProjectionEnvelope>,
    _life_projection: Option<SoulLifeProjectionReport>,
    _disclosure_integrity: Option<PrivateDisclosureIntegrityReport>,
    _work_integrity: Option<WorkIntegrityReport>,
) {
}

#[test]
fn sdk_runtime_does_not_use_post_reply_mental_privacy_rewriter() {
    let sdk_runtime_source = include_str!("../src/runtime.rs");

    assert!(!sdk_runtime_source.contains("run_mental_privacy_review"));
    assert!(!sdk_runtime_source.contains("MENTAL_PRIVACY_SYSTEM_PROMPT"));
}

#[test]
fn sdk_and_gateway_do_not_keep_flat_projection_renderer_switches() {
    let sdk_runtime_source = include_str!("../src/runtime.rs");
    let gateway_openai_source = include_str!("../../llm-gateway/src/openai.rs");
    let gateway_ollama_source = include_str!("../../llm-gateway/src/ollama.rs");

    for source in [
        sdk_runtime_source,
        gateway_openai_source,
        gateway_ollama_source,
    ] {
        for forbidden in [
            "sdk_projection_text_parts",
            "render_sdk_projection_block",
            "legacy_flat_projection",
            "flat_projection_compat",
            "use_flat_projection",
        ] {
            assert!(
                !source.contains(forbidden),
                "old flat projection renderer path leaked back: {forbidden}"
            );
        }
    }
}

#[test]
fn public_skill_surface_does_not_expose_memory_owned_agent_skill_crud() {
    let sdk_ops = include_str!("../src/ops.rs");
    let sdk_runtime = include_str!("../src/runtime.rs");
    let sdk_lib = include_str!("../src/lib.rs");

    for source in [sdk_ops, sdk_runtime, sdk_lib] {
        for forbidden in [
            concat!("Memory", "Skill", "Origin"),
            concat!("Memory", "Skill", "Kind"),
            concat!("Memory", "Skill", "Upsert", "Request"),
            concat!("Memory", "Skill", "List", "Request"),
            concat!("Memory", "Skill", "Detail", "Request"),
            concat!("Memory", "Skill", "SetEnabled", "Request"),
            concat!("Memory", "Skill", "Delete", "Request"),
            concat!("upsert", "_", "skill"),
            concat!("list", "_", "skills"),
        ] {
            assert!(
                !source.contains(forbidden),
                "old Skill CRUD public surface leaked back: {forbidden}"
            );
        }
    }
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
        .scope(MemoryScope::new("local", "chat-1").unwrap())
        .profile(ProfileId::ServerLinuxDevFull)
        .store_platform(store)
        .build()
        .unwrap();

    assert_eq!(runtime.identity().agent_id, "agent-main");
    assert_eq!(runtime.subject_id(), default_agent_subject_id("agent-main"));
    assert_eq!(
        runtime.memory_space_id(),
        default_memory_space_id("owner-default")
    );
    assert_eq!(default_agent_subject_id("agent-main"), "agent:agent-main");
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
    let rerank = rerank_recall_with_temporal_graph(
        "current",
        vec!["fact:current".to_string()],
        &graph,
        GraphRecallExpansionBudget::runtime_default(),
    );
    assert_eq!(rerank.selected_ids, vec!["fact:current"]);
    let _budget_report: GraphRecallExpansionBudgetReport = rerank.expansion_budget.clone();
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
