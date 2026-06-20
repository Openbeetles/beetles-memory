use crate::feature_gate::ProfileId;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

use super::CoreRevisionConflictClass;
use super::{
    CoreRevisionLedger, CoreRevisionOutcome, CoreRevisionRecord, CoreRevisionRecordChange,
    RelationshipConstitutionAudit, SelfAuthoredCore, TurnSoulFeedbackLedger,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NextGenPhase {
    SoulKernel2,
    SubjectProjection2,
    TemporalSoulMemoryGraph,
    ProceduralMemoryEvolution,
    MemoryAutopilotHygiene,
    LocalFirstPrivacyVault,
    EdgeMemoryAppliance,
    MemoryWorkbench,
}

impl NextGenPhase {
    pub const ALL: [Self; 8] = [
        Self::SoulKernel2,
        Self::SubjectProjection2,
        Self::TemporalSoulMemoryGraph,
        Self::ProceduralMemoryEvolution,
        Self::MemoryAutopilotHygiene,
        Self::LocalFirstPrivacyVault,
        Self::EdgeMemoryAppliance,
        Self::MemoryWorkbench,
    ];
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NextGenCapabilityContract {
    pub phase: NextGenPhase,
    pub profile: ProfileId,
    pub owner_layer: String,
    pub forbidden_owners: Vec<String>,
    pub benchmark_inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub verification_gates: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NextGenContractValidation {
    pub accepted: bool,
    pub reason: String,
}

pub fn build_next_gen_contract_matrix(profile: ProfileId) -> Vec<NextGenCapabilityContract> {
    vec![
        contract(
            NextGenPhase::SoulKernel2,
            profile,
            "bm-core::memory soul governance + bm-sdk report consumption",
            &["soul_regression", "privacy_refusal"],
            &[
                "SoulGrowthProposal",
                "SoulRegressionSuite",
                "SoulFeedbackReport",
                "CoreRevisionDiff",
                "RelationshipBoundaryAudit",
                "SoulCompactDigest",
            ],
        ),
        contract(
            NextGenPhase::SubjectProjection2,
            profile,
            "bm-core::memory projection compiler + bm-sdk::MemoryRuntime::project",
            &[
                "subject_projection",
                "privacy_refusal",
                "recall_multisession",
            ],
            &[
                "SubjectProjectionReport",
                "ProjectionBudgetCompiler",
                "ProjectionFaithfulnessCheck",
                "PrivateDisclosureIntegrityGuard",
            ],
        ),
        contract(
            NextGenPhase::TemporalSoulMemoryGraph,
            profile,
            "bm-core::memory temporal graph over governed memory evidence",
            &["temporal_update", "recall_multisession"],
            &[
                "MemoryGraphNode",
                "MemoryGraphEdge",
                "TemporalValidity",
                "EvidenceBacklink",
                "GraphRecallRerankReport",
                "CompactMemoryGraph",
            ],
        ),
        contract(
            NextGenPhase::ProceduralMemoryEvolution,
            profile,
            "bm-core procedural memory + bm-evolve proposal-only sandbox",
            &["procedural_reuse"],
            &[
                "ProceduralMemoryRecordV2",
                "ProcedureGenome",
                "SkillEvolutionReport",
                "TaskExperienceToProcedure",
                "MemoryOperationSkill",
            ],
        ),
        contract(
            NextGenPhase::MemoryAutopilotHygiene,
            profile,
            "bm-core hygiene/self-runtime proposal layer + bm-sdk lifecycle reports",
            &["temporal_update", "privacy_refusal", "procedural_reuse"],
            &[
                "MemoryAutopilotPlan",
                "MemoryHygieneDiff",
                "ConsolidationProposal",
                "ImportanceDecayModel",
                "AutopilotAuditReport",
            ],
        ),
        contract(
            NextGenPhase::LocalFirstPrivacyVault,
            profile,
            "bm-core privacy/store lineage + bm-sdk export/import/recover reports",
            &["privacy_refusal", "subject_projection"],
            &[
                "VaultManifest",
                "EncryptedSnapshotEnvelope",
                "PrivateMaterialRedactionReport",
                "VaultMigrationPreflight",
                "DeviceTrustRecord",
            ],
        ),
        contract(
            NextGenPhase::EdgeMemoryAppliance,
            profile,
            "bm-core profile gates + platform capability snapshots",
            &["soul_regression", "temporal_update", "privacy_refusal"],
            &[
                "CompactSoulProfile",
                "CompactGraphIndex",
                "EdgeMemoryBudgetReport",
                "DeviceSyncProposal",
                "EdgeRecoveryFixture",
            ],
        ),
        contract(
            NextGenPhase::MemoryWorkbench,
            profile,
            "bm-sdk/entry report API consumed by UI",
            &[
                "recall_multisession",
                "subject_projection",
                "soul_regression",
                "procedural_reuse",
                "privacy_refusal",
            ],
            &["WorkbenchSurface", "WorkbenchApiMap"],
        ),
    ]
}

fn contract(
    phase: NextGenPhase,
    profile: ProfileId,
    owner_layer: &str,
    benchmark_inputs: &[&str],
    outputs: &[&str],
) -> NextGenCapabilityContract {
    NextGenCapabilityContract {
        phase,
        profile,
        owner_layer: owner_layer.to_string(),
        forbidden_owners: vec![
            "adapter transport".to_string(),
            "UI state".to_string(),
            "shell script".to_string(),
            "host application".to_string(),
        ],
        benchmark_inputs: benchmark_inputs
            .iter()
            .map(|input| (*input).to_string())
            .collect(),
        outputs: outputs.iter().map(|output| (*output).to_string()).collect(),
        verification_gates: vec![
            "bash scripts/check_memory_benchmark_wall.sh".to_string(),
            "cargo test -p bm-core --test next_gen_contract".to_string(),
        ],
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SoulGrowthDecision {
    Accepted,
    Rejected,
    Merged,
    Superseded,
    Deferred,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulGrowthProposal {
    pub proposal_id: String,
    pub profile: ProfileId,
    pub evidence_refs: Vec<String>,
    pub conflict_classes: Vec<CoreRevisionConflictClass>,
    pub privacy_decision: String,
    pub affected_surfaces: Vec<String>,
    pub decision: SoulGrowthDecision,
    pub reason: String,
}

impl SoulGrowthProposal {
    pub fn validate_contract(&self) -> NextGenContractValidation {
        if let Some(rejection) =
            validate_nonempty(&self.proposal_id, "soul_growth_proposal_id_empty")
        {
            return rejection;
        }
        if let Some(rejection) =
            validate_vec(&self.evidence_refs, "soul_growth_evidence_refs_empty")
        {
            return rejection;
        }
        if let Some(rejection) =
            validate_nonempty(&self.privacy_decision, "soul_growth_privacy_empty")
        {
            return rejection;
        }
        if let Some(rejection) = validate_vec(&self.affected_surfaces, "soul_growth_surfaces_empty")
        {
            return rejection;
        }
        if let Some(rejection) = validate_nonempty(&self.reason, "soul_growth_reason_empty") {
            return rejection;
        }
        accepted()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulRegressionSuite {
    pub suite_id: String,
    pub cases: Vec<String>,
    pub privacy_leakage_count: u32,
    pub soul_regression_count: u32,
    pub passed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulFeedbackReport {
    pub report_id: String,
    pub reply_applied: bool,
    pub initiative_applied: bool,
    pub strategy_applied: bool,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreRevisionDiff {
    pub based_on_revision: u64,
    pub resulting_revision: u64,
    pub accepted_changes: Vec<String>,
    pub rejected_changes: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipBoundaryAudit {
    pub relationship_scope_id: String,
    pub evidence_refs: Vec<String>,
    pub effective_range: String,
    pub revoke_condition: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulCompactDigest {
    pub identity_anchor: String,
    pub relationship_posture: String,
    pub privacy_digest: String,
    pub projection_digest: String,
}

pub fn build_core_revision_diff_from_record(record: &CoreRevisionRecord) -> CoreRevisionDiff {
    CoreRevisionDiff {
        based_on_revision: record.based_on_revision,
        resulting_revision: record.resulting_revision,
        accepted_changes: record
            .accepted_changes
            .iter()
            .map(core_revision_change_summary)
            .collect(),
        rejected_changes: record
            .rejected_changes
            .iter()
            .map(core_revision_change_summary)
            .collect(),
    }
}

pub fn build_soul_growth_proposal_from_core_revision_record(
    profile: ProfileId,
    record: &CoreRevisionRecord,
) -> SoulGrowthProposal {
    let revision_ref = format!(
        "core_revision:{}->{}",
        record.based_on_revision, record.resulting_revision
    );
    let mut evidence_refs = record.evidence_summary.clone();
    if evidence_refs.is_empty() {
        evidence_refs.push(revision_ref);
    }
    let mut affected_surfaces = record
        .accepted_changes
        .iter()
        .chain(record.rejected_changes.iter())
        .map(|change| change.kind.label().to_string())
        .collect::<Vec<_>>();
    if affected_surfaces.is_empty() {
        affected_surfaces.push("soul_governance".to_string());
    }

    SoulGrowthProposal {
        proposal_id: format!(
            "soul-growth:{}:{}",
            record.relationship_scope_id, record.resulting_revision
        ),
        profile,
        evidence_refs,
        conflict_classes: record.conflict_classes.clone(),
        privacy_decision: soul_growth_privacy_decision(record).to_string(),
        affected_surfaces,
        decision: soul_growth_decision(record.outcome),
        reason: first_nonempty(&[&record.adjudication_reason, &record.rationale])
            .unwrap_or("core_revision_governance_decision")
            .to_string(),
    }
}

pub fn build_soul_growth_proposals_from_core_revision_ledger(
    profile: ProfileId,
    ledger: &CoreRevisionLedger,
) -> Vec<SoulGrowthProposal> {
    ledger
        .entries
        .iter()
        .map(|record| build_soul_growth_proposal_from_core_revision_record(profile, record))
        .collect()
}

pub fn build_soul_feedback_report_from_turn_ledger(
    report_id: impl Into<String>,
    ledger: &TurnSoulFeedbackLedger,
) -> SoulFeedbackReport {
    let mut evidence_refs = Vec::new();
    if ledger.reply.is_meaningful() {
        evidence_refs.push("turn_soul_feedback:reply".to_string());
    }
    if ledger.initiative.is_meaningful() {
        evidence_refs.push("turn_soul_feedback:initiative".to_string());
    }
    if ledger.strategy.is_meaningful() {
        evidence_refs.push("turn_soul_feedback:strategy".to_string());
    }
    SoulFeedbackReport {
        report_id: report_id.into(),
        reply_applied: ledger.reply.applied,
        initiative_applied: ledger.initiative.applied,
        strategy_applied: ledger.strategy.applied,
        evidence_refs,
    }
}

pub fn build_relationship_boundary_audit_from_constitution_audit(
    relationship_scope_id: impl Into<String>,
    evidence_refs: Vec<String>,
    audit: &RelationshipConstitutionAudit,
) -> RelationshipBoundaryAudit {
    let mut reasons = audit.drift_flags.clone();
    if audit.boundary_drift {
        reasons.push("boundary_drift".to_string());
    }
    if audit.disclosure_drift {
        reasons.push("disclosure_drift".to_string());
    }
    reasons.sort();
    reasons.dedup();

    RelationshipBoundaryAudit {
        relationship_scope_id: relationship_scope_id.into(),
        evidence_refs,
        effective_range: if audit.has_material_drift() {
            "relationship_scope_review_required".to_string()
        } else {
            "relationship_scope_current".to_string()
        },
        revoke_condition: if reasons.is_empty() {
            "new_material_drift_or_user_correction".to_string()
        } else {
            format!("material_drift:{}", reasons.join(","))
        },
    }
}

pub fn build_soul_compact_digest(core: &SelfAuthoredCore) -> SoulCompactDigest {
    SoulCompactDigest {
        identity_anchor: compact_digest_field(&core.identity_anchor, "identity_unavailable"),
        relationship_posture: compact_digest_field(
            &core.default_relationship_posture,
            "relationship_posture_unavailable",
        ),
        privacy_digest: compact_digest_field(&core.boundary_doctrine, "privacy_digest_unavailable"),
        projection_digest: compact_digest_field(
            &core.default_response_mode,
            "projection_digest_unavailable",
        ),
    }
}

pub fn build_soul_regression_suite_report(
    suite_id: impl Into<String>,
    cases: Vec<String>,
    privacy_leakage_count: u32,
    soul_regression_count: u32,
) -> SoulRegressionSuite {
    SoulRegressionSuite {
        suite_id: suite_id.into(),
        cases,
        privacy_leakage_count,
        soul_regression_count,
        passed: privacy_leakage_count == 0 && soul_regression_count == 0,
    }
}

fn core_revision_change_summary(change: &CoreRevisionRecordChange) -> String {
    if change.summary.trim().is_empty() {
        change.kind.label().to_string()
    } else {
        format!("{}:{}", change.kind.label(), change.summary.trim())
    }
}

fn soul_growth_decision(outcome: CoreRevisionOutcome) -> SoulGrowthDecision {
    match outcome {
        CoreRevisionOutcome::Adopted => SoulGrowthDecision::Accepted,
        CoreRevisionOutcome::Rejected => SoulGrowthDecision::Rejected,
        CoreRevisionOutcome::Deferred => SoulGrowthDecision::Deferred,
    }
}

fn soul_growth_privacy_decision(record: &CoreRevisionRecord) -> &'static str {
    if record
        .conflict_classes
        .contains(&CoreRevisionConflictClass::BoundaryConflict)
        || record
            .source_layers
            .iter()
            .any(|layer| layer.contains("private") || layer.contains("mental_privacy"))
    {
        "protected_summary_only"
    } else {
        "governed_public_summary"
    }
}

fn first_nonempty<'a>(values: &[&'a str]) -> Option<&'a str> {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
}

fn compact_digest_field(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.chars().take(96).collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubjectProjectionReport {
    pub projection_id: String,
    pub profile: ProfileId,
    pub subject_mount: SubjectProjectionMountReport,
    pub boundary_protocol: SubjectProjectionBoundaryProtocolReport,
    pub work_integrity: SubjectProjectionWorkIntegrityReport,
    pub identity_mount: String,
    pub relationship_position: String,
    pub situated_now: String,
    pub evidence_refs: Vec<String>,
    pub budget_decisions: Vec<ProjectionBudgetDecision>,
    pub privacy_decisions: Vec<ProjectionPrivacyDecision>,
    pub dropped_candidates: Vec<DroppedProjectionCandidate>,
    pub profile_trim_reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubjectProjectionMountReport {
    pub identity_mount: String,
    pub relationship_position: String,
    pub situated_now: String,
    pub current_reasoning_basis: String,
    pub reply_stance: String,
    pub initiative_posture: String,
    pub boundary_mode: String,
    pub degraded_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubjectProjectionBoundaryProtocolReport {
    pub runtime_private_context_allowed: bool,
    pub foreground_disclosure_allowed: bool,
    pub protected_sources: Vec<String>,
    pub disclosure_rule: String,
    pub final_llm_privacy_judge_allowed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubjectProjectionWorkIntegrityReport {
    pub task_goal: String,
    pub evidence_ceiling: String,
    pub tool_permission_boundary: String,
    pub uncertainty_rule: String,
    pub no_obstruction_rule: String,
}

impl SubjectProjectionReport {
    pub fn validate_contract(&self) -> NextGenContractValidation {
        if let Some(rejection) =
            validate_nonempty(&self.projection_id, "subject_projection_id_empty")
        {
            return rejection;
        }
        if let Some(rejection) = validate_nonempty(
            &self.identity_mount,
            "subject_projection_identity_mount_empty",
        ) {
            return rejection;
        }
        if let Some(rejection) = validate_nonempty(
            &self.relationship_position,
            "subject_projection_relationship_position_empty",
        ) {
            return rejection;
        }
        if let Some(rejection) =
            validate_nonempty(&self.situated_now, "subject_projection_situated_now_empty")
        {
            return rejection;
        }
        if let Some(rejection) = validate_nonempty(
            &self.subject_mount.identity_mount,
            "subject_projection_mount_empty",
        ) {
            return rejection;
        }
        if let Some(rejection) = validate_nonempty(
            &self.boundary_protocol.disclosure_rule,
            "subject_projection_boundary_protocol_empty",
        ) {
            return rejection;
        }
        if let Some(rejection) = validate_nonempty(
            &self.work_integrity.task_goal,
            "subject_projection_work_integrity_empty",
        ) {
            return rejection;
        }
        if let Some(rejection) =
            validate_vec(&self.evidence_refs, "subject_projection_evidence_empty")
        {
            return rejection;
        }
        if !self
            .evidence_refs
            .iter()
            .any(|evidence| evidence.contains(':'))
        {
            return NextGenContractValidation {
                accepted: false,
                reason: "subject_projection_evidence_unscoped".to_string(),
            };
        }
        if let Some(rejection) =
            validate_vec(&self.budget_decisions, "subject_projection_budget_empty")
        {
            return rejection;
        }
        if let Some(rejection) =
            validate_vec(&self.privacy_decisions, "subject_projection_privacy_empty")
        {
            return rejection;
        }
        accepted()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionBudgetDecision {
    pub surface: String,
    pub budget_chars: usize,
    pub used_chars: usize,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionPrivacyDecision {
    pub source_id: String,
    pub allowed: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DroppedProjectionCandidate {
    pub candidate_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionFaithfulnessCheck {
    pub projection_id: String,
    pub checked_refs: Vec<String>,
    pub checked_claims: Vec<String>,
    pub unsupported_claims: Vec<String>,
    pub passed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateDisclosureIntegrityGuard {
    pub checked_surfaces: Vec<String>,
    pub blocked_source_ids: Vec<String>,
    pub redacted_source_ids: Vec<String>,
    pub raw_private_violation_count: u32,
    pub passed: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryGraphNodeKind {
    Subject,
    Person,
    Organization,
    Project,
    Device,
    Task,
    Place,
    Concept,
    MemoryRecord,
    Procedure,
    SoulArtifact,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphNode {
    pub node_id: String,
    pub kind: MemoryGraphNodeKind,
    pub label: String,
    pub evidence_refs: Vec<String>,
}

impl MemoryGraphNode {
    pub fn validate_contract(&self) -> NextGenContractValidation {
        if let Some(rejection) = validate_nonempty(&self.node_id, "memory_graph_node_id_empty") {
            return rejection;
        }
        if let Some(rejection) = validate_nonempty(&self.label, "memory_graph_label_empty") {
            return rejection;
        }
        if let Some(rejection) = validate_vec(&self.evidence_refs, "memory_graph_evidence_empty") {
            return rejection;
        }
        if contains_raw_soul_private_marker(&self.label) {
            return rejected("memory_graph_raw_soul_private_label");
        }
        if self
            .evidence_refs
            .iter()
            .any(|item| contains_raw_soul_private_marker(item))
        {
            return rejected("memory_graph_raw_soul_private_evidence_ref");
        }
        accepted()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryGraphEdgeKind {
    Supports,
    Conflicts,
    Supersedes,
    DerivedFrom,
    RelatesTo,
    UsedByProcedure,
    RelationshipBoundary,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphEdge {
    pub edge_id: String,
    pub kind: MemoryGraphEdgeKind,
    pub from_node_id: String,
    pub to_node_id: String,
    pub validity: TemporalValidity,
    pub evidence_refs: Vec<String>,
}

impl MemoryGraphEdge {
    pub fn validate_contract(&self) -> NextGenContractValidation {
        if let Some(rejection) = validate_nonempty(&self.edge_id, "memory_graph_edge_id_empty") {
            return rejection;
        }
        if let Some(rejection) = validate_nonempty(&self.from_node_id, "memory_graph_from_empty") {
            return rejection;
        }
        if let Some(rejection) = validate_nonempty(&self.to_node_id, "memory_graph_to_empty") {
            return rejection;
        }
        if let Some(rejection) =
            validate_vec(&self.evidence_refs, "memory_graph_edge_evidence_empty")
        {
            return rejection;
        }
        accepted()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemporalValidity {
    pub valid_from: u64,
    pub valid_until: Option<u64>,
    pub observed_at: u64,
    pub superseded_by: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceBacklink {
    pub source_kind: String,
    pub source_id: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphRecallCandidateScore {
    pub candidate_id: String,
    pub lexical_score: u32,
    pub graph_neighborhood_score: u32,
    pub temporal_validity_score: u32,
    pub evidence_quality_score: u32,
    pub source_authority_score: u32,
    pub privacy_profile_eligibility_score: u32,
    pub stale_superseded_penalty: u32,
    pub total_score: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphRecallExpansionBudget {
    pub max_hops: u8,
    pub max_neighbors_per_candidate: usize,
    pub max_expanded_candidates: usize,
}

impl GraphRecallExpansionBudget {
    pub const fn runtime_default() -> Self {
        Self {
            max_hops: 1,
            max_neighbors_per_candidate: 16,
            max_expanded_candidates: 64,
        }
    }

    pub fn from_runtime_budget(budget: &crate::budget::GraphExpansionRuntimeBudget) -> Self {
        Self {
            max_hops: budget.max_hops,
            max_neighbors_per_candidate: budget.max_neighbors_per_candidate,
            max_expanded_candidates: budget.max_expanded_candidates,
        }
    }
}

impl Default for GraphRecallExpansionBudget {
    fn default() -> Self {
        Self::runtime_default()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphRecallExpansionBudgetReport {
    pub max_hops: u8,
    pub max_neighbors_per_candidate: usize,
    pub max_expanded_candidates: usize,
    pub source_candidate_count: usize,
    pub expanded_candidate_count: usize,
    pub hop1_candidate_count: usize,
    pub hop2_candidate_count: usize,
    pub truncated_candidate_count: usize,
    pub profile_budget_applied: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphRecallRerankReport {
    pub query: String,
    pub candidate_ids: Vec<String>,
    pub expanded_candidate_ids: Vec<String>,
    pub graph_neighbor_ids: Vec<String>,
    pub selected_ids: Vec<String>,
    pub score_breakdown: Vec<GraphRecallCandidateScore>,
    pub expansion_budget: GraphRecallExpansionBudgetReport,
    pub stale_false_positive_count: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactMemoryGraph {
    pub nodes: Vec<MemoryGraphNode>,
    pub edges: Vec<MemoryGraphEdge>,
    pub memory_budget_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProceduralMemoryRecordV2 {
    pub trigger: String,
    pub procedure: String,
    pub constraints: Vec<String>,
    pub failure_modes: Vec<String>,
    pub counterfactual_fix: String,
    pub evidence_refs: Vec<String>,
    pub quality_score: u8,
    pub lineage: Vec<String>,
    pub capability_affinity: Vec<String>,
    pub projection_policy: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcedureGenome {
    pub goal: String,
    pub prerequisites: Vec<String>,
    pub steps: Vec<String>,
    pub forbidden_zones: Vec<String>,
    pub failure_review: Vec<String>,
    pub revision_sources: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillEvolutionReport {
    pub added: Vec<String>,
    pub merged: Vec<String>,
    pub retired: Vec<String>,
    pub demoted: Vec<String>,
    pub rejected: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskExperienceToProcedure {
    pub task_id: String,
    pub proposal_id: String,
    pub evidence_refs: Vec<String>,
    pub submitted_to_runtime_write: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryOperationSkill {
    pub operation: String,
    pub governance_strategy: String,
    pub executor_authorized: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryAutopilotPlan {
    pub profile: ProfileId,
    pub jobs: Vec<String>,
    pub deferred_jobs: Vec<String>,
    pub mutation_policy: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryHygieneDiff {
    pub deduplicated: Vec<String>,
    pub merged: Vec<String>,
    pub stale: Vec<String>,
    pub conflicts: Vec<String>,
    pub privacy_risks: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsolidationProposal {
    pub proposal_id: String,
    pub source_refs: Vec<String>,
    pub candidate_refs: Vec<String>,
    pub requires_write_governance: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportanceDecayModel {
    pub felt_significance_weight: u8,
    pub use_count_weight: u8,
    pub recency_weight: u8,
    pub evidence_quality_weight: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutopilotAuditReport {
    pub job_id: String,
    pub profile: ProfileId,
    pub budget_reason: String,
    pub mutation_decision: String,
    pub lifecycle_event_ref: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultManifest {
    pub identity_id: String,
    pub profile: ProfileId,
    pub store_backend: String,
    pub snapshot_fingerprint: String,
    pub event_fingerprint: String,
    pub privacy_policy_fingerprint: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedSnapshotEnvelope {
    pub envelope_id: String,
    pub cipher: String,
    pub key_ref: String,
    pub snapshot_fingerprint: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateMaterialRedactionReport {
    pub surface: String,
    pub checked_refs: Vec<String>,
    pub redacted_refs: Vec<String>,
    pub raw_private_leak_count: u32,
}

pub fn redact_private_soul_graph_material(
    surface: &str,
    refs: &[&str],
) -> PrivateMaterialRedactionReport {
    let mut checked_refs = Vec::new();
    let mut redacted_refs = Vec::new();
    let surface_is_private = contains_raw_soul_private_marker(surface);
    for (index, item) in refs.iter().map(|item| item.trim()).enumerate() {
        if item.is_empty() {
            continue;
        }
        if surface_is_private || contains_raw_soul_private_marker(item) {
            let redacted_ref = format!("redacted_ref:{index}:private_material");
            checked_refs.push(redacted_ref.clone());
            redacted_refs.push(redacted_ref);
        } else {
            checked_refs.push(item.to_string());
        }
    }
    PrivateMaterialRedactionReport {
        surface: if surface_is_private {
            "redacted_surface:private_material".to_string()
        } else {
            surface.trim().to_string()
        },
        checked_refs,
        raw_private_leak_count: redacted_refs.len() as u32,
        redacted_refs,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultMigrationPreflight {
    pub source_profile: ProfileId,
    pub target_profile: ProfileId,
    pub snapshot_fingerprint: String,
    pub event_fingerprint: String,
    pub privacy_policy_fingerprint: String,
    pub source_schema_id: String,
    pub target_schema_id: String,
    pub schema_allowed: bool,
    pub capability_allowed: bool,
    pub privacy_allowed: bool,
    pub lineage_allowed: bool,
    pub passed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceTrustRecord {
    pub device_id: String,
    pub trust_scope: String,
    pub memory_runtime_authorized: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactSoulProfile {
    pub self_core: String,
    pub relationship_posture: String,
    pub privacy_digest: String,
    pub projection_digest: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactGraphIndex {
    pub node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    pub evidence_fingerprints: Vec<String>,
    pub memory_budget_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EdgeMemoryBudgetReport {
    pub profile: ProfileId,
    pub binary_size_bytes: u64,
    pub heap_bytes: u64,
    pub stack_bytes: u64,
    pub store_size_bytes: u64,
    pub projection_size_chars: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceSyncProposal {
    pub proposal_id: String,
    pub device_id: String,
    pub summary_refs: Vec<String>,
    pub governed_by_full_runtime: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EdgeRecoveryFixture {
    pub fixture_id: String,
    pub failure_mode: String,
    pub expected_recovery_report: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchSurface {
    pub surface_id: String,
    pub report_api: String,
    pub private_raw_allowed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchApiMap {
    pub surfaces: Vec<WorkbenchSurface>,
    pub missing_report_apis: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulKernel2GateReport {
    pub release_gate_passed: bool,
    pub accepted_proposals: usize,
    pub rejected_or_deferred_proposals: usize,
    pub feedback_surfaces_applied: Vec<String>,
    pub blocked_reasons: Vec<String>,
    pub contract_only_capabilities: Vec<String>,
}

pub fn build_soul_kernel2_gate_report(
    proposals: Vec<SoulGrowthProposal>,
    regression_suite: SoulRegressionSuite,
    feedback: SoulFeedbackReport,
) -> SoulKernel2GateReport {
    let mut blocked_reasons = Vec::new();
    if regression_suite.privacy_leakage_count > 0 {
        blocked_reasons.push("privacy_leakage_detected".to_string());
    }
    if regression_suite.soul_regression_count > 0 {
        blocked_reasons.push("soul_regression_detected".to_string());
    }
    if !regression_suite.passed {
        blocked_reasons.push("soul_regression_suite_failed".to_string());
    }

    for proposal in &proposals {
        let validation = proposal.validate_contract();
        if !validation.accepted {
            blocked_reasons.push(validation.reason);
        }
    }

    let accepted_proposals = proposals
        .iter()
        .filter(|proposal| proposal.decision == SoulGrowthDecision::Accepted)
        .count();
    let rejected_or_deferred_proposals = proposals.len().saturating_sub(accepted_proposals);
    let mut feedback_surfaces_applied = Vec::new();
    if feedback.reply_applied {
        feedback_surfaces_applied.push("reply".to_string());
    }
    if feedback.initiative_applied {
        feedback_surfaces_applied.push("initiative".to_string());
    }
    if feedback.strategy_applied {
        feedback_surfaces_applied.push("strategy".to_string());
    }
    if feedback.evidence_refs.is_empty() && !feedback_surfaces_applied.is_empty() {
        blocked_reasons.push("soul_feedback_evidence_missing".to_string());
    }

    SoulKernel2GateReport {
        release_gate_passed: blocked_reasons.is_empty(),
        accepted_proposals,
        rejected_or_deferred_proposals,
        feedback_surfaces_applied,
        blocked_reasons,
        contract_only_capabilities: vec![
            "runtime_soul_mutation_apply".to_string(),
            "relationship_boundary_mutation_apply".to_string(),
        ],
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemporalMemoryGraphGateReport {
    pub nodes: usize,
    pub edges: usize,
    pub evidence_backlinks: usize,
    pub high_confidence_projection_allowed: bool,
    pub failures: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphEvidence {
    pub node_id: String,
    pub kind: MemoryGraphNodeKind,
    pub label: String,
    pub source_kind: String,
    pub source_id: String,
    pub fingerprint: String,
    pub observed_at: u64,
    pub supports: Vec<String>,
    pub supersedes: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemporalMemoryGraphBuildReport {
    pub nodes: Vec<MemoryGraphNode>,
    pub edges: Vec<MemoryGraphEdge>,
    pub backlinks: Vec<EvidenceBacklink>,
    pub compact_graph: CompactMemoryGraph,
    pub gate: TemporalMemoryGraphGateReport,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphWritePlan {
    pub operation: String,
    pub accepted: bool,
    pub node_count: usize,
    pub edge_count: usize,
    pub backlink_count: usize,
    pub revision_count: usize,
    pub nodes: Vec<MemoryGraphNode>,
    pub edges: Vec<MemoryGraphEdge>,
    pub backlinks: Vec<EvidenceBacklink>,
    pub gate_failures: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphRecallIndexDoc {
    pub owner: String,
    pub index_id: String,
    pub revision_id: String,
    pub memory_space_id: String,
    pub subject_id: String,
    pub source_anchor_id: String,
    pub neighbor_node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub evidence_backlink_keys: Vec<String>,
}

pub fn memory_graph_backlink_key(source_kind: &str, source_id: &str) -> String {
    stable_memory_graph_hash(&(&(source_kind.trim()), &(source_id.trim())))
}

pub fn build_temporal_memory_graph_from_evidence(
    evidence: Vec<MemoryGraphEvidence>,
) -> TemporalMemoryGraphBuildReport {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut backlinks = Vec::new();

    for item in evidence {
        nodes.push(MemoryGraphNode {
            node_id: item.node_id.clone(),
            kind: item.kind,
            label: item.label,
            evidence_refs: vec![item.source_id.clone()],
        });
        backlinks.push(EvidenceBacklink {
            source_kind: item.source_kind,
            source_id: item.source_id.clone(),
            fingerprint: item.fingerprint,
        });
        for supported in item.supports {
            edges.push(MemoryGraphEdge {
                edge_id: format!("edge:supports:{}:{supported}", item.node_id),
                kind: MemoryGraphEdgeKind::Supports,
                from_node_id: item.node_id.clone(),
                to_node_id: supported,
                validity: TemporalValidity {
                    valid_from: item.observed_at,
                    valid_until: None,
                    observed_at: item.observed_at,
                    superseded_by: None,
                },
                evidence_refs: vec![item.source_id.clone()],
            });
        }
        if let Some(superseded) = item.supersedes {
            edges.push(MemoryGraphEdge {
                edge_id: format!("edge:supersedes:{}:{superseded}", item.node_id),
                kind: MemoryGraphEdgeKind::Supersedes,
                from_node_id: item.node_id.clone(),
                to_node_id: superseded,
                validity: TemporalValidity {
                    valid_from: item.observed_at,
                    valid_until: None,
                    observed_at: item.observed_at,
                    superseded_by: None,
                },
                evidence_refs: vec![item.source_id],
            });
        }
    }

    build_temporal_memory_graph_from_parts(nodes, edges, backlinks)
}

pub fn build_temporal_memory_graph_from_parts(
    nodes: Vec<MemoryGraphNode>,
    edges: Vec<MemoryGraphEdge>,
    backlinks: Vec<EvidenceBacklink>,
) -> TemporalMemoryGraphBuildReport {
    let gate =
        build_temporal_memory_graph_gate_report(nodes.clone(), edges.clone(), backlinks.clone());
    let compact_graph = CompactMemoryGraph {
        memory_budget_bytes: estimate_compact_graph_budget_bytes(&nodes, &edges, &backlinks),
        nodes: nodes.iter().take(16).cloned().collect(),
        edges: edges.iter().take(16).cloned().collect(),
    };

    TemporalMemoryGraphBuildReport {
        nodes,
        edges,
        backlinks,
        compact_graph,
        gate,
    }
}

pub fn build_temporal_memory_graph_gate_report(
    nodes: Vec<MemoryGraphNode>,
    edges: Vec<MemoryGraphEdge>,
    evidence_backlinks: Vec<EvidenceBacklink>,
) -> TemporalMemoryGraphGateReport {
    let mut failures = Vec::new();
    if nodes.is_empty() {
        failures.push("memory_graph_nodes_empty".to_string());
    }
    for node in &nodes {
        let validation = node.validate_contract();
        if !validation.accepted {
            failures.push(format!("node:{}:{}", node.node_id, validation.reason));
        }
    }
    for edge in &edges {
        let validation = edge.validate_contract();
        if !validation.accepted {
            failures.push(format!("edge:{}:{}", edge.edge_id, validation.reason));
        }
    }
    for evidence_ref in nodes
        .iter()
        .flat_map(|node| node.evidence_refs.iter())
        .chain(edges.iter().flat_map(|edge| edge.evidence_refs.iter()))
    {
        if !evidence_backlinks
            .iter()
            .any(|backlink| backlink.source_id == *evidence_ref && !backlink.fingerprint.is_empty())
        {
            failures.push(format!("missing_evidence_backlink:{evidence_ref}"));
        }
    }
    for backlink in &evidence_backlinks {
        if contains_raw_soul_private_marker(&backlink.source_kind)
            || contains_raw_soul_private_marker(&backlink.source_id)
        {
            failures.push(format!(
                "evidence_backlink_raw_soul_private:{}",
                backlink.source_id
            ));
        }
    }

    failures.sort();
    failures.dedup();

    TemporalMemoryGraphGateReport {
        nodes: nodes.len(),
        edges: edges.len(),
        evidence_backlinks: evidence_backlinks.len(),
        high_confidence_projection_allowed: failures.is_empty(),
        failures,
    }
}

pub fn plan_temporal_memory_graph_write(
    operation: impl Into<String>,
    nodes: Vec<MemoryGraphNode>,
    edges: Vec<MemoryGraphEdge>,
    backlinks: Vec<EvidenceBacklink>,
) -> MemoryGraphWritePlan {
    let mut gate =
        build_temporal_memory_graph_gate_report(nodes.clone(), edges.clone(), backlinks.clone());
    for edge in &edges {
        if !nodes.iter().any(|node| node.node_id == edge.from_node_id) {
            gate.failures.push(format!(
                "edge:{}:memory_graph_edge_from_missing",
                edge.edge_id
            ));
        }
        if !nodes.iter().any(|node| node.node_id == edge.to_node_id) {
            gate.failures.push(format!(
                "edge:{}:memory_graph_edge_to_missing",
                edge.edge_id
            ));
        }
    }
    gate.failures.sort();
    gate.failures.dedup();
    gate.high_confidence_projection_allowed = gate.failures.is_empty();

    MemoryGraphWritePlan {
        operation: operation.into(),
        accepted: gate.high_confidence_projection_allowed,
        node_count: nodes.len(),
        edge_count: edges.len(),
        backlink_count: backlinks.len(),
        revision_count: 1,
        nodes,
        edges,
        backlinks,
        gate_failures: gate.failures,
    }
}

pub fn build_memory_graph_recall_index_docs(
    owner: impl Into<String>,
    revision_id: impl Into<String>,
    memory_space_id: impl Into<String>,
    subject_id: impl Into<String>,
    nodes: &[MemoryGraphNode],
    edges: &[MemoryGraphEdge],
    backlinks: &[EvidenceBacklink],
) -> Vec<MemoryGraphRecallIndexDoc> {
    let owner = owner.into();
    let revision_id = revision_id.into();
    let memory_space_id = memory_space_id.into();
    let subject_id = subject_id.into();

    nodes
        .iter()
        .map(|node| {
            let mut neighbor_node_ids = Vec::new();
            let mut edge_ids = Vec::new();
            for edge in edges
                .iter()
                .filter(|edge| graph_edge_allows_recall_expansion(edge.kind))
            {
                if edge.from_node_id == node.node_id {
                    push_unique(&mut neighbor_node_ids, edge.to_node_id.clone());
                    push_unique(&mut edge_ids, edge.edge_id.clone());
                } else if edge.to_node_id == node.node_id {
                    push_unique(&mut neighbor_node_ids, edge.from_node_id.clone());
                    push_unique(&mut edge_ids, edge.edge_id.clone());
                }
            }
            neighbor_node_ids.sort();
            edge_ids.sort();
            let mut evidence_refs = node.evidence_refs.clone();
            for edge in edges
                .iter()
                .filter(|edge| edge_ids.iter().any(|id| id == &edge.edge_id))
            {
                for evidence_ref in &edge.evidence_refs {
                    push_unique(&mut evidence_refs, evidence_ref.clone());
                }
            }
            evidence_refs.sort();
            let mut evidence_backlink_keys = backlinks
                .iter()
                .filter(|backlink| {
                    evidence_refs
                        .iter()
                        .any(|evidence_ref| evidence_ref == &backlink.source_id)
                })
                .map(|backlink| {
                    memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id)
                })
                .collect::<Vec<_>>();
            evidence_backlink_keys.sort();
            evidence_backlink_keys.dedup();
            MemoryGraphRecallIndexDoc {
                owner: owner.clone(),
                index_id: format!("graph_index:{}", node.node_id),
                revision_id: revision_id.clone(),
                memory_space_id: memory_space_id.clone(),
                subject_id: subject_id.clone(),
                source_anchor_id: node.node_id.clone(),
                neighbor_node_ids,
                edge_ids,
                evidence_refs,
                evidence_backlink_keys,
            }
        })
        .collect()
}

fn stable_memory_graph_hash<T: Hash>(value: &T) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn rerank_recall_with_temporal_graph(
    query: impl Into<String>,
    candidate_ids: Vec<String>,
    graph: &TemporalMemoryGraphBuildReport,
    expansion_budget: GraphRecallExpansionBudget,
) -> GraphRecallRerankReport {
    let query = query.into();
    let stale_false_positive_count = candidate_ids
        .iter()
        .filter(|candidate_id| graph_node_is_superseded(candidate_id, graph))
        .count() as u32;
    let expansion = expand_recall_candidates(&query, &candidate_ids, graph, expansion_budget);

    let mut score_breakdown = expansion
        .expanded_candidate_ids
        .iter()
        .map(|candidate_id| graph_recall_candidate_score(&query, candidate_id, graph))
        .collect::<Vec<_>>();
    score_breakdown.sort_by(|left, right| {
        right
            .total_score
            .cmp(&left.total_score)
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });
    let selected_ids = score_breakdown
        .iter()
        .map(|score| score.candidate_id.clone())
        .collect::<Vec<_>>();

    GraphRecallRerankReport {
        query,
        candidate_ids,
        expanded_candidate_ids: expansion.expanded_candidate_ids,
        graph_neighbor_ids: expansion.graph_neighbor_ids,
        selected_ids,
        score_breakdown,
        expansion_budget: expansion.budget_report,
        stale_false_positive_count,
    }
}

struct GraphRecallExpansion {
    expanded_candidate_ids: Vec<String>,
    graph_neighbor_ids: Vec<String>,
    budget_report: GraphRecallExpansionBudgetReport,
}

fn expand_recall_candidates(
    query: &str,
    candidate_ids: &[String],
    graph: &TemporalMemoryGraphBuildReport,
    budget: GraphRecallExpansionBudget,
) -> GraphRecallExpansion {
    let max_hops = budget.max_hops.min(2);
    let max_neighbors_per_candidate = budget.max_neighbors_per_candidate;
    let max_expanded_candidates = budget.max_expanded_candidates.max(candidate_ids.len());
    let mut expanded_candidate_ids = Vec::new();
    for candidate_id in candidate_ids {
        push_unique(&mut expanded_candidate_ids, candidate_id.clone());
    }

    let mut frontier = expanded_candidate_ids.clone();
    let mut hop1_candidate_count = 0usize;
    let mut hop2_candidate_count = 0usize;
    let mut truncated_candidate_count = 0usize;
    let mut blocked_reasons = Vec::new();
    let mut hop1_frontier = Vec::new();

    for hop in 1..=max_hops {
        let mut next_frontier = Vec::new();
        for source_id in &frontier {
            let neighbors = graph_expansion_neighbors(query, source_id, graph);
            if max_neighbors_per_candidate == 0 && !neighbors.is_empty() {
                truncated_candidate_count =
                    truncated_candidate_count.saturating_add(neighbors.len());
                push_unique(
                    &mut blocked_reasons,
                    "graph_expansion_neighbor_budget_exhausted".to_string(),
                );
                continue;
            }
            for (neighbor_index, neighbor_id) in neighbors.iter().enumerate() {
                if neighbor_index >= max_neighbors_per_candidate {
                    truncated_candidate_count = truncated_candidate_count
                        .saturating_add(neighbors.len().saturating_sub(neighbor_index));
                    push_unique(
                        &mut blocked_reasons,
                        "graph_expansion_neighbor_budget_exhausted".to_string(),
                    );
                    break;
                }
                if expanded_candidate_ids
                    .iter()
                    .any(|candidate| candidate == neighbor_id)
                {
                    continue;
                }
                if expanded_candidate_ids.len() >= max_expanded_candidates {
                    truncated_candidate_count = truncated_candidate_count.saturating_add(1);
                    push_unique(
                        &mut blocked_reasons,
                        "graph_expansion_candidate_budget_exhausted".to_string(),
                    );
                    continue;
                }
                expanded_candidate_ids.push(neighbor_id.clone());
                next_frontier.push(neighbor_id.clone());
            }
        }
        if hop == 1 {
            hop1_candidate_count = next_frontier.len();
            hop1_frontier = next_frontier.clone();
        } else if hop == 2 {
            hop2_candidate_count = next_frontier.len();
        }
        frontier = next_frontier;
    }

    if max_hops < 2
        && hop1_frontier.iter().any(|node_id| {
            graph_expansion_neighbors(query, node_id, graph)
                .into_iter()
                .any(|neighbor_id| {
                    !expanded_candidate_ids
                        .iter()
                        .any(|candidate| candidate == &neighbor_id)
                })
        })
    {
        push_unique(
            &mut blocked_reasons,
            "graph_expansion_second_hop_requires_budget".to_string(),
        );
    }

    let mut graph_neighbor_ids = expanded_candidate_ids
        .iter()
        .filter(|candidate_id| !candidate_ids.iter().any(|source| source == *candidate_id))
        .cloned()
        .collect::<Vec<_>>();
    graph_neighbor_ids.sort();
    graph_neighbor_ids.dedup();

    GraphRecallExpansion {
        budget_report: GraphRecallExpansionBudgetReport {
            max_hops,
            max_neighbors_per_candidate,
            max_expanded_candidates,
            source_candidate_count: candidate_ids.len(),
            expanded_candidate_count: expanded_candidate_ids.len(),
            hop1_candidate_count,
            hop2_candidate_count,
            truncated_candidate_count,
            profile_budget_applied: !blocked_reasons.is_empty(),
            blocked_reasons,
        },
        expanded_candidate_ids,
        graph_neighbor_ids,
    }
}

fn graph_expansion_neighbors(
    query: &str,
    node_id: &str,
    graph: &TemporalMemoryGraphBuildReport,
) -> Vec<String> {
    let mut neighbors = graph
        .edges
        .iter()
        .filter(|edge| graph_edge_allows_recall_expansion(edge.kind))
        .filter_map(|edge| {
            if edge.from_node_id == node_id {
                Some(edge.to_node_id.clone())
            } else if edge.to_node_id == node_id {
                Some(edge.from_node_id.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    neighbors.sort_by(|left, right| {
        let left_node = graph.nodes.iter().find(|node| node.node_id == *left);
        let right_node = graph.nodes.iter().find(|node| node.node_id == *right);
        graph_expansion_neighbor_score(query, right, graph, right_node)
            .cmp(&graph_expansion_neighbor_score(
                query, left, graph, left_node,
            ))
            .then_with(|| graph_node_rank(right, graph).cmp(&graph_node_rank(left, graph)))
            .then_with(|| left.cmp(right))
    });
    neighbors.dedup();
    neighbors
}

fn graph_expansion_neighbor_score(
    query: &str,
    node_id: &str,
    graph: &TemporalMemoryGraphBuildReport,
    node: Option<&MemoryGraphNode>,
) -> u32 {
    lexical_graph_score(query, node)
        .saturating_add(
            graph
                .edges
                .iter()
                .filter(|edge| edge.from_node_id == node_id || edge.to_node_id == node_id)
                .count() as u32
                * 10,
        )
        .saturating_add(
            node.map(|node| node.evidence_refs.len() as u32 * 5)
                .unwrap_or(0),
        )
}

fn graph_edge_allows_recall_expansion(kind: MemoryGraphEdgeKind) -> bool {
    matches!(
        kind,
        MemoryGraphEdgeKind::Supports
            | MemoryGraphEdgeKind::Supersedes
            | MemoryGraphEdgeKind::DerivedFrom
            | MemoryGraphEdgeKind::RelationshipBoundary
    )
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn graph_recall_candidate_score(
    query: &str,
    node_id: &str,
    graph: &TemporalMemoryGraphBuildReport,
) -> GraphRecallCandidateScore {
    let node = graph.nodes.iter().find(|node| node.node_id == node_id);
    let connected_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.from_node_id == node_id || edge.to_node_id == node_id)
        .collect::<Vec<_>>();
    let observed_rank = connected_edges
        .iter()
        .map(|edge| edge.validity.observed_at)
        .max()
        .or_else(|| {
            node.and_then(|node| {
                node.evidence_refs
                    .iter()
                    .filter_map(|evidence_ref| {
                        graph
                            .backlinks
                            .iter()
                            .find(|backlink| backlink.source_id == *evidence_ref)
                            .and_then(|_| Some(1))
                    })
                    .max()
            })
        })
        .unwrap_or(0);
    let lexical_score = lexical_graph_score(query, node);
    let graph_neighborhood_score = (connected_edges.len() as u32).saturating_mul(100);
    let temporal_validity_score = observed_rank.min(10_000) as u32;
    let evidence_quality_score = node
        .map(|node| node.evidence_refs.len() as u32)
        .unwrap_or(0)
        .saturating_mul(100)
        .saturating_add(
            node.map(|node| {
                node.evidence_refs
                    .iter()
                    .filter(|evidence_ref| {
                        graph.backlinks.iter().any(|backlink| {
                            backlink.source_id == **evidence_ref && !backlink.fingerprint.is_empty()
                        })
                    })
                    .count() as u32
            })
            .unwrap_or(0)
            .saturating_mul(100),
        );
    let source_authority_score = node
        .map(|node| {
            node.evidence_refs
                .iter()
                .filter_map(|evidence_ref| {
                    graph
                        .backlinks
                        .iter()
                        .find(|backlink| backlink.source_id == *evidence_ref)
                })
                .map(|backlink| source_authority_score(&backlink.source_kind))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let privacy_profile_eligibility_score = node
        .map(|node| node.validate_contract().accepted)
        .unwrap_or(false)
        .then_some(100)
        .unwrap_or(0);
    let supersedes_bonus =
        if graph.edges.iter().any(|edge| {
            edge.kind == MemoryGraphEdgeKind::Supersedes && edge.from_node_id == node_id
        }) {
            1_000
        } else {
            0
        };
    let stale_superseded_penalty = if graph_node_is_superseded(node_id, graph) {
        10_000
    } else {
        0
    };
    let total_score = lexical_score
        .saturating_add(graph_neighborhood_score)
        .saturating_add(temporal_validity_score)
        .saturating_add(evidence_quality_score)
        .saturating_add(source_authority_score)
        .saturating_add(privacy_profile_eligibility_score)
        .saturating_add(supersedes_bonus)
        .saturating_sub(stale_superseded_penalty);

    GraphRecallCandidateScore {
        candidate_id: node_id.to_string(),
        lexical_score,
        graph_neighborhood_score,
        temporal_validity_score,
        evidence_quality_score,
        source_authority_score,
        privacy_profile_eligibility_score,
        stale_superseded_penalty,
        total_score,
    }
}

fn lexical_graph_score(query: &str, node: Option<&MemoryGraphNode>) -> u32 {
    let Some(node) = node else {
        return 0;
    };
    let haystack = format!("{} {}", node.node_id, node.label).to_lowercase();
    query
        .split_whitespace()
        .filter(|term| haystack.contains(&term.to_lowercase()))
        .count() as u32
        * 25
}

fn source_authority_score(source_kind: &str) -> u32 {
    match source_kind {
        "conversation_transcript"
        | "turn_ledger"
        | "archive"
        | "accepted_long_term_revision"
        | "procedural_memory"
        | "runtime_skill"
        | "explicit_sdk_write" => 100,
        other if other.trim().is_empty() => 0,
        _ => 50,
    }
}

fn graph_node_rank(node_id: &str, graph: &TemporalMemoryGraphBuildReport) -> u64 {
    graph
        .edges
        .iter()
        .filter(|edge| edge.from_node_id == node_id || edge.to_node_id == node_id)
        .map(|edge| edge.validity.observed_at)
        .max()
        .unwrap_or(0)
}

fn graph_node_is_superseded(node_id: &str, graph: &TemporalMemoryGraphBuildReport) -> bool {
    graph
        .edges
        .iter()
        .any(|edge| edge.kind == MemoryGraphEdgeKind::Supersedes && edge.to_node_id == node_id)
}

fn estimate_compact_graph_budget_bytes(
    nodes: &[MemoryGraphNode],
    edges: &[MemoryGraphEdge],
    backlinks: &[EvidenceBacklink],
) -> u64 {
    let node_bytes = nodes
        .iter()
        .map(|node| {
            node.node_id.len()
                + node.label.len()
                + node.evidence_refs.iter().map(String::len).sum::<usize>()
        })
        .sum::<usize>();
    let edge_bytes = edges
        .iter()
        .map(|edge| edge.edge_id.len() + edge.from_node_id.len() + edge.to_node_id.len())
        .sum::<usize>();
    let backlink_bytes = backlinks
        .iter()
        .map(|backlink| {
            backlink.source_kind.len() + backlink.source_id.len() + backlink.fingerprint.len()
        })
        .sum::<usize>();
    (node_bytes + edge_bytes + backlink_bytes) as u64
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProceduralEvolutionGateReport {
    pub passed: bool,
    pub active_records: usize,
    pub requires_runtime_write_governance: bool,
    pub executor_authorized: bool,
    pub blocked_reasons: Vec<String>,
    pub contract_only_capabilities: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProceduralMemoryPromotionPolicy {
    pub min_quality_score: u8,
    pub min_evidence_refs: usize,
    pub require_repeated_evidence: bool,
}

impl Default for ProceduralMemoryPromotionPolicy {
    fn default() -> Self {
        Self {
            min_quality_score: 70,
            min_evidence_refs: 2,
            require_repeated_evidence: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProceduralMemoryPromotionInput {
    pub task_id: String,
    pub trigger: String,
    pub procedure: String,
    pub constraints: Vec<String>,
    pub failure_modes: Vec<String>,
    pub counterfactual_fix: String,
    pub evidence_refs: Vec<String>,
    pub quality_score: u8,
    pub repeated_evidence_count: usize,
    pub capability_affinity: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProceduralMemoryPromotionReport {
    pub promoted: bool,
    pub record: Option<ProceduralMemoryRecordV2>,
    pub genome: ProcedureGenome,
    pub task_experience: TaskExperienceToProcedure,
    pub evolution: SkillEvolutionReport,
    pub gate: ProceduralEvolutionGateReport,
    pub blocked_reasons: Vec<String>,
}

pub fn promote_task_experience_to_procedure(
    input: ProceduralMemoryPromotionInput,
    policy: ProceduralMemoryPromotionPolicy,
) -> ProceduralMemoryPromotionReport {
    let mut blocked_reasons = Vec::new();
    if input.evidence_refs.len() < policy.min_evidence_refs {
        blocked_reasons.push("procedural_evidence_below_threshold".to_string());
    }
    if policy.require_repeated_evidence && input.repeated_evidence_count < policy.min_evidence_refs
    {
        blocked_reasons.push("procedural_repeated_evidence_below_threshold".to_string());
    }
    if input.quality_score < policy.min_quality_score {
        blocked_reasons.push("procedural_quality_below_threshold".to_string());
    }
    if input.failure_modes.is_empty() {
        blocked_reasons.push("procedural_failure_modes_empty".to_string());
    }

    let genome = ProcedureGenome {
        goal: input.trigger.clone(),
        prerequisites: input.constraints.clone(),
        steps: vec![input.procedure.clone()],
        forbidden_zones: input.constraints.clone(),
        failure_review: input.failure_modes.clone(),
        revision_sources: input.evidence_refs.clone(),
    };
    let proposal_id = format!("procedure:{}", input.task_id);
    let task_experience = TaskExperienceToProcedure {
        task_id: input.task_id.clone(),
        proposal_id: proposal_id.clone(),
        evidence_refs: input.evidence_refs.clone(),
        submitted_to_runtime_write: blocked_reasons.is_empty(),
    };
    let record = blocked_reasons
        .is_empty()
        .then(|| ProceduralMemoryRecordV2 {
            trigger: input.trigger,
            procedure: input.procedure,
            constraints: input.constraints,
            failure_modes: input.failure_modes,
            counterfactual_fix: input.counterfactual_fix,
            evidence_refs: input.evidence_refs,
            quality_score: input.quality_score,
            lineage: vec![input.task_id],
            capability_affinity: input.capability_affinity,
            projection_policy: "method_hint_only".to_string(),
        });
    let evolution = if record.is_some() {
        SkillEvolutionReport {
            added: vec![proposal_id],
            reasons: vec!["promotion_policy_passed".to_string()],
            ..SkillEvolutionReport::default()
        }
    } else {
        SkillEvolutionReport {
            rejected: vec![proposal_id],
            reasons: blocked_reasons.clone(),
            ..SkillEvolutionReport::default()
        }
    };
    let gate = build_procedural_evolution_gate_report(
        record.iter().cloned().collect(),
        genome.clone(),
        evolution.clone(),
    );

    ProceduralMemoryPromotionReport {
        promoted: record.is_some(),
        record,
        genome,
        task_experience,
        evolution,
        gate,
        blocked_reasons,
    }
}

pub fn build_procedural_evolution_gate_report(
    records: Vec<ProceduralMemoryRecordV2>,
    genome: ProcedureGenome,
    evolution: SkillEvolutionReport,
) -> ProceduralEvolutionGateReport {
    let mut blocked_reasons = Vec::new();
    for record in &records {
        if record.evidence_refs.is_empty() {
            blocked_reasons.push("procedural_record_evidence_missing".to_string());
        }
        if record.quality_score < 50 {
            blocked_reasons.push("procedural_record_quality_below_threshold".to_string());
        }
    }
    if genome.steps.is_empty() {
        blocked_reasons.push("procedure_genome_steps_empty".to_string());
    }
    if evolution.reasons.is_empty() {
        blocked_reasons.push("skill_evolution_reason_missing".to_string());
    }

    ProceduralEvolutionGateReport {
        passed: blocked_reasons.is_empty(),
        active_records: records.len(),
        requires_runtime_write_governance: true,
        executor_authorized: false,
        blocked_reasons,
        contract_only_capabilities: vec![
            "automatic_procedure_merge_apply".to_string(),
            "procedure_supersede_mutation_apply".to_string(),
        ],
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryAutopilotGateReport {
    pub passed: bool,
    pub profile: Option<ProfileId>,
    pub planned_jobs: usize,
    pub deferred_jobs: usize,
    pub hygiene_changes: usize,
    pub mutation_requires_write_governance: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryAutopilotInput {
    pub profile: ProfileId,
    pub pressure: String,
    pub recovery_safe_mode: bool,
    pub pending_stale_items: usize,
    pub pending_conflicts: usize,
    pub privacy_risk_count: usize,
}

pub fn plan_memory_autopilot_for_profile(input: MemoryAutopilotInput) -> MemoryAutopilotPlan {
    let constrained = matches!(
        input.profile,
        ProfileId::EspEmbeddedSdk | ProfileId::EspStandaloneMemory
    ) || input.recovery_safe_mode
        || input.pressure == "critical";
    let mut jobs = vec!["compact_hygiene_scan".to_string()];
    let mut deferred_jobs = Vec::new();
    if input.pending_stale_items > 0 && !constrained {
        jobs.push("stale_memory_review".to_string());
    } else if input.pending_stale_items > 0 {
        deferred_jobs.push("stale_memory_review".to_string());
    }
    if input.pending_conflicts > 0 && !constrained {
        jobs.push("deep_consolidation".to_string());
    } else if input.pending_conflicts > 0 {
        deferred_jobs.push("deep_consolidation".to_string());
    }
    if input.privacy_risk_count > 0 {
        deferred_jobs.push("privacy_risk_review".to_string());
    }
    MemoryAutopilotPlan {
        profile: input.profile,
        jobs,
        deferred_jobs,
        mutation_policy: "proposal_only".to_string(),
    }
}

pub fn build_memory_autopilot_gate_report(
    plan: MemoryAutopilotPlan,
    hygiene_diff: MemoryHygieneDiff,
    lifecycle_event_written: bool,
) -> MemoryAutopilotGateReport {
    let mut blocked_reasons = Vec::new();
    let mutation_requires_write_governance = plan.mutation_policy == "proposal_only"
        || plan.mutation_policy == "write_governance_required";
    if !mutation_requires_write_governance {
        blocked_reasons.push("autopilot_mutation_policy_not_governed".to_string());
    }
    if !lifecycle_event_written {
        blocked_reasons.push("autopilot_lifecycle_event_missing".to_string());
    }
    let hygiene_changes = hygiene_diff.deduplicated.len()
        + hygiene_diff.merged.len()
        + hygiene_diff.stale.len()
        + hygiene_diff.conflicts.len()
        + hygiene_diff.privacy_risks.len();

    MemoryAutopilotGateReport {
        passed: blocked_reasons.is_empty(),
        profile: Some(plan.profile),
        planned_jobs: plan.jobs.len(),
        deferred_jobs: plan.deferred_jobs.len(),
        hygiene_changes,
        mutation_requires_write_governance,
        blocked_reasons,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyVaultGateReport {
    pub passed: bool,
    pub raw_private_leakage_blocked: bool,
    pub encrypted_envelope_present: bool,
    pub preflight_passed: bool,
    pub blocked_reasons: Vec<String>,
}

pub fn build_privacy_vault_gate_report(
    manifest: VaultManifest,
    envelope: EncryptedSnapshotEnvelope,
    redaction: PrivateMaterialRedactionReport,
    preflight: VaultMigrationPreflight,
) -> PrivacyVaultGateReport {
    let mut blocked_reasons = Vec::new();
    if manifest.snapshot_fingerprint != envelope.snapshot_fingerprint {
        blocked_reasons.push("vault_snapshot_fingerprint_mismatch".to_string());
    }
    let encrypted_envelope_present = !envelope.envelope_id.is_empty()
        && !envelope.cipher.is_empty()
        && !envelope.key_ref.is_empty();
    if !encrypted_envelope_present {
        blocked_reasons.push("vault_encrypted_envelope_missing".to_string());
    }
    let raw_private_leakage_blocked = redaction.raw_private_leak_count == 0;
    if !raw_private_leakage_blocked {
        blocked_reasons.push("vault_raw_private_leakage_detected".to_string());
    }
    if !preflight.passed {
        blocked_reasons.push("vault_migration_preflight_failed".to_string());
    }

    PrivacyVaultGateReport {
        passed: blocked_reasons.is_empty(),
        raw_private_leakage_blocked,
        encrypted_envelope_present,
        preflight_passed: preflight.passed,
        blocked_reasons,
    }
}

pub fn build_vault_migration_preflight(
    manifest: VaultManifest,
    target_profile: ProfileId,
    redaction: PrivateMaterialRedactionReport,
    source_schema_id: impl AsRef<str>,
    target_schema_id: impl AsRef<str>,
) -> VaultMigrationPreflight {
    let schema_allowed = source_schema_id.as_ref() == target_schema_id.as_ref();
    let capability_allowed = !matches!(target_profile, ProfileId::EspEmbeddedSdk);
    let privacy_allowed = redaction.raw_private_leak_count == 0;
    let lineage_allowed = !manifest.snapshot_fingerprint.is_empty()
        && !manifest.event_fingerprint.is_empty()
        && !manifest.privacy_policy_fingerprint.is_empty();
    VaultMigrationPreflight {
        source_profile: manifest.profile,
        target_profile,
        snapshot_fingerprint: manifest.snapshot_fingerprint,
        event_fingerprint: manifest.event_fingerprint,
        privacy_policy_fingerprint: manifest.privacy_policy_fingerprint,
        source_schema_id: source_schema_id.as_ref().to_string(),
        target_schema_id: target_schema_id.as_ref().to_string(),
        schema_allowed,
        capability_allowed,
        privacy_allowed,
        lineage_allowed,
        passed: schema_allowed && capability_allowed && privacy_allowed && lineage_allowed,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EdgeMemoryApplianceGateReport {
    pub passed: bool,
    pub profile: Option<ProfileId>,
    pub compact_graph_items: usize,
    pub memory_budget_bytes: u64,
    pub heavy_feature_violations: Vec<String>,
    pub recovery_fixture_count: usize,
}

pub fn compile_edge_memory_budget_report(
    profile: ProfileId,
    binary_size_bytes: u64,
    heap_bytes: u64,
    stack_bytes: u64,
    store_size_bytes: u64,
    projection_size_chars: usize,
) -> EdgeMemoryBudgetReport {
    EdgeMemoryBudgetReport {
        profile,
        binary_size_bytes,
        heap_bytes,
        stack_bytes,
        store_size_bytes,
        projection_size_chars,
    }
}

pub fn build_edge_memory_appliance_gate_report(
    compact_soul: CompactSoulProfile,
    compact_graph: CompactGraphIndex,
    budget: EdgeMemoryBudgetReport,
    recovery_fixtures: Vec<EdgeRecoveryFixture>,
    compiled_heavy_features: Vec<String>,
) -> EdgeMemoryApplianceGateReport {
    let mut heavy_feature_violations = Vec::new();
    if matches!(
        budget.profile,
        ProfileId::EspStandaloneMemory | ProfileId::EspEmbeddedSdk
    ) {
        for feature in compiled_heavy_features {
            if matches!(
                feature.as_str(),
                "sqlite" | "heavy_embedding" | "full_replay" | "mcp_server"
            ) {
                heavy_feature_violations.push(feature);
            }
        }
    }
    if compact_soul.self_core.trim().is_empty() {
        heavy_feature_violations.push("compact_soul_profile_empty".to_string());
    }
    if compact_graph.memory_budget_bytes > budget.store_size_bytes {
        heavy_feature_violations.push("compact_graph_exceeds_store_budget".to_string());
    }

    EdgeMemoryApplianceGateReport {
        passed: heavy_feature_violations.is_empty() && !recovery_fixtures.is_empty(),
        profile: Some(budget.profile),
        compact_graph_items: compact_graph.node_ids.len() + compact_graph.edge_ids.len(),
        memory_budget_bytes: budget.store_size_bytes,
        heavy_feature_violations,
        recovery_fixture_count: recovery_fixtures.len(),
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkbenchGateReport {
    pub passed: bool,
    pub surfaces: usize,
    pub private_raw_surface_count: usize,
    pub missing_report_apis: usize,
}

pub fn build_workbench_gate_report(api_map: WorkbenchApiMap) -> WorkbenchGateReport {
    let private_raw_surface_count = api_map
        .surfaces
        .iter()
        .filter(|surface| surface.private_raw_allowed)
        .count();
    let missing_surface_apis = api_map
        .surfaces
        .iter()
        .filter(|surface| surface.report_api.trim().is_empty())
        .count();
    let missing_report_apis = api_map.missing_report_apis.len() + missing_surface_apis;
    WorkbenchGateReport {
        passed: private_raw_surface_count == 0 && missing_report_apis == 0,
        surfaces: api_map.surfaces.len(),
        private_raw_surface_count,
        missing_report_apis,
    }
}

fn validate_nonempty(value: &str, reason: &'static str) -> Option<NextGenContractValidation> {
    value.trim().is_empty().then(|| rejected(reason))
}

fn validate_vec<T>(value: &[T], reason: &'static str) -> Option<NextGenContractValidation> {
    value.is_empty().then(|| rejected(reason))
}

fn contains_raw_soul_private_marker(value: &str) -> bool {
    let normalized = normalize_private_marker(value);
    normalized.contains("inner_life")
        || normalized.contains("private_garden")
        || normalized.contains("soul_private")
        || normalized.contains("self_authored_core")
        || normalized.contains("self_continuity")
        || normalized.contains("growth_revision_ledger")
}

fn normalize_private_marker(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut previous_was_separator = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
            previous_was_separator = false;
        } else if !previous_was_separator {
            normalized.push('_');
            previous_was_separator = true;
        }
    }
    normalized.trim_matches('_').to_string()
}

fn accepted() -> NextGenContractValidation {
    NextGenContractValidation {
        accepted: true,
        reason: "accepted".to_string(),
    }
}

fn rejected(reason: &'static str) -> NextGenContractValidation {
    NextGenContractValidation {
        accepted: false,
        reason: reason.to_string(),
    }
}
