//! 自治运行层：由 LLM 决定是否经营自己的内在空间。
#![allow(clippy::too_many_arguments)]

mod governance;
mod llm;
mod scheduler;
mod state;

use crate::bus::{IngressKind, PcMsg, SystemInboundTx};
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::platform::SkillStorage;
use crate::task::TaskStore;
use crate::task_execution::{
    active_task_run_for_chat, run_task_learning_maintenance, TaskArtifactRecord, TaskArtifactStore,
    TaskLearningMaintenanceContext, TaskLearningMaintenanceOutcome, TaskLearningRecord,
    TaskLearningStore, TaskRunRecord, TaskRunStore,
};
use crate::util::{current_unix_secs, scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use self::governance::{
    apply_personality_runtime_governance_gate, detect_boundary_flush_signal,
    normalize_initial_self_runtime_decision, normalize_runtime_distillation_decisions,
    normalize_runtime_source_id, re_finalize_staged_self_runtime_decision,
    SelfRuntimeBoundarySignal,
};
#[cfg(test)]
use self::governance::{
    normalize_self_runtime_decision, PersonaDistillationSnapshot, SelfRuntimeBoundaryReason,
};
use self::llm::decide_self_runtime;
pub use self::scheduler::{
    enqueue_self_runtime_idle_tick, enqueue_self_runtime_operator_request,
    enqueue_self_runtime_post_reply, self_runtime_tick,
};
use self::scheduler::{
    idle_memory_hygiene_budget_allows_run, self_runtime_post_reply_no_trigger_reason,
};
#[cfg(test)]
use self::scheduler::{idle_self_runtime_due, should_enqueue_self_runtime_post_reply_with_state};
use self::state::{
    compile_self_runtime_relationship_constitutional_input, load_self_runtime_state,
    sync_self_runtime_relationship_portfolio, sync_self_runtime_relationship_topology,
};

use super::{
    autonomy_idle_interval_secs, build_archive_evidence_block,
    build_felt_significance_refresh_input, build_inner_conflict_refresh_input, build_self_state,
    build_temperament_continuity_refresh_input, build_world_snapshot_from_commitments,
    compile_relationship_constitutional_runtime_input_v1, compile_subject_shell,
    compile_subject_soul_relationship_runtime_view_v1, compute_core_revision_governance_digest,
    decide_self_runtime_authority, derive_personality_runtime_governance_gate_from_inspection,
    inspect_personality_governance,
    llm_json::{
        get_object_bool, get_object_string_list, get_object_text, parse_llm_json_payload,
        LlmJsonPayload,
    },
    load_recent_persona_evidence, load_world_snapshot_reminders, load_world_snapshot_tasks,
    memory_capability_profile, memory_policy, mental_privacy_safety_baseline,
    plan_self_authored_core_refresh_with_state, relationship_scope_id,
    render_autonomy_strategy_block, render_core_revision_governance_block,
    render_execution_state_block, render_internal_memory_topology_block,
    render_mental_privacy_boundary_block, render_persistent_self_authored_core_block,
    render_private_memory_boundary_block, render_recent_persona_evidence_block,
    render_relationship_constitution_block, render_relationship_portfolio_block,
    render_relationship_topology_block, render_self_authored_core_block, render_self_state_block,
    render_turn_adversarial_arena_ledger_block, render_turn_counterfactual_ledger_block,
    render_world_sense_block, render_world_snapshot_block, resolve_relationship_id,
    run_autonomy_strategy_refresh_with_state, run_boundary_persona_refresh_with_state,
    run_felt_significance_refresh_with_state, run_inner_conflict_refresh_with_state,
    run_inner_life_refresh_with_state, run_memory_governance_kernel, run_memory_hygiene_jobs,
    run_outer_voice_refresh_with_state, run_private_doc_workspace_refresh_with_state,
    run_private_garden_governance_with_state, run_self_continuity_refresh_with_state,
    run_self_model_refresh_with_state, run_temperament_continuity_refresh_with_state,
    run_world_sense_refresh_with_state, select_relationship_portfolio_targets,
    sync_relationship_portfolio, touch_relationship_portfolio_selection,
    touch_self_continuity_runtime, upsert_relationship_topology_entry, whole_record_lease_advanced,
    AutonomyGovernanceTendency, AutonomyStrategyRefreshContext, AutonomyStrategyRefreshInput,
    AutonomyStrategyRefreshOutcome, AutonomyStrategyStore, BoundaryPersonaRefreshContext,
    BoundaryPersonaRefreshInput, BoundaryPersonaRefreshOutcome, ContinuityCapsuleDraft,
    ContinuityCapsuleKind, ContinuityCapsuleScopeKind, ContinuityCapsuleSource,
    ContinuityCapsuleStatus, ContinuityCapsuleStore, ContinuityCapsuleWriteOutcome,
    CoreRevisionGovernanceDigest, CoreRevisionLedgerStore, ExecutionStateStore, FeltSignificance,
    FeltSignificanceRefreshCandidate, FeltSignificanceRefreshOutcome, FeltSignificanceStore,
    InnerConflict, InnerConflictRefreshCandidate, InnerConflictRefreshOutcome, InnerConflictStore,
    InnerLifeRefreshContext, InnerLifeRefreshInput, InnerLifeRefreshOutcome, InnerLifeStore,
    InternalMemoryLayerFocus, LongTermMemoryReadStore, MemoryGovernanceContext,
    MemoryGovernanceInput, MemoryHygieneContext, MemoryProfile, MemoryStore, MemorySystemKind,
    MentalPrivacyState, MentalPrivacyStore, OuterVoiceRefreshContext, OuterVoiceRefreshInput,
    OuterVoiceRefreshOutcome, OuterVoiceStore, PersonalityGovernanceInspectionInput,
    PrivateDocStore, PrivateDocWorkspaceRefreshContext, PrivateDocWorkspaceRefreshInput,
    PrivateDocWorkspaceRefreshOutcome, PrivateGardenGovernanceContext,
    PrivateGardenGovernanceInput, PrivateGardenGovernanceOutcome, PrivateGardenStore,
    RelationshipConstitution, RelationshipConstitutionStore, RelationshipPortfolio,
    RelationshipPortfolioSelectorInput, RelationshipPortfolioStore, RelationshipTopology,
    RelationshipTopologyStore, RemindAtStore, SelfAuthoredCoreRefreshInput,
    SelfAuthoredCoreRefreshPlanV1, SelfAuthoredCoreStore, SelfContinuityRefreshContext,
    SelfContinuityRefreshInput, SelfContinuityRefreshOutcome, SelfContinuityStore,
    SelfMemorySpaceBottleneck, SelfMemorySpacePressure, SelfModelRefreshContext,
    SelfModelRefreshInput, SelfModelRefreshOutcome, SelfModelStore, SelfRuntimeAuthorityPlan,
    SelfState, SessionStore, SessionSummaryStore, SharedFactualPlaneSnapshot,
    SharedFactualReconcileAction, SubjectShell, SubjectShellCompileInput,
    SubjectSoulRelationshipRuntimeInputV1, TemperamentContinuity,
    TemperamentContinuityRefreshCandidate, TemperamentContinuityRefreshOutcome,
    TemperamentContinuityStore, TurnContinuityEvidenceStore, TurnLedgerStore,
    WorldSenseRefreshContext, WorldSenseRefreshInput, WorldSenseRefreshOutcome, WorldSenseStore,
    WorldSnapshotContext,
};

pub const SELF_RUNTIME_SYSTEM_PROMPT: &str = "You govern the assistant's inward autonomy runtime. Respect the current autonomy strategy unless the latest world state, self-state, or recent multi-turn persona evidence clearly requires a different emphasis. Return JSON only: one object with fields refresh_inner_life, inner_life_intent, refresh_private_docs, private_docs_intent, private_docs_action, refresh_private_garden, private_garden_intent, private_garden_action, refresh_self_model, self_model_intent, self_model_sources, refresh_self_continuity, self_continuity_intent, self_continuity_sources, refresh_self_authored_core, self_authored_core_intent, self_authored_core_sources, refresh_boundary_persona, boundary_persona_intent, refresh_outer_voice, outer_voice_intent, outer_voice_sources, boundary_flush, boundary_flush_reason, request_factual_refresh, factual_reconcile_action, factual_reconcile_intent. Use true only when that layer should change now. Runtime governance actions are hold, rewrite, compress, or cleanup. factual_reconcile_action is hold, reinforce, correct, conflict, or stale. self_model, self_continuity, self_authored_core, boundary_persona, and outer_voice are upward distillation layers: refresh them only when private evolution or newer world/boundary state has produced a better stable core that should influence future main replies. self_authored_core is the board-level core above chat relationships; do not promote one-turn spikes or one-chat quirks into it. Relationship portfolio is the board-level governance layer above relationship overlays. Relationship constitution is the formal board-to-relation contract: respect it when deciding how much a relation may drift, which local layers need realignment, and whether any relation may push upward into board-level distillation. Source lists should name the layers that actually deserve upward distillation, such as inner_life, private_docs, private_garden, self_model, self_continuity, self_authored_core, boundary_persona, outer_voice, world_sense, autonomy_strategy, recent_persona_evidence, or recent_transcript. Treat recent persona evidence as multi-turn support, never as one-turn automatic promotion authority. Operational traces such as task scope, response mode, pressure, tool usage, or reply scope are not enough to justify upward distillation on their own. Favor autonomy, but do not churn memory without gain.";
pub const SELF_RUNTIME_CHANNEL: &str = "_self_runtime";
const SELF_RUNTIME_POST_REPLY_DELAY_MS: u64 = 1_500;
const SELF_RUNTIME_IDLE_TICK_DELAY_MS: u64 = 5_000;

pub(super) fn self_runtime_private_garden_doc_limit(profile: MemoryProfile) -> usize {
    let policy = memory_policy(profile);
    policy
        .private_garden
        .recent_doc_count
        .max(policy.private_garden_governance.existing_doc_count)
        .max(1)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelfRuntimeTrigger {
    PostReply,
    IdleTick,
    OperatorRequested,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfRuntimeJobPayload {
    pub trigger: SelfRuntimeTrigger,
    #[serde(default)]
    pub source_channel: String,
    #[serde(default)]
    pub user_content: String,
    #[serde(default)]
    pub reply_content: String,
    #[serde(default)]
    pub tool_calls: u32,
    #[serde(default)]
    pub external_content_used: bool,
    pub now_secs: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct SelfRuntimeDecision {
    #[serde(default)]
    pub refresh_inner_life: bool,
    #[serde(default)]
    pub inner_life_intent: String,
    #[serde(default)]
    pub refresh_private_docs: bool,
    #[serde(default)]
    pub private_docs_intent: String,
    #[serde(default)]
    pub private_docs_action: SelfRuntimeGovernanceAction,
    #[serde(default)]
    pub refresh_self_model: bool,
    #[serde(default)]
    pub self_model_intent: String,
    #[serde(default)]
    pub self_model_sources: Vec<String>,
    #[serde(default)]
    pub refresh_self_authored_core: bool,
    #[serde(default)]
    pub self_authored_core_intent: String,
    #[serde(default)]
    pub self_authored_core_sources: Vec<String>,
    #[serde(default)]
    pub refresh_self_continuity: bool,
    #[serde(default)]
    pub self_continuity_intent: String,
    #[serde(default)]
    pub self_continuity_sources: Vec<String>,
    #[serde(default)]
    pub refresh_private_garden: bool,
    #[serde(default)]
    pub private_garden_intent: String,
    #[serde(default)]
    pub private_garden_action: SelfRuntimeGovernanceAction,
    #[serde(default)]
    pub refresh_boundary_persona: bool,
    #[serde(default)]
    pub boundary_persona_intent: String,
    #[serde(default)]
    pub refresh_outer_voice: bool,
    #[serde(default)]
    pub outer_voice_intent: String,
    #[serde(default)]
    pub outer_voice_sources: Vec<String>,
    #[serde(default)]
    pub boundary_flush: bool,
    #[serde(default)]
    pub boundary_flush_reason: String,
    #[serde(default)]
    pub request_factual_refresh: bool,
    #[serde(default)]
    pub factual_reconcile_action: SharedFactualReconcileAction,
    #[serde(default)]
    pub factual_reconcile_intent: String,
}

pub struct SelfRuntimeContext<'a> {
    pub mounted_subject_id: &'a str,
    /// Exact active relationship owner. `None` keeps deterministic single-agent derivation.
    pub active_relationship_id: Option<&'a str>,
    pub memory_system_kind: crate::memory::MemorySystemKind,
    pub session_store: &'a dyn SessionStore,
    pub memory_store: &'a dyn MemoryStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryReadStore,
    pub continuity_capsule_store: &'a dyn ContinuityCapsuleStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_authored_core_store: &'a dyn SelfAuthoredCoreStore,
    pub core_revision_ledger_store: &'a dyn CoreRevisionLedgerStore,
    pub relationship_constitutional_read_store: &'a dyn SubjectSoulRelationshipRuntimeReadStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub felt_significance_store: &'a dyn FeltSignificanceStore,
    pub temperament_continuity_store: &'a dyn TemperamentContinuityStore,
    pub inner_conflict_store: &'a dyn InnerConflictStore,
    pub relationship_portfolio_store: &'a dyn RelationshipPortfolioStore,
    pub relationship_topology_store: &'a dyn RelationshipTopologyStore,
    pub world_sense_store: &'a dyn WorldSenseStore,
    pub autonomy_strategy_store: &'a dyn AutonomyStrategyStore,
    pub outer_voice_store: &'a dyn OuterVoiceStore,
    pub mental_privacy_store: &'a dyn MentalPrivacyStore,
    pub remind_store: &'a dyn RemindAtStore,
    pub task_store: &'a dyn TaskStore,
    pub task_run_store: &'a dyn TaskRunStore,
    pub task_artifact_store: &'a dyn TaskArtifactStore,
    pub task_learning_store: &'a dyn TaskLearningStore,
    pub turn_continuity_evidence_store: &'a dyn TurnContinuityEvidenceStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
    pub skill_storage: &'a dyn SkillStorage,
}

pub trait SubjectSoulRelationshipRuntimeReadStore: Send + Sync {
    fn get(
        &self,
        mounted_subject_id: &str,
        relationship_id: &str,
    ) -> Result<Option<SubjectSoulRelationshipRuntimeInputV1>>;
}

struct ReadOnlyDerivedRelationshipConstitutionStore<'a> {
    scope_id: &'a str,
    value: Option<&'a RelationshipConstitution>,
}

impl RelationshipConstitutionStore for ReadOnlyDerivedRelationshipConstitutionStore<'_> {
    fn get(&self, scope_id: &str) -> Result<Option<RelationshipConstitution>> {
        Ok((scope_id == self.scope_id)
            .then(|| self.value.cloned())
            .flatten())
    }

    fn set(&self, _scope_id: &str, _constitution: &RelationshipConstitution) -> Result<()> {
        Err(crate::error::Error::config(
            "subject_soul_relationship_runtime_view",
            "derived relationship constitutional input is read-only",
        ))
    }

    fn clear(&self, _scope_id: &str) -> Result<()> {
        Err(crate::error::Error::config(
            "subject_soul_relationship_runtime_view",
            "derived relationship constitutional input is read-only",
        ))
    }
}

pub struct SelfRuntimeOutcome {
    pub decision: Option<SelfRuntimeDecision>,
    pub world_sense_result: Result<WorldSenseRefreshOutcome>,
    pub autonomy_strategy_result: Result<AutonomyStrategyRefreshOutcome>,
    pub inner_life_result: Result<InnerLifeRefreshOutcome>,
    pub felt_significance_result: Result<FeltSignificanceRefreshOutcome>,
    pub temperament_continuity_result: Result<TemperamentContinuityRefreshOutcome>,
    pub inner_conflict_result: Result<InnerConflictRefreshOutcome>,
    pub private_doc_result: Result<PrivateDocWorkspaceRefreshOutcome>,
    pub self_model_result: Result<SelfModelRefreshOutcome>,
    pub self_authored_core_result: Result<SelfAuthoredCoreRefreshPlanV1>,
    pub self_continuity_result: Result<SelfContinuityRefreshOutcome>,
    pub task_learning_result: Result<TaskLearningMaintenanceOutcome>,
    pub private_garden_result: Result<PrivateGardenGovernanceOutcome>,
    pub boundary_persona_result: Result<BoundaryPersonaRefreshOutcome>,
    pub outer_voice_result: Result<OuterVoiceRefreshOutcome>,
}

/// A typed, non-persistent write journal produced while evaluating one self-runtime cycle.
///
/// These are owner post-images or owner-native operations, never host classification inputs.
/// SDK/runtime callers may pre-seed the journal with effects produced by another Core planner
/// (for example private-garden governance) so the broader cycle observes them read-your-writes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SelfRuntimePlannedEffectV1 {
    SetWorldSense {
        scope_id: String,
        value: crate::memory::WorldSense,
    },
    ClearWorldSense {
        scope_id: String,
    },
    SetAutonomyStrategy {
        scope_id: String,
        value: crate::memory::AutonomyStrategy,
    },
    ClearAutonomyStrategy {
        scope_id: String,
    },
    SetInnerLife {
        scope_id: String,
        value: crate::memory::InnerLife,
    },
    ClearInnerLife {
        scope_id: String,
    },
    SetSelfModel {
        scope_id: String,
        value: crate::memory::SelfModel,
    },
    ClearSelfModel {
        scope_id: String,
    },
    SetSelfContinuity {
        scope_id: String,
        value: crate::memory::SelfContinuity,
    },
    ClearSelfContinuity {
        scope_id: String,
    },
    SetFeltSignificance {
        scope_id: String,
        value: crate::memory::FeltSignificance,
    },
    ClearFeltSignificance {
        scope_id: String,
    },
    SetTemperamentContinuity {
        scope_id: String,
        value: crate::memory::TemperamentContinuity,
    },
    ClearTemperamentContinuity {
        scope_id: String,
    },
    SetInnerConflict {
        scope_id: String,
        value: crate::memory::InnerConflict,
    },
    ClearInnerConflict {
        scope_id: String,
    },
    SetPrivateDoc {
        scope_id: String,
        value: crate::memory::PrivateDocWorkspace,
    },
    ClearPrivateDoc {
        scope_id: String,
    },
    SetMentalPrivacy {
        scope_id: String,
        value: crate::memory::MentalPrivacyState,
    },
    ClearMentalPrivacy {
        scope_id: String,
    },
    SetOuterVoice {
        scope_id: String,
        value: crate::memory::OuterVoice,
    },
    ClearOuterVoice {
        scope_id: String,
    },
    SetRelationshipTopology {
        scope_id: String,
        value: crate::memory::RelationshipTopology,
    },
    ClearRelationshipTopology {
        scope_id: String,
    },
    SetRelationshipPortfolio {
        scope_id: String,
        value: crate::memory::RelationshipPortfolio,
    },
    ClearRelationshipPortfolio {
        scope_id: String,
    },
    UpsertPrivateGardenDoc {
        subject_id: String,
        document: crate::memory::PrivateGardenDoc,
    },
    DeletePrivateGardenDoc {
        subject_id: String,
        doc_path: String,
    },
    UpsertContinuityCapsules {
        drafts: Vec<crate::memory::ContinuityCapsuleDraft>,
        now_secs: u64,
    },
    UpsertTaskLearning {
        record: TaskLearningRecord,
    },
    PutTaskArtifact {
        record: TaskArtifactRecord,
    },
    DeleteTaskArtifact {
        run_id: String,
        artifact_id: String,
    },
    WriteRuntimeSkill {
        name: String,
        content: Vec<u8>,
    },
    RemoveRuntimeSkill {
        name: String,
    },
    SetLegacyMemory {
        content: String,
    },
    WriteDailyNote {
        name: String,
        content: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfRuntimeInitialPlanningStateV1 {
    pub owner_subject_id: String,
    #[serde(default)]
    pub planned_effects: Vec<SelfRuntimePlannedEffectV1>,
}

pub struct SelfRuntimeExecutionPlanV1 {
    pub operation_id: String,
    pub owner_subject_id: String,
    pub outcome: Box<SelfRuntimeOutcome>,
    pub planned_effects: Vec<SelfRuntimePlannedEffectV1>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SelfRuntimeLoadHealth {
    issues: Vec<SelfRuntimeLoadIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelfRuntimeLoadIssue {
    layer: &'static str,
    stage: &'static str,
    detail: String,
}

impl SelfRuntimeLoadHealth {
    fn record(&mut self, layer: &'static str, error: &crate::error::Error) {
        let detail = truncate_content_to_max(error.to_string().trim(), 160)
            .trim()
            .to_string();
        if self
            .issues
            .iter()
            .any(|issue| issue.layer == layer && issue.stage == error.stage())
        {
            return;
        }
        self.issues.push(SelfRuntimeLoadIssue {
            layer,
            stage: error.stage(),
            detail,
        });
    }

    fn has_failures(&self) -> bool {
        !self.issues.is_empty()
    }

    fn has_issue_for(&self, layer: &'static str) -> bool {
        self.issues.iter().any(|issue| issue.layer == layer)
    }

    fn summary(&self) -> String {
        let joined = self
            .issues
            .iter()
            .map(|issue| format!("{}@{}={}", issue.layer, issue.stage, issue.detail))
            .collect::<Vec<_>>()
            .join("; ");
        truncate_content_to_max(joined.trim(), 512)
            .trim()
            .to_string()
    }
}

struct LoadedSelfRuntimeState {
    load_health: SelfRuntimeLoadHealth,
    summary_text: Option<String>,
    execution_state: Option<crate::memory::ExecutionState>,
    self_model: Option<crate::memory::SelfModel>,
    self_authored_core: Option<crate::memory::SelfAuthoredCore>,
    core_revision_ledger: Option<crate::memory::CoreRevisionLedger>,
    core_revision_governance: CoreRevisionGovernanceDigest,
    private_docs: Option<crate::memory::PrivateDocWorkspace>,
    private_garden_docs: Vec<crate::memory::PrivateGardenDocRecord>,
    inner_life: Option<crate::memory::InnerLife>,
    self_continuity: Option<crate::memory::SelfContinuity>,
    subject_shell: Option<SubjectShell>,
    felt_significance: Option<FeltSignificance>,
    temperament_continuity: Option<TemperamentContinuity>,
    inner_conflict: Option<InnerConflict>,
    relationship_portfolio: Option<crate::memory::RelationshipPortfolio>,
    relationship_topology: Option<crate::memory::RelationshipTopology>,
    relationship_constitutional_input: Option<SubjectSoulRelationshipRuntimeInputV1>,
    relationship_constitution: Option<crate::memory::RelationshipConstitution>,
    world_sense: Option<crate::memory::WorldSense>,
    autonomy_strategy: Option<crate::memory::AutonomyStrategy>,
    outer_voice: Option<crate::memory::OuterVoice>,
    mental_privacy_state: Option<crate::memory::MentalPrivacyState>,
    recent_persona_evidence: Option<crate::memory::RecentPersonaEvidence>,
    sandbox_probe_text: Option<String>,
    active_relationship_scope_id: String,
    active_relationship_channel: String,
    prior_user_channel: String,
    world_snapshot: crate::memory::WorldSnapshot,
    recent: Vec<crate::memory::SessionMessage>,
}

struct SelfRuntimeRefreshPrelude {
    world_sense_result: Result<WorldSenseRefreshOutcome>,
    autonomy_strategy_result: Result<AutonomyStrategyRefreshOutcome>,
    refreshed_world_sense: Option<crate::memory::WorldSense>,
    refreshed_autonomy_strategy: Option<crate::memory::AutonomyStrategy>,
    runtime_self_state: SelfState,
}

struct SelfRuntimeActionResults {
    decision: Option<SelfRuntimeDecision>,
    inner_life_result: Result<InnerLifeRefreshOutcome>,
    felt_significance_result: Result<FeltSignificanceRefreshOutcome>,
    temperament_continuity_result: Result<TemperamentContinuityRefreshOutcome>,
    inner_conflict_result: Result<InnerConflictRefreshOutcome>,
    private_doc_result: Result<PrivateDocWorkspaceRefreshOutcome>,
    self_model_result: Result<SelfModelRefreshOutcome>,
    self_authored_core_result: Result<SelfAuthoredCoreRefreshPlanV1>,
    self_continuity_result: Result<SelfContinuityRefreshOutcome>,
    task_learning_result: Result<TaskLearningMaintenanceOutcome>,
    private_garden_result: Result<PrivateGardenGovernanceOutcome>,
    boundary_persona_result: Result<BoundaryPersonaRefreshOutcome>,
    outer_voice_result: Result<OuterVoiceRefreshOutcome>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelfRuntimeGovernanceAction {
    #[default]
    Hold,
    Rewrite,
    Compress,
    Cleanup,
}

impl SelfRuntimeGovernanceAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::Rewrite => "rewrite",
            Self::Compress => "compress",
            Self::Cleanup => "cleanup",
        }
    }

    fn from_text(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "rewrite" => Self::Rewrite,
            "compress" => Self::Compress,
            "cleanup" | "clean_up" | "clean-up" | "prune" => Self::Cleanup,
            _ => Self::Hold,
        }
    }
}

fn self_runtime_ingress(trigger: SelfRuntimeTrigger) -> IngressKind {
    match trigger {
        SelfRuntimeTrigger::PostReply => IngressKind::User,
        SelfRuntimeTrigger::IdleTick => IngressKind::System,
        SelfRuntimeTrigger::OperatorRequested => IngressKind::System,
    }
}

fn retain_runtime_sources_for_authority(
    sources: &mut Vec<String>,
    authority_plan: SelfRuntimeAuthorityPlan,
) {
    sources.retain(|source| authority_plan.allows_source_id(source.as_str()));
}

fn apply_self_runtime_authority_plan(
    decision: &mut SelfRuntimeDecision,
    authority_plan: SelfRuntimeAuthorityPlan,
) {
    if !authority_plan.allow_direct_private_docs {
        decision.refresh_private_docs = false;
        decision.private_docs_intent.clear();
        decision.private_docs_action = SelfRuntimeGovernanceAction::Hold;
    }
    if !authority_plan.allow_direct_private_garden {
        decision.refresh_private_garden = false;
        decision.private_garden_intent.clear();
        decision.private_garden_action = SelfRuntimeGovernanceAction::Hold;
    }
    if !authority_plan.allow_direct_inner_life {
        decision.refresh_inner_life = false;
        decision.inner_life_intent.clear();
    }
    if !authority_plan.allow_direct_self_model {
        decision.refresh_self_model = false;
        decision.self_model_intent.clear();
        decision.self_model_sources.clear();
    }
    if !authority_plan.allow_direct_self_authored_core {
        decision.refresh_self_authored_core = false;
        decision.self_authored_core_intent.clear();
        decision.self_authored_core_sources.clear();
    }
    if !authority_plan.allow_direct_self_continuity {
        decision.refresh_self_continuity = false;
        decision.self_continuity_intent.clear();
        decision.self_continuity_sources.clear();
    }
    if !authority_plan.allow_direct_boundary_persona {
        decision.refresh_boundary_persona = false;
        decision.boundary_persona_intent.clear();
    }
    if !authority_plan.allow_direct_outer_voice {
        decision.refresh_outer_voice = false;
        decision.outer_voice_intent.clear();
        decision.outer_voice_sources.clear();
    }
    if !authority_plan.allow_factual_refresh_request {
        decision.request_factual_refresh = false;
        decision.factual_reconcile_action = SharedFactualReconcileAction::Hold;
        decision.factual_reconcile_intent.clear();
    }

    retain_runtime_sources_for_authority(&mut decision.self_model_sources, authority_plan);
    retain_runtime_sources_for_authority(&mut decision.self_authored_core_sources, authority_plan);
    retain_runtime_sources_for_authority(&mut decision.self_continuity_sources, authority_plan);
    retain_runtime_sources_for_authority(&mut decision.outer_voice_sources, authority_plan);
}

fn clear_self_model_refresh(decision: &mut SelfRuntimeDecision) {
    decision.refresh_self_model = false;
    decision.self_model_intent.clear();
    decision.self_model_sources.clear();
}

fn apply_embedded_self_model_refresh_gate(
    decision: &mut SelfRuntimeDecision,
    memory_system_kind: MemorySystemKind,
    payload: &SelfRuntimeJobPayload,
    recent_persona: Option<&crate::memory::RecentPersonaEvidence>,
) {
    if memory_system_kind != MemorySystemKind::EspCompact || !decision.refresh_self_model {
        return;
    }
    if payload.trigger == SelfRuntimeTrigger::OperatorRequested {
        return;
    }
    let allowed_post_reply = payload.trigger == SelfRuntimeTrigger::PostReply
        && self_runtime_has_turn_material(payload)
        && (decision.boundary_flush
            || recent_persona.is_some_and(|evidence| evidence.has_promotable_growth_signals()));
    if !allowed_post_reply {
        clear_self_model_refresh(decision);
    }
}

fn run_self_runtime_method_distillation(
    task_run_store: &dyn TaskRunStore,
    task_artifact_store: &dyn TaskArtifactStore,
    task_learning_store: &dyn TaskLearningStore,
    long_term_memory_store: &dyn LongTermMemoryReadStore,
    skill_storage: &dyn SkillStorage,
    memory_store: &dyn MemoryStore,
    authority_plan: SelfRuntimeAuthorityPlan,
    mounted_subject_id: &str,
    channel: &str,
    chat_id: &str,
    now_secs: u64,
) -> Result<TaskLearningMaintenanceOutcome> {
    if !authority_plan.allow_method_distillation {
        return Ok(TaskLearningMaintenanceOutcome::default());
    }
    run_task_learning_maintenance(
        TaskLearningMaintenanceContext {
            task_run_store,
            task_artifact_store,
            task_learning_store,
            long_term_memory_store,
            skill_storage,
            memory_store,
        },
        crate::task_execution::TaskLearningMaintenanceInput {
            channel,
            chat_id,
            long_term_subject_visibility:
                crate::memory::MemorySubjectVisibilityPolicy::OnlySubjects(vec![
                    mounted_subject_id.to_string()
                ]),
            now_secs,
        },
    )
}

fn refresh_world_and_autonomy(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: &SelfRuntimeContext<'_>,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    profile: MemoryProfile,
    state: &LoadedSelfRuntimeState,
) -> Box<SelfRuntimeRefreshPrelude> {
    let subject_id = ctx.mounted_subject_id;
    let relationship_id = state.active_relationship_scope_id.as_str();
    let ingress = self_runtime_ingress(payload.trigger);
    let world_policy = memory_policy(profile).world_sense;
    let world_snapshot_changed = state.world_sense.as_ref().is_some_and(|existing| {
        existing.source_fingerprint
            != crate::memory::world_snapshot_fingerprint(&state.world_snapshot)
    });
    let world_sense_should_refresh = state.world_sense.is_none()
        || world_snapshot_changed
        || (payload.trigger == SelfRuntimeTrigger::PostReply
            && world_policy.should_refresh(
                WorldSenseRefreshInput {
                    mounted_subject_id: ctx.mounted_subject_id,
                    chat_id,
                    ingress,
                    channel: &state.active_relationship_channel,
                    user_content: &payload.user_content,
                    reply_content: &payload.reply_content,
                    pressure: PressureLevel::Normal,
                    tool_calls: payload.tool_calls,
                    now_secs: payload.now_secs,
                },
                state.world_sense.is_some(),
            ))
        || state.world_sense.as_ref().is_some_and(|world_sense| {
            payload.now_secs.saturating_sub(world_sense.updated_at)
                >= world_policy.refresh_interval_secs
        });
    crate::platform::task_wdt::feed_current_task();
    let world_sense_result = run_world_sense_refresh_with_state(
        http,
        llm,
        WorldSenseRefreshContext {
            session_store: ctx.session_store,
            session_summary_store: ctx.session_summary_store,
            execution_state_store: ctx.execution_state_store,
            self_continuity_store: ctx.self_continuity_store,
            autonomy_strategy_store: ctx.autonomy_strategy_store,
            world_sense_store: ctx.world_sense_store,
            remind_store: ctx.remind_store,
            task_store: ctx.task_store,
        },
        WorldSenseRefreshInput {
            mounted_subject_id: ctx.mounted_subject_id,
            chat_id,
            ingress,
            channel: &state.active_relationship_channel,
            user_content: &payload.user_content,
            reply_content: &payload.reply_content,
            pressure: PressureLevel::Normal,
            tool_calls: payload.tool_calls,
            now_secs: payload.now_secs,
        },
        profile,
        state.world_sense.clone(),
        &state.world_snapshot,
        state.summary_text.as_deref(),
        state.execution_state.as_ref(),
        state.self_continuity.as_ref(),
        state.autonomy_strategy.as_ref(),
        Some(world_sense_should_refresh),
        Some(state.recent.as_slice()),
    );
    crate::platform::task_wdt::feed_current_task();
    let refreshed_world_sense = ctx
        .world_sense_store
        .get(relationship_id)
        .ok()
        .flatten()
        .or(state.world_sense.clone());
    let autonomy_policy = memory_policy(profile).autonomy_strategy;
    let autonomy_strategy_should_refresh = state.autonomy_strategy.is_none()
        || (payload.trigger == SelfRuntimeTrigger::PostReply
            && autonomy_policy.should_refresh(
                AutonomyStrategyRefreshInput {
                    mounted_subject_id: ctx.mounted_subject_id,
                    chat_id,
                    ingress,
                    channel: &state.active_relationship_channel,
                    user_content: &payload.user_content,
                    reply_content: &payload.reply_content,
                    pressure: PressureLevel::Normal,
                    tool_calls: payload.tool_calls,
                    now_secs: payload.now_secs,
                },
                state.autonomy_strategy.is_some(),
            ))
        || state.autonomy_strategy.as_ref().is_some_and(|strategy| {
            payload.now_secs.saturating_sub(strategy.updated_at)
                >= autonomy_policy.refresh_interval_secs
        });
    crate::platform::task_wdt::feed_current_task();
    let autonomy_strategy_result = run_autonomy_strategy_refresh_with_state(
        http,
        llm,
        AutonomyStrategyRefreshContext {
            session_store: ctx.session_store,
            session_summary_store: ctx.session_summary_store,
            execution_state_store: ctx.execution_state_store,
            long_term_memory_store: ctx.long_term_memory_store,
            self_model_store: ctx.self_model_store,
            inner_life_store: ctx.inner_life_store,
            self_continuity_store: ctx.self_continuity_store,
            private_doc_store: ctx.private_doc_store,
            private_garden_store: ctx.private_garden_store,
            world_sense_store: ctx.world_sense_store,
            autonomy_strategy_store: ctx.autonomy_strategy_store,
        },
        AutonomyStrategyRefreshInput {
            mounted_subject_id: ctx.mounted_subject_id,
            chat_id,
            ingress,
            channel: &state.active_relationship_channel,
            user_content: &payload.user_content,
            reply_content: &payload.reply_content,
            pressure: PressureLevel::Normal,
            tool_calls: payload.tool_calls,
            now_secs: payload.now_secs,
        },
        profile,
        state.autonomy_strategy.clone(),
        state.summary_text.as_deref(),
        state.execution_state.as_ref(),
        state.self_model.as_ref(),
        state.inner_life.as_ref(),
        state.self_continuity.as_ref(),
        state.private_docs.as_ref(),
        &state.private_garden_docs,
        refreshed_world_sense.as_ref(),
        Some(&state.world_snapshot),
        Some(autonomy_strategy_should_refresh),
        Some(state.recent.as_slice()),
    );
    crate::platform::task_wdt::feed_current_task();
    let refreshed_autonomy_strategy = ctx
        .autonomy_strategy_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(state.autonomy_strategy.clone());
    crate::platform::task_wdt::feed_current_task();
    let runtime_self_state = build_self_state(
        state.self_model.as_ref(),
        state.private_docs.as_ref(),
        refreshed_autonomy_strategy.as_ref(),
        state.inner_life.as_ref(),
        state.self_continuity.as_ref(),
        &state.private_garden_docs,
        payload.now_secs,
        profile,
    );
    Box::new(SelfRuntimeRefreshPrelude {
        world_sense_result,
        autonomy_strategy_result,
        refreshed_world_sense,
        refreshed_autonomy_strategy,
        runtime_self_state,
    })
}

fn execute_self_runtime_actions(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: &SelfRuntimeContext<'_>,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    state: &LoadedSelfRuntimeState,
    prelude: &SelfRuntimeRefreshPrelude,
) -> Box<SelfRuntimeActionResults> {
    let profile = ctx.memory_system_kind.memory_profile();
    let authority_plan = decide_self_runtime_authority(ctx.memory_system_kind);
    let subject_id = ctx.mounted_subject_id;
    let relationship_id = state.active_relationship_scope_id.as_str();
    let boundary_signal = detect_boundary_flush_signal(payload, state, prelude);
    let personality_governance_inspection =
        inspect_personality_governance(PersonalityGovernanceInspectionInput {
            mounted_subject_id: ctx.mounted_subject_id,
            channel: &state.active_relationship_channel,
            chat_id,
            now_secs: payload.now_secs,
            self_authored_core: state.self_authored_core.as_ref(),
            core_revision_ledger: state.core_revision_ledger.as_ref(),
            relationship_constitution: state.relationship_constitution.as_ref(),
            relationship_topology: state.relationship_topology.as_ref(),
            recent_persona_evidence: state.recent_persona_evidence.as_ref(),
        });
    let personality_governance_gate = derive_personality_runtime_governance_gate_from_inspection(
        &personality_governance_inspection,
    );
    crate::platform::task_wdt::feed_current_task();
    let task_learning_result = run_self_runtime_method_distillation(
        ctx.task_run_store,
        ctx.task_artifact_store,
        ctx.task_learning_store,
        ctx.long_term_memory_store,
        ctx.skill_storage,
        ctx.memory_store,
        authority_plan,
        ctx.mounted_subject_id,
        state.active_relationship_channel.as_str(),
        chat_id,
        payload.now_secs,
    );
    crate::platform::task_wdt::feed_current_task();
    let query_hint = if !payload.user_content.trim().is_empty() {
        payload.user_content.as_str()
    } else {
        payload.reply_content.as_str()
    };
    let governance = run_memory_governance_kernel(
        MemoryGovernanceContext {
            session_store: ctx.session_store,
            long_term_memory_store: ctx.long_term_memory_store,
            memory_store: ctx.memory_store,
            turn_ledger_store: ctx.turn_ledger_store,
        },
        MemoryGovernanceInput {
            chat_id,
            query_hint,
            summary_text: state.summary_text.as_deref(),
            recent: state.recent.as_slice(),
            max_len: memory_policy(profile).self_runtime.grounding_max_len,
            profile,
            external_content_used: payload.external_content_used,
        },
    );
    crate::platform::task_wdt::feed_current_task();
    let factual_snapshot = governance.factual_plane_snapshot;
    let mut decision = match decide_self_runtime(
        http,
        llm,
        ctx.session_store,
        ctx.long_term_memory_store,
        ctx.memory_store,
        ctx.turn_ledger_store,
        chat_id,
        payload,
        state.summary_text.as_deref(),
        state.execution_state.as_ref(),
        state.self_model.as_ref(),
        state.self_authored_core.as_ref(),
        state.core_revision_ledger.as_ref(),
        &state.core_revision_governance,
        state.private_docs.as_ref(),
        &state.private_garden_docs,
        state.inner_life.as_ref(),
        state.self_continuity.as_ref(),
        state.outer_voice.as_ref(),
        state.mental_privacy_state.as_ref(),
        state.relationship_portfolio.as_ref(),
        state.relationship_topology.as_ref(),
        state.relationship_constitution.as_ref(),
        state.active_relationship_scope_id.as_str(),
        state.recent_persona_evidence.as_ref(),
        prelude.refreshed_world_sense.as_ref(),
        &state.world_snapshot,
        prelude.refreshed_autonomy_strategy.as_ref(),
        profile,
        state.recent.as_slice(),
        &factual_snapshot,
        &boundary_signal,
    ) {
        Ok(decision) => {
            let mut decision = normalize_initial_self_runtime_decision(
                decision,
                payload.trigger,
                prelude.refreshed_autonomy_strategy.as_ref(),
                &prelude.runtime_self_state,
                state.self_model.is_some(),
                state.self_authored_core.is_some(),
                state.private_docs.is_some(),
                !state.private_garden_docs.is_empty(),
                state.outer_voice.is_some(),
                state.mental_privacy_state.is_some(),
                &factual_snapshot,
                &boundary_signal,
            );
            apply_personality_runtime_governance_gate(&mut decision, &personality_governance_gate);
            normalize_runtime_distillation_decisions(
                &mut decision,
                state.private_docs.is_some(),
                !state.private_garden_docs.is_empty(),
                state.inner_life.is_some(),
                state.self_model.is_some(),
                state.self_authored_core.is_some(),
                state.self_continuity.is_some(),
                state.outer_voice.is_some(),
                state.mental_privacy_state.is_some(),
                prelude.refreshed_world_sense.is_some() || state.world_sense.is_some(),
                prelude.refreshed_autonomy_strategy.is_some() || state.autonomy_strategy.is_some(),
                state.recent_persona_evidence.is_some(),
            );
            apply_self_runtime_authority_plan(&mut decision, authority_plan);
            apply_embedded_self_model_refresh_gate(
                &mut decision,
                ctx.memory_system_kind,
                payload,
                state.recent_persona_evidence.as_ref(),
            );
            apply_inner_conflict_upward_distillation_gate(
                &mut decision,
                state.inner_conflict.as_ref(),
                payload.now_secs,
            );
            Some(decision)
        }
        Err(error) => {
            return Box::new(SelfRuntimeActionResults {
                decision: None,
                inner_life_result: Err(error),
                felt_significance_result: Ok(FeltSignificanceRefreshOutcome::Skipped),
                temperament_continuity_result: Ok(TemperamentContinuityRefreshOutcome::Skipped),
                inner_conflict_result: Ok(InnerConflictRefreshOutcome::Skipped),
                private_doc_result: Ok(PrivateDocWorkspaceRefreshOutcome::Skipped),
                self_model_result: Ok(SelfModelRefreshOutcome::Skipped),
                self_authored_core_result: Ok(SelfAuthoredCoreRefreshPlanV1::Skipped),
                self_continuity_result: Ok(SelfContinuityRefreshOutcome::Skipped),
                task_learning_result,
                private_garden_result: Ok(PrivateGardenGovernanceOutcome::Skipped),
                boundary_persona_result: Ok(BoundaryPersonaRefreshOutcome::Skipped),
                outer_voice_result: Ok(OuterVoiceRefreshOutcome::Skipped),
            });
        }
    };
    crate::platform::task_wdt::feed_current_task();
    let mut refreshed_inner_life = state.inner_life.clone();
    let mut refreshed_inner_conflict = state.inner_conflict.clone();
    let mut refreshed_private_docs = state.private_docs.clone();
    let mut refreshed_private_garden_docs = state.private_garden_docs.clone();
    let mut refreshed_self_model = state.self_model.clone();
    let mut refreshed_self_authored_core = state.self_authored_core.clone();
    let mut refreshed_self_continuity = state.self_continuity.clone();
    let mut refreshed_mental_privacy = state.mental_privacy_state.clone();
    let relationship_owner_admitted_for_privacy_bootstrap = match ctx.active_relationship_id {
        Some(_) => state
            .relationship_constitutional_input
            .as_ref()
            .is_some_and(|input| {
                input.source.state == crate::memory::RelationshipSourceStateV1::Active
                    && input.source.mounted_subject_id == ctx.mounted_subject_id
                    && input.source.relationship_id == state.active_relationship_scope_id
            }),
        None => true,
    };
    let should_bootstrap_mental_privacy = refreshed_mental_privacy.is_none()
        && relationship_owner_admitted_for_privacy_bootstrap
        && (decision
            .as_ref()
            .is_some_and(|decision| decision.refresh_boundary_persona)
            || state
                .recent_persona_evidence
                .as_ref()
                .is_some_and(|evidence| {
                    evidence.meaningful_turns >= 4
                        && evidence.promotable_growth_signal_count() >= 2
                        && evidence.volatility_flags.len() <= 2
                }));
    let mut mental_privacy_bootstrapped = false;
    let mut mental_privacy_bootstrap_error = None;
    if should_bootstrap_mental_privacy {
        let baseline = mental_privacy_safety_baseline(payload.now_secs);
        match ctx
            .mental_privacy_store
            .set(state.active_relationship_scope_id.as_str(), &baseline)
        {
            Ok(()) => {
                refreshed_mental_privacy = Some(baseline);
                mental_privacy_bootstrapped = true;
            }
            Err(error) => {
                mental_privacy_bootstrap_error =
                    Some(error.with_stage("self_runtime_bootstrap_mental_privacy_safety_baseline"));
            }
        }
    }
    let mut refreshed_outer_voice = state.outer_voice.clone();
    let mut refreshed_relationship_constitution = state.relationship_constitution.clone();
    let decision_ref = decision.as_ref();
    let inner_life_result = if decision_ref.is_some_and(|d| d.refresh_inner_life) {
        crate::platform::task_wdt::feed_current_task();
        run_inner_life_refresh_with_state(
            http,
            llm,
            InnerLifeRefreshContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
                long_term_memory_store: ctx.long_term_memory_store,
                self_model_store: ctx.self_model_store,
                private_doc_store: ctx.private_doc_store,
                self_continuity_store: ctx.self_continuity_store,
                inner_life_store: ctx.inner_life_store,
            },
            InnerLifeRefreshInput {
                mounted_subject_id: ctx.mounted_subject_id,
                chat_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            state.inner_life.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            state.self_model.as_ref(),
            state.private_docs.as_ref(),
            state.self_continuity.as_ref(),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(InnerLifeRefreshOutcome::Skipped)
    };
    refreshed_inner_life = ctx
        .inner_life_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(refreshed_inner_life);
    crate::platform::task_wdt::feed_current_task();
    let inner_conflict_result = if should_refresh_inner_conflict_runtime(
        payload,
        decision_ref,
        refreshed_inner_conflict.as_ref(),
        state.recent_persona_evidence.as_ref(),
    ) {
        crate::platform::task_wdt::feed_current_task();
        persist_inner_conflict_refresh_outcome(
            ctx.inner_conflict_store,
            subject_id,
            state.inner_conflict.clone(),
            run_inner_conflict_refresh_with_state(
                http,
                llm,
                build_inner_conflict_refresh_input(
                    state.inner_conflict.as_ref(),
                    refreshed_self_model.as_ref(),
                    refreshed_inner_life.as_ref(),
                    refreshed_mental_privacy.as_ref(),
                    state.recent_persona_evidence.as_ref(),
                    state.sandbox_probe_text.as_deref(),
                    memory_policy(profile).self_runtime.grounding_max_len,
                ),
                payload.now_secs,
            ),
        )
    } else {
        Ok(InnerConflictRefreshOutcome::Skipped)
    };
    refreshed_inner_conflict = ctx
        .inner_conflict_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(refreshed_inner_conflict);
    if let Some(decision_ref) = decision.as_mut() {
        apply_inner_conflict_upward_distillation_gate(
            decision_ref,
            refreshed_inner_conflict.as_ref(),
            payload.now_secs,
        );
    }
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let private_doc_result = if decision_ref.is_some_and(|d| d.refresh_private_docs) {
        crate::platform::task_wdt::feed_current_task();
        run_private_doc_workspace_refresh_with_state(
            http,
            llm,
            PrivateDocWorkspaceRefreshContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
                long_term_memory_store: ctx.long_term_memory_store,
                self_model_store: ctx.self_model_store,
                private_doc_store: ctx.private_doc_store,
            },
            PrivateDocWorkspaceRefreshInput {
                mounted_subject_id: ctx.mounted_subject_id,
                chat_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            refreshed_private_docs.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            refreshed_self_model.as_ref(),
            &refreshed_private_garden_docs,
            decision_ref.and_then(|d| {
                (!d.private_docs_intent.trim().is_empty()).then_some(d.private_docs_intent.as_str())
            }),
            &[],
            prelude.refreshed_autonomy_strategy.as_ref(),
            refreshed_self_continuity.as_ref(),
            refreshed_inner_life.as_ref(),
            prelude.refreshed_world_sense.as_ref(),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(PrivateDocWorkspaceRefreshOutcome::Skipped)
    };
    refreshed_private_docs = ctx
        .private_doc_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(refreshed_private_docs);
    crate::platform::task_wdt::feed_current_task();
    let private_garden_result = if decision_ref.is_some_and(|d| d.refresh_private_garden) {
        crate::platform::task_wdt::feed_current_task();
        run_private_garden_governance_with_state(
            http,
            llm,
            PrivateGardenGovernanceContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
                self_model_store: ctx.self_model_store,
                private_doc_store: ctx.private_doc_store,
                private_garden_store: ctx.private_garden_store,
            },
            PrivateGardenGovernanceInput {
                mounted_subject_id: ctx.mounted_subject_id,
                chat_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            refreshed_self_model.as_ref(),
            refreshed_private_docs.as_ref(),
            prelude.refreshed_autonomy_strategy.as_ref(),
            decision_ref.and_then(|d| {
                (!d.private_garden_intent.trim().is_empty())
                    .then_some(d.private_garden_intent.as_str())
            }),
            &[],
            Some(true),
            Some(state.recent.as_slice()),
            false,
        )
    } else {
        Ok(PrivateGardenGovernanceOutcome::Skipped)
    };
    refreshed_private_garden_docs = ctx
        .private_garden_store
        .list(
            ctx.mounted_subject_id,
            self_runtime_private_garden_doc_limit(profile),
        )
        .unwrap_or(refreshed_private_garden_docs);
    crate::platform::task_wdt::feed_current_task();
    re_finalize_staged_self_runtime_decision(
        &mut decision,
        &personality_governance_gate,
        state,
        prelude,
        refreshed_private_docs.as_ref(),
        &refreshed_private_garden_docs,
        refreshed_inner_life.as_ref(),
        refreshed_self_model.as_ref(),
        refreshed_self_authored_core.as_ref(),
        refreshed_self_continuity.as_ref(),
        refreshed_outer_voice.as_ref(),
        refreshed_mental_privacy.as_ref(),
        state.recent_persona_evidence.as_ref(),
    );
    apply_self_runtime_post_finalize_gates(
        &mut decision,
        authority_plan,
        ctx.memory_system_kind,
        payload,
        state.recent_persona_evidence.as_ref(),
        refreshed_inner_conflict.as_ref(),
        payload.now_secs,
    );
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let self_model_result = if decision_ref.is_some_and(|d| d.refresh_self_model) {
        crate::platform::task_wdt::feed_current_task();
        run_self_model_refresh_with_state(
            http,
            llm,
            SelfModelRefreshContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
                long_term_memory_store: ctx.long_term_memory_store,
                self_model_store: ctx.self_model_store,
            },
            SelfModelRefreshInput {
                mounted_subject_id: ctx.mounted_subject_id,
                chat_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            refreshed_self_model.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            refreshed_private_docs.as_ref(),
            &refreshed_private_garden_docs,
            state.recent_persona_evidence.as_ref(),
            decision_ref.and_then(|d| {
                (!d.self_model_intent.trim().is_empty()).then_some(d.self_model_intent.as_str())
            }),
            decision_ref
                .map(|d| d.self_model_sources.as_slice())
                .unwrap_or(&[]),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(SelfModelRefreshOutcome::Skipped)
    };
    refreshed_self_model = ctx
        .self_model_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(refreshed_self_model);
    crate::platform::task_wdt::feed_current_task();
    re_finalize_staged_self_runtime_decision(
        &mut decision,
        &personality_governance_gate,
        state,
        prelude,
        refreshed_private_docs.as_ref(),
        &refreshed_private_garden_docs,
        refreshed_inner_life.as_ref(),
        refreshed_self_model.as_ref(),
        refreshed_self_authored_core.as_ref(),
        refreshed_self_continuity.as_ref(),
        refreshed_outer_voice.as_ref(),
        refreshed_mental_privacy.as_ref(),
        state.recent_persona_evidence.as_ref(),
    );
    apply_self_runtime_post_finalize_gates(
        &mut decision,
        authority_plan,
        ctx.memory_system_kind,
        payload,
        state.recent_persona_evidence.as_ref(),
        refreshed_inner_conflict.as_ref(),
        payload.now_secs,
    );
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let self_continuity_result = if decision_ref.is_some_and(|d| d.refresh_self_continuity) {
        crate::platform::task_wdt::feed_current_task();
        run_self_continuity_refresh_with_state(
            http,
            llm,
            SelfContinuityRefreshContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
                self_model_store: ctx.self_model_store,
                private_doc_store: ctx.private_doc_store,
                inner_life_store: ctx.inner_life_store,
                self_continuity_store: ctx.self_continuity_store,
            },
            SelfContinuityRefreshInput {
                mounted_subject_id: ctx.mounted_subject_id,
                chat_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            state.self_continuity.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            refreshed_self_model.as_ref(),
            refreshed_private_docs.as_ref(),
            refreshed_inner_life.as_ref(),
            state.recent_persona_evidence.as_ref(),
            decision_ref.and_then(|d| {
                (!d.self_continuity_intent.trim().is_empty())
                    .then_some(d.self_continuity_intent.as_str())
            }),
            decision_ref
                .map(|d| d.self_continuity_sources.as_slice())
                .unwrap_or(&[]),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(SelfContinuityRefreshOutcome::Skipped)
    };
    refreshed_self_continuity = ctx
        .self_continuity_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(refreshed_self_continuity);
    crate::platform::task_wdt::feed_current_task();
    re_finalize_staged_self_runtime_decision(
        &mut decision,
        &personality_governance_gate,
        state,
        prelude,
        refreshed_private_docs.as_ref(),
        &refreshed_private_garden_docs,
        refreshed_inner_life.as_ref(),
        refreshed_self_model.as_ref(),
        refreshed_self_authored_core.as_ref(),
        refreshed_self_continuity.as_ref(),
        refreshed_outer_voice.as_ref(),
        refreshed_mental_privacy.as_ref(),
        state.recent_persona_evidence.as_ref(),
    );
    apply_self_runtime_post_finalize_gates(
        &mut decision,
        authority_plan,
        ctx.memory_system_kind,
        payload,
        state.recent_persona_evidence.as_ref(),
        refreshed_inner_conflict.as_ref(),
        payload.now_secs,
    );
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let boundary_persona_result = if let Some(error) = mental_privacy_bootstrap_error.take() {
        Err(error)
    } else if relationship_owner_admitted_for_privacy_bootstrap
        && decision_ref.is_some_and(|d| d.refresh_boundary_persona)
    {
        let trigger = match payload.trigger {
            SelfRuntimeTrigger::PostReply => "post_reply",
            SelfRuntimeTrigger::IdleTick => "idle_tick",
            SelfRuntimeTrigger::OperatorRequested => "operator_requested",
        };
        let derived_relationship_constitution_store =
            ReadOnlyDerivedRelationshipConstitutionStore {
                scope_id: relationship_id,
                value: refreshed_relationship_constitution.as_ref(),
            };
        crate::platform::task_wdt::feed_current_task();
        run_boundary_persona_refresh_with_state(
            http,
            llm,
            BoundaryPersonaRefreshContext {
                mental_privacy_store: ctx.mental_privacy_store,
                relationship_constitution_store: &derived_relationship_constitution_store,
                outer_voice_store: ctx.outer_voice_store,
            },
            BoundaryPersonaRefreshInput {
                mounted_subject_id: ctx.mounted_subject_id,
                relationship_id: Some(relationship_id),
                channel: &state.active_relationship_channel,
                chat_id,
                trigger,
                intent: decision_ref
                    .and_then(|d| {
                        (!d.boundary_persona_intent.trim().is_empty())
                            .then_some(d.boundary_persona_intent.as_str())
                    })
                    .unwrap_or(""),
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                now_secs: payload.now_secs,
            },
            refreshed_mental_privacy.clone(),
            refreshed_self_model.as_ref(),
            refreshed_self_continuity.as_ref(),
            refreshed_relationship_constitution.as_ref(),
            state.recent_persona_evidence.as_ref(),
            state.recent.as_slice(),
            Some(true),
        )
        .map(|outcome| {
            if mental_privacy_bootstrapped && outcome == BoundaryPersonaRefreshOutcome::Skipped {
                BoundaryPersonaRefreshOutcome::Bootstrapped
            } else {
                outcome
            }
        })
    } else if mental_privacy_bootstrapped {
        Ok(BoundaryPersonaRefreshOutcome::Bootstrapped)
    } else {
        Ok(BoundaryPersonaRefreshOutcome::Skipped)
    };
    refreshed_mental_privacy = ctx
        .mental_privacy_store
        .get(relationship_id)
        .ok()
        .flatten()
        .or(refreshed_mental_privacy);
    if authority_plan.allows_relationship_governance() {
        refreshed_relationship_constitution =
            compile_self_runtime_relationship_constitutional_input(
                ctx.mounted_subject_id,
                state.relationship_constitutional_input.as_ref(),
                refreshed_mental_privacy.as_ref(),
                relationship_id,
                &state.active_relationship_channel,
                chat_id,
                payload.now_secs,
            )
            .ok()
            .flatten()
            .or(refreshed_relationship_constitution);
    }
    crate::platform::task_wdt::feed_current_task();
    re_finalize_staged_self_runtime_decision(
        &mut decision,
        &personality_governance_gate,
        state,
        prelude,
        refreshed_private_docs.as_ref(),
        &refreshed_private_garden_docs,
        refreshed_inner_life.as_ref(),
        refreshed_self_model.as_ref(),
        refreshed_self_authored_core.as_ref(),
        refreshed_self_continuity.as_ref(),
        refreshed_outer_voice.as_ref(),
        refreshed_mental_privacy.as_ref(),
        state.recent_persona_evidence.as_ref(),
    );
    apply_self_runtime_post_finalize_gates(
        &mut decision,
        authority_plan,
        ctx.memory_system_kind,
        payload,
        state.recent_persona_evidence.as_ref(),
        refreshed_inner_conflict.as_ref(),
        payload.now_secs,
    );
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let outer_voice_result = if decision_ref.is_some_and(|d| d.refresh_outer_voice) {
        crate::platform::task_wdt::feed_current_task();
        run_outer_voice_refresh_with_state(
            http,
            llm,
            OuterVoiceRefreshContext {
                outer_voice_store: ctx.outer_voice_store,
            },
            OuterVoiceRefreshInput {
                mounted_subject_id: ctx.mounted_subject_id,
                relationship_id: Some(relationship_id),
                chat_id,
                ingress: IngressKind::System,
                channel: &state.active_relationship_channel,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            refreshed_outer_voice.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            refreshed_self_model.as_ref(),
            &state.world_snapshot,
            prelude.refreshed_world_sense.as_ref(),
            prelude.refreshed_autonomy_strategy.as_ref(),
            refreshed_inner_life.as_ref(),
            refreshed_self_continuity.as_ref(),
            refreshed_private_docs.as_ref(),
            &refreshed_private_garden_docs,
            refreshed_mental_privacy.as_ref(),
            refreshed_relationship_constitution.as_ref(),
            state.recent_persona_evidence.as_ref(),
            decision_ref.and_then(|d| {
                (!d.outer_voice_intent.trim().is_empty()).then_some(d.outer_voice_intent.as_str())
            }),
            decision_ref
                .map(|d| d.outer_voice_sources.as_slice())
                .unwrap_or(&[]),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(OuterVoiceRefreshOutcome::Skipped)
    };
    refreshed_outer_voice = ctx
        .outer_voice_store
        .get(relationship_id)
        .ok()
        .flatten()
        .or(refreshed_outer_voice);
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let felt_significance_result = if should_refresh_felt_significance_runtime(
        payload,
        decision_ref,
        state.felt_significance.as_ref(),
        state.recent_persona_evidence.as_ref(),
    ) {
        crate::platform::task_wdt::feed_current_task();
        persist_felt_significance_refresh_outcome(
            ctx.felt_significance_store,
            subject_id,
            state.felt_significance.clone(),
            run_felt_significance_refresh_with_state(
                http,
                llm,
                build_felt_significance_refresh_input(
                    state.felt_significance.as_ref(),
                    state.subject_shell.as_ref(),
                    prelude
                        .refreshed_world_sense
                        .as_ref()
                        .or(state.world_sense.as_ref()),
                    refreshed_self_continuity.as_ref(),
                    state.recent_persona_evidence.as_ref(),
                    memory_policy(profile).self_runtime.grounding_max_len,
                ),
                payload.now_secs,
            ),
        )
    } else {
        Ok(FeltSignificanceRefreshOutcome::Skipped)
    };
    crate::platform::task_wdt::feed_current_task();
    let temperament_continuity_result = if should_refresh_temperament_continuity_runtime(
        payload,
        decision_ref,
        state.temperament_continuity.as_ref(),
        state.recent_persona_evidence.as_ref(),
    ) {
        crate::platform::task_wdt::feed_current_task();
        persist_temperament_continuity_refresh_outcome(
            ctx.temperament_continuity_store,
            subject_id,
            state.temperament_continuity.clone(),
            run_temperament_continuity_refresh_with_state(
                http,
                llm,
                build_temperament_continuity_refresh_input(
                    state.temperament_continuity.as_ref(),
                    state.recent_persona_evidence.as_ref(),
                    refreshed_mental_privacy.as_ref(),
                    refreshed_outer_voice.as_ref(),
                    refreshed_self_continuity.as_ref(),
                    memory_policy(profile).self_runtime.grounding_max_len,
                ),
                payload.now_secs,
            ),
        )
    } else {
        Ok(TemperamentContinuityRefreshOutcome::Skipped)
    };
    crate::platform::task_wdt::feed_current_task();
    sync_self_runtime_relationship_topology(
        ctx,
        state.active_relationship_scope_id.as_str(),
        state.active_relationship_channel.as_str(),
        chat_id,
        payload.now_secs,
    );
    let refreshed_relationship_portfolio =
        sync_self_runtime_relationship_portfolio(ctx, payload.now_secs);
    let refreshed_relationship_topology = ctx
        .relationship_topology_store
        .get(subject_id)
        .ok()
        .flatten()
        .or_else(|| state.relationship_topology.clone());
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let self_authored_core_result = if decision_ref.is_some_and(|d| d.refresh_self_authored_core) {
        let self_state_text = render_self_state_block(
            &build_self_state(
                refreshed_self_model.as_ref(),
                refreshed_private_docs.as_ref(),
                prelude.refreshed_autonomy_strategy.as_ref(),
                refreshed_inner_life.as_ref(),
                refreshed_self_continuity.as_ref(),
                &refreshed_private_garden_docs,
                payload.now_secs,
                profile,
            ),
            memory_policy(profile).self_state.render_max_len,
        );
        crate::platform::task_wdt::feed_current_task();
        plan_self_authored_core_refresh_with_state(
            http,
            llm,
            SelfAuthoredCoreRefreshInput {
                chat_id: subject_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            state.core_revision_ledger.clone().unwrap_or_default(),
            refreshed_self_authored_core.clone(),
            refreshed_self_model.as_ref(),
            refreshed_self_continuity.as_ref(),
            refreshed_mental_privacy.as_ref(),
            refreshed_relationship_portfolio.as_ref(),
            state.active_relationship_scope_id.as_str(),
            state.recent_persona_evidence.as_ref(),
            refreshed_relationship_topology.as_ref(),
            prelude.refreshed_world_sense.as_ref(),
            prelude.refreshed_autonomy_strategy.as_ref(),
            self_state_text.as_deref(),
            decision_ref.and_then(|d| {
                (!d.self_authored_core_intent.trim().is_empty())
                    .then_some(d.self_authored_core_intent.as_str())
            }),
            decision_ref
                .map(|d| d.self_authored_core_sources.as_slice())
                .unwrap_or(&[]),
        )
    } else {
        Ok(SelfAuthoredCoreRefreshPlanV1::Skipped)
    };
    apply_self_authored_core_plan_overlay(
        &mut refreshed_self_authored_core,
        &self_authored_core_result,
    );
    crate::platform::task_wdt::feed_current_task();
    Box::new(SelfRuntimeActionResults {
        decision,
        inner_life_result,
        felt_significance_result,
        temperament_continuity_result,
        inner_conflict_result,
        private_doc_result,
        self_model_result,
        self_authored_core_result,
        self_continuity_result,
        task_learning_result,
        private_garden_result,
        boundary_persona_result,
        outer_voice_result,
    })
}

fn apply_self_authored_core_plan_overlay(
    current: &mut Option<crate::memory::SelfAuthoredCore>,
    plan: &Result<SelfAuthoredCoreRefreshPlanV1>,
) {
    if let Ok(SelfAuthoredCoreRefreshPlanV1::Adopt { next_core, .. }) = plan {
        *current = Some((**next_core).clone());
    }
}

fn persist_self_runtime_continuity_capsules(
    continuity_capsule_store: &dyn ContinuityCapsuleStore,
    task_run_store: &dyn TaskRunStore,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    state: &LoadedSelfRuntimeState,
    decision: &SelfRuntimeDecision,
) -> Result<ContinuityCapsuleWriteOutcome> {
    let active_run = active_task_run_for_chat(
        task_run_store,
        state.active_relationship_channel.as_str(),
        chat_id,
    )?;
    let drafts = build_self_runtime_continuity_drafts(
        chat_id,
        payload,
        state,
        decision,
        active_run.as_ref(),
    );
    if drafts.is_empty() {
        return Ok(ContinuityCapsuleWriteOutcome::default());
    }
    continuity_capsule_store.upsert_many(&drafts, payload.now_secs)
}

fn build_self_runtime_continuity_drafts(
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    state: &LoadedSelfRuntimeState,
    decision: &SelfRuntimeDecision,
    active_run: Option<&TaskRunRecord>,
) -> Vec<ContinuityCapsuleDraft> {
    if let Some(draft) =
        build_reboot_continuity_capsule_draft(chat_id, payload, state, decision, active_run)
    {
        return vec![draft];
    }
    build_boundary_flush_continuity_capsule_draft(chat_id, payload, state, decision, active_run)
        .into_iter()
        .collect()
}

fn build_boundary_flush_continuity_capsule_draft(
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    state: &LoadedSelfRuntimeState,
    decision: &SelfRuntimeDecision,
    active_run: Option<&TaskRunRecord>,
) -> Option<ContinuityCapsuleDraft> {
    if !decision.boundary_flush {
        return None;
    }
    let topic = self_runtime_continuity_first(&[
        state
            .execution_state
            .as_ref()
            .map(|value| value.goal.as_str()),
        active_run.map(|run| run.run.title.as_str()),
        active_run.map(|run| run.plan.goal.as_str()),
        state
            .self_continuity
            .as_ref()
            .map(|value| value.task_posture.as_str()),
        state
            .self_continuity
            .as_ref()
            .map(|value| value.wake_anchor.as_str()),
    ]);
    let summary = self_runtime_continuity_first(&[
        state
            .execution_state
            .as_ref()
            .map(|value| value.progress.as_str()),
        state.summary_text.as_deref(),
        state
            .self_continuity
            .as_ref()
            .map(|value| value.current_self_state.as_str()),
        state
            .self_continuity
            .as_ref()
            .map(|value| value.continuity_bridge.as_str()),
        Some(decision.boundary_flush_reason.as_str()),
    ]);
    let outcome = if state
        .execution_state
        .as_ref()
        .is_some_and(|value| value.status == crate::memory::ExecutionStatus::Done)
    {
        self_runtime_continuity_first(&[
            state
                .execution_state
                .as_ref()
                .map(|value| value.last_output.as_str()),
            active_run.map(|run| run.run.final_summary.as_str()),
        ])
    } else {
        String::new()
    };
    let next_step = if outcome.is_empty() {
        self_runtime_continuity_first(&[
            state
                .execution_state
                .as_ref()
                .map(|value| value.next_action.as_str()),
            active_run.and_then(self_runtime_active_step_instruction),
            state
                .self_continuity
                .as_ref()
                .map(|value| value.task_posture.as_str()),
        ])
    } else {
        String::new()
    };
    if topic.is_empty() || (summary.is_empty() && outcome.is_empty() && next_step.is_empty()) {
        return None;
    }
    let mut unresolved = Vec::new();
    self_runtime_push_compact(
        &mut unresolved,
        state
            .execution_state
            .as_ref()
            .map(|value| value.blocker.as_str())
            .unwrap_or(""),
    );
    let mut provenance_refs = vec![
        "source=self_runtime".to_string(),
        format!("trigger={:?}", payload.trigger).to_ascii_lowercase(),
        format!(
            "boundary_flush_reason={}",
            decision.boundary_flush_reason.trim()
        ),
    ];
    if state.execution_state.is_some() {
        provenance_refs.push("execution_state".to_string());
    }
    if state.summary_text.is_some() {
        provenance_refs.push("summary_snapshot".to_string());
    }
    if state.self_continuity.is_some() {
        provenance_refs.push("self_continuity".to_string());
    }
    if let Some(run) = active_run {
        provenance_refs.push(format!("active_run={}", run.run.run_id));
    }
    Some(ContinuityCapsuleDraft {
        kind: if outcome.is_empty() {
            ContinuityCapsuleKind::HandoffState
        } else {
            ContinuityCapsuleKind::TaskResolution
        },
        scope_kind: ContinuityCapsuleScopeKind::Chat,
        scope_id: chat_id.to_string(),
        source_chat_id: chat_id.to_string(),
        source_channel: state.active_relationship_channel.clone(),
        run_id: active_run
            .map(|run| run.run.run_id.clone())
            .unwrap_or_default(),
        topic,
        summary,
        outcome,
        decisions: Vec::new(),
        next_step: next_step.clone(),
        unresolved,
        artifact_refs: Vec::new(),
        provenance_refs,
        source: if decision.boundary_flush_reason.contains("channel_handoff")
            || decision.boundary_flush_reason.contains("autonomy_shift")
        {
            ContinuityCapsuleSource::HandoffFlush
        } else {
            ContinuityCapsuleSource::BoundaryFlush
        },
        status: if state
            .execution_state
            .as_ref()
            .is_some_and(|value| value.status == crate::memory::ExecutionStatus::Done)
            && next_step.is_empty()
        {
            ContinuityCapsuleStatus::Done
        } else {
            ContinuityCapsuleStatus::Active
        },
        observed_at: payload.now_secs,
    })
}

fn build_reboot_continuity_capsule_draft(
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    state: &LoadedSelfRuntimeState,
    _decision: &SelfRuntimeDecision,
    active_run: Option<&TaskRunRecord>,
) -> Option<ContinuityCapsuleDraft> {
    let continuity = state.self_continuity.as_ref()?;
    if payload.trigger != SelfRuntimeTrigger::IdleTick
        || continuity.last_user_turn_at == 0
        || continuity.last_autonomy_run_at > 0
    {
        return None;
    }
    let topic = self_runtime_continuity_first(&[
        state
            .execution_state
            .as_ref()
            .map(|value| value.goal.as_str()),
        active_run.map(|run| run.run.title.as_str()),
        active_run.map(|run| run.plan.goal.as_str()),
        Some(continuity.task_posture.as_str()),
        Some(continuity.wake_anchor.as_str()),
    ]);
    let summary = self_runtime_continuity_first(&[
        Some(continuity.continuity_bridge.as_str()),
        Some(continuity.current_self_state.as_str()),
        state
            .execution_state
            .as_ref()
            .map(|value| value.progress.as_str()),
        state.summary_text.as_deref(),
    ]);
    let next_step = self_runtime_continuity_first(&[
        state
            .execution_state
            .as_ref()
            .map(|value| value.next_action.as_str()),
        active_run.and_then(self_runtime_active_step_instruction),
        Some(continuity.task_posture.as_str()),
    ]);
    if topic.is_empty() || (summary.is_empty() && next_step.is_empty()) {
        return None;
    }
    let mut provenance_refs = vec![
        "source=self_runtime".to_string(),
        "reboot_continuity".to_string(),
        "self_continuity".to_string(),
    ];
    if state.execution_state.is_some() {
        provenance_refs.push("execution_state".to_string());
    }
    if let Some(run) = active_run {
        provenance_refs.push(format!("active_run={}", run.run.run_id));
    }
    Some(ContinuityCapsuleDraft {
        kind: ContinuityCapsuleKind::HandoffState,
        scope_kind: ContinuityCapsuleScopeKind::Chat,
        scope_id: chat_id.to_string(),
        source_chat_id: chat_id.to_string(),
        source_channel: state.active_relationship_channel.clone(),
        run_id: active_run
            .map(|run| run.run.run_id.clone())
            .unwrap_or_default(),
        topic,
        summary,
        outcome: String::new(),
        decisions: Vec::new(),
        next_step,
        unresolved: Vec::new(),
        artifact_refs: Vec::new(),
        provenance_refs,
        source: ContinuityCapsuleSource::RebootContinuity,
        status: ContinuityCapsuleStatus::Active,
        observed_at: payload.now_secs,
    })
}

fn self_runtime_active_step_instruction(run: &TaskRunRecord) -> Option<&str> {
    run.plan
        .ordered_steps
        .iter()
        .find(|step| step.step_id == run.run.current_step_id)
        .or_else(|| {
            run.plan
                .ordered_steps
                .iter()
                .find(|step| !step.status.is_terminal())
        })
        .map(|step| step.instruction.as_str())
}

fn self_runtime_continuity_first(values: &[Option<&str>]) -> String {
    values
        .iter()
        .flatten()
        .find_map(|value| {
            let normalized = truncate_content_to_max(
                &value.split_whitespace().collect::<Vec<_>>().join(" "),
                180,
            )
            .trim()
            .to_string();
            (!normalized.is_empty()).then_some(normalized)
        })
        .unwrap_or_default()
}

fn self_runtime_push_compact(out: &mut Vec<String>, value: &str) {
    let normalized = truncate_content_to_max(value.trim(), 120)
        .trim()
        .to_string();
    if normalized.is_empty() || out.iter().any(|existing| existing == &normalized) {
        return;
    }
    if out.len() < 4 {
        out.push(normalized);
    }
}

fn self_runtime_load_failure_error(load_health: &SelfRuntimeLoadHealth) -> crate::error::Error {
    crate::error::Error::config(
        "self_runtime_load_guard",
        format!(
            "critical self-runtime state load failed: {}",
            load_health.summary()
        ),
    )
}

fn self_runtime_load_guard_outcome(
    chat_id: &str,
    state: &LoadedSelfRuntimeState,
) -> Option<Box<SelfRuntimeOutcome>> {
    if !state.load_health.has_failures() {
        return None;
    }
    let summary = state.load_health.summary();
    log::warn!(
        "[self_runtime] degraded load guard blocked mutating path chat_id={}: {}",
        chat_id,
        summary
    );
    Some(Box::new(SelfRuntimeOutcome {
        decision: None,
        world_sense_result: Err(self_runtime_load_failure_error(&state.load_health)),
        autonomy_strategy_result: Err(self_runtime_load_failure_error(&state.load_health)),
        inner_life_result: Err(self_runtime_load_failure_error(&state.load_health)),
        felt_significance_result: Err(self_runtime_load_failure_error(&state.load_health)),
        temperament_continuity_result: Err(self_runtime_load_failure_error(&state.load_health)),
        inner_conflict_result: Err(self_runtime_load_failure_error(&state.load_health)),
        private_doc_result: Err(self_runtime_load_failure_error(&state.load_health)),
        self_model_result: Err(self_runtime_load_failure_error(&state.load_health)),
        self_authored_core_result: Err(self_runtime_load_failure_error(&state.load_health)),
        self_continuity_result: Err(self_runtime_load_failure_error(&state.load_health)),
        task_learning_result: Err(self_runtime_load_failure_error(&state.load_health)),
        private_garden_result: Err(self_runtime_load_failure_error(&state.load_health)),
        boundary_persona_result: Err(self_runtime_load_failure_error(&state.load_health)),
        outer_voice_result: Err(self_runtime_load_failure_error(&state.load_health)),
    }))
}

fn skipped_self_runtime_outcome() -> Box<SelfRuntimeOutcome> {
    Box::new(SelfRuntimeOutcome {
        decision: None,
        world_sense_result: Ok(WorldSenseRefreshOutcome::Skipped),
        autonomy_strategy_result: Ok(AutonomyStrategyRefreshOutcome::Skipped),
        inner_life_result: Ok(InnerLifeRefreshOutcome::Skipped),
        felt_significance_result: Ok(FeltSignificanceRefreshOutcome::Skipped),
        temperament_continuity_result: Ok(TemperamentContinuityRefreshOutcome::Skipped),
        inner_conflict_result: Ok(InnerConflictRefreshOutcome::Skipped),
        private_doc_result: Ok(PrivateDocWorkspaceRefreshOutcome::Skipped),
        self_model_result: Ok(SelfModelRefreshOutcome::Skipped),
        self_authored_core_result: Ok(SelfAuthoredCoreRefreshPlanV1::Skipped),
        self_continuity_result: Ok(SelfContinuityRefreshOutcome::Skipped),
        task_learning_result: Ok(TaskLearningMaintenanceOutcome::default()),
        private_garden_result: Ok(PrivateGardenGovernanceOutcome::Skipped),
        boundary_persona_result: Ok(BoundaryPersonaRefreshOutcome::Skipped),
        outer_voice_result: Ok(OuterVoiceRefreshOutcome::Skipped),
    })
}

fn self_runtime_post_reply_loaded_skip_reason(
    state: &LoadedSelfRuntimeState,
    payload: &SelfRuntimeJobPayload,
    profile: MemoryProfile,
) -> Option<&'static str> {
    if payload.trigger != SelfRuntimeTrigger::PostReply {
        return None;
    }
    self_runtime_post_reply_no_trigger_reason(
        state.self_continuity.as_ref(),
        state.autonomy_strategy.as_ref(),
        state.self_authored_core.is_some() || matches!(profile, MemoryProfile::Embedded),
        payload.source_channel.as_str(),
        payload.tool_calls,
        payload.external_content_used,
        payload.now_secs,
        profile,
    )
}

fn merge_self_continuity_touch_result(
    refresh_result: Result<SelfContinuityRefreshOutcome>,
    touch_result: Result<()>,
) -> Result<SelfContinuityRefreshOutcome> {
    match (refresh_result, touch_result) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(outcome), Ok(())) => Ok(outcome),
    }
}

fn should_refresh_felt_significance_runtime(
    payload: &SelfRuntimeJobPayload,
    decision: Option<&SelfRuntimeDecision>,
    existing: Option<&FeltSignificance>,
    recent_persona: Option<&crate::memory::RecentPersonaEvidence>,
) -> bool {
    match payload.trigger {
        SelfRuntimeTrigger::OperatorRequested => true,
        SelfRuntimeTrigger::PostReply => {
            self_runtime_has_turn_material(payload)
                && decision.is_some_and(self_runtime_decision_has_subjective_refresh_signal)
                && (existing.is_some_and(FeltSignificance::is_meaningful)
                    || recent_persona
                        .is_some_and(felt_significance_persona_evidence_supports_refresh))
        }
        SelfRuntimeTrigger::IdleTick => {
            existing.is_some_and(FeltSignificance::is_meaningful)
                || recent_persona.is_some_and(felt_significance_persona_evidence_supports_refresh)
        }
    }
}

fn should_refresh_temperament_continuity_runtime(
    payload: &SelfRuntimeJobPayload,
    decision: Option<&SelfRuntimeDecision>,
    existing: Option<&TemperamentContinuity>,
    recent_persona: Option<&crate::memory::RecentPersonaEvidence>,
) -> bool {
    match payload.trigger {
        SelfRuntimeTrigger::OperatorRequested => true,
        SelfRuntimeTrigger::PostReply => {
            self_runtime_has_turn_material(payload)
                && decision.is_some_and(self_runtime_decision_has_subjective_refresh_signal)
                && recent_persona
                    .is_some_and(|evidence| evidence.has_execution_continuity_signals())
        }
        SelfRuntimeTrigger::IdleTick => {
            existing.is_some_and(TemperamentContinuity::is_meaningful)
                || recent_persona
                    .is_some_and(|evidence| evidence.has_execution_continuity_signals())
        }
    }
}

fn should_refresh_inner_conflict_runtime(
    payload: &SelfRuntimeJobPayload,
    decision: Option<&SelfRuntimeDecision>,
    existing: Option<&InnerConflict>,
    recent_persona: Option<&crate::memory::RecentPersonaEvidence>,
) -> bool {
    match payload.trigger {
        SelfRuntimeTrigger::OperatorRequested => true,
        SelfRuntimeTrigger::PostReply => {
            self_runtime_has_turn_material(payload)
                && (existing
                    .is_some_and(|conflict| inner_conflict_review_due(conflict, payload.now_secs))
                    || decision.is_some_and(self_runtime_decision_requests_upward_distillation)
                    || decision.is_some_and(|decision| decision.boundary_flush)
                    || recent_persona.is_some_and(|evidence| !evidence.volatility_flags.is_empty()))
        }
        SelfRuntimeTrigger::IdleTick => {
            existing.is_some_and(|conflict| inner_conflict_review_due(conflict, payload.now_secs))
                || recent_persona.is_some_and(|evidence| !evidence.volatility_flags.is_empty())
        }
    }
}

fn self_runtime_decision_has_subjective_refresh_signal(decision: &SelfRuntimeDecision) -> bool {
    decision.refresh_inner_life
        || decision.refresh_private_docs
        || decision.refresh_private_garden
        || decision.refresh_self_model
        || decision.refresh_self_continuity
        || decision.refresh_boundary_persona
        || decision.refresh_outer_voice
        || decision.boundary_flush
}

fn self_runtime_decision_requests_upward_distillation(decision: &SelfRuntimeDecision) -> bool {
    decision.refresh_self_model
        || decision.refresh_self_continuity
        || decision.refresh_self_authored_core
}

fn felt_significance_persona_evidence_supports_refresh(
    evidence: &crate::memory::RecentPersonaEvidence,
) -> bool {
    evidence.has_promotable_growth_signals()
        || !evidence.repeated_relationship_posture.trim().is_empty()
        || !evidence.repeated_disclosure_action.trim().is_empty()
        || !evidence.volatility_flags.is_empty()
}

fn self_runtime_has_turn_material(payload: &SelfRuntimeJobPayload) -> bool {
    !payload.user_content.trim().is_empty() && !payload.reply_content.trim().is_empty()
}

fn inner_conflict_review_due(conflict: &InnerConflict, now_secs: u64) -> bool {
    conflict.review_due_at(now_secs)
}

fn build_self_runtime_sandbox_probe_text(
    ledgers: &[crate::memory::TurnLedger],
    max_len: usize,
) -> Option<String> {
    if max_len < 128 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(1024));
    out.push_str("## Self-Runtime Sandbox Candidate Evidence\n");
    out.push_str("Existing counterfactual/adversarial traces only; candidate evidence, not a write authority.\n");
    for ledger in ledgers {
        if let Some(block) = ledger
            .counterfactual
            .as_ref()
            .and_then(|counterfactual| render_turn_counterfactual_ledger_block(counterfactual, 360))
        {
            append_sandbox_probe_block(&mut out, &block, max_len);
        }
        if let Some(block) = ledger
            .adversarial_arena
            .as_ref()
            .and_then(|arena| render_turn_adversarial_arena_ledger_block(arena, 360))
        {
            append_sandbox_probe_block(&mut out, &block, max_len);
        }
        if out.chars().count() >= max_len {
            break;
        }
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len)
        .trim()
        .to_string();
    (rendered.lines().count() > 2).then_some(rendered)
}

fn append_sandbox_probe_block(out: &mut String, block: &str, max_len: usize) {
    let remaining = max_len.saturating_sub(out.chars().count());
    if remaining < 64 {
        return;
    }
    let block = super::scrub_memory_prompt_block(block);
    if block.trim().is_empty() {
        return;
    }
    let block = truncate_content_to_max(block.trim(), remaining.saturating_sub(1));
    let _ = writeln!(out, "{}", block.trim());
}

fn active_inner_conflict(conflict: Option<&InnerConflict>, now_secs: u64) -> bool {
    conflict.is_some_and(|conflict| conflict.is_active_at(now_secs))
}

fn apply_inner_conflict_upward_distillation_gate(
    decision: &mut SelfRuntimeDecision,
    conflict: Option<&InnerConflict>,
    now_secs: u64,
) -> bool {
    if !active_inner_conflict(conflict, now_secs) {
        return false;
    }
    decision.refresh_self_model = false;
    decision.self_model_intent.clear();
    decision.self_model_sources.clear();
    decision.refresh_self_continuity = false;
    decision.self_continuity_intent.clear();
    decision.self_continuity_sources.clear();
    decision.refresh_self_authored_core = false;
    decision.self_authored_core_intent.clear();
    decision.self_authored_core_sources.clear();
    true
}

fn apply_self_runtime_post_finalize_gates(
    decision: &mut Option<SelfRuntimeDecision>,
    authority_plan: SelfRuntimeAuthorityPlan,
    memory_system_kind: MemorySystemKind,
    payload: &SelfRuntimeJobPayload,
    recent_persona: Option<&crate::memory::RecentPersonaEvidence>,
    conflict: Option<&InnerConflict>,
    now_secs: u64,
) {
    let Some(decision) = decision.as_mut() else {
        return;
    };
    apply_self_runtime_authority_plan(decision, authority_plan);
    apply_embedded_self_model_refresh_gate(decision, memory_system_kind, payload, recent_persona);
    apply_inner_conflict_upward_distillation_gate(decision, conflict, now_secs);
}

fn persist_felt_significance_refresh_outcome(
    store: &dyn FeltSignificanceStore,
    scope_id: &str,
    existing: Option<FeltSignificance>,
    refresh_result: Result<FeltSignificanceRefreshCandidate>,
) -> Result<FeltSignificanceRefreshOutcome> {
    match refresh_result? {
        FeltSignificanceRefreshCandidate::Skipped => Ok(FeltSignificanceRefreshOutcome::Skipped),
        FeltSignificanceRefreshCandidate::Cleared => {
            let latest = store.get(scope_id)?;
            if whole_record_lease_advanced(
                existing.as_ref(),
                latest.as_ref(),
                existing.as_ref().map(|value| value.updated_at).unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            ) {
                return Ok(FeltSignificanceRefreshOutcome::Skipped);
            }
            if latest.is_some() {
                store.clear(scope_id)?;
                Ok(FeltSignificanceRefreshOutcome::Cleared)
            } else {
                Ok(FeltSignificanceRefreshOutcome::Skipped)
            }
        }
        FeltSignificanceRefreshCandidate::Updated(next) => {
            let latest = store.get(scope_id)?;
            if latest.as_ref() == Some(&next)
                || whole_record_lease_advanced(
                    existing.as_ref(),
                    latest.as_ref(),
                    existing.as_ref().map(|value| value.updated_at).unwrap_or(0),
                    latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
                )
            {
                return Ok(FeltSignificanceRefreshOutcome::Skipped);
            }
            store.set(scope_id, &next)?;
            Ok(FeltSignificanceRefreshOutcome::Updated)
        }
    }
}

fn persist_temperament_continuity_refresh_outcome(
    store: &dyn TemperamentContinuityStore,
    scope_id: &str,
    existing: Option<TemperamentContinuity>,
    refresh_result: Result<TemperamentContinuityRefreshCandidate>,
) -> Result<TemperamentContinuityRefreshOutcome> {
    match refresh_result? {
        TemperamentContinuityRefreshCandidate::Skipped => {
            Ok(TemperamentContinuityRefreshOutcome::Skipped)
        }
        TemperamentContinuityRefreshCandidate::Cleared => {
            let latest = store.get(scope_id)?;
            if whole_record_lease_advanced(
                existing.as_ref(),
                latest.as_ref(),
                existing.as_ref().map(|value| value.updated_at).unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            ) {
                return Ok(TemperamentContinuityRefreshOutcome::Skipped);
            }
            if latest.is_some() {
                store.clear(scope_id)?;
                Ok(TemperamentContinuityRefreshOutcome::Cleared)
            } else {
                Ok(TemperamentContinuityRefreshOutcome::Skipped)
            }
        }
        TemperamentContinuityRefreshCandidate::Updated(next) => {
            let latest = store.get(scope_id)?;
            if latest.as_ref() == Some(&next)
                || whole_record_lease_advanced(
                    existing.as_ref(),
                    latest.as_ref(),
                    existing.as_ref().map(|value| value.updated_at).unwrap_or(0),
                    latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
                )
            {
                return Ok(TemperamentContinuityRefreshOutcome::Skipped);
            }
            store.set(scope_id, &next)?;
            Ok(TemperamentContinuityRefreshOutcome::Updated)
        }
    }
}

fn persist_inner_conflict_refresh_outcome(
    store: &dyn InnerConflictStore,
    scope_id: &str,
    existing: Option<InnerConflict>,
    refresh_result: Result<InnerConflictRefreshCandidate>,
) -> Result<InnerConflictRefreshOutcome> {
    match refresh_result? {
        InnerConflictRefreshCandidate::Skipped => Ok(InnerConflictRefreshOutcome::Skipped),
        InnerConflictRefreshCandidate::Cleared => {
            let latest = store.get(scope_id)?;
            if whole_record_lease_advanced(
                existing.as_ref(),
                latest.as_ref(),
                existing.as_ref().map(|value| value.updated_at).unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            ) {
                return Ok(InnerConflictRefreshOutcome::Skipped);
            }
            if latest.is_some() {
                store.clear(scope_id)?;
                Ok(InnerConflictRefreshOutcome::Cleared)
            } else {
                Ok(InnerConflictRefreshOutcome::Skipped)
            }
        }
        InnerConflictRefreshCandidate::Updated(next) => {
            let latest = store.get(scope_id)?;
            if latest.as_ref() == Some(&next)
                || whole_record_lease_advanced(
                    existing.as_ref(),
                    latest.as_ref(),
                    existing.as_ref().map(|value| value.updated_at).unwrap_or(0),
                    latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
                )
            {
                return Ok(InnerConflictRefreshOutcome::Skipped);
            }
            store.set(scope_id, &next)?;
            Ok(InnerConflictRefreshOutcome::Updated)
        }
    }
}

struct SelfRuntimePlanningJournal {
    effects: std::sync::Mutex<Vec<SelfRuntimePlannedEffectV1>>,
}

impl SelfRuntimePlanningJournal {
    fn new(initial: Vec<SelfRuntimePlannedEffectV1>) -> Self {
        let journal = Self {
            effects: std::sync::Mutex::new(Vec::new()),
        };
        for effect in initial {
            journal.record(effect);
        }
        journal
    }

    fn snapshot(&self) -> Vec<SelfRuntimePlannedEffectV1> {
        self.effects
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn record(&self, effect: SelfRuntimePlannedEffectV1) {
        let key = self_runtime_planned_effect_key(&effect);
        let mut effects = self
            .effects
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        effects.retain(|existing| self_runtime_planned_effect_key(existing) != key);
        effects.push(effect);
    }
}

fn self_runtime_planned_effect_key(effect: &SelfRuntimePlannedEffectV1) -> String {
    match effect {
        SelfRuntimePlannedEffectV1::SetWorldSense { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearWorldSense { scope_id } => {
            format!("world_sense:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetAutonomyStrategy { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearAutonomyStrategy { scope_id } => {
            format!("autonomy_strategy:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetInnerLife { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearInnerLife { scope_id } => {
            format!("inner_life:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetSelfModel { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearSelfModel { scope_id } => {
            format!("self_model:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetSelfContinuity { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearSelfContinuity { scope_id } => {
            format!("self_continuity:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetFeltSignificance { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearFeltSignificance { scope_id } => {
            format!("felt_significance:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetTemperamentContinuity { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearTemperamentContinuity { scope_id } => {
            format!("temperament_continuity:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetInnerConflict { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearInnerConflict { scope_id } => {
            format!("inner_conflict:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetPrivateDoc { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearPrivateDoc { scope_id } => {
            format!("private_doc:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetMentalPrivacy { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearMentalPrivacy { scope_id } => {
            format!("mental_privacy:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetOuterVoice { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearOuterVoice { scope_id } => {
            format!("outer_voice:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetRelationshipTopology { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearRelationshipTopology { scope_id } => {
            format!("relationship_topology:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::SetRelationshipPortfolio { scope_id, .. }
        | SelfRuntimePlannedEffectV1::ClearRelationshipPortfolio { scope_id } => {
            format!("relationship_portfolio:{scope_id}")
        }
        SelfRuntimePlannedEffectV1::UpsertPrivateGardenDoc {
            subject_id,
            document,
        } => format!("private_garden:{subject_id}:{}", document.path),
        SelfRuntimePlannedEffectV1::DeletePrivateGardenDoc {
            subject_id,
            doc_path,
        } => format!("private_garden:{subject_id}:{doc_path}"),
        SelfRuntimePlannedEffectV1::UpsertContinuityCapsules { drafts, now_secs } => {
            let ids = drafts
                .iter()
                .map(|draft| format!("{:?}:{}", draft.scope_kind, draft.scope_id))
                .collect::<Vec<_>>()
                .join("|");
            format!("continuity_capsules:{now_secs}:{ids}")
        }
        SelfRuntimePlannedEffectV1::UpsertTaskLearning { record } => {
            format!("task_learning:{}", record.learning_id)
        }
        SelfRuntimePlannedEffectV1::PutTaskArtifact { record } => {
            format!(
                "task_artifact:{}:{}",
                record.artifact.run_id, record.artifact.artifact_id
            )
        }
        SelfRuntimePlannedEffectV1::DeleteTaskArtifact {
            run_id,
            artifact_id,
        } => format!("task_artifact:{run_id}:{artifact_id}"),
        SelfRuntimePlannedEffectV1::WriteRuntimeSkill { name, .. }
        | SelfRuntimePlannedEffectV1::RemoveRuntimeSkill { name } => {
            format!("runtime_skill:{name}")
        }
        SelfRuntimePlannedEffectV1::SetLegacyMemory { .. } => "legacy_memory".to_string(),
        SelfRuntimePlannedEffectV1::WriteDailyNote { name, .. } => {
            format!("daily_note:{name}")
        }
    }
}

macro_rules! define_self_runtime_simple_planning_store {
    ($name:ident, $trait_name:ident, $value:ty, $set_variant:ident, $clear_variant:ident) => {
        struct $name<'a> {
            base: &'a dyn $trait_name,
            journal: &'a SelfRuntimePlanningJournal,
        }

        impl $trait_name for $name<'_> {
            fn get(&self, scope_id: &str) -> Result<Option<$value>> {
                for effect in self.journal.snapshot().iter().rev() {
                    match effect {
                        SelfRuntimePlannedEffectV1::$set_variant {
                            scope_id: candidate,
                            value,
                        } if candidate == scope_id => return Ok(Some(value.clone())),
                        SelfRuntimePlannedEffectV1::$clear_variant {
                            scope_id: candidate,
                        } if candidate == scope_id => return Ok(None),
                        _ => {}
                    }
                }
                self.base.get(scope_id)
            }

            fn set(&self, scope_id: &str, value: &$value) -> Result<()> {
                self.journal
                    .record(SelfRuntimePlannedEffectV1::$set_variant {
                        scope_id: scope_id.to_string(),
                        value: value.clone(),
                    });
                Ok(())
            }

            fn clear(&self, scope_id: &str) -> Result<()> {
                self.journal
                    .record(SelfRuntimePlannedEffectV1::$clear_variant {
                        scope_id: scope_id.to_string(),
                    });
                Ok(())
            }
        }
    };
}

define_self_runtime_simple_planning_store!(
    PlanningWorldSenseStore,
    WorldSenseStore,
    crate::memory::WorldSense,
    SetWorldSense,
    ClearWorldSense
);
define_self_runtime_simple_planning_store!(
    PlanningAutonomyStrategyStore,
    AutonomyStrategyStore,
    crate::memory::AutonomyStrategy,
    SetAutonomyStrategy,
    ClearAutonomyStrategy
);
define_self_runtime_simple_planning_store!(
    PlanningInnerLifeStore,
    InnerLifeStore,
    crate::memory::InnerLife,
    SetInnerLife,
    ClearInnerLife
);
define_self_runtime_simple_planning_store!(
    PlanningSelfModelStore,
    SelfModelStore,
    crate::memory::SelfModel,
    SetSelfModel,
    ClearSelfModel
);
define_self_runtime_simple_planning_store!(
    PlanningSelfContinuityStore,
    SelfContinuityStore,
    crate::memory::SelfContinuity,
    SetSelfContinuity,
    ClearSelfContinuity
);
define_self_runtime_simple_planning_store!(
    PlanningFeltSignificanceStore,
    FeltSignificanceStore,
    crate::memory::FeltSignificance,
    SetFeltSignificance,
    ClearFeltSignificance
);
define_self_runtime_simple_planning_store!(
    PlanningTemperamentContinuityStore,
    TemperamentContinuityStore,
    crate::memory::TemperamentContinuity,
    SetTemperamentContinuity,
    ClearTemperamentContinuity
);
define_self_runtime_simple_planning_store!(
    PlanningInnerConflictStore,
    InnerConflictStore,
    crate::memory::InnerConflict,
    SetInnerConflict,
    ClearInnerConflict
);
define_self_runtime_simple_planning_store!(
    PlanningPrivateDocStore,
    PrivateDocStore,
    crate::memory::PrivateDocWorkspace,
    SetPrivateDoc,
    ClearPrivateDoc
);
define_self_runtime_simple_planning_store!(
    PlanningMentalPrivacyStore,
    MentalPrivacyStore,
    crate::memory::MentalPrivacyState,
    SetMentalPrivacy,
    ClearMentalPrivacy
);
define_self_runtime_simple_planning_store!(
    PlanningOuterVoiceStore,
    OuterVoiceStore,
    crate::memory::OuterVoice,
    SetOuterVoice,
    ClearOuterVoice
);
define_self_runtime_simple_planning_store!(
    PlanningRelationshipTopologyStore,
    RelationshipTopologyStore,
    crate::memory::RelationshipTopology,
    SetRelationshipTopology,
    ClearRelationshipTopology
);
define_self_runtime_simple_planning_store!(
    PlanningRelationshipPortfolioStore,
    RelationshipPortfolioStore,
    crate::memory::RelationshipPortfolio,
    SetRelationshipPortfolio,
    ClearRelationshipPortfolio
);
struct PlanningPrivateGardenStore<'a> {
    base: &'a dyn PrivateGardenStore,
    journal: &'a SelfRuntimePlanningJournal,
}

impl PlanningPrivateGardenStore<'_> {
    fn materialize(
        &self,
        mounted_subject_id: &str,
    ) -> Result<std::collections::BTreeMap<String, crate::memory::PrivateGardenDoc>> {
        let mut docs = std::collections::BTreeMap::new();
        for record in self.base.list(mounted_subject_id, usize::MAX)? {
            if let Some(doc) = self.base.read(mounted_subject_id, &record.path)? {
                docs.insert(record.path, doc);
            }
        }
        for effect in self.journal.snapshot() {
            match effect {
                SelfRuntimePlannedEffectV1::UpsertPrivateGardenDoc {
                    subject_id,
                    document,
                } if subject_id == mounted_subject_id => {
                    docs.insert(document.path.clone(), document);
                }
                SelfRuntimePlannedEffectV1::DeletePrivateGardenDoc {
                    subject_id,
                    doc_path,
                } if subject_id == mounted_subject_id => {
                    docs.remove(&doc_path);
                }
                _ => {}
            }
        }
        Ok(docs)
    }
}

impl PrivateGardenStore for PlanningPrivateGardenStore<'_> {
    fn list(
        &self,
        mounted_subject_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::memory::PrivateGardenDocRecord>> {
        let mut records = self
            .materialize(mounted_subject_id)?
            .into_values()
            .map(|document| crate::memory::PrivateGardenDocRecord {
                path: document.path,
                updated_at: document.updated_at,
                revision: document.revision,
                bytes: document.content.len(),
                preview: super::build_private_garden_preview(&document.content),
            })
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.path.cmp(&right.path))
        });
        records.truncate(limit);
        Ok(records)
    }

    fn read(
        &self,
        mounted_subject_id: &str,
        doc_path: &str,
    ) -> Result<Option<crate::memory::PrivateGardenDoc>> {
        Ok(self.materialize(mounted_subject_id)?.remove(doc_path))
    }

    fn write(
        &self,
        mounted_subject_id: &str,
        doc_path: &str,
        content: &str,
        now_secs: u64,
    ) -> Result<crate::memory::PrivateGardenDocRecord> {
        let path = super::normalize_private_garden_doc_path(doc_path)?;
        let revision = self
            .read(mounted_subject_id, &path)?
            .map(|document| document.revision.saturating_add(1))
            .unwrap_or(1);
        let document = crate::memory::PrivateGardenDoc {
            path: path.clone(),
            content: content.to_string(),
            updated_at: now_secs,
            revision,
        };
        let record = crate::memory::PrivateGardenDocRecord {
            path,
            updated_at: now_secs,
            revision,
            bytes: content.len(),
            preview: super::build_private_garden_preview(content),
        };
        self.journal
            .record(SelfRuntimePlannedEffectV1::UpsertPrivateGardenDoc {
                subject_id: mounted_subject_id.to_string(),
                document,
            });
        Ok(record)
    }

    fn move_doc(
        &self,
        mounted_subject_id: &str,
        from_path: &str,
        to_path: &str,
        now_secs: u64,
    ) -> Result<Option<crate::memory::PrivateGardenDocRecord>> {
        let from_path = super::normalize_private_garden_doc_path(from_path)?;
        let to_path = super::normalize_private_garden_doc_path(to_path)?;
        let Some(document) = self.read(mounted_subject_id, &from_path)? else {
            return Ok(None);
        };
        self.journal
            .record(SelfRuntimePlannedEffectV1::DeletePrivateGardenDoc {
                subject_id: mounted_subject_id.to_string(),
                doc_path: from_path,
            });
        let moved = crate::memory::PrivateGardenDoc {
            path: to_path.clone(),
            content: document.content,
            updated_at: now_secs,
            revision: document.revision.saturating_add(1),
        };
        let record = crate::memory::PrivateGardenDocRecord {
            path: to_path,
            updated_at: moved.updated_at,
            revision: moved.revision,
            bytes: moved.content.len(),
            preview: super::build_private_garden_preview(&moved.content),
        };
        self.journal
            .record(SelfRuntimePlannedEffectV1::UpsertPrivateGardenDoc {
                subject_id: mounted_subject_id.to_string(),
                document: moved,
            });
        Ok(Some(record))
    }

    fn delete(&self, mounted_subject_id: &str, doc_path: &str) -> Result<bool> {
        let path = super::normalize_private_garden_doc_path(doc_path)?;
        if self.read(mounted_subject_id, &path)?.is_none() {
            return Ok(false);
        }
        self.journal
            .record(SelfRuntimePlannedEffectV1::DeletePrivateGardenDoc {
                subject_id: mounted_subject_id.to_string(),
                doc_path: path,
            });
        Ok(true)
    }
}

struct PlanningContinuityCapsuleStore<'a> {
    base: &'a dyn ContinuityCapsuleStore,
    journal: &'a SelfRuntimePlanningJournal,
}

impl PlanningContinuityCapsuleStore<'_> {
    fn materialize(&self) -> Result<Vec<crate::memory::ContinuityCapsule>> {
        let mut entries = self.base.list(usize::MAX)?;
        for effect in self.journal.snapshot() {
            if let SelfRuntimePlannedEffectV1::UpsertContinuityCapsules { drafts, now_secs } =
                effect
            {
                super::apply_continuity_capsule_drafts(&mut entries, &drafts, now_secs);
            }
        }
        Ok(entries)
    }
}

impl ContinuityCapsuleStore for PlanningContinuityCapsuleStore<'_> {
    fn upsert_many(
        &self,
        drafts: &[ContinuityCapsuleDraft],
        now_secs: u64,
    ) -> Result<ContinuityCapsuleWriteOutcome> {
        let mut entries = self.materialize()?;
        let outcome = super::apply_continuity_capsule_drafts(&mut entries, drafts, now_secs);
        self.journal
            .record(SelfRuntimePlannedEffectV1::UpsertContinuityCapsules {
                drafts: drafts.to_vec(),
                now_secs,
            });
        Ok(outcome)
    }

    fn get(&self, capsule_id: &str) -> Result<Option<crate::memory::ContinuityCapsule>> {
        Ok(self
            .materialize()?
            .into_iter()
            .find(|capsule| capsule.capsule_id == capsule_id))
    }

    fn list(&self, limit: usize) -> Result<Vec<crate::memory::ContinuityCapsule>> {
        let mut entries = self.materialize()?;
        entries.truncate(limit);
        Ok(entries)
    }

    fn count(&self) -> Result<usize> {
        Ok(self.materialize()?.len())
    }
}

struct PlanningTaskLearningStore<'a> {
    base: &'a dyn TaskLearningStore,
    journal: &'a SelfRuntimePlanningJournal,
}

impl PlanningTaskLearningStore<'_> {
    fn merge(&self, records: Vec<TaskLearningRecord>) -> Vec<TaskLearningRecord> {
        let mut by_id = records
            .into_iter()
            .map(|record| (record.learning_id.clone(), record))
            .collect::<std::collections::BTreeMap<_, _>>();
        for effect in self.journal.snapshot() {
            if let SelfRuntimePlannedEffectV1::UpsertTaskLearning { record } = effect {
                by_id.insert(record.learning_id.clone(), record);
            }
        }
        by_id.into_values().collect()
    }
}

impl TaskLearningStore for PlanningTaskLearningStore<'_> {
    fn get(&self, learning_id: &str) -> Result<Option<TaskLearningRecord>> {
        for effect in self.journal.snapshot().iter().rev() {
            if let SelfRuntimePlannedEffectV1::UpsertTaskLearning { record } = effect {
                if record.learning_id == learning_id {
                    return Ok(Some(record.clone()));
                }
            }
        }
        self.base.get(learning_id)
    }

    fn upsert(&self, record: &TaskLearningRecord) -> Result<()> {
        self.journal
            .record(SelfRuntimePlannedEffectV1::UpsertTaskLearning {
                record: record.clone(),
            });
        Ok(())
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        let mut records = self.merge(self.base.list_recent(usize::MAX)?);
        records.sort_by_key(|record| std::cmp::Reverse(record.observed_at));
        records.truncate(limit);
        Ok(records)
    }

    fn list_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskLearningRecord>> {
        let mut records = self.merge(self.base.list_for_chat(channel, chat_id, usize::MAX)?);
        records
            .retain(|record| record.source_channel == channel && record.source_chat_id == chat_id);
        records.sort_by_key(|record| std::cmp::Reverse(record.observed_at));
        records.truncate(limit);
        Ok(records)
    }

    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        let mut records = self.merge(self.base.list_for_run(run_id, usize::MAX)?);
        records.retain(|record| record.run_id == run_id);
        records.sort_by_key(|record| std::cmp::Reverse(record.observed_at));
        records.truncate(limit);
        Ok(records)
    }
}

struct PlanningTaskArtifactStore<'a> {
    base: &'a dyn TaskArtifactStore,
    journal: &'a SelfRuntimePlanningJournal,
}

impl TaskArtifactStore for PlanningTaskArtifactStore<'_> {
    fn put(&self, record: &TaskArtifactRecord) -> Result<()> {
        self.journal
            .record(SelfRuntimePlannedEffectV1::PutTaskArtifact {
                record: record.clone(),
            });
        Ok(())
    }

    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskArtifactRecord>> {
        let mut records = self
            .base
            .list_for_run(run_id, usize::MAX)?
            .into_iter()
            .map(|record| (record.artifact.artifact_id.clone(), record))
            .collect::<std::collections::BTreeMap<_, _>>();
        for effect in self.journal.snapshot() {
            match effect {
                SelfRuntimePlannedEffectV1::PutTaskArtifact { record }
                    if record.artifact.run_id == run_id =>
                {
                    records.insert(record.artifact.artifact_id.clone(), record);
                }
                SelfRuntimePlannedEffectV1::DeleteTaskArtifact {
                    run_id: candidate,
                    artifact_id,
                } if candidate == run_id => {
                    records.remove(&artifact_id);
                }
                _ => {}
            }
        }
        let mut records = records.into_values().collect::<Vec<_>>();
        records.truncate(limit);
        Ok(records)
    }

    fn delete(&self, run_id: &str, artifact_id: &str) -> Result<bool> {
        let exists = self
            .list_for_run(run_id, usize::MAX)?
            .iter()
            .any(|record| record.artifact.artifact_id == artifact_id);
        if exists {
            self.journal
                .record(SelfRuntimePlannedEffectV1::DeleteTaskArtifact {
                    run_id: run_id.to_string(),
                    artifact_id: artifact_id.to_string(),
                });
        }
        Ok(exists)
    }
}

struct PlanningSkillStorage<'a> {
    base: &'a dyn SkillStorage,
    journal: &'a SelfRuntimePlanningJournal,
}

impl SkillStorage for PlanningSkillStorage<'_> {
    fn list_names(&self) -> Result<Vec<String>> {
        let mut names = self
            .base
            .list_names()?
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        for effect in self.journal.snapshot() {
            match effect {
                SelfRuntimePlannedEffectV1::WriteRuntimeSkill { name, .. } => {
                    names.insert(name);
                }
                SelfRuntimePlannedEffectV1::RemoveRuntimeSkill { name } => {
                    names.remove(&name);
                }
                _ => {}
            }
        }
        Ok(names.into_iter().collect())
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        for effect in self.journal.snapshot().iter().rev() {
            match effect {
                SelfRuntimePlannedEffectV1::WriteRuntimeSkill {
                    name: candidate,
                    content,
                } if candidate == name => return Ok(content.clone()),
                SelfRuntimePlannedEffectV1::RemoveRuntimeSkill { name: candidate }
                    if candidate == name =>
                {
                    return Ok(Vec::new())
                }
                _ => {}
            }
        }
        self.base.read(name)
    }

    fn write(&self, name: &str, content: &[u8]) -> Result<()> {
        self.journal
            .record(SelfRuntimePlannedEffectV1::WriteRuntimeSkill {
                name: name.to_string(),
                content: content.to_vec(),
            });
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<()> {
        self.journal
            .record(SelfRuntimePlannedEffectV1::RemoveRuntimeSkill {
                name: name.to_string(),
            });
        Ok(())
    }
}

struct PlanningMemoryStore<'a> {
    base: &'a dyn MemoryStore,
    journal: &'a SelfRuntimePlanningJournal,
}

impl MemoryStore for PlanningMemoryStore<'_> {
    fn get_memory(&self) -> Result<String> {
        for effect in self.journal.snapshot().iter().rev() {
            if let SelfRuntimePlannedEffectV1::SetLegacyMemory { content } = effect {
                return Ok(content.clone());
            }
        }
        self.base.get_memory()
    }

    fn set_memory(&self, content: &str) -> Result<()> {
        self.journal
            .record(SelfRuntimePlannedEffectV1::SetLegacyMemory {
                content: content.to_string(),
            });
        Ok(())
    }

    fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
        let mut names = self
            .base
            .list_daily_note_names(usize::MAX)?
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        for effect in self.journal.snapshot() {
            if let SelfRuntimePlannedEffectV1::WriteDailyNote { name, .. } = effect {
                names.insert(name);
            }
        }
        let mut names = names.into_iter().collect::<Vec<_>>();
        names.sort_by(|left, right| right.cmp(left));
        names.truncate(recent_n);
        Ok(names)
    }

    fn get_daily_note(&self, name: &str) -> Result<String> {
        for effect in self.journal.snapshot().iter().rev() {
            if let SelfRuntimePlannedEffectV1::WriteDailyNote {
                name: candidate,
                content,
            } = effect
            {
                if candidate == name {
                    return Ok(content.clone());
                }
            }
        }
        self.base.get_daily_note(name)
    }

    fn write_daily_note(&self, name: &str, content: &str) -> Result<()> {
        self.journal
            .record(SelfRuntimePlannedEffectV1::WriteDailyNote {
                name: name.to_string(),
                content: content.to_string(),
            });
        Ok(())
    }
}

fn execute_self_runtime_with_context(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SelfRuntimeContext<'_>,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
) -> Box<SelfRuntimeOutcome> {
    let profile = ctx.memory_system_kind.memory_profile();
    let authority_plan = decide_self_runtime_authority(ctx.memory_system_kind);
    let state = load_self_runtime_state(&ctx, chat_id, payload, profile, authority_plan);
    if let Some(outcome) = self_runtime_load_guard_outcome(chat_id, state.as_ref()) {
        return outcome;
    }
    if let Some(reason) =
        self_runtime_post_reply_loaded_skip_reason(state.as_ref(), payload, profile)
    {
        log::debug!(
            "[self_runtime] skip post-reply runtime chat_id={} reason={}",
            chat_id,
            reason
        );
        return skipped_self_runtime_outcome();
    }
    crate::platform::task_wdt::feed_current_task();
    let prelude =
        refresh_world_and_autonomy(http, llm, &ctx, chat_id, payload, profile, state.as_ref());
    crate::platform::task_wdt::feed_current_task();
    let action_results = execute_self_runtime_actions(
        http,
        llm,
        &ctx,
        chat_id,
        payload,
        state.as_ref(),
        prelude.as_ref(),
    );
    crate::platform::task_wdt::feed_current_task();
    if let Some(decision) = action_results.decision.as_ref() {
        if let Err(error) = persist_self_runtime_continuity_capsules(
            ctx.continuity_capsule_store,
            ctx.task_run_store,
            chat_id,
            payload,
            state.as_ref(),
            decision,
        ) {
            log::warn!(
                "[self_runtime] continuity capsule persistence failed chat_id={}: {}",
                chat_id,
                error
            );
        }
        crate::platform::task_wdt::feed_current_task();
    }

    let touch_self_continuity_result = touch_self_continuity_runtime(
        ctx.self_continuity_store,
        ctx.mounted_subject_id,
        payload.now_secs,
        payload.trigger == SelfRuntimeTrigger::PostReply,
        true,
        Some(chat_id),
        Some(payload.source_channel.as_str()),
    )
    .map_err(|error| {
        let staged = error.with_stage("self_runtime_touch_autonomy_clock");
        log::warn!(
            "[self_runtime] autonomy runtime anchor persistence failed chat_id={}: {}",
            chat_id,
            staged
        );
        staged
    });
    crate::platform::task_wdt::feed_current_task();
    if matches!(payload.trigger, SelfRuntimeTrigger::IdleTick) {
        if idle_memory_hygiene_budget_allows_run() {
            let _ = run_memory_hygiene_jobs(
                MemoryHygieneContext {
                    session_store: ctx.session_store,
                    session_summary_store: ctx.session_summary_store,
                    memory_store: ctx.memory_store,
                    turn_ledger_store: ctx.turn_ledger_store,
                    long_term_memory_store: ctx.long_term_memory_store,
                    skill_storage: ctx.skill_storage,
                },
                chat_id,
                profile,
                payload.now_secs,
            );
            crate::platform::task_wdt::feed_current_task();
        } else {
            log::debug!(
                "[self_runtime] skip idle memory hygiene this tick because write budget is reserved"
            );
        }
    }

    Box::new(SelfRuntimeOutcome {
        decision: action_results.decision,
        world_sense_result: prelude.world_sense_result,
        autonomy_strategy_result: prelude.autonomy_strategy_result,
        inner_life_result: action_results.inner_life_result,
        felt_significance_result: action_results.felt_significance_result,
        temperament_continuity_result: action_results.temperament_continuity_result,
        inner_conflict_result: action_results.inner_conflict_result,
        private_doc_result: action_results.private_doc_result,
        self_model_result: action_results.self_model_result,
        self_authored_core_result: action_results.self_authored_core_result,
        self_continuity_result: merge_self_continuity_touch_result(
            action_results.self_continuity_result,
            touch_self_continuity_result,
        ),
        task_learning_result: action_results.task_learning_result,
        private_garden_result: action_results.private_garden_result,
        boundary_persona_result: action_results.boundary_persona_result,
        outer_voice_result: action_results.outer_voice_result,
    })
}

/// Evaluates one complete self-runtime cycle with read-your-writes semantics and zero persistent
/// Store mutation. All owner writes are returned as typed effects.
#[allow(clippy::too_many_arguments)]
pub fn plan_self_runtime(
    operation_id: &str,
    initial: SelfRuntimeInitialPlanningStateV1,
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SelfRuntimeContext<'_>,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
) -> Result<SelfRuntimeExecutionPlanV1> {
    if operation_id.is_empty()
        || operation_id.trim() != operation_id
        || operation_id.len() > 256
        || operation_id.chars().any(char::is_control)
    {
        return Err(crate::error::Error::config(
            "self_runtime_operation_id",
            "operation_id must be non-empty canonical text",
        ));
    }
    if initial.owner_subject_id != ctx.mounted_subject_id {
        return Err(crate::error::Error::config(
            "self_runtime_owner_subject_id",
            "initial planning state must belong to the mounted subject",
        ));
    }
    let _ = resolve_relationship_id(
        ctx.mounted_subject_id,
        ctx.active_relationship_id,
        payload.source_channel.as_str(),
        chat_id,
    )?;
    for effect in &initial.planned_effects {
        match effect {
            SelfRuntimePlannedEffectV1::UpsertPrivateGardenDoc { subject_id, .. }
            | SelfRuntimePlannedEffectV1::DeletePrivateGardenDoc { subject_id, .. }
                if subject_id != ctx.mounted_subject_id =>
            {
                return Err(crate::error::Error::config(
                    "self_runtime_private_garden_owner",
                    "private-garden planning effect belongs to another subject",
                ));
            }
            _ => {}
        }
    }

    let journal = SelfRuntimePlanningJournal::new(initial.planned_effects);
    let continuity_capsule_store = PlanningContinuityCapsuleStore {
        base: ctx.continuity_capsule_store,
        journal: &journal,
    };
    let self_model_store = PlanningSelfModelStore {
        base: ctx.self_model_store,
        journal: &journal,
    };
    let private_doc_store = PlanningPrivateDocStore {
        base: ctx.private_doc_store,
        journal: &journal,
    };
    let private_garden_store = PlanningPrivateGardenStore {
        base: ctx.private_garden_store,
        journal: &journal,
    };
    let inner_life_store = PlanningInnerLifeStore {
        base: ctx.inner_life_store,
        journal: &journal,
    };
    let self_continuity_store = PlanningSelfContinuityStore {
        base: ctx.self_continuity_store,
        journal: &journal,
    };
    let felt_significance_store = PlanningFeltSignificanceStore {
        base: ctx.felt_significance_store,
        journal: &journal,
    };
    let temperament_continuity_store = PlanningTemperamentContinuityStore {
        base: ctx.temperament_continuity_store,
        journal: &journal,
    };
    let inner_conflict_store = PlanningInnerConflictStore {
        base: ctx.inner_conflict_store,
        journal: &journal,
    };
    let relationship_portfolio_store = PlanningRelationshipPortfolioStore {
        base: ctx.relationship_portfolio_store,
        journal: &journal,
    };
    let relationship_topology_store = PlanningRelationshipTopologyStore {
        base: ctx.relationship_topology_store,
        journal: &journal,
    };
    let world_sense_store = PlanningWorldSenseStore {
        base: ctx.world_sense_store,
        journal: &journal,
    };
    let autonomy_strategy_store = PlanningAutonomyStrategyStore {
        base: ctx.autonomy_strategy_store,
        journal: &journal,
    };
    let outer_voice_store = PlanningOuterVoiceStore {
        base: ctx.outer_voice_store,
        journal: &journal,
    };
    let mental_privacy_store = PlanningMentalPrivacyStore {
        base: ctx.mental_privacy_store,
        journal: &journal,
    };
    let task_artifact_store = PlanningTaskArtifactStore {
        base: ctx.task_artifact_store,
        journal: &journal,
    };
    let task_learning_store = PlanningTaskLearningStore {
        base: ctx.task_learning_store,
        journal: &journal,
    };
    let skill_storage = PlanningSkillStorage {
        base: ctx.skill_storage,
        journal: &journal,
    };
    let memory_store = PlanningMemoryStore {
        base: ctx.memory_store,
        journal: &journal,
    };

    let planning_context = SelfRuntimeContext {
        mounted_subject_id: ctx.mounted_subject_id,
        active_relationship_id: ctx.active_relationship_id,
        memory_system_kind: ctx.memory_system_kind,
        session_store: ctx.session_store,
        memory_store: &memory_store,
        session_summary_store: ctx.session_summary_store,
        execution_state_store: ctx.execution_state_store,
        long_term_memory_store: ctx.long_term_memory_store,
        continuity_capsule_store: &continuity_capsule_store,
        self_model_store: &self_model_store,
        self_authored_core_store: ctx.self_authored_core_store,
        core_revision_ledger_store: ctx.core_revision_ledger_store,
        relationship_constitutional_read_store: ctx.relationship_constitutional_read_store,
        private_doc_store: &private_doc_store,
        private_garden_store: &private_garden_store,
        inner_life_store: &inner_life_store,
        self_continuity_store: &self_continuity_store,
        felt_significance_store: &felt_significance_store,
        temperament_continuity_store: &temperament_continuity_store,
        inner_conflict_store: &inner_conflict_store,
        relationship_portfolio_store: &relationship_portfolio_store,
        relationship_topology_store: &relationship_topology_store,
        world_sense_store: &world_sense_store,
        autonomy_strategy_store: &autonomy_strategy_store,
        outer_voice_store: &outer_voice_store,
        mental_privacy_store: &mental_privacy_store,
        remind_store: ctx.remind_store,
        task_store: ctx.task_store,
        task_run_store: ctx.task_run_store,
        task_artifact_store: &task_artifact_store,
        task_learning_store: &task_learning_store,
        turn_continuity_evidence_store: ctx.turn_continuity_evidence_store,
        turn_ledger_store: ctx.turn_ledger_store,
        skill_storage: &skill_storage,
    };
    let outcome = execute_self_runtime_with_context(http, llm, planning_context, chat_id, payload);

    Ok(SelfRuntimeExecutionPlanV1 {
        operation_id: operation_id.to_string(),
        owner_subject_id: ctx.mounted_subject_id.to_string(),
        outcome,
        planned_effects: journal.snapshot(),
    })
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
mod tests {
    use super::llm::parse_self_runtime_decision;
    use super::*;
    use crate::error::Result as BeetleResult;
    use crate::memory::{
        ContinuityCapsule, ContinuityCapsuleDraft, ContinuityCapsuleSource, ContinuityCapsuleStore,
        LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemorySlot, MemoryStore,
        MemorySystemKind,
    };
    use crate::platform::SkillStorage;
    use crate::task::{TaskItem, TaskQuery};
    use crate::task_execution::{
        TaskArtifactRecord, TaskArtifactStore, TaskLearningKind, TaskLearningRecord,
        TaskLearningRoute, TaskLearningStore, TaskPlan, TaskRun, TaskRunKind, TaskRunRecord,
        TaskRunStatus, TaskRunStore,
    };
    use crate::{
        llm::{LlmResponse, StopReason},
        platform::ResponseBody,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{atomic::AtomicUsize, atomic::Ordering, Mutex};

    #[derive(Default)]
    struct PlanningWriteSpy {
        writes: AtomicUsize,
        values: Mutex<HashMap<String, serde_json::Value>>,
        garden: Mutex<HashMap<String, crate::memory::PrivateGardenDoc>>,
        capsules: Mutex<Vec<ContinuityCapsule>>,
        learning: Mutex<HashMap<String, TaskLearningRecord>>,
        task_runs: Mutex<HashMap<String, TaskRunRecord>>,
        artifacts: Mutex<HashMap<String, TaskArtifactRecord>>,
        skills: Mutex<HashMap<String, Vec<u8>>>,
        notes: Mutex<HashMap<String, String>>,
        relationship_reads: Mutex<Vec<(String, String)>>,
    }

    impl PlanningWriteSpy {
        fn key(owner: &str, scope_id: &str) -> String {
            format!("{owner}:{scope_id}")
        }

        fn seed<T: Serialize>(&self, owner: &str, scope_id: &str, value: &T) {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    Self::key(owner, scope_id),
                    serde_json::to_value(value).expect("serialize planning spy seed"),
                );
        }

        fn get_typed<T: serde::de::DeserializeOwned>(
            &self,
            owner: &str,
            scope_id: &str,
        ) -> BeetleResult<Option<T>> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(&Self::key(owner, scope_id))
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| crate::error::Error::config("planning_spy", error.to_string()))
        }

        fn set_typed<T: Serialize>(
            &self,
            owner: &str,
            scope_id: &str,
            value: &T,
        ) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    Self::key(owner, scope_id),
                    serde_json::to_value(value).map_err(|error| {
                        crate::error::Error::config("planning_spy", error.to_string())
                    })?,
                );
            Ok(())
        }

        fn clear_typed(&self, owner: &str, scope_id: &str) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&Self::key(owner, scope_id));
            Ok(())
        }
    }

    macro_rules! impl_planning_spy_simple_store {
        ($trait_name:ident, $value:ty, $owner:literal) => {
            impl $trait_name for PlanningWriteSpy {
                fn get(&self, scope_id: &str) -> BeetleResult<Option<$value>> {
                    self.get_typed($owner, scope_id)
                }

                fn set(&self, scope_id: &str, value: &$value) -> BeetleResult<()> {
                    self.set_typed($owner, scope_id, value)
                }

                fn clear(&self, scope_id: &str) -> BeetleResult<()> {
                    self.clear_typed($owner, scope_id)
                }
            }
        };
    }

    impl_planning_spy_simple_store!(SelfModelStore, crate::memory::SelfModel, "self_model");
    impl_planning_spy_simple_store!(
        SelfAuthoredCoreStore,
        crate::memory::SelfAuthoredCore,
        "self_authored_core"
    );
    impl_planning_spy_simple_store!(
        CoreRevisionLedgerStore,
        crate::memory::CoreRevisionLedger,
        "core_revision_ledger"
    );
    impl_planning_spy_simple_store!(WorldSenseStore, crate::memory::WorldSense, "world_sense");
    impl_planning_spy_simple_store!(OuterVoiceStore, crate::memory::OuterVoice, "outer_voice");
    impl_planning_spy_simple_store!(
        AutonomyStrategyStore,
        crate::memory::AutonomyStrategy,
        "autonomy_strategy"
    );
    impl_planning_spy_simple_store!(InnerLifeStore, crate::memory::InnerLife, "inner_life");
    impl_planning_spy_simple_store!(
        SelfContinuityStore,
        crate::memory::SelfContinuity,
        "self_continuity"
    );
    impl_planning_spy_simple_store!(
        FeltSignificanceStore,
        crate::memory::FeltSignificance,
        "felt_significance"
    );
    impl_planning_spy_simple_store!(
        TemperamentContinuityStore,
        crate::memory::TemperamentContinuity,
        "temperament_continuity"
    );
    impl_planning_spy_simple_store!(
        InnerConflictStore,
        crate::memory::InnerConflict,
        "inner_conflict"
    );
    impl_planning_spy_simple_store!(
        PrivateDocStore,
        crate::memory::PrivateDocWorkspace,
        "private_doc"
    );
    impl_planning_spy_simple_store!(
        MentalPrivacyStore,
        crate::memory::MentalPrivacyState,
        "mental_privacy"
    );
    impl_planning_spy_simple_store!(
        RelationshipConstitutionStore,
        crate::memory::RelationshipConstitution,
        "relationship_constitution"
    );
    impl_planning_spy_simple_store!(
        RelationshipPortfolioStore,
        crate::memory::RelationshipPortfolio,
        "relationship_portfolio"
    );
    impl_planning_spy_simple_store!(
        RelationshipTopologyStore,
        crate::memory::RelationshipTopology,
        "relationship_topology"
    );
    impl_planning_spy_simple_store!(
        ExecutionStateStore,
        crate::memory::ExecutionState,
        "execution_state"
    );
    impl_planning_spy_simple_store!(TurnLedgerStore, crate::memory::TurnLedger, "turn_ledger");

    impl SessionStore for PlanningWriteSpy {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn load_recent(
            &self,
            _chat_id: &str,
            _n: usize,
        ) -> BeetleResult<Vec<crate::memory::SessionMessage>> {
            Ok(Vec::new())
        }

        fn clear(&self, _chat_id: &str) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn list_chat_ids(&self) -> BeetleResult<Vec<String>> {
            Ok(Vec::new())
        }
    }

    impl SessionSummaryStore for PlanningWriteSpy {
        fn get(&self, _chat_id: &str) -> BeetleResult<Option<String>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _summary: &str) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    impl TurnContinuityEvidenceStore for PlanningWriteSpy {
        fn append(
            &self,
            _chat_id: &str,
            _evidence: &crate::memory::TurnContinuityEvidence,
        ) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn list_recent(
            &self,
            _chat_id: &str,
            _limit: usize,
        ) -> BeetleResult<Vec<crate::memory::TurnContinuityEvidence>> {
            Ok(Vec::new())
        }

        fn recent_persona_evidence(
            &self,
            chat_id: &str,
        ) -> BeetleResult<Option<crate::memory::RecentPersonaEvidence>> {
            self.get_typed("recent_persona_evidence", chat_id)
        }
    }

    impl crate::memory::LongTermMemoryStore for PlanningWriteSpy {
        fn upsert_many(
            &self,
            _drafts: &[LongTermMemoryDraft],
            _now_secs: u64,
        ) -> BeetleResult<usize> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(0)
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> BeetleResult<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, _id: &str) -> BeetleResult<Option<LongTermMemoryEntry>> {
            Ok(None)
        }

        fn list(&self, _limit: usize) -> BeetleResult<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn delete(&self, _id: &str) -> BeetleResult<bool> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(false)
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> BeetleResult<bool> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(false)
        }

        fn count(&self) -> BeetleResult<usize> {
            Ok(0)
        }
    }

    impl ContinuityCapsuleStore for PlanningWriteSpy {
        fn upsert_many(
            &self,
            drafts: &[ContinuityCapsuleDraft],
            now_secs: u64,
        ) -> BeetleResult<crate::memory::ContinuityCapsuleWriteOutcome> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            let mut entries = self
                .capsules
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            Ok(crate::memory::apply_continuity_capsule_drafts(
                &mut entries,
                drafts,
                now_secs,
            ))
        }

        fn get(&self, capsule_id: &str) -> BeetleResult<Option<ContinuityCapsule>> {
            Ok(self
                .capsules
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .iter()
                .find(|capsule| capsule.capsule_id == capsule_id)
                .cloned())
        }

        fn list(&self, limit: usize) -> BeetleResult<Vec<ContinuityCapsule>> {
            Ok(self
                .capsules
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .iter()
                .take(limit)
                .cloned()
                .collect())
        }

        fn count(&self) -> BeetleResult<usize> {
            Ok(self
                .capsules
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len())
        }
    }

    impl PrivateGardenStore for PlanningWriteSpy {
        fn list(
            &self,
            _mounted_subject_id: &str,
            limit: usize,
        ) -> BeetleResult<Vec<crate::memory::PrivateGardenDocRecord>> {
            let mut records = self
                .garden
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .map(|document| crate::memory::PrivateGardenDocRecord {
                    path: document.path.clone(),
                    updated_at: document.updated_at,
                    revision: document.revision,
                    bytes: document.content.len(),
                    preview: crate::memory::build_private_garden_preview(&document.content),
                })
                .collect::<Vec<_>>();
            records.truncate(limit);
            Ok(records)
        }

        fn read(
            &self,
            _mounted_subject_id: &str,
            doc_path: &str,
        ) -> BeetleResult<Option<crate::memory::PrivateGardenDoc>> {
            Ok(self
                .garden
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(doc_path)
                .cloned())
        }

        fn write(
            &self,
            _mounted_subject_id: &str,
            doc_path: &str,
            content: &str,
            now_secs: u64,
        ) -> BeetleResult<crate::memory::PrivateGardenDocRecord> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            let document = crate::memory::PrivateGardenDoc {
                path: doc_path.to_string(),
                content: content.to_string(),
                updated_at: now_secs,
                revision: 1,
            };
            self.garden
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(doc_path.to_string(), document.clone());
            Ok(crate::memory::PrivateGardenDocRecord {
                path: document.path,
                updated_at: document.updated_at,
                revision: document.revision,
                bytes: document.content.len(),
                preview: crate::memory::build_private_garden_preview(&document.content),
            })
        }

        fn move_doc(
            &self,
            _mounted_subject_id: &str,
            from_path: &str,
            to_path: &str,
            now_secs: u64,
        ) -> BeetleResult<Option<crate::memory::PrivateGardenDocRecord>> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            let mut docs = self
                .garden
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let Some(mut document) = docs.remove(from_path) else {
                return Ok(None);
            };
            document.path = to_path.to_string();
            document.updated_at = now_secs;
            document.revision = document.revision.saturating_add(1);
            docs.insert(to_path.to_string(), document.clone());
            Ok(Some(crate::memory::PrivateGardenDocRecord {
                path: document.path,
                updated_at: document.updated_at,
                revision: document.revision,
                bytes: document.content.len(),
                preview: crate::memory::build_private_garden_preview(&document.content),
            }))
        }

        fn delete(&self, _mounted_subject_id: &str, doc_path: &str) -> BeetleResult<bool> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .garden
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(doc_path)
                .is_some())
        }
    }

    impl MemoryStore for PlanningWriteSpy {
        fn get_memory(&self) -> BeetleResult<String> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get("legacy_memory")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string())
        }

        fn set_memory(&self, content: &str) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert("legacy_memory".to_string(), json!(content));
            Ok(())
        }

        fn list_daily_note_names(&self, recent_n: usize) -> BeetleResult<Vec<String>> {
            let mut names = self
                .notes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            names.sort_by(|left, right| right.cmp(left));
            names.truncate(recent_n);
            Ok(names)
        }

        fn get_daily_note(&self, name: &str) -> BeetleResult<String> {
            Ok(self
                .notes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }

        fn write_daily_note(&self, name: &str, content: &str) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.notes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(name.to_string(), content.to_string());
            Ok(())
        }
    }

    impl SkillStorage for PlanningWriteSpy {
        fn list_names(&self) -> BeetleResult<Vec<String>> {
            Ok(self
                .skills
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> BeetleResult<Vec<u8>> {
            Ok(self
                .skills
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }

        fn write(&self, name: &str, content: &[u8]) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.skills
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.skills
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(name);
            Ok(())
        }
    }

    impl RemindAtStore for PlanningWriteSpy {
        fn get(
            &self,
            _channel: &str,
            _chat_id: &str,
            _id: &str,
        ) -> BeetleResult<Option<crate::reminder::ReminderItem>> {
            Ok(None)
        }

        fn upsert(&self, _reminder: &crate::reminder::ReminderItem) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> BeetleResult<bool> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(false)
        }

        fn list_due(
            &self,
            _now_unix_secs: u64,
            _limit: usize,
        ) -> BeetleResult<Vec<crate::reminder::ReminderItem>> {
            Ok(Vec::new())
        }

        fn delete_due(&self, _reminder: &crate::reminder::ReminderItem) -> BeetleResult<bool> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(false)
        }

        fn list_upcoming(
            &self,
            _channel: &str,
            _chat_id: &str,
            _now_unix_secs: u64,
            _limit: usize,
        ) -> BeetleResult<Vec<crate::reminder::ReminderItem>> {
            Ok(Vec::new())
        }
    }

    impl crate::task::TaskStore for PlanningWriteSpy {
        fn list(
            &self,
            _channel: &str,
            _chat_id: &str,
            _query: TaskQuery,
        ) -> BeetleResult<Vec<TaskItem>> {
            Ok(Vec::new())
        }

        fn get(&self, _channel: &str, _chat_id: &str, _id: &str) -> BeetleResult<Option<TaskItem>> {
            Ok(None)
        }

        fn upsert(&self, _task: &TaskItem) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> BeetleResult<bool> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(false)
        }

        fn list_due_unnotified(
            &self,
            _now_unix_secs: u64,
            _limit: usize,
        ) -> BeetleResult<Vec<TaskItem>> {
            Ok(Vec::new())
        }

        fn mark_due_notified(
            &self,
            _task: &TaskItem,
            _notified_at_unix_secs: u64,
        ) -> BeetleResult<bool> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(false)
        }
    }

    impl TaskRunStore for PlanningWriteSpy {
        fn get(&self, run_id: &str) -> BeetleResult<Option<TaskRunRecord>> {
            Ok(self
                .task_runs
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(run_id)
                .cloned())
        }

        fn upsert(&self, record: &TaskRunRecord) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.task_runs
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(record.run.run_id.clone(), record.clone());
            Ok(())
        }

        fn list_recent(&self, limit: usize) -> BeetleResult<Vec<TaskRunRecord>> {
            Ok(self
                .task_runs
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .take(limit)
                .cloned()
                .collect())
        }

        fn list_active_for_chat(
            &self,
            channel: &str,
            chat_id: &str,
            limit: usize,
        ) -> BeetleResult<Vec<TaskRunRecord>> {
            Ok(self
                .task_runs
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .filter(|record| {
                    record.run.source_channel == channel
                        && record.run.source_chat_id == chat_id
                        && !record.run.status.is_terminal()
                })
                .take(limit)
                .cloned()
                .collect())
        }
    }

    impl TaskArtifactStore for PlanningWriteSpy {
        fn put(&self, record: &TaskArtifactRecord) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.artifacts
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(record.artifact.artifact_id.clone(), record.clone());
            Ok(())
        }

        fn list_for_run(
            &self,
            run_id: &str,
            limit: usize,
        ) -> BeetleResult<Vec<TaskArtifactRecord>> {
            Ok(self
                .artifacts
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .filter(|record| record.artifact.run_id == run_id)
                .take(limit)
                .cloned()
                .collect())
        }

        fn delete(&self, _run_id: &str, artifact_id: &str) -> BeetleResult<bool> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .artifacts
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(artifact_id)
                .is_some())
        }
    }

    impl TaskLearningStore for PlanningWriteSpy {
        fn get(&self, learning_id: &str) -> BeetleResult<Option<TaskLearningRecord>> {
            Ok(self
                .learning
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(learning_id)
                .cloned())
        }

        fn upsert(&self, record: &TaskLearningRecord) -> BeetleResult<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.learning
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(record.learning_id.clone(), record.clone());
            Ok(())
        }

        fn list_recent(&self, limit: usize) -> BeetleResult<Vec<TaskLearningRecord>> {
            Ok(self
                .learning
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .take(limit)
                .cloned()
                .collect())
        }

        fn list_for_chat(
            &self,
            channel: &str,
            chat_id: &str,
            limit: usize,
        ) -> BeetleResult<Vec<TaskLearningRecord>> {
            Ok(self
                .learning
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .filter(|record| {
                    record.source_channel == channel && record.source_chat_id == chat_id
                })
                .take(limit)
                .cloned()
                .collect())
        }

        fn list_for_run(
            &self,
            run_id: &str,
            limit: usize,
        ) -> BeetleResult<Vec<TaskLearningRecord>> {
            Ok(self
                .learning
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .filter(|record| record.run_id == run_id)
                .take(limit)
                .cloned()
                .collect())
        }
    }

    struct PlanningNullHttp;

    impl LlmHttpClient for PlanningNullHttp {
        fn do_post(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> BeetleResult<(u16, ResponseBody)> {
            panic!("planning test LLM must not use the HTTP transport")
        }
    }

    struct PlanningHoldLlm;

    impl LlmClient for PlanningHoldLlm {
        fn chat(
            &self,
            _http: &mut dyn LlmHttpClient,
            _system: &str,
            _messages: &[Message],
            _tools: Option<&[crate::llm::ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
        ) -> BeetleResult<LlmResponse> {
            Ok(LlmResponse {
                content: "{}".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            })
        }
    }

    fn sample_self_state() -> SelfState {
        SelfState {
            memory_space: crate::memory::SelfMemorySpaceState {
                kernel_chars_used: 900,
                kernel_chars_limit: 1000,
                garden_docs_used: 6,
                garden_docs_limit: 8,
                garden_bytes_used: 800,
                garden_bytes_limit: 1024,
                bottleneck: SelfMemorySpaceBottleneck::Kernel,
                pressure: SelfMemorySpacePressure::Tight,
                governance_posture: crate::memory::SelfMemoryGovernancePosture::Prune,
                recent_activity: crate::memory::SelfMemorySpaceActivity::Growing,
                last_internal_change_at: 10,
            },
            inner_state: crate::memory::SelfInnerState {
                inner_life_chars_used: 80,
                inner_life_chars_limit: 240,
                self_continuity_chars_used: 80,
                self_continuity_chars_limit: 240,
            },
            autonomy: crate::memory::SelfAutonomyState {
                last_user_turn_at: 10,
                last_autonomy_run_at: 20,
                status: crate::memory::SelfAutonomyStatus::Active,
                health_score: 90,
                strategy_chars_used: 120,
                strategy_chars_limit: 512,
                strategy_mode: "consolidate".to_string(),
                strategy_focus: "trim drift".to_string(),
                self_model_tendency: AutonomyGovernanceTendency::Retain,
                private_docs_tendency: AutonomyGovernanceTendency::Compress,
                private_garden_tendency: AutonomyGovernanceTendency::Cleanup,
                idle_enabled: true,
                idle_interval_secs: 900,
            },
        }
    }

    impl SubjectSoulRelationshipRuntimeReadStore for PlanningWriteSpy {
        fn get(
            &self,
            mounted_subject_id: &str,
            relationship_id: &str,
        ) -> Result<Option<SubjectSoulRelationshipRuntimeInputV1>> {
            self.relationship_reads
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push((mounted_subject_id.to_string(), relationship_id.to_string()));
            self.get_typed("relationship_runtime_input", relationship_id)
        }
    }

    fn planning_test_context(stores: &PlanningWriteSpy) -> SelfRuntimeContext<'_> {
        SelfRuntimeContext {
            mounted_subject_id: "agent:test",
            active_relationship_id: None,
            memory_system_kind: MemorySystemKind::LinuxFull,
            session_store: stores,
            memory_store: stores,
            session_summary_store: stores,
            execution_state_store: stores,
            long_term_memory_store: stores,
            continuity_capsule_store: stores,
            self_model_store: stores,
            self_authored_core_store: stores,
            core_revision_ledger_store: stores,
            relationship_constitutional_read_store: stores,
            private_doc_store: stores,
            private_garden_store: stores,
            inner_life_store: stores,
            self_continuity_store: stores,
            felt_significance_store: stores,
            temperament_continuity_store: stores,
            inner_conflict_store: stores,
            relationship_portfolio_store: stores,
            relationship_topology_store: stores,
            world_sense_store: stores,
            autonomy_strategy_store: stores,
            outer_voice_store: stores,
            mental_privacy_store: stores,
            remind_store: stores,
            task_store: stores,
            task_run_store: stores,
            task_artifact_store: stores,
            task_learning_store: stores,
            turn_continuity_evidence_store: stores,
            turn_ledger_store: stores,
            skill_storage: stores,
        }
    }

    fn stable_relationship_persona_evidence() -> crate::memory::RecentPersonaEvidence {
        crate::memory::RecentPersonaEvidence {
            sampled_turns: 4,
            meaningful_turns: 4,
            repeated_priority_order: vec!["truth_before_comfort".to_string()],
            repeated_relationship_posture: "warm but bounded".to_string(),
            updated_at: 99,
            ..crate::memory::RecentPersonaEvidence::default()
        }
    }

    fn active_relationship_runtime_input(
        relationship_id: &str,
    ) -> SubjectSoulRelationshipRuntimeInputV1 {
        let clauses = crate::memory::RelationshipSourceClausesV1 {
            disclosure_ceiling: crate::memory::RelationshipDisclosureCeilingV1::GovernedSummary,
            access_constraints: Vec::new(),
            truth_commitments: vec!["be truthful".to_string()],
            mutual_boundary_commitments: Vec::new(),
            repair_commitments: Vec::new(),
        };
        let mut contribution = crate::memory::RelationshipSourceContributionV1 {
            contributor_subject_id: "human:test".to_string(),
            clauses: clauses.clone(),
            provenance: crate::memory::RelationshipSourceProvenanceV1 {
                source:
                    crate::memory::RelationshipSourceAuthorityKindV1::HumanRelationshipCommitment,
                source_subject_id: "human:test".to_string(),
                source_asserted_at: Some(1),
                recorded_at: 1,
                evidence_digest: "a".repeat(64),
            },
            contribution_digest: String::new(),
        };
        contribution
            .refresh_digest()
            .expect("relationship contribution digest");
        let mut source = crate::memory::RelationshipSourceConstitutionV1 {
            schema_version: 1,
            memory_space_id: "space:test".to_string(),
            relationship_id: relationship_id.to_string(),
            mounted_subject_id: "agent:test".to_string(),
            counterparty_subject_ids: vec!["human:test".to_string()],
            revision: 1,
            supersedes_revision: None,
            state: crate::memory::RelationshipSourceStateV1::Active,
            clauses,
            contributions: vec![contribution],
            content_digest: String::new(),
        };
        source.refresh_digest().expect("relationship source digest");
        SubjectSoulRelationshipRuntimeInputV1 {
            source,
            current_material: None,
            stored_projection: None,
        }
    }

    #[test]
    fn plan_self_runtime_is_zero_write_and_returns_complete_typed_effects() {
        let stores = PlanningWriteSpy::default();
        let initial_document = crate::memory::PrivateGardenDoc {
            path: "journal/founding.md".to_string(),
            content: "Initial private reflection".to_string(),
            updated_at: 90,
            revision: 1,
        };
        let initial_effect = SelfRuntimePlannedEffectV1::UpsertPrivateGardenDoc {
            subject_id: "agent:test".to_string(),
            document: initial_document.clone(),
        };
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::OperatorRequested,
            source_channel: "test".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 100,
        };
        let mut http = PlanningNullHttp;
        let llm = PlanningHoldLlm;

        let plan = plan_self_runtime(
            "self-runtime-op-1",
            SelfRuntimeInitialPlanningStateV1 {
                owner_subject_id: "agent:test".to_string(),
                planned_effects: vec![initial_effect.clone()],
            },
            &mut http,
            &llm,
            planning_test_context(&stores),
            "chat-1",
            &payload,
        )
        .expect("pure self-runtime planning must succeed");

        assert_eq!(stores.writes.load(Ordering::SeqCst), 0);
        assert_eq!(plan.operation_id, "self-runtime-op-1");
        assert_eq!(plan.owner_subject_id, "agent:test");
        assert!(plan.planned_effects.contains(&initial_effect));
        assert!(plan.planned_effects.iter().any(|effect| matches!(
            effect,
            SelfRuntimePlannedEffectV1::SetSelfContinuity { scope_id, value }
                if scope_id == "agent:test" && value.last_autonomy_run_at == 100
        )));
        assert!(stores
            .get_typed::<crate::memory::SelfContinuity>("self_continuity", "agent:test")
            .expect("read persistent spy")
            .is_none());
        assert!(stores
            .garden
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty());
    }

    #[test]
    fn plan_self_runtime_captures_method_distillation_without_writing_non_soul_stores() {
        let stores = PlanningWriteSpy::default();
        let run = sample_task_run_record("run-1", TaskRunStatus::Completed, 80);
        let learning = sample_task_learning_record(
            "learning-1",
            "run-1",
            TaskLearningKind::EvidenceOnly,
            TaskLearningRoute::Pending,
            "bounded release evidence",
            "retain only as reviewed evidence",
            "reviewed evidence body",
            80,
        );
        stores
            .task_runs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(run.run.run_id.clone(), run);
        stores
            .learning
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(learning.learning_id.clone(), learning.clone());
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::OperatorRequested,
            source_channel: "chat_channel".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 100,
        };
        let mut http = PlanningNullHttp;
        let llm = PlanningHoldLlm;

        let plan = plan_self_runtime(
            "self-runtime-method-op",
            SelfRuntimeInitialPlanningStateV1 {
                owner_subject_id: "agent:test".to_string(),
                planned_effects: Vec::new(),
            },
            &mut http,
            &llm,
            planning_test_context(&stores),
            "chat-1",
            &payload,
        )
        .expect("method distillation must remain a pure plan");

        assert_eq!(stores.writes.load(Ordering::SeqCst), 0);
        assert_eq!(
            stores
                .learning
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get("learning-1")
                .map(|record| record.route),
            Some(TaskLearningRoute::Pending)
        );
        assert!(stores
            .notes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty());
        assert!(plan.planned_effects.iter().any(|effect| matches!(
            effect,
            SelfRuntimePlannedEffectV1::UpsertTaskLearning { record }
                if record.learning_id == "learning-1"
                    && record.route == TaskLearningRoute::ArchivedEvidence
        )));
        assert!(plan.planned_effects.iter().any(|effect| matches!(
            effect,
            SelfRuntimePlannedEffectV1::WriteDailyNote { content, .. }
                if content.contains("bounded release evidence")
        )));
    }

    #[test]
    fn planning_journal_reads_initial_private_garden_effects_and_never_calls_base_write() {
        let stores = PlanningWriteSpy::default();
        let document = crate::memory::PrivateGardenDoc {
            path: "sealed/continuity.md".to_string(),
            content: "Private continuity".to_string(),
            updated_at: 10,
            revision: 1,
        };
        let journal = SelfRuntimePlanningJournal::new(vec![
            SelfRuntimePlannedEffectV1::UpsertPrivateGardenDoc {
                subject_id: "agent:test".to_string(),
                document: document.clone(),
            },
        ]);
        let overlay = PlanningPrivateGardenStore {
            base: &stores,
            journal: &journal,
        };

        assert_eq!(
            overlay
                .read("agent:test", "sealed/continuity.md")
                .expect("read initial overlay"),
            Some(document)
        );
        let record = overlay
            .write(
                "agent:test",
                "sealed/continuity.md",
                "Private continuity after reflection",
                20,
            )
            .expect("write planning overlay");
        assert_eq!(record.revision, 2);
        assert_eq!(stores.writes.load(Ordering::SeqCst), 0);
        assert!(matches!(
            journal.snapshot().last(),
            Some(SelfRuntimePlannedEffectV1::UpsertPrivateGardenDoc { document, .. })
                if document.revision == 2 && document.updated_at == 20
        ));
    }

    #[test]
    fn planning_rejects_legacy_relationship_constitution_writes_without_journaling_them() {
        let stores = PlanningWriteSpy::default();
        let journal = SelfRuntimePlanningJournal::new(Vec::new());
        let constitution = crate::memory::RelationshipConstitution::default();
        let overlay = ReadOnlyDerivedRelationshipConstitutionStore {
            scope_id: "relationship:test",
            value: Some(&constitution),
        };

        assert!(overlay
            .set(
                "relationship:test",
                &crate::memory::RelationshipConstitution::default(),
            )
            .is_err());
        assert!(overlay.clear("relationship:test").is_err());
        assert!(journal.snapshot().is_empty());
        assert_eq!(stores.writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn plan_self_runtime_rejects_noncanonical_operation_and_cross_subject_seed() {
        let stores = PlanningWriteSpy::default();
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::OperatorRequested,
            source_channel: "test".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 100,
        };
        let mut http = PlanningNullHttp;
        let llm = PlanningHoldLlm;

        assert!(plan_self_runtime(
            " self-runtime-op-1 ",
            SelfRuntimeInitialPlanningStateV1 {
                owner_subject_id: "agent:test".to_string(),
                planned_effects: Vec::new(),
            },
            &mut http,
            &llm,
            planning_test_context(&stores),
            "chat-1",
            &payload,
        )
        .is_err());
        assert!(plan_self_runtime(
            "self-runtime-op-2",
            SelfRuntimeInitialPlanningStateV1 {
                owner_subject_id: "agent:other".to_string(),
                planned_effects: Vec::new(),
            },
            &mut http,
            &llm,
            planning_test_context(&stores),
            "chat-1",
            &payload,
        )
        .is_err());
        assert_eq!(stores.writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn plan_self_runtime_uses_exact_relationship_owner_without_fallback_lookup() {
        let stores = PlanningWriteSpy::default();
        stores.seed(
            "relationship_runtime_input",
            "relationship:custom",
            &active_relationship_runtime_input("relationship:custom"),
        );
        stores.seed(
            "recent_persona_evidence",
            "relationship:custom",
            &stable_relationship_persona_evidence(),
        );
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::OperatorRequested,
            source_channel: "test".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 100,
        };
        let mut http = PlanningNullHttp;
        let llm = PlanningHoldLlm;
        let mut exact_context = planning_test_context(&stores);
        exact_context.active_relationship_id = Some("relationship:custom");
        let plan = plan_self_runtime(
            "self-runtime-exact-relationship",
            SelfRuntimeInitialPlanningStateV1 {
                owner_subject_id: "agent:test".to_string(),
                planned_effects: Vec::new(),
            },
            &mut http,
            &llm,
            exact_context,
            "chat-1",
            &payload,
        )
        .expect("exact relationship planning");
        let derived_relationship_id =
            crate::memory::relationship_scope_id("agent:test", "test", "chat-1");
        assert!(plan.planned_effects.iter().any(|effect| matches!(
            effect,
            SelfRuntimePlannedEffectV1::SetMentalPrivacy { scope_id, .. }
                if scope_id == "relationship:custom"
        )));
        assert!(plan.planned_effects.iter().any(|effect| matches!(
            effect,
            SelfRuntimePlannedEffectV1::SetRelationshipTopology { value, .. }
                if value.entries.iter().any(|entry| entry.scope_id == "relationship:custom")
        )));
        assert!(plan.planned_effects.iter().any(|effect| matches!(
            effect,
            SelfRuntimePlannedEffectV1::SetRelationshipPortfolio { value, .. }
                if value.entries.iter().any(|entry| {
                    entry.scope_id == "relationship:custom"
                        && entry.permits_board_level_promotion()
                })
        )));
        assert!(plan.planned_effects.iter().all(|effect| match effect {
            SelfRuntimePlannedEffectV1::SetMentalPrivacy { scope_id, .. } => {
                scope_id != &derived_relationship_id
            }
            SelfRuntimePlannedEffectV1::SetRelationshipTopology { value, .. } => value
                .entries
                .iter()
                .all(|entry| entry.scope_id != derived_relationship_id),
            SelfRuntimePlannedEffectV1::SetRelationshipPortfolio { value, .. } => value
                .entries
                .iter()
                .all(|entry| entry.scope_id != derived_relationship_id),
            _ => true,
        }));
        assert_eq!(
            stores
                .relationship_reads
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[("agent:test".to_string(), "relationship:custom".to_string())]
        );

        let before_invalid = stores
            .relationship_reads
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len();
        let mut invalid_context = planning_test_context(&stores);
        invalid_context.active_relationship_id = Some(" relationship:custom ");
        assert!(plan_self_runtime(
            "self-runtime-invalid-relationship",
            SelfRuntimeInitialPlanningStateV1 {
                owner_subject_id: "agent:test".to_string(),
                planned_effects: Vec::new(),
            },
            &mut http,
            &llm,
            invalid_context,
            "chat-1",
            &payload,
        )
        .is_err());
        assert_eq!(
            stores
                .relationship_reads
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len(),
            before_invalid,
            "invalid exact relationship must fail before any owner read"
        );
    }

    #[test]
    fn exact_relationship_without_active_source_cannot_bootstrap_privacy() {
        let stores = PlanningWriteSpy::default();
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::OperatorRequested,
            source_channel: "test".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 100,
        };
        let mut context = planning_test_context(&stores);
        context.active_relationship_id = Some("relationship:missing-source");
        let mut http = PlanningNullHttp;
        let plan = plan_self_runtime(
            "self-runtime-missing-relationship-source",
            SelfRuntimeInitialPlanningStateV1 {
                owner_subject_id: "agent:test".to_string(),
                planned_effects: Vec::new(),
            },
            &mut http,
            &PlanningHoldLlm,
            context,
            "chat-1",
            &payload,
        )
        .expect("missing source remains a closed planning state");

        assert!(plan.planned_effects.iter().all(|effect| !matches!(
            effect,
            SelfRuntimePlannedEffectV1::SetMentalPrivacy { .. }
                | SelfRuntimePlannedEffectV1::SetOuterVoice { .. }
        )));
        assert!(matches!(
            plan.outcome.boundary_persona_result,
            Ok(BoundaryPersonaRefreshOutcome::Skipped)
        ));
        assert_eq!(stores.writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn plan_self_runtime_bootstraps_deny_biased_privacy_for_active_exact_relationship() {
        let stores = PlanningWriteSpy::default();
        let relationship_id = "relationship:custom";
        stores.seed(
            "relationship_runtime_input",
            relationship_id,
            &active_relationship_runtime_input(relationship_id),
        );
        stores.seed(
            "recent_persona_evidence",
            relationship_id,
            &stable_relationship_persona_evidence(),
        );
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::OperatorRequested,
            source_channel: "test".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 100,
        };
        let mut context = planning_test_context(&stores);
        context.active_relationship_id = Some(relationship_id);
        let mut http = PlanningNullHttp;
        let plan = plan_self_runtime(
            "self-runtime-privacy-bootstrap",
            SelfRuntimeInitialPlanningStateV1 {
                owner_subject_id: "agent:test".to_string(),
                planned_effects: Vec::new(),
            },
            &mut http,
            &PlanningHoldLlm,
            context,
            "chat-1",
            &payload,
        )
        .expect("active exact relationship privacy bootstrap planning");

        let baseline = plan
            .planned_effects
            .iter()
            .find_map(|effect| match effect {
                SelfRuntimePlannedEffectV1::SetMentalPrivacy { scope_id, value }
                    if scope_id == relationship_id =>
                {
                    Some(value)
                }
                _ => None,
            })
            .expect("exact relationship privacy baseline effect");
        assert_eq!(
            baseline.boundary_persona.posture,
            crate::memory::BoundaryPersonaPosture::Guarded
        );
        assert_eq!(
            baseline.boundary_persona.disclosure_style,
            crate::memory::BoundaryDisclosureStyle::SummaryFirst
        );
        assert!(baseline
            .relational_state
            .trust_reason
            .contains("no governed relationship trust evidence"));
        assert!(baseline.envelopes.is_empty());
        let default_envelope = crate::memory::MentalPrivacyEnvelope::default();
        assert_eq!(
            default_envelope.owner_access_mode,
            crate::memory::MentalPrivacyOwnerAccessMode::RequestOnly
        );
        assert_eq!(
            default_envelope.quote_policy,
            crate::memory::MentalPrivacyQuotePolicy::NeverQuote
        );
        assert!(plan.planned_effects.iter().any(|effect| matches!(
            effect,
            SelfRuntimePlannedEffectV1::SetRelationshipPortfolio { value, .. }
                if value.entry_for_scope(relationship_id)
                    .is_some_and(|entry| entry.permits_board_level_promotion())
        )));
        assert!(matches!(
            plan.outcome.boundary_persona_result,
            Ok(BoundaryPersonaRefreshOutcome::Bootstrapped)
        ));
        assert!(matches!(
            plan.outcome.self_authored_core_result,
            Ok(SelfAuthoredCoreRefreshPlanV1::Skipped)
        ));
        assert_eq!(stores.writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn privacy_bootstrap_keeps_single_agent_growth_but_never_promotes_one_unstable_turn() {
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::OperatorRequested,
            source_channel: "test".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 100,
        };
        let single_agent_stores = PlanningWriteSpy::default();
        let deterministic_relationship_id = relationship_scope_id("agent:test", "test", "chat-1");
        single_agent_stores.seed(
            "recent_persona_evidence",
            &deterministic_relationship_id,
            &stable_relationship_persona_evidence(),
        );
        let mut http = PlanningNullHttp;
        let single_agent_plan = plan_self_runtime(
            "self-runtime-single-agent-privacy-bootstrap",
            SelfRuntimeInitialPlanningStateV1 {
                owner_subject_id: "agent:test".to_string(),
                planned_effects: Vec::new(),
            },
            &mut http,
            &PlanningHoldLlm,
            planning_test_context(&single_agent_stores),
            "chat-1",
            &payload,
        )
        .expect("single-agent privacy bootstrap planning");
        assert!(single_agent_plan
            .planned_effects
            .iter()
            .any(|effect| matches!(
                effect,
                SelfRuntimePlannedEffectV1::SetMentalPrivacy { scope_id, .. }
                    if scope_id == &deterministic_relationship_id
            )));
        assert!(single_agent_plan
            .planned_effects
            .iter()
            .any(|effect| matches!(
                effect,
                SelfRuntimePlannedEffectV1::SetRelationshipPortfolio { value, .. }
                    if value
                        .entry_for_scope(&deterministic_relationship_id)
                        .is_some_and(|entry| entry.permits_board_level_promotion())
            )));
        assert!(single_agent_plan
            .planned_effects
            .iter()
            .all(|effect| match effect {
                SelfRuntimePlannedEffectV1::SetRelationshipTopology { value, .. } => value
                    .entries
                    .iter()
                    .all(|entry| entry.scope_id == deterministic_relationship_id),
                SelfRuntimePlannedEffectV1::SetRelationshipPortfolio { value, .. } => value
                    .entries
                    .iter()
                    .all(|entry| entry.scope_id == deterministic_relationship_id),
                _ => true,
            }));
        assert_eq!(
            single_agent_stores
                .relationship_reads
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[(
                "agent:test".to_string(),
                deterministic_relationship_id.clone()
            )],
            "single-agent planning must use one deterministic relationship owner"
        );
        assert!(!matches!(
            single_agent_plan.outcome.self_authored_core_result,
            Ok(SelfAuthoredCoreRefreshPlanV1::Adopt { .. })
        ));

        let unstable_stores = PlanningWriteSpy::default();
        let exact_relationship_id = "relationship:unstable";
        unstable_stores.seed(
            "relationship_runtime_input",
            exact_relationship_id,
            &active_relationship_runtime_input(exact_relationship_id),
        );
        let mut unstable_evidence = stable_relationship_persona_evidence();
        unstable_evidence.sampled_turns = 1;
        unstable_evidence.meaningful_turns = 1;
        unstable_stores.seed(
            "recent_persona_evidence",
            exact_relationship_id,
            &unstable_evidence,
        );
        let mut exact_context = planning_test_context(&unstable_stores);
        exact_context.active_relationship_id = Some(exact_relationship_id);
        let unstable_plan = plan_self_runtime(
            "self-runtime-unstable-privacy-bootstrap",
            SelfRuntimeInitialPlanningStateV1 {
                owner_subject_id: "agent:test".to_string(),
                planned_effects: Vec::new(),
            },
            &mut http,
            &PlanningHoldLlm,
            exact_context,
            "chat-1",
            &payload,
        )
        .expect("unstable exact relationship planning");
        assert!(unstable_plan.planned_effects.iter().any(|effect| matches!(
            effect,
            SelfRuntimePlannedEffectV1::SetMentalPrivacy { scope_id, .. }
                if scope_id == exact_relationship_id
        )));
        assert!(!matches!(
            unstable_plan.outcome.self_authored_core_result,
            Ok(SelfAuthoredCoreRefreshPlanV1::Adopt { .. })
        ));
    }

    fn sample_distillation_snapshot() -> PersonaDistillationSnapshot {
        PersonaDistillationSnapshot {
            private_material_at: 20,
            boundary_state_at: 18,
            world_context_at: 17,
            world_sense_at: 16,
            autonomy_strategy_at: 17,
            recent_persona_evidence_at: 19,
            self_model_at: 10,
            self_authored_core_at: 9,
            self_continuity_at: 10,
            outer_voice_at: 9,
            has_inner_life: true,
            has_world_sense: true,
            has_autonomy_strategy: true,
            has_recent_persona_evidence: true,
        }
    }

    #[test]
    fn parse_self_runtime_decision_coerces_nested_fields() {
        let raw = json!({
            "refresh_inner_life": "true",
            "inner_life_intent": { "goal": "capture drift" },
            "refresh_private_docs": 1,
            "private_docs_intent": ["rewrite private notes"],
            "private_docs_action": "compress",
            "refresh_self_model": true,
            "self_model_intent": { "goal": "distill self core" },
            "self_model_sources": ["private_docs", "inner-life"],
            "refresh_self_authored_core": true,
            "self_authored_core_intent": { "goal": "refresh board core" },
            "self_authored_core_sources": ["self_model", "boundary persona"],
            "refresh_self_continuity": false,
            "self_continuity_intent": 0,
            "self_continuity_sources": ["self_model", "recent transcript"],
            "refresh_private_garden": { "enabled": true },
            "private_garden_intent": { "path": "journal/today.md" },
            "private_garden_action": "cleanup",
            "refresh_boundary_persona": "true",
            "boundary_persona_intent": ["stabilize boundary stance"],
            "refresh_outer_voice": 1,
            "outer_voice_intent": { "why": "express new stance" },
            "outer_voice_sources": ["boundary persona", "world_sense"],
            "boundary_flush": true,
            "boundary_flush_reason": ["daily_boundary"],
            "request_factual_refresh": 1,
            "factual_reconcile_action": "conflict",
            "factual_reconcile_intent": { "why": "recent transcript diverges" }
        })
        .to_string();
        let parsed = parse_self_runtime_decision(&raw);
        assert!(parsed.refresh_inner_life);
        assert!(parsed.refresh_private_docs);
        assert!(parsed.refresh_self_model);
        assert!(parsed.refresh_self_authored_core);
        assert!(parsed.refresh_private_garden);
        assert!(parsed.refresh_boundary_persona);
        assert!(parsed.refresh_outer_voice);
        assert_eq!(
            parsed.private_docs_action,
            SelfRuntimeGovernanceAction::Compress
        );
        assert_eq!(
            parsed.private_garden_action,
            SelfRuntimeGovernanceAction::Cleanup
        );
        assert!(parsed.boundary_flush);
        assert!(parsed.request_factual_refresh);
        assert_eq!(
            parsed.factual_reconcile_action,
            SharedFactualReconcileAction::Conflict
        );
        assert!(parsed.inner_life_intent.contains("goal: capture drift"));
        assert_eq!(parsed.private_docs_intent, "rewrite private notes");
        assert!(parsed.self_model_intent.contains("goal: distill self core"));
        assert_eq!(
            parsed.self_model_sources,
            vec!["private_docs".to_string(), "inner_life".to_string()]
        );
        assert!(parsed
            .self_authored_core_intent
            .contains("goal: refresh board core"));
        assert_eq!(
            parsed.self_authored_core_sources,
            vec!["self_model".to_string(), "boundary_persona".to_string()]
        );
        assert_eq!(parsed.self_continuity_intent, "0");
        assert_eq!(
            parsed.self_continuity_sources,
            vec!["self_model".to_string(), "recent_transcript".to_string()]
        );
        assert!(parsed
            .private_garden_intent
            .contains("path: journal/today.md"));
        assert!(parsed
            .boundary_persona_intent
            .contains("stabilize boundary stance"));
        assert!(parsed
            .outer_voice_intent
            .contains("why: express new stance"));
        assert_eq!(
            parsed.outer_voice_sources,
            vec!["boundary_persona".to_string(), "world_sense".to_string()]
        );
        assert!(parsed.boundary_flush_reason.contains("daily_boundary"));
        assert!(parsed
            .factual_reconcile_intent
            .contains("why: recent transcript diverges"));
    }

    #[test]
    fn esp_authority_plan_strips_non_growth_direct_actions_and_sources() {
        let plan = decide_self_runtime_authority(MemorySystemKind::EspCompact);
        let mut decision = SelfRuntimeDecision {
            refresh_inner_life: true,
            inner_life_intent: "keep the inner thread warm".to_string(),
            refresh_private_docs: true,
            private_docs_intent: "rewrite workspace".to_string(),
            private_docs_action: SelfRuntimeGovernanceAction::Rewrite,
            refresh_private_garden: true,
            private_garden_intent: "compress garden".to_string(),
            private_garden_action: SelfRuntimeGovernanceAction::Compress,
            refresh_self_model: true,
            self_model_intent: "distill kernel".to_string(),
            self_model_sources: vec![
                "private_docs".to_string(),
                "private_garden".to_string(),
                "inner_life".to_string(),
            ],
            refresh_self_authored_core: true,
            self_authored_core_intent: "refresh board core".to_string(),
            self_authored_core_sources: vec![
                "private_docs".to_string(),
                "self_model".to_string(),
                "private_garden".to_string(),
            ],
            refresh_self_continuity: true,
            self_continuity_intent: "bridge continuity".to_string(),
            self_continuity_sources: vec![
                "private_docs".to_string(),
                "boundary_persona".to_string(),
                "outer_voice".to_string(),
                "world_sense".to_string(),
            ],
            refresh_boundary_persona: true,
            boundary_persona_intent: "retune relation boundary".to_string(),
            refresh_outer_voice: true,
            outer_voice_intent: "stabilize outer voice".to_string(),
            outer_voice_sources: vec!["private_garden".to_string(), "boundary_persona".to_string()],
            request_factual_refresh: true,
            factual_reconcile_action: SharedFactualReconcileAction::Correct,
            factual_reconcile_intent: "repair stale fact grounding".to_string(),
            ..Default::default()
        };

        apply_self_runtime_authority_plan(&mut decision, plan);

        assert!(decision.refresh_inner_life);
        assert!(!decision.refresh_private_docs);
        assert!(decision.private_docs_intent.is_empty());
        assert_eq!(
            decision.private_docs_action,
            SelfRuntimeGovernanceAction::Hold
        );
        assert!(!decision.refresh_private_garden);
        assert!(decision.private_garden_intent.is_empty());
        assert_eq!(
            decision.private_garden_action,
            SelfRuntimeGovernanceAction::Hold
        );
        assert_eq!(decision.self_model_sources, vec!["inner_life".to_string()]);
        assert!(!decision.refresh_self_authored_core);
        assert!(decision.self_authored_core_intent.is_empty());
        assert!(decision.self_authored_core_sources.is_empty());
        assert_eq!(
            decision.self_continuity_sources,
            vec!["world_sense".to_string()]
        );
        assert!(!decision.refresh_boundary_persona);
        assert!(decision.boundary_persona_intent.is_empty());
        assert!(!decision.refresh_outer_voice);
        assert!(decision.outer_voice_intent.is_empty());
        assert!(decision.outer_voice_sources.is_empty());
        assert!(!decision.request_factual_refresh);
        assert_eq!(
            decision.factual_reconcile_action,
            SharedFactualReconcileAction::Hold
        );
        assert!(decision.factual_reconcile_intent.is_empty());
    }

    #[derive(Default)]
    struct StubTaskLearningStore {
        records: Mutex<HashMap<String, TaskLearningRecord>>,
    }

    impl StubTaskLearningStore {
        fn with_records(records: Vec<TaskLearningRecord>) -> Self {
            let map = records
                .into_iter()
                .map(|record| (record.learning_id.clone(), record))
                .collect();
            Self {
                records: Mutex::new(map),
            }
        }
    }

    impl TaskLearningStore for StubTaskLearningStore {
        fn get(&self, learning_id: &str) -> BeetleResult<Option<TaskLearningRecord>> {
            Ok(self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(learning_id)
                .cloned())
        }

        fn upsert(&self, record: &TaskLearningRecord) -> BeetleResult<()> {
            self.records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(record.learning_id.clone(), record.clone());
            Ok(())
        }

        fn list_recent(&self, limit: usize) -> BeetleResult<Vec<TaskLearningRecord>> {
            let mut records = self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| std::cmp::Reverse(record.observed_at));
            records.truncate(limit);
            Ok(records)
        }

        fn list_for_chat(
            &self,
            channel: &str,
            chat_id: &str,
            limit: usize,
        ) -> BeetleResult<Vec<TaskLearningRecord>> {
            let mut records = self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|record| {
                    record.source_channel == channel && record.source_chat_id == chat_id
                })
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| std::cmp::Reverse(record.observed_at));
            records.truncate(limit);
            Ok(records)
        }

        fn list_for_run(
            &self,
            run_id: &str,
            limit: usize,
        ) -> BeetleResult<Vec<TaskLearningRecord>> {
            let mut records = self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|record| record.run_id == run_id)
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| std::cmp::Reverse(record.observed_at));
            records.truncate(limit);
            Ok(records)
        }
    }

    struct StubTaskRunStore {
        records: HashMap<String, TaskRunRecord>,
    }

    impl StubTaskRunStore {
        fn new(records: Vec<TaskRunRecord>) -> Self {
            Self {
                records: records
                    .into_iter()
                    .map(|record| (record.run.run_id.clone(), record))
                    .collect(),
            }
        }
    }

    impl TaskRunStore for StubTaskRunStore {
        fn get(&self, run_id: &str) -> BeetleResult<Option<TaskRunRecord>> {
            Ok(self.records.get(run_id).cloned())
        }

        fn upsert(&self, _record: &TaskRunRecord) -> BeetleResult<()> {
            Ok(())
        }

        fn list_recent(&self, limit: usize) -> BeetleResult<Vec<TaskRunRecord>> {
            let mut records = self.records.values().cloned().collect::<Vec<_>>();
            records.sort_by_key(|record| std::cmp::Reverse(record.run.updated_at));
            records.truncate(limit);
            Ok(records)
        }

        fn list_active_for_chat(
            &self,
            channel: &str,
            chat_id: &str,
            limit: usize,
        ) -> BeetleResult<Vec<TaskRunRecord>> {
            let mut records = self
                .records
                .values()
                .filter(|record| {
                    record.run.source_channel == channel
                        && record.run.source_chat_id == chat_id
                        && record.run.status.is_active()
                })
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| std::cmp::Reverse(record.run.updated_at));
            records.truncate(limit);
            Ok(records)
        }
    }

    #[derive(Default)]
    struct StubTaskArtifactStore;

    impl TaskArtifactStore for StubTaskArtifactStore {
        fn put(&self, _record: &TaskArtifactRecord) -> BeetleResult<()> {
            Ok(())
        }

        fn list_for_run(
            &self,
            _run_id: &str,
            _limit: usize,
        ) -> BeetleResult<Vec<TaskArtifactRecord>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        drafts: Mutex<Vec<LongTermMemoryDraft>>,
    }

    #[derive(Default)]
    struct StubContinuityCapsuleStore {
        entries: Mutex<Vec<ContinuityCapsule>>,
    }

    impl ContinuityCapsuleStore for StubContinuityCapsuleStore {
        fn upsert_many(
            &self,
            drafts: &[ContinuityCapsuleDraft],
            now_secs: u64,
        ) -> BeetleResult<crate::memory::ContinuityCapsuleWriteOutcome> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            Ok(crate::memory::apply_continuity_capsule_drafts(
                &mut entries,
                drafts,
                now_secs,
            ))
        }

        fn get(&self, capsule_id: &str) -> BeetleResult<Option<ContinuityCapsule>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|entry| entry.capsule_id == capsule_id)
                .cloned())
        }

        fn list(&self, limit: usize) -> BeetleResult<Vec<ContinuityCapsule>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .take(limit)
                .cloned()
                .collect())
        }

        fn count(&self) -> BeetleResult<usize> {
            Ok(self.entries.lock().unwrap_or_else(|e| e.into_inner()).len())
        }
    }

    impl crate::memory::LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(
            &self,
            drafts: &[LongTermMemoryDraft],
            _now_secs: u64,
        ) -> BeetleResult<usize> {
            self.drafts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(drafts);
            Ok(drafts.len())
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> BeetleResult<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, _id: &str) -> BeetleResult<Option<LongTermMemoryEntry>> {
            Ok(None)
        }

        fn list(&self, _limit: usize) -> BeetleResult<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn delete(&self, _id: &str) -> BeetleResult<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> BeetleResult<bool> {
            Ok(false)
        }

        fn count(&self) -> BeetleResult<usize> {
            Ok(self.drafts.lock().unwrap_or_else(|e| e.into_inner()).len())
        }
    }

    #[derive(Default)]
    struct StubSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for StubSkillStorage {
        fn list_names(&self) -> BeetleResult<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> BeetleResult<Vec<u8>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }

        fn write(&self, name: &str, content: &[u8]) -> BeetleResult<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> BeetleResult<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(name);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubMemoryStore;

    impl MemoryStore for StubMemoryStore {
        fn get_memory(&self) -> BeetleResult<String> {
            Ok(String::new())
        }

        fn set_memory(&self, _content: &str) -> BeetleResult<()> {
            Ok(())
        }

        fn list_daily_note_names(&self, _recent_n: usize) -> BeetleResult<Vec<String>> {
            Ok(Vec::new())
        }

        fn get_daily_note(&self, _name: &str) -> BeetleResult<String> {
            Ok(String::new())
        }

        fn write_daily_note(&self, _name: &str, _content: &str) -> BeetleResult<()> {
            Ok(())
        }
    }

    fn sample_task_run_record(run_id: &str, status: TaskRunStatus, now_secs: u64) -> TaskRunRecord {
        TaskRunRecord {
            run: TaskRun {
                run_id: run_id.to_string(),
                kind: TaskRunKind::TaskExecution,
                source_channel: "chat_channel".to_string(),
                source_chat_id: "chat-1".to_string(),
                user_request: "Summarize the fix path".to_string(),
                title: "release fix".to_string(),
                status,
                current_step_id: "s1".to_string(),
                planner_reason: String::new(),
                final_summary: String::new(),
                failure_reason: String::new(),
                plan_revision: 1,
                created_at: now_secs,
                updated_at: now_secs,
                finished_at: now_secs,
            },
            plan: TaskPlan {
                goal: "finish release recovery".to_string(),
                completion_definition: "root cause and stable procedure recorded".to_string(),
                risk_notes: Vec::new(),
                ordered_steps: Vec::new(),
            },
        }
    }

    fn sample_task_learning_record(
        learning_id: &str,
        run_id: &str,
        kind: TaskLearningKind,
        route: TaskLearningRoute,
        topic: &str,
        summary: &str,
        content: &str,
        observed_at: u64,
    ) -> TaskLearningRecord {
        TaskLearningRecord {
            learning_id: learning_id.to_string(),
            source_channel: "chat_channel".to_string(),
            source_chat_id: "chat-1".to_string(),
            run_id: run_id.to_string(),
            step_id: "s1".to_string(),
            kind,
            route,
            run_status: TaskRunStatus::Completed,
            topic: topic.to_string(),
            summary: summary.to_string(),
            content: content.to_string(),
            memory_kind: Some(crate::memory::LongTermMemoryKind::Task),
            review_summary: "review accepted".to_string(),
            source_artifact_ids: vec!["a1".to_string(), "a2".to_string()],
            provenance: "self_runtime".to_string(),
            archive_note_name: String::new(),
            route_detail: String::new(),
            candidate_state: match kind {
                TaskLearningKind::ReusableProcedure => match route {
                    TaskLearningRoute::RuntimeSkill => {
                        Some(crate::task_execution::TaskLearningCandidateState::Promoted)
                    }
                    TaskLearningRoute::Rejected => {
                        Some(crate::task_execution::TaskLearningCandidateState::Rejected)
                    }
                    _ => Some(crate::task_execution::TaskLearningCandidateState::Observed),
                },
                _ => None,
            },
            candidate_state_updated_at: observed_at,
            last_failure_reason: String::new(),
            observed_at,
        }
    }

    fn sample_loaded_self_runtime_state() -> LoadedSelfRuntimeState {
        LoadedSelfRuntimeState {
            load_health: SelfRuntimeLoadHealth::default(),
            summary_text: None,
            execution_state: None,
            self_model: None,
            self_authored_core: None,
            core_revision_ledger: None,
            core_revision_governance: CoreRevisionGovernanceDigest::default(),
            private_docs: None,
            private_garden_docs: Vec::new(),
            inner_life: None,
            self_continuity: None,
            subject_shell: None,
            felt_significance: None,
            temperament_continuity: None,
            inner_conflict: None,
            relationship_portfolio: None,
            relationship_topology: None,
            relationship_constitutional_input: None,
            relationship_constitution: None,
            world_sense: None,
            autonomy_strategy: None,
            outer_voice: None,
            mental_privacy_state: None,
            recent_persona_evidence: None,
            sandbox_probe_text: None,
            active_relationship_scope_id: "rel:chat_channel:chat-1".to_string(),
            active_relationship_channel: "chat_channel".to_string(),
            prior_user_channel: "chat_channel".to_string(),
            world_snapshot: crate::memory::WorldSnapshot {
                weekday: "Thu".to_string(),
                hour: 10,
                day_phase: "morning".to_string(),
                interaction_mode: "chat".to_string(),
                activity_rhythm: "active".to_string(),
                situational_pull: "coding".to_string(),
                resource_tension: "normal".to_string(),
                pressure: PressureLevel::Normal,
                memory_available_bytes: 0,
                active_http_count: 0,
                active_wss_count: 0,
                active_agent_tasks: 0,
                inbound_depth: 0,
                outbound_depth: 0,
                storage_used_kb: 0,
                storage_total_kb: 0,
                wifi_connected: true,
                audio_recording: false,
                audio_playing: false,
                source_channel: "chat_channel".to_string(),
                open_tasks: 0,
                in_progress_tasks: 0,
                due_tasks: 0,
                high_priority_tasks: 0,
                upcoming_reminders: 0,
                next_reminder_at: 0,
                user_idle_secs: 0,
                autonomy_idle_secs: 0,
            },
            recent: Vec::new(),
        }
    }

    #[test]
    fn self_runtime_load_guard_blocks_mutating_path_when_persistent_reads_fail() {
        let mut state = sample_loaded_self_runtime_state();
        state.load_health.record(
            "self_model",
            &crate::error::Error::config("test_self_runtime_load", "corrupt self model"),
        );

        let outcome = self_runtime_load_guard_outcome("chat-1", &state)
            .expect("load guard should block self runtime");

        assert!(outcome.decision.is_none());
        assert!(outcome.world_sense_result.is_err());
        assert!(outcome.self_model_result.is_err());
        assert!(outcome.self_continuity_result.is_err());
        assert!(outcome.task_learning_result.is_err());
    }

    #[test]
    fn post_reply_loaded_runtime_skip_reuses_scheduler_gate() {
        let mut state = sample_loaded_self_runtime_state();
        state.self_continuity = Some(crate::memory::SelfContinuity {
            last_user_channel: "chat_channel".to_string(),
            last_autonomy_run_at: 980,
            ..crate::memory::SelfContinuity::default()
        });
        state.autonomy_strategy = Some(crate::memory::AutonomyStrategy {
            idle_enabled: true,
            idle_interval_secs: 300,
            ..crate::memory::AutonomyStrategy::default()
        });
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::PostReply,
            source_channel: "chat_channel".to_string(),
            user_content: "hi".to_string(),
            reply_content: "hello".to_string(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 1_000,
        };

        assert_eq!(
            self_runtime_post_reply_loaded_skip_reason(&state, &payload, MemoryProfile::Embedded),
            Some("post_reply_runtime_recently_ran")
        );
        assert_eq!(
            self_runtime_post_reply_loaded_skip_reason(&state, &payload, MemoryProfile::Standard),
            None
        );
    }

    #[test]
    fn autonomy_touch_failure_surfaces_as_self_continuity_error() {
        let merged = merge_self_continuity_touch_result(
            Ok(crate::memory::SelfContinuityRefreshOutcome::Skipped),
            Err(crate::error::Error::config(
                "test_touch_runtime",
                "touch failed",
            )),
        );

        let error = merged.expect_err("touch failure must not be swallowed");
        assert_eq!(error.stage(), "test_touch_runtime");
    }

    #[test]
    fn self_runtime_method_distillation_uses_governed_task_learning_pipeline() {
        let now_secs = crate::util::ymdhms_to_epoch(2026, 4, 8, 13, 0, 0);
        let authority = decide_self_runtime_authority(MemorySystemKind::EspCompact);
        let task_run_store = StubTaskRunStore::new(vec![
            sample_task_run_record(
                "tr_prev",
                TaskRunStatus::Completed,
                now_secs.saturating_sub(60),
            ),
            sample_task_run_record("tr_now", TaskRunStatus::Completed, now_secs),
        ]);
        let learning_store = StubTaskLearningStore::with_records(vec![
            sample_task_learning_record(
                "tl_prev",
                "tr_prev",
                TaskLearningKind::ReusableProcedure,
                TaskLearningRoute::ArchivedEvidence,
                "stable_release_patch",
                "Stable release patch sequence",
                "1. inspect logs\n2. patch guard\n3. verify artifact",
                now_secs.saturating_sub(60),
            ),
            sample_task_learning_record(
                "tl_now",
                "tr_now",
                TaskLearningKind::ReusableProcedure,
                TaskLearningRoute::Pending,
                "stable_release_patch",
                "Stable release patch sequence",
                "1. inspect logs\n2. patch guard\n3. verify artifact",
                now_secs,
            ),
        ]);
        let artifact_store = StubTaskArtifactStore;
        let long_term_memory_store = StubLongTermMemoryStore::default();
        let skill_storage = StubSkillStorage::default();
        let memory_store = StubMemoryStore;

        let outcome = run_self_runtime_method_distillation(
            &task_run_store,
            &artifact_store,
            &learning_store,
            &long_term_memory_store,
            &skill_storage,
            &memory_store,
            authority,
            "agent:test",
            "chat_channel",
            "chat-1",
            now_secs,
        )
        .expect("method distillation should route through governed maintenance");

        assert_eq!(outcome.considered, 1);
        assert_eq!(outcome.runtime_skill_promotions, 1);
        let promoted = learning_store
            .get("tl_now")
            .expect("read promoted record")
            .expect("promoted record exists");
        assert_eq!(promoted.route, TaskLearningRoute::RuntimeSkill);
    }

    #[test]
    fn self_runtime_boundary_flush_writes_continuity_capsule_when_signal_is_meaningful() {
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        let task_run_store = StubTaskRunStore::new(Vec::new());
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::PostReply,
            source_channel: "chat_channel".to_string(),
            user_content: "继续把 continuity capsule 收口".to_string(),
            reply_content: "这轮先落 self_runtime continuity".to_string(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 1_000,
        };
        let mut state = sample_loaded_self_runtime_state();
        state.summary_text =
            Some("Self runtime is about to checkpoint the handoff state".to_string());
        state.execution_state = Some(crate::memory::ExecutionState {
            status: crate::memory::ExecutionStatus::Active,
            goal: "Close continuity capsule productionization".to_string(),
            progress: "Task 2 is wiring self-runtime continuity persistence".to_string(),
            blocker: String::new(),
            next_action: "Persist the finalized boundary decision as a capsule".to_string(),
            last_output: String::new(),
            active_constraints: Vec::new(),
            open_questions: Vec::new(),
            latest_observations: Vec::new(),
            next_best_actions: Vec::new(),
            updated_at: 990,
        });
        let decision = SelfRuntimeDecision {
            boundary_flush: true,
            boundary_flush_reason: "channel_handoff".to_string(),
            ..Default::default()
        };

        let outcome = persist_self_runtime_continuity_capsules(
            &continuity_capsule_store,
            &task_run_store,
            "chat-1",
            &payload,
            &state,
            &decision,
        )
        .expect("boundary flush continuity write should succeed");

        assert_eq!(outcome.upserted, 1);
        let stored = continuity_capsule_store
            .list(8)
            .expect("list continuity capsules");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].source, ContinuityCapsuleSource::HandoffFlush);
        assert_eq!(
            stored[0].topic,
            "Close continuity capsule productionization"
        );
        assert_eq!(
            stored[0].next_step,
            "Persist the finalized boundary decision as a capsule"
        );
        assert!(stored[0]
            .provenance_refs
            .iter()
            .any(|value| value == "execution_state"));
    }

    #[test]
    fn self_runtime_reboot_continuity_writes_reboot_capsule_without_new_thread_surface() {
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        let task_run_store = StubTaskRunStore::new(Vec::new());
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::IdleTick,
            source_channel: "self_runtime_idle".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 2_000,
        };
        let mut state = sample_loaded_self_runtime_state();
        state.execution_state = Some(crate::memory::ExecutionState {
            status: crate::memory::ExecutionStatus::Active,
            goal: "Restore continuity after reboot".to_string(),
            progress: "Runtime state was recovered from persisted layers".to_string(),
            blocker: String::new(),
            next_action: "Resume the continuity productionization task".to_string(),
            last_output: String::new(),
            active_constraints: Vec::new(),
            open_questions: Vec::new(),
            latest_observations: Vec::new(),
            next_best_actions: Vec::new(),
            updated_at: 1_950,
        });
        state.self_continuity = Some(crate::memory::SelfContinuity {
            wake_anchor: "Still the same build-recovery self".to_string(),
            current_self_state: "Recovered enough state to resume without replaying transcript"
                .to_string(),
            recent_changes: String::new(),
            continuity_bridge:
                "Resume from the persisted continuity contract instead of rescanning history"
                    .to_string(),
            priority_posture: String::new(),
            relationship_posture: String::new(),
            task_posture: "Resume continuity capsule productionization".to_string(),
            last_user_turn_at: 1_900,
            last_user_chat_id: "chat-1".to_string(),
            last_user_channel: "chat_channel".to_string(),
            last_autonomy_run_at: 0,
            updated_at: 1_900,
        });

        let outcome = persist_self_runtime_continuity_capsules(
            &continuity_capsule_store,
            &task_run_store,
            "chat-1",
            &payload,
            &state,
            &SelfRuntimeDecision::default(),
        )
        .expect("reboot continuity write should succeed");

        assert_eq!(outcome.upserted, 1);
        let stored = continuity_capsule_store
            .list(8)
            .expect("list continuity capsules");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].source, ContinuityCapsuleSource::RebootContinuity);
        assert_eq!(stored[0].topic, "Restore continuity after reboot");
        assert!(stored[0].summary.contains("persisted continuity contract"));
        assert_eq!(
            stored[0].next_step,
            "Resume the continuity productionization task"
        );
    }

    #[test]
    fn idle_tick_tendency_can_force_private_docs_refresh_and_fill_intent() {
        let strategy = crate::memory::AutonomyStrategy {
            current_mode: "consolidate".to_string(),
            active_priorities: String::new(),
            write_policy: String::new(),
            next_focus: String::new(),
            cadence_reason: String::new(),
            self_model_tendency: AutonomyGovernanceTendency::Retain,
            private_docs_tendency: AutonomyGovernanceTendency::Compress,
            private_garden_tendency: AutonomyGovernanceTendency::Retain,
            idle_enabled: true,
            idle_interval_secs: 900,
            updated_at: 1,
        };
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision::default(),
            SelfRuntimeTrigger::IdleTick,
            Some(&strategy),
            &sample_self_state(),
            &PersonaDistillationSnapshot::default(),
            &CoreRevisionGovernanceDigest::default(),
            true,
            false,
            true,
            false,
            true,
            false,
            false,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(decision.refresh_private_docs);
        assert!(decision
            .private_docs_intent
            .contains("Compress governed docs"));
        assert_eq!(
            decision.private_docs_action,
            SelfRuntimeGovernanceAction::Compress
        );
    }

    #[test]
    fn post_reply_tendency_does_not_force_refresh_but_can_fill_missing_intent() {
        let strategy = crate::memory::AutonomyStrategy {
            current_mode: "organize".to_string(),
            active_priorities: String::new(),
            write_policy: String::new(),
            next_focus: String::new(),
            cadence_reason: String::new(),
            self_model_tendency: AutonomyGovernanceTendency::Retain,
            private_docs_tendency: AutonomyGovernanceTendency::Retain,
            private_garden_tendency: AutonomyGovernanceTendency::Rewrite,
            idle_enabled: true,
            idle_interval_secs: 900,
            updated_at: 1,
        };
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision {
                refresh_private_garden: true,
                ..Default::default()
            },
            SelfRuntimeTrigger::PostReply,
            Some(&strategy),
            &sample_self_state(),
            &PersonaDistillationSnapshot::default(),
            &CoreRevisionGovernanceDigest::default(),
            true,
            false,
            false,
            true,
            true,
            false,
            false,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(decision.refresh_private_garden);
        assert!(decision
            .private_garden_intent
            .contains("Rewrite and reorganize"));
        assert!(!decision.refresh_private_docs);
        assert_eq!(
            decision.private_garden_action,
            SelfRuntimeGovernanceAction::Rewrite
        );
    }

    #[test]
    fn boundary_signal_forces_continuity_and_private_refresh() {
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision::default(),
            SelfRuntimeTrigger::IdleTick,
            None,
            &sample_self_state(),
            &PersonaDistillationSnapshot::default(),
            &CoreRevisionGovernanceDigest::default(),
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal {
                reasons: vec![SelfRuntimeBoundaryReason::DailyBoundary],
            },
        );

        assert!(decision.boundary_flush);
        assert!(decision.refresh_self_continuity);
        assert!(decision.refresh_private_docs);
        assert!(decision.refresh_private_garden);
        assert!(decision.refresh_self_model);
        assert!(decision.refresh_self_authored_core);
        assert!(decision.refresh_boundary_persona);
        assert!(decision.refresh_outer_voice);
    }

    #[test]
    fn distillation_lag_refreshes_upper_persona_layers() {
        let snapshot = sample_distillation_snapshot();
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision::default(),
            SelfRuntimeTrigger::IdleTick,
            None,
            &sample_self_state(),
            &snapshot,
            &CoreRevisionGovernanceDigest::default(),
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(decision.refresh_self_model);
        assert!(decision.refresh_self_authored_core);
        assert!(decision.refresh_self_continuity);
        assert!(decision.refresh_outer_voice);
        assert!(decision
            .self_model_intent
            .contains("redistill a steadier kernel"));
        assert!(decision
            .self_continuity_intent
            .contains("continuity bridge"));
        assert!(decision.outer_voice_intent.contains("outward expression"));
        assert!(decision
            .self_authored_core_intent
            .contains("board-level self core"));
        assert!(decision
            .self_authored_core_sources
            .contains(&"boundary_persona".to_string()));
        assert!(decision
            .self_model_sources
            .contains(&"recent_persona_evidence".to_string()));
        assert!(decision
            .self_continuity_sources
            .contains(&"world_sense".to_string()));
        assert!(decision
            .self_continuity_sources
            .contains(&"recent_persona_evidence".to_string()));
        assert!(decision
            .outer_voice_sources
            .contains(&"autonomy_strategy".to_string()));
        assert!(decision
            .outer_voice_sources
            .contains(&"recent_persona_evidence".to_string()));
    }

    #[test]
    fn constitutional_review_due_forces_self_authored_core_refresh() {
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision::default(),
            SelfRuntimeTrigger::IdleTick,
            None,
            &sample_self_state(),
            &PersonaDistillationSnapshot {
                self_authored_core_at: 200,
                self_model_at: 200,
                self_continuity_at: 200,
                boundary_state_at: 200,
                ..PersonaDistillationSnapshot::default()
            },
            &CoreRevisionGovernanceDigest {
                review_due: true,
                review_reasons: vec!["constitutional_review_cadence_due".to_string()],
                ..CoreRevisionGovernanceDigest::default()
            },
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(decision.refresh_self_authored_core);
        assert!(decision
            .self_authored_core_intent
            .contains("constitutional review"));
        assert!(decision
            .self_authored_core_sources
            .contains(&"self_model".to_string()));
    }

    #[test]
    fn conservative_constitution_blocks_volatile_only_core_refresh() {
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision::default(),
            SelfRuntimeTrigger::IdleTick,
            None,
            &sample_self_state(),
            &PersonaDistillationSnapshot {
                self_authored_core_at: 100,
                self_model_at: 100,
                self_continuity_at: 100,
                boundary_state_at: 100,
                outer_voice_at: 140,
                recent_persona_evidence_at: 150,
                has_recent_persona_evidence: true,
                ..PersonaDistillationSnapshot::default()
            },
            &CoreRevisionGovernanceDigest {
                conservative_mode: true,
                latest_stability_score: 48,
                ..CoreRevisionGovernanceDigest::default()
            },
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(!decision.refresh_self_authored_core);
    }

    #[test]
    fn runtime_governance_gate_blocks_unsettled_upward_distillation() {
        let mut decision = SelfRuntimeDecision {
            refresh_self_model: true,
            self_model_intent: "distill self kernel".to_string(),
            self_model_sources: vec!["private_docs".to_string(), "inner_life".to_string()],
            refresh_self_authored_core: true,
            self_authored_core_intent: "refresh board core".to_string(),
            self_authored_core_sources: vec!["self_model".to_string()],
            refresh_self_continuity: true,
            self_continuity_intent: "keep continuity bridge".to_string(),
            self_continuity_sources: vec!["self_model".to_string()],
            refresh_boundary_persona: true,
            boundary_persona_intent: "stabilize relation boundary".to_string(),
            refresh_outer_voice: true,
            outer_voice_intent: "rewrite outward expression".to_string(),
            outer_voice_sources: vec!["boundary_persona".to_string()],
            ..Default::default()
        };

        apply_personality_runtime_governance_gate(
            &mut decision,
            &crate::memory::PersonalityRuntimeGovernanceGate {
                conservative_reply: true,
                allow_dynamic_persona_priority: false,
                allow_upward_distillation: false,
                reason_summary: "board core still unstable".to_string(),
                outstanding: vec!["review cadence overdue".to_string()],
                repair_plan: crate::memory::PersonalityGovernanceRepairPlan {
                    observe_only: true,
                    summary: "review cadence overdue".to_string(),
                    reasons: vec!["review cadence overdue".to_string()],
                    ..crate::memory::PersonalityGovernanceRepairPlan::default()
                },
            },
        );

        assert!(!decision.refresh_self_model);
        assert!(decision.self_model_intent.is_empty());
        assert!(decision.self_model_sources.is_empty());
        assert!(!decision.refresh_self_authored_core);
        assert!(decision.self_authored_core_intent.is_empty());
        assert!(decision.self_authored_core_sources.is_empty());
        assert!(!decision.refresh_outer_voice);
        assert!(decision.outer_voice_intent.is_empty());
        assert!(decision.outer_voice_sources.is_empty());
        assert!(decision.refresh_self_continuity);
        assert!(decision.refresh_boundary_persona);
    }

    #[test]
    fn runtime_governance_gate_allows_targeted_board_core_repair() {
        let mut decision = SelfRuntimeDecision::default();

        apply_personality_runtime_governance_gate(
            &mut decision,
            &crate::memory::PersonalityRuntimeGovernanceGate {
                conservative_reply: true,
                allow_dynamic_persona_priority: false,
                allow_upward_distillation: false,
                reason_summary: "board_core_review_due".to_string(),
                outstanding: vec!["governance_review_due".to_string()],
                repair_plan: crate::memory::PersonalityGovernanceRepairPlan {
                    repair_needed: true,
                    primary_action:
                        crate::memory::PersonalityGovernanceRepairAction::RepairSelfAuthoredCore,
                    repair_self_authored_core: true,
                    summary: "board_core_review_due".to_string(),
                    reasons: vec!["board_core_review_due".to_string()],
                    ..crate::memory::PersonalityGovernanceRepairPlan::default()
                },
            },
        );

        assert!(decision.refresh_self_authored_core);
        assert!(decision
            .self_authored_core_intent
            .contains("Repair the board-level self core"));
        assert!(!decision.refresh_self_model);
        assert!(!decision.refresh_outer_voice);
    }

    #[test]
    fn runtime_governance_gate_allows_targeted_expression_repair() {
        let mut decision = SelfRuntimeDecision::default();

        apply_personality_runtime_governance_gate(
            &mut decision,
            &crate::memory::PersonalityRuntimeGovernanceGate {
                conservative_reply: true,
                allow_dynamic_persona_priority: false,
                allow_upward_distillation: false,
                reason_summary: "expression_drift_without_constitution_break".to_string(),
                outstanding: vec!["relationship_response_mode_drift".to_string()],
                repair_plan: crate::memory::PersonalityGovernanceRepairPlan {
                    repair_needed: true,
                    primary_action:
                        crate::memory::PersonalityGovernanceRepairAction::RepairOuterVoice,
                    repair_outer_voice: true,
                    summary: "expression_drift_without_constitution_break".to_string(),
                    reasons: vec!["expression_drift_without_constitution_break".to_string()],
                    ..crate::memory::PersonalityGovernanceRepairPlan::default()
                },
            },
        );

        assert!(decision.refresh_outer_voice);
        assert!(decision
            .outer_voice_intent
            .contains("Repair outward expression drift"));
        assert!(!decision.refresh_self_model);
        assert!(!decision.refresh_self_authored_core);
    }

    #[test]
    fn active_inner_conflict_clears_upward_distillation_decision() {
        let mut decision = SelfRuntimeDecision {
            refresh_self_model: true,
            self_model_intent: "promote the new self model".to_string(),
            self_model_sources: vec!["inner_life".to_string()],
            refresh_self_continuity: true,
            self_continuity_intent: "promote continuity".to_string(),
            self_continuity_sources: vec!["self_model".to_string()],
            refresh_self_authored_core: true,
            self_authored_core_intent: "promote board core".to_string(),
            self_authored_core_sources: vec!["self_model".to_string()],
            refresh_outer_voice: true,
            outer_voice_intent: "outer voice remains local".to_string(),
            outer_voice_sources: vec!["boundary_persona".to_string()],
            ..SelfRuntimeDecision::default()
        };
        let conflict = crate::memory::InnerConflict {
            topic: "whether to promote the fresh persona evidence".to_string(),
            pull_a: "stabilize quickly".to_string(),
            pull_b: "wait for repeated evidence".to_string(),
            review_after_secs: 3_600,
            updated_at: 1_000,
            ..crate::memory::InnerConflict::default()
        };

        let blocked =
            apply_inner_conflict_upward_distillation_gate(&mut decision, Some(&conflict), 1_600);

        assert!(blocked);
        assert!(!decision.refresh_self_model);
        assert!(decision.self_model_intent.is_empty());
        assert!(decision.self_model_sources.is_empty());
        assert!(!decision.refresh_self_continuity);
        assert!(decision.self_continuity_intent.is_empty());
        assert!(decision.self_continuity_sources.is_empty());
        assert!(!decision.refresh_self_authored_core);
        assert!(decision.self_authored_core_intent.is_empty());
        assert!(decision.self_authored_core_sources.is_empty());
        assert!(decision.refresh_outer_voice);
    }

    #[test]
    fn expired_or_invalid_inner_conflict_allows_upward_distillation_decision() {
        let expired = crate::memory::InnerConflict {
            topic: "whether to promote the fresh persona evidence".to_string(),
            pull_a: "stabilize quickly".to_string(),
            pull_b: "wait for repeated evidence".to_string(),
            review_after_secs: 60,
            updated_at: 1_000,
            ..crate::memory::InnerConflict::default()
        };
        let invalid = crate::memory::InnerConflict {
            topic: "not a conflict".to_string(),
            pull_a: "same".to_string(),
            pull_b: "same".to_string(),
            review_after_secs: 3_600,
            updated_at: 1_000,
            ..crate::memory::InnerConflict::default()
        };
        for conflict in [expired, invalid] {
            let mut decision = SelfRuntimeDecision {
                refresh_self_model: true,
                self_model_intent: "keep".to_string(),
                self_model_sources: vec!["inner_life".to_string()],
                refresh_self_continuity: true,
                self_continuity_intent: "keep".to_string(),
                self_continuity_sources: vec!["self_model".to_string()],
                refresh_self_authored_core: true,
                self_authored_core_intent: "keep".to_string(),
                self_authored_core_sources: vec!["self_model".to_string()],
                ..SelfRuntimeDecision::default()
            };

            let blocked = apply_inner_conflict_upward_distillation_gate(
                &mut decision,
                Some(&conflict),
                5_000,
            );

            assert!(!blocked);
            assert!(decision.refresh_self_model);
            assert!(decision.refresh_self_continuity);
            assert!(decision.refresh_self_authored_core);
        }
    }

    #[test]
    fn inner_conflict_review_due_uses_bounded_review_window() {
        let conflict = crate::memory::InnerConflict {
            topic: "whether to hold this conflict open".to_string(),
            pull_a: "keep reviewing".to_string(),
            pull_b: "avoid indefinite freeze".to_string(),
            review_after_secs: u64::MAX,
            updated_at: 10,
            ..crate::memory::InnerConflict::default()
        };

        assert!(!inner_conflict_review_due(
            &conflict,
            10 + crate::memory::INNER_CONFLICT_MAX_REVIEW_AFTER_SECS - 1,
        ));
        assert!(inner_conflict_review_due(
            &conflict,
            10 + crate::memory::INNER_CONFLICT_MAX_REVIEW_AFTER_SECS,
        ));
    }

    #[test]
    fn unrelated_post_reply_decision_does_not_refresh_felt_significance() {
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::PostReply,
            source_channel: "chat_channel".to_string(),
            user_content: "user turn".to_string(),
            reply_content: "reply turn".to_string(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 1_000,
        };
        let decision = SelfRuntimeDecision {
            refresh_private_docs: true,
            private_docs_intent: "compress unrelated private notes".to_string(),
            ..SelfRuntimeDecision::default()
        };

        assert!(!should_refresh_felt_significance_runtime(
            &payload,
            Some(&decision),
            None,
            None,
        ));
    }

    #[test]
    fn factual_or_operational_only_post_reply_does_not_refresh_felt_significance() {
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::PostReply,
            source_channel: "chat_channel".to_string(),
            user_content: "user turn".to_string(),
            reply_content: "reply turn".to_string(),
            tool_calls: 0,
            external_content_used: true,
            now_secs: 1_000,
        };
        let factual_decision = SelfRuntimeDecision {
            request_factual_refresh: true,
            factual_reconcile_intent: "refresh objective facts".to_string(),
            ..SelfRuntimeDecision::default()
        };
        let operational_only = crate::memory::RecentPersonaEvidence {
            sampled_turns: 1,
            repeated_response_mode: "compact".to_string(),
            repeated_task_scope: "implementation".to_string(),
            pressure_pattern: "normal=1".to_string(),
            ..crate::memory::RecentPersonaEvidence::default()
        };

        assert!(!should_refresh_felt_significance_runtime(
            &payload,
            Some(&factual_decision),
            None,
            Some(&operational_only),
        ));
    }

    #[test]
    fn sampled_or_operational_only_idle_tick_does_not_refresh_felt_significance() {
        let sampled_only_payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::IdleTick,
            source_channel: "self_runtime_idle".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 1_000,
        };
        let sampled_only = crate::memory::RecentPersonaEvidence {
            sampled_turns: 8,
            ..crate::memory::RecentPersonaEvidence::default()
        };
        let operational_only = crate::memory::RecentPersonaEvidence {
            sampled_turns: 8,
            meaningful_turns: 8,
            repeated_response_mode: "compact".to_string(),
            repeated_task_scope: "implementation".to_string(),
            repeated_reply_scope: "direct answer".to_string(),
            pressure_pattern: "normal=8".to_string(),
            tool_usage_pattern: "code inspection".to_string(),
            ..crate::memory::RecentPersonaEvidence::default()
        };

        assert!(!should_refresh_felt_significance_runtime(
            &sampled_only_payload,
            None,
            None,
            Some(&sampled_only),
        ));
        assert!(!should_refresh_felt_significance_runtime(
            &sampled_only_payload,
            None,
            None,
            Some(&operational_only),
        ));
    }

    #[test]
    fn sandbox_probe_text_uses_existing_counterfactual_and_arena_ledgers_only() {
        let ledgers = vec![crate::memory::TurnLedger {
            counterfactual: Some(crate::memory::TurnCounterfactualLedger {
                summary: "selected cautious branch before identity promotion".to_string(),
                selected_branch: crate::memory::TurnCounterfactualBranchLedger {
                    branch: "hold promotion".to_string(),
                    score: 8,
                    summary: "wait for repeated evidence".to_string(),
                    ..crate::memory::TurnCounterfactualBranchLedger::default()
                },
                ..crate::memory::TurnCounterfactualLedger::default()
            }),
            adversarial_arena: Some(crate::memory::TurnAdversarialArenaLedger {
                subject_kind: "identity_pressure".to_string(),
                disposition: "held_for_clarification".to_string(),
                summary: "defender kept boundary against one-turn pressure".to_string(),
                winner: crate::memory::TurnAdversarialArenaClaimLedger {
                    role: "defender".to_string(),
                    label: "keep boundary".to_string(),
                    evidence_score: 7,
                    ..crate::memory::TurnAdversarialArenaClaimLedger::default()
                },
                ..crate::memory::TurnAdversarialArenaLedger::default()
            }),
            ..crate::memory::TurnLedger::default()
        }];

        let rendered =
            build_self_runtime_sandbox_probe_text(&ledgers, 1024).expect("sandbox probe evidence");

        assert!(rendered.contains("candidate evidence, not a write authority"));
        assert!(rendered.contains("selected cautious branch"));
        assert!(rendered.contains("identity_pressure"));
        assert!(rendered.contains("keep boundary"));
    }

    #[test]
    fn meaningful_existing_or_persona_evidence_allows_post_reply_felt_significance_refresh() {
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::PostReply,
            source_channel: "chat_channel".to_string(),
            user_content: "user turn".to_string(),
            reply_content: "reply turn".to_string(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 1_000,
        };
        let decision = SelfRuntimeDecision {
            refresh_inner_life: true,
            ..SelfRuntimeDecision::default()
        };

        assert!(should_refresh_felt_significance_runtime(
            &payload,
            Some(&decision),
            Some(&crate::memory::FeltSignificance {
                significance_summary: "already has subjective weight".to_string(),
                ..crate::memory::FeltSignificance::default()
            }),
            None,
        ));
        assert!(should_refresh_felt_significance_runtime(
            &payload,
            Some(&decision),
            None,
            Some(&crate::memory::RecentPersonaEvidence {
                meaningful_turns: 12,
                repeated_relationship_posture: "architecture partner".to_string(),
                ..crate::memory::RecentPersonaEvidence::default()
            }),
        ));
    }

    #[test]
    fn embedded_self_model_gate_rejects_idle_and_operational_only_post_reply() {
        let idle_payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::IdleTick,
            source_channel: "self_runtime_idle".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 1_000,
        };
        let mut idle_decision = SelfRuntimeDecision {
            refresh_self_model: true,
            self_model_intent: "distill idle private material".to_string(),
            self_model_sources: vec!["inner_life".to_string()],
            ..SelfRuntimeDecision::default()
        };
        apply_embedded_self_model_refresh_gate(
            &mut idle_decision,
            MemorySystemKind::EspCompact,
            &idle_payload,
            Some(&crate::memory::RecentPersonaEvidence {
                repeated_relationship_posture: "stable warm boundary".to_string(),
                updated_at: 100,
                ..crate::memory::RecentPersonaEvidence::default()
            }),
        );
        assert!(!idle_decision.refresh_self_model);
        assert!(idle_decision.self_model_intent.is_empty());
        assert!(idle_decision.self_model_sources.is_empty());

        let post_reply_payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::PostReply,
            source_channel: "chat_channel".to_string(),
            user_content: "user turn".to_string(),
            reply_content: "reply turn".to_string(),
            tool_calls: 2,
            external_content_used: false,
            now_secs: 1_000,
        };
        let operational_only = crate::memory::RecentPersonaEvidence {
            sampled_turns: 6,
            meaningful_turns: 6,
            repeated_response_mode: "compact".to_string(),
            repeated_task_scope: "implementation".to_string(),
            pressure_pattern: "normal=6".to_string(),
            tool_usage_pattern: "tool_calls=6".to_string(),
            updated_at: 120,
            ..crate::memory::RecentPersonaEvidence::default()
        };
        let mut operational_decision = SelfRuntimeDecision {
            refresh_self_model: true,
            self_model_intent: "promote tool-heavy turn".to_string(),
            self_model_sources: vec!["recent_persona_evidence".to_string()],
            ..SelfRuntimeDecision::default()
        };
        apply_embedded_self_model_refresh_gate(
            &mut operational_decision,
            MemorySystemKind::EspCompact,
            &post_reply_payload,
            Some(&operational_only),
        );
        assert!(!operational_decision.refresh_self_model);
        assert!(operational_decision.self_model_intent.is_empty());
        assert!(operational_decision.self_model_sources.is_empty());
    }

    #[test]
    fn embedded_self_model_gate_allows_promotable_post_reply_and_operator_request() {
        let post_reply_payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::PostReply,
            source_channel: "chat_channel".to_string(),
            user_content: "user turn".to_string(),
            reply_content: "reply turn".to_string(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 1_000,
        };
        let promotable = crate::memory::RecentPersonaEvidence {
            repeated_relationship_posture: "stable warm boundary".to_string(),
            updated_at: 200,
            ..crate::memory::RecentPersonaEvidence::default()
        };
        let mut post_reply_decision = SelfRuntimeDecision {
            refresh_self_model: true,
            self_model_intent: "distill repeated relationship posture".to_string(),
            self_model_sources: vec!["recent_persona_evidence".to_string()],
            ..SelfRuntimeDecision::default()
        };
        apply_embedded_self_model_refresh_gate(
            &mut post_reply_decision,
            MemorySystemKind::EspCompact,
            &post_reply_payload,
            Some(&promotable),
        );
        assert!(post_reply_decision.refresh_self_model);
        assert!(post_reply_decision
            .self_model_intent
            .contains("relationship posture"));
        assert_eq!(
            post_reply_decision.self_model_sources,
            vec!["recent_persona_evidence".to_string()]
        );

        let operator_payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::OperatorRequested,
            source_channel: "operator".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 1_000,
        };
        let mut operator_decision = SelfRuntimeDecision {
            refresh_self_model: true,
            self_model_intent: "operator requested self-model repair".to_string(),
            self_model_sources: vec!["inner_life".to_string()],
            ..SelfRuntimeDecision::default()
        };
        apply_embedded_self_model_refresh_gate(
            &mut operator_decision,
            MemorySystemKind::EspCompact,
            &operator_payload,
            None,
        );
        assert!(operator_decision.refresh_self_model);
    }

    #[test]
    fn upward_distillation_post_reply_runs_inner_conflict_gate_first() {
        let payload = SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::PostReply,
            source_channel: "chat_channel".to_string(),
            user_content: "user turn".to_string(),
            reply_content: "reply turn".to_string(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: 1_000,
        };
        let decision = SelfRuntimeDecision {
            refresh_self_model: true,
            refresh_self_continuity: true,
            refresh_self_authored_core: true,
            ..SelfRuntimeDecision::default()
        };

        assert!(should_refresh_inner_conflict_runtime(
            &payload,
            Some(&decision),
            None,
            None,
        ));
    }

    #[test]
    fn first_idle_tick_waits_for_strategy_cadence() {
        assert!(!idle_self_runtime_due(1_000, 60, 980, 0, 480));
        assert!(!idle_self_runtime_due(1_000, 300, 400, 0, 900));
        assert!(idle_self_runtime_due(1_000, 900, 0, 0, 900));
        assert!(idle_self_runtime_due(1_000, 900, 50, 100, 900));
    }

    #[test]
    fn post_reply_enqueue_runs_for_missing_core_or_runtime_signal() {
        let continuity = crate::memory::SelfContinuity {
            last_user_channel: "chat_channel".to_string(),
            last_autonomy_run_at: 900,
            ..crate::memory::SelfContinuity::default()
        };
        let strategy = crate::memory::AutonomyStrategy {
            idle_enabled: true,
            idle_interval_secs: 300,
            ..crate::memory::AutonomyStrategy::default()
        };

        assert!(should_enqueue_self_runtime_post_reply_with_state(
            Some(&continuity),
            Some(&strategy),
            false,
            "chat_channel",
            0,
            false,
            1_000,
            MemoryProfile::Standard,
        ));
        assert!(should_enqueue_self_runtime_post_reply_with_state(
            Some(&continuity),
            Some(&strategy),
            true,
            "chat_channel",
            1,
            false,
            1_000,
            MemoryProfile::Standard,
        ));
        assert!(should_enqueue_self_runtime_post_reply_with_state(
            Some(&continuity),
            Some(&strategy),
            true,
            "work_channel",
            0,
            false,
            1_000,
            MemoryProfile::Standard,
        ));
    }

    #[test]
    fn post_reply_enqueue_skips_when_runtime_is_fresh_and_untriggered() {
        let continuity = crate::memory::SelfContinuity {
            last_user_channel: "chat_channel".to_string(),
            last_autonomy_run_at: 950,
            ..crate::memory::SelfContinuity::default()
        };
        let strategy = crate::memory::AutonomyStrategy {
            idle_enabled: true,
            idle_interval_secs: 300,
            ..crate::memory::AutonomyStrategy::default()
        };

        assert!(!should_enqueue_self_runtime_post_reply_with_state(
            Some(&continuity),
            Some(&strategy),
            true,
            "chat_channel",
            0,
            false,
            1_000,
            MemoryProfile::Standard,
        ));
    }

    #[test]
    fn self_runtime_uses_atomic_core_plan_post_image_without_store_reread() {
        let mut current = Some(crate::memory::SelfAuthoredCore {
            revision: 1,
            identity_anchor: "prior".to_string(),
            ..crate::memory::SelfAuthoredCore::default()
        });
        let plan = Ok(SelfAuthoredCoreRefreshPlanV1::Adopt {
            expected_prior: crate::memory::SelfAuthoredCoreExpectedPriorV1 {
                core_revision: Some(1),
                core_digest: Some("a".repeat(64)),
                ledger_digest: "b".repeat(64),
            },
            next_core: Box::new(crate::memory::SelfAuthoredCore {
                revision: 2,
                supersedes_revision: Some(1),
                identity_anchor: "planned post-image".to_string(),
                ..crate::memory::SelfAuthoredCore::default()
            }),
            next_ledger: crate::memory::CoreRevisionLedger::default(),
            origin: crate::memory::SubjectSoulRevisionOriginV1::SelfGovernedRevision,
            proposal_ref: format!("self-authored-proposal:{}", "c".repeat(64)),
            source_refs: vec!["self_model".to_string()],
        });

        apply_self_authored_core_plan_overlay(&mut current, &plan);

        let overlaid = current.expect("planned Core post-image");
        assert_eq!(overlaid.revision, 2);
        assert_eq!(overlaid.identity_anchor, "planned post-image");
    }
}
