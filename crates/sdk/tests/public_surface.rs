#![allow(dead_code)]

mod support;

use bm_sdk::{
    build_temporal_memory_graph_from_evidence, build_vault_migration_preflight,
    compile_edge_memory_budget_report, default_agent_subject_id, default_memory_space_id,
    plan_memory_autopilot_for_profile, promote_task_experience_to_procedure,
    rerank_recall_with_temporal_graph, ActorAttribution, AgentSkillDirConfig,
    AgentSkillDirectoryReport, AgentSkillProjectionAudit, AgentToolDescriptor,
    AgentToolExperienceGovernanceReport, AgentToolHint, AgentToolObservationDigest,
    AgentToolProjectionAudit, AgentToolRegistryRef, AgentToolRegistryReport,
    AgentToolRegistrySnapshot, AgentToolUsageFeedback, ConversationKey, DerivedMemoryPlane,
    DerivedMemoryRef, GovernedStateMemoryCapability, GraphRecallExpansionBudget,
    GraphRecallExpansionBudgetReport, HostOpaqueRef, HostRefRelation, HostRefVisibility,
    MemoryAutopilotInput, MemoryCapabilityCatalog, MemoryCapabilityPolicy,
    MemoryConversationListRequest, MemoryGovernancePolicyMutation,
    MemoryGovernancePolicyMutationReport, MemoryGovernanceSelector,
    MemoryGovernanceSuppressionDuration, MemoryGraphEvidence, MemoryGraphNodeKind, MemoryIdentity,
    MemoryLearningCycleOutcome, MemoryLearningCycleRequest, MemoryLearningEngine,
    MemoryLongTermControlView, MemoryLongTermDetailReport, MemoryLongTermDetailRequest,
    MemoryLongTermGovernancePolicy, MemoryLongTermListReport, MemoryLongTermListRequest,
    MemoryLongTermMutation, MemoryLongTermMutationReport, MemoryLongTermMutationRequest,
    MemoryLongTermPolicyRequest, MemoryLongTermSelector, MemoryLongTermTarget, MemoryPrivacyClass,
    MemoryProfile, MemoryProjectionOutput, MemoryProjectionReport, MemoryProjectionSafeAuditReport,
    MemoryRuntime, MemoryRuntimeSystemKind, MemoryScope, MemorySubjectVisibilityPolicy,
    MemoryTranscriptActivityRequest, MemoryTranscriptCommitRequest, MemoryTranscriptExportRequest,
    MemoryTranscriptLifecycleRequest, MemoryTranscriptRepairRequest, MemoryTranscriptReplayRequest,
    MemoryTranscriptSearchRequest, MemoryTranscriptSearchScope, MemoryTranscriptTimelineRequest,
    MemoryWriteRequest, PostReplyMemoryMaintenanceContext, PrivateMaterialRedactionReport,
    ProceduralMemoryPromotionInput, ProceduralMemoryPromotionPolicy, ProfileId,
    ProjectedAgentSkillHint, PromptMemoryContextParams, PromptParticipationPlan,
    ProviderProjectionPayload, RedactedTranscriptSlice, RuntimeSkillRecallTransport,
    StoreBackendConfig, SubjectKind, SubjectRegistry, SubjectRelationshipGraph,
    SubjectScopedRuntime, TranscriptCatalogLifecycle, TranscriptEvidenceRef,
    TranscriptLifecycleTransition, TranscriptRedactionReason, TranscriptRedactionReportItem,
    TranscriptRepairIssue, TranscriptRepairIssueKind, TranscriptRepairReport,
    TranscriptReplayAudit, TranscriptReplayView, TranscriptSearchLifecycle, TranscriptSearchSort,
    TranscriptTimelineAnchor, TranscriptTurnPage, TranscriptTurnRecord, TranscriptUtcRange,
    VaultManifest, AGENT_TOOL_NO_EXPERIENCE_REASON, AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH,
    AGENT_TOOL_REGISTRY_FORBIDDEN_BY_PROFILE,
};

fn p8_contract_types_are_sdk_importable(
    _capability: Option<GovernedStateMemoryCapability>,
    _transport: RuntimeSkillRecallTransport,
) {
}

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

fn memory_learning_engine_contract_is_sdk_importable(
    _engine: Option<MemoryLearningEngine>,
    _request: Option<MemoryLearningCycleRequest>,
    _outcome: Option<MemoryLearningCycleOutcome>,
) {
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
    _catalog: MemoryConversationListRequest,
    _timeline: MemoryTranscriptTimelineRequest,
    _search: MemoryTranscriptSearchRequest,
    _activity: MemoryTranscriptActivityRequest,
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
fn transcript_catalog_timeline_search_activity_requests_are_public() {
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
    let key = ConversationKey::new(
        replay.memory_space_id.clone(),
        replay.channel_id.clone(),
        replay.conversation_id.clone(),
    )
    .unwrap();
    let catalog = MemoryConversationListRequest {
        channel_id: None,
        lifecycle: TranscriptCatalogLifecycle::ActiveOnly,
        limit: 8,
        cursor: None,
        view: TranscriptReplayView::HostUi,
    };
    let timeline = MemoryTranscriptTimelineRequest {
        channel_id: key.channel_id.clone(),
        conversation_id: key.conversation_id.clone(),
        anchor: TranscriptTimelineAnchor::Latest,
        limit: 8,
        cursor: None,
        view: TranscriptReplayView::HostUi,
    };
    let search = MemoryTranscriptSearchRequest {
        scope: MemoryTranscriptSearchScope::MountedSubject { channel_id: None },
        query_text: "memory".to_string(),
        sort: TranscriptSearchSort::RelevanceThenObservedAt,
        lifecycle: TranscriptSearchLifecycle::ActiveOnly,
        limit: 8,
        cursor: None,
        view: TranscriptReplayView::HostUi,
    };
    let activity = MemoryTranscriptActivityRequest {
        channel_id: key.channel_id,
        conversation_id: key.conversation_id,
        ranges: vec![TranscriptUtcRange {
            start_inclusive: 1,
            end_exclusive: 86_401,
        }],
        lifecycle: TranscriptSearchLifecycle::ActiveOnly,
        view: TranscriptReplayView::HostUi,
    };

    assert_eq!(export.cursor.as_deref(), Some("1:turn-a"));
    assert_eq!(repair.conversation_id, "conversation");
    assert_eq!(catalog.limit, 8);
    assert_eq!(timeline.limit, 8);
    assert_eq!(search.limit, 8);
    assert_eq!(activity.ranges[0].end_exclusive, 86_401);

    let _list = MemoryRuntime::list_conversations;
    let _timeline = MemoryRuntime::query_transcript_timeline;
    let _search = MemoryRuntime::search_transcripts;
    let _activity = MemoryRuntime::query_transcript_activity;
}

#[test]
fn transcript_public_surface_has_no_host_ui_window_or_cursor_authority() {
    let runtime_source = include_str!("../src/runtime.rs");
    let ops_source = include_str!("../src/ops.rs");
    let lib_source = include_str!("../src/lib.rs");

    for forbidden in [
        "replay_transcript_window",
        "MemoryTranscriptWindowRequest",
        "MemoryTranscriptWindowReport",
        "TranscriptHistoryCursorAuthority",
        "TranscriptHistoryPage",
    ] {
        assert!(!runtime_source.contains(forbidden), "{forbidden}");
        assert!(!ops_source.contains(forbidden), "{forbidden}");
        assert!(!lib_source.contains(forbidden), "{forbidden}");
    }
    assert!(!ops_source.contains("governance_context_digest"));
    assert!(!lib_source.contains("TranscriptCursorDisclosurePolicyV1"));
}

fn sdk_projection_report_set_types_are_importable(
    _output: Option<MemoryProjectionOutput>,
    _provider: Option<ProviderProjectionPayload>,
    _report: Option<MemoryProjectionReport>,
    _audit: Option<MemoryProjectionSafeAuditReport>,
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
fn production_sdk_surface_does_not_reexport_composable_worker_primitives() {
    let sdk_lib = include_str!("../src/lib.rs");
    let production_exports = sdk_lib
        .split("#[cfg(feature = \"nonproduction-replay-harness\")]\npub use ops::{")
        .next()
        .expect("production export section");
    for forbidden in [
        "MemoryGovernanceJobClaimRequest",
        "MemoryGovernanceJobRetryRequest",
        "MemoryGovernanceJobRenewRequest",
        "MemoryGovernanceJobRunRequest",
        "MemoryGovernanceJobBlockRequest",
        "MemoryGovernanceClaimedJobBlockRequest",
        "MemoryGovernanceJobResumeRequest",
        "MemoryGovernanceJobFailRequest",
        "MemoryGovernanceReconcileRequest",
    ] {
        assert!(
            !production_exports.contains(forbidden),
            "production SDK re-exported composable worker primitive: {forbidden}"
        );
    }
}

#[test]
fn profile_and_system_kind_aliases_are_unambiguous() {
    let runtime_kind: MemoryRuntimeSystemKind = MemoryProfile::Embedded.memory_system_kind();
    assert_eq!(runtime_kind, MemoryRuntimeSystemKind::EspCompact);
    assert_eq!(runtime_kind.memory_profile(), MemoryProfile::Embedded);
}

#[test]
fn sdk_runtime_uses_opaque_memory_store_handle_as_public_store_entry() {
    let store_surface = include_str!("../src/store.rs");
    let sdk_surface = include_str!("../src/lib.rs");
    let runtime_surface = include_str!("../src/runtime.rs");
    assert!(!store_surface.contains("pub fn read_events("));
    assert!(!store_surface.contains("pub fn read_file_store_events("));
    assert!(!sdk_surface.contains("profile_memory_system_kind, MemoryStoreEvent,"));
    assert!(!runtime_surface.contains("pub fn runtime_metrics_report_from_events("));
    assert!(!sdk_surface.contains("pub use crate::store_internal::*"));

    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
    )
    .unwrap();

    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").unwrap())
        .scope(MemoryScope::new("local", "chat-1").unwrap())
        .store(store)
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
    assert_eq!(runtime.capabilities().profile, support::host_test_profile());
}

#[test]
fn runtime_skill_operation_authority_stays_out_of_the_sdk_public_surface() {
    let sdk_root = include_str!("../src/lib.rs");
    for private_type in [
        "RuntimeSkillMaterializedViewRef",
        "RuntimeSkillOperationAuthorityRef",
        "RuntimeSkillRecallAuthority",
        "RuntimeSkillRecallBudgetAuthority",
        "RuntimeSkillRecallPlan",
        "RuntimeSkillRecallQuery",
    ] {
        assert!(
            !sdk_root.contains(private_type),
            "{private_type} must remain core-private operational authority"
        );
    }
}

#[test]
fn clean_break_removes_the_old_store_migration_surface() {
    let public_store = include_str!("../src/store.rs");
    let public_exports = include_str!("../src/lib.rs");
    let platform = include_str!("../src/store_internal/platform.rs");
    let file = include_str!("../src/store_internal/file.rs");
    let sqlite = include_str!("../src/store_internal/sqlite.rs");
    for source in [public_store, public_exports, platform, file, sqlite] {
        for forbidden in [
            "StoreMigrationReport",
            "migrate_v10_to_v11",
            "migrate_v10_snapshot_to_v11",
            "migrate_sqlite_v10_to_v11_explicit",
        ] {
            assert!(
                !source.contains(forbidden),
                "clean-break source still contains {forbidden}"
            );
        }
    }
}

#[test]
fn production_long_term_owner_mutation_is_not_exposed_by_host_store_surfaces() {
    let sdk_runtime = include_str!("../src/runtime.rs");
    let core_platform = include_str!("../../core/src/platform/mod.rs");
    let store_platform = include_str!("../src/store_internal/platform.rs");
    let shared_governance = include_str!("../../core/src/memory/shared_memory_governance.rs");

    assert!(!sdk_runtime.contains("pub fn platform("));
    assert!(!core_platform.contains("fn long_term_memory_store("));
    assert!(!store_platform.contains("impl LongTermMemoryStore for MemoryStoreHandle"));
    assert!(!store_platform.contains("pub fn scoped_long_term_memory_store("));
    assert!(!shared_governance.contains("pub fn write_governed_shared_memory("));
    assert!(!shared_governance.contains("pub fn write_governed_shared_memory_in_space("));
    assert!(!store_platform.contains("pub fn commit_mutation_batch("));
    assert!(!store_platform.contains("pub fn commit_mutation_batch_with_preconditions("));
}

#[test]
fn production_long_term_control_mutation_is_not_exposed_by_host_store_surfaces() {
    let core_platform = include_str!("../../core/src/platform/mod.rs");
    let store_platform = include_str!("../src/store_internal/platform.rs");

    assert!(!core_platform.contains("fn long_term_memory_control_store("));
    assert!(!store_platform.contains("impl LongTermMemoryControlStore for MemoryStoreHandle"));
    assert!(!store_platform.contains("pub struct ScopedLongTermMemoryControlStore"));
    assert!(!store_platform.contains("pub fn scoped_long_term_memory_control_store("));
}

#[test]
fn production_continuity_and_shared_memory_planners_are_read_only() {
    let continuity = include_str!("../../core/src/memory/continuity_snapshot.rs");
    let shared_governance = include_str!("../../core/src/memory/shared_memory_governance.rs");
    let hygiene = include_str!("../../core/src/memory/hygiene.rs");
    let extraction = include_str!("../../core/src/memory/long_term_extraction.rs");
    let maintenance = include_str!("../../core/src/memory/maintenance.rs");
    let task_learning = include_str!("../../core/src/task_execution/learning.rs");
    let import_context = continuity
        .split("pub struct ContinuitySnapshotImportContext")
        .nth(1)
        .and_then(|source| source.split('}').next())
        .expect("continuity import context source");

    assert!(import_context.contains("LongTermMemoryReadStore"));
    assert!(!import_context.contains("LongTermMemoryStore"));
    assert!(shared_governance.contains("S: LongTermMemoryReadStore + ?Sized"));
    for source in [hygiene, extraction, maintenance, task_learning] {
        assert!(!source.contains("long_term_memory_store: &'a dyn LongTermMemoryStore"));
    }
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
    assert_eq!(rerank.reranked_candidate_ids, vec!["fact:current"]);
    let _budget_report: GraphRecallExpansionBudgetReport = rerank.expansion_budget.clone();
    assert!(graph.gate.high_confidence_projection_allowed);
    assert_eq!(graph.compact_graph.nodes.len(), 1);

    let promotion = promote_task_experience_to_procedure(
        ProceduralMemoryPromotionInput {
            task_id: "task-1".to_string(),
            learning_id: "learning:task-1".to_string(),
            learning_digest:
                "sha256:1111111111111111111111111111111111111111111111111111111111111111"
                    .to_string(),
            trigger: "repeatable task".to_string(),
            procedure: "Use the proven path.".to_string(),
            constraints: vec!["stay in SDK".to_string()],
            failure_modes: vec!["adapter shortcut".to_string()],
            counterfactual_fix: "route through runtime".to_string(),
            evidence_refs: vec!["task:1".to_string(), "task:2".to_string()],
            quality_score: 80,
            repeated_evidence_count: 2,
            capability_affinity: vec!["sdk".to_string()],
            privacy_class: MemoryPrivacyClass::SharedWithSubject,
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
