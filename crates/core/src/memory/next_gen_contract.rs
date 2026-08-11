use crate::feature_gate::ProfileId;
use crate::skills::RuntimeSkillCreationRef;
use crate::util::{collect_retrieval_terms, normalize_retrieval_text};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use super::governed_evidence_document::{
    scoped_governed_evidence_document_key, validate_governed_evidence_document,
    GovernedEvidenceDocument,
};
use super::governed_post_image::{
    revision_is_exact_successor, GovernedDocumentImage, GovernedPostImageValidation,
};
use super::long_term_version::LongTermMemoryVersionMaterialImage;
use super::recall_anchor::canonical_recall_evidence_group;
use super::CoreRevisionConflictClass;
use super::{
    governed_memory_recall_candidate_id, CoreRevisionLedger, CoreRevisionOutcome,
    CoreRevisionRecord, CoreRevisionRecordChange, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    MemoryPrivacyClass, RelationshipConstitutionAudit, SelfAuthoredCore, TurnSoulFeedbackLedger,
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

pub const MEMORY_GRAPH_SCHEMA_VERSION: u32 = 4;
pub const MEMORY_GRAPH_NODE_NAMESPACE: &str = "memory_graph_nodes";
pub const MEMORY_GRAPH_EDGE_NAMESPACE: &str = "memory_graph_edges";
pub const MEMORY_GRAPH_BACKLINK_NAMESPACE: &str = "memory_graph_backlinks";
pub const MEMORY_GRAPH_INDEX_NAMESPACE: &str = "memory_graph_indexes";
pub const MEMORY_GRAPH_REVISION_NAMESPACE: &str = "memory_graph_revisions";
pub const MEMORY_GRAPH_MANIFEST_NAMESPACE: &str = "memory_graph_manifests";
pub const MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE: &str = "memory_graph_node_memberships";
pub const MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE: &str = "memory_graph_edge_memberships";
pub const MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE: &str = "memory_graph_backlink_memberships";

const MEMORY_GRAPH_SCOPE_MANIFEST_LOGICAL_KEY: &str = "graph_scope_manifest";
const MEMORY_GRAPH_REVISION_LOGICAL_KEY: &str = "graph_revision";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemoryGraphDependencyRef {
    pub storage_key: String,
    pub dependency_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphScopeManifest {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub scope_digest: String,
    pub manifest_generation: u64,
    pub graph_revision: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub backlink_count: usize,
    pub index_count: usize,
    pub node_memberships: Vec<MemoryGraphDependencyRef>,
    pub edge_memberships: Vec<MemoryGraphDependencyRef>,
    pub backlink_memberships: Vec<MemoryGraphDependencyRef>,
    pub recall_indexes: Vec<MemoryGraphDependencyRef>,
    pub revision: MemoryGraphDependencyRef,
    pub dependency_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphNodeMembership {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub scope_digest: String,
    pub manifest_generation: u64,
    pub graph_revision: String,
    pub membership_key: String,
    pub node_id: String,
    pub document_key: String,
    pub document_digest: String,
    pub owner_ref: GovernedMemoryOwnerRef,
    pub owner_revision: u64,
    pub index_key: String,
    pub backlink_membership_keys: Vec<String>,
    pub dependency_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphEdgeMembership {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub scope_digest: String,
    pub manifest_generation: u64,
    pub graph_revision: String,
    pub membership_key: String,
    pub edge_id: String,
    pub document_key: String,
    pub document_digest: String,
    pub from_node_membership_key: String,
    pub to_node_membership_key: String,
    pub backlink_membership_keys: Vec<String>,
    pub dependency_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphBacklinkMembership {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub scope_digest: String,
    pub manifest_generation: u64,
    pub graph_revision: String,
    pub membership_key: String,
    pub backlink_key: String,
    pub document_key: String,
    pub document_digest: String,
    pub node_membership_keys: Vec<String>,
    pub edge_membership_keys: Vec<String>,
    pub index_keys: Vec<String>,
    pub dependency_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphRevisionDoc {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub scope_digest: String,
    pub manifest_generation: u64,
    pub graph_revision: String,
    pub revision_key: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub backlink_count: usize,
    pub index_count: usize,
    pub dependency_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphOwnerBinding {
    pub node_id: String,
    pub owner_ref: GovernedMemoryOwnerRef,
    pub owner_revision: u64,
    pub visible: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphPersistencePlan {
    pub accepted: bool,
    pub failures: Vec<String>,
    pub scope_manifest: Option<MemoryGraphScopeManifest>,
    pub revision: Option<MemoryGraphRevisionDoc>,
    pub node_memberships: Vec<MemoryGraphNodeMembership>,
    pub edge_memberships: Vec<MemoryGraphEdgeMembership>,
    pub backlink_memberships: Vec<MemoryGraphBacklinkMembership>,
    pub recall_indexes: Vec<MemoryGraphRecallIndexDoc>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGraphReadChainValidation {
    pub verified: bool,
    pub failures: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGraphPostImageClosure {
    pub memory_space_id: String,
    pub long_term_owner_id: String,
    pub mounted_subject_id: String,
    pub allow_missing_before_owners: bool,
    pub validate_transition_successors: bool,
    pub long_term_owners: Vec<LongTermMemoryVersionMaterialImage>,
    pub evidence_document_owners: Vec<GovernedDocumentImage<GovernedEvidenceDocument>>,
    pub manifest: GovernedDocumentImage<MemoryGraphScopeManifest>,
    pub revision: GovernedDocumentImage<MemoryGraphRevisionDoc>,
    pub node_memberships: Vec<GovernedDocumentImage<MemoryGraphNodeMembership>>,
    pub edge_memberships: Vec<GovernedDocumentImage<MemoryGraphEdgeMembership>>,
    pub backlink_memberships: Vec<GovernedDocumentImage<MemoryGraphBacklinkMembership>>,
    pub indexes: Vec<GovernedDocumentImage<MemoryGraphRecallIndexDoc>>,
    pub nodes: Vec<GovernedDocumentImage<MemoryGraphNode>>,
    pub edges: Vec<GovernedDocumentImage<MemoryGraphEdge>>,
    pub backlinks: Vec<GovernedDocumentImage<EvidenceBacklink>>,
}

pub fn validate_memory_graph_post_image(
    closure: &MemoryGraphPostImageClosure,
) -> GovernedPostImageValidation {
    let memory_space_id = closure.memory_space_id.trim();
    let long_term_owner_id = closure.long_term_owner_id.trim();
    let mounted_subject_id = closure.mounted_subject_id.trim();
    let mut failures = Vec::new();
    if memory_space_id.is_empty() || long_term_owner_id.is_empty() || mounted_subject_id.is_empty()
    {
        failures.push("memory_graph_post_image_scope_invalid".to_string());
        return GovernedPostImageValidation::from_failures(failures);
    }

    if closure.manifest.before.is_some() {
        let before_validation =
            validate_memory_graph_post_image(&memory_graph_before_image(closure));
        let missing_before_owner_ids = graph_missing_before_owner_ids(closure);
        for failure in before_validation.failures {
            if closure.allow_missing_before_owners
                && !missing_before_owner_ids.is_empty()
                && failure == "memory_graph_persistent_node_owner_missing"
            {
                continue;
            }
            failures.push(format!("memory_graph_before_image_invalid:{failure}"));
        }
    }

    let mut owner_bindings = Vec::new();
    let mut owner_refs = BTreeSet::new();
    let mut memberships_by_owner =
        BTreeMap::<GovernedMemoryOwnerRef, Vec<&MemoryGraphNodeMembership>>::new();
    for membership in closure
        .node_memberships
        .iter()
        .filter_map(|image| image.after.as_ref())
    {
        memberships_by_owner
            .entry(membership.owner_ref.clone())
            .or_default()
            .push(membership);
    }
    for image in &closure.long_term_owners {
        if image.after.is_none() && image.before.is_none() {
            continue;
        }
        if !image.has_exact_physical_closure(memory_space_id, long_term_owner_id) {
            failures.push("memory_graph_owner_physical_key_drift".to_string());
        }
        if closure.validate_transition_successors
            && image.before != image.after
            && !revision_is_exact_successor(
                image.before.as_ref().map(|owner| owner.owner_revision),
                image.after.as_ref().map(|owner| owner.owner_revision),
            )
        {
            failures.push("memory_graph_owner_revision_successor_drift".to_string());
        }
        if let Some(owner) = image.after.as_ref() {
            append_graph_owner_bindings(
                owner.owner_ref.clone(),
                owner.owner_revision,
                owner.privacy_class.projection_content_allowed(),
                &memberships_by_owner,
                &mut owner_refs,
                &mut owner_bindings,
                &mut failures,
            );
        }
    }
    for image in &closure.evidence_document_owners {
        let Some(observed_owner) = image.after.as_ref().or(image.before.as_ref()) else {
            continue;
        };
        let logical_id = observed_owner.document_id.as_str();
        if scoped_governed_evidence_document_key(memory_space_id, logical_id)
            .map(|expected| {
                image.physical_key != expected || observed_owner.physical_key != expected
            })
            .unwrap_or(true)
        {
            failures.push("memory_graph_owner_physical_key_drift".to_string());
        }
        if closure.validate_transition_successors
            && image.before != image.after
            && !revision_is_exact_successor(
                image.before.as_ref().map(|owner| owner.owner_revision),
                image.after.as_ref().map(|owner| owner.owner_revision),
            )
        {
            failures.push("memory_graph_owner_revision_successor_drift".to_string());
        }
        let Some(owner) = image.after.as_ref() else {
            continue;
        };
        if validate_governed_evidence_document(owner).is_err()
            || owner.memory_space_id != memory_space_id
            || owner.mounted_subject_id != mounted_subject_id
        {
            failures.push("memory_graph_owner_scope_or_schema_drift".to_string());
        }
        let owner_ref = GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::EvidenceDocument,
            owner.document_id.clone(),
        );
        if !memberships_by_owner.contains_key(&owner_ref) {
            failures.push("memory_graph_evidence_owner_membership_missing".to_string());
        }
        append_graph_owner_bindings(
            owner_ref,
            owner.owner_revision,
            owner.privacy.projection_content_allowed(),
            &memberships_by_owner,
            &mut owner_refs,
            &mut owner_bindings,
            &mut failures,
        );
    }

    for owner_ref in memberships_by_owner.keys() {
        if !owner_refs.contains(owner_ref) {
            failures.push("memory_graph_persistent_node_owner_missing".to_string());
        }
    }

    let Some(manifest) = closure.manifest.after.as_ref() else {
        validate_memory_graph_deleted_post_image(closure, &mut failures);
        return GovernedPostImageValidation::from_failures(failures);
    };
    if closure.manifest.physical_key
        != memory_graph_scope_manifest_key(memory_space_id, mounted_subject_id)
    {
        failures.push("memory_graph_manifest_physical_key_drift".to_string());
    }
    if closure.validate_transition_successors
        && closure.manifest.before != closure.manifest.after
        && !revision_is_exact_successor(
            closure
                .manifest
                .before
                .as_ref()
                .map(|before| before.manifest_generation),
            Some(manifest.manifest_generation),
        )
    {
        failures.push("memory_graph_manifest_generation_successor_drift".to_string());
    }
    validate_graph_effect_closure(closure, manifest, &mut failures);

    let nodes = collect_graph_post_image_docs(
        &closure.nodes,
        |node| node.node_id.as_str(),
        "memory_graph_node_duplicate",
        &mut failures,
    );
    let edges = collect_graph_post_image_docs(
        &closure.edges,
        |edge| edge.edge_id.as_str(),
        "memory_graph_edge_duplicate",
        &mut failures,
    );
    let mut backlink_ids = BTreeSet::new();
    let mut backlinks = Vec::new();
    for image in &closure.backlinks {
        let Some(backlink) = image.after.as_ref() else {
            continue;
        };
        if !backlink_ids.insert(memory_graph_backlink_key(
            &backlink.source_kind,
            &backlink.source_id,
        )) {
            failures.push("memory_graph_backlink_duplicate".to_string());
        }
        backlinks.push(backlink.clone());
    }
    let expected = build_memory_graph_persistence_plan(
        memory_space_id,
        mounted_subject_id,
        manifest.manifest_generation,
        nodes,
        edges,
        backlinks,
        owner_bindings,
    );
    if !expected.accepted {
        failures.extend(expected.failures.clone());
        return GovernedPostImageValidation::from_failures(failures);
    }

    if expected.scope_manifest.as_ref() != Some(manifest) {
        failures.push("memory_graph_manifest_exact_dependency_closure_drift".to_string());
    }
    match closure.revision.after.as_ref() {
        Some(revision) if expected.revision.as_ref() == Some(revision) => {
            if closure.revision.physical_key != revision.revision_key {
                failures.push("memory_graph_revision_physical_key_drift".to_string());
            }
        }
        _ => failures.push("memory_graph_revision_exact_closure_drift".to_string()),
    }

    validate_graph_typed_images(
        &closure.node_memberships,
        &expected.node_memberships,
        |value| value.membership_key.as_str(),
        "memory_graph_node_membership_physical_key_drift",
        "memory_graph_node_membership_exact_closure_drift",
        &mut failures,
    );
    validate_graph_typed_images(
        &closure.edge_memberships,
        &expected.edge_memberships,
        |value| value.membership_key.as_str(),
        "memory_graph_edge_membership_physical_key_drift",
        "memory_graph_edge_membership_exact_closure_drift",
        &mut failures,
    );
    validate_graph_typed_images(
        &closure.backlink_memberships,
        &expected.backlink_memberships,
        |value| value.membership_key.as_str(),
        "memory_graph_backlink_membership_physical_key_drift",
        "memory_graph_backlink_membership_exact_closure_drift",
        &mut failures,
    );
    validate_graph_typed_images(
        &closure.indexes,
        &expected.recall_indexes,
        |value| value.index_key.as_str(),
        "memory_graph_index_physical_key_drift",
        "memory_graph_index_exact_closure_drift",
        &mut failures,
    );

    let expected_node_keys = expected
        .node_memberships
        .iter()
        .map(|membership| {
            (
                membership.node_id.as_str(),
                membership.document_key.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    validate_graph_document_keys(
        &closure.nodes,
        |node| node.node_id.as_str(),
        &expected_node_keys,
        |node| {
            scoped_memory_graph_storage_key(
                memory_space_id,
                mounted_subject_id,
                &format!("node:{}", node.node_id),
            )
        },
        "memory_graph_node_document_physical_key_drift",
        &mut failures,
    );
    let expected_edge_keys = expected
        .edge_memberships
        .iter()
        .map(|membership| {
            (
                membership.edge_id.as_str(),
                membership.document_key.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    validate_graph_document_keys(
        &closure.edges,
        |edge| edge.edge_id.as_str(),
        &expected_edge_keys,
        |edge| {
            scoped_memory_graph_storage_key(
                memory_space_id,
                mounted_subject_id,
                &format!("edge:{}", edge.edge_id),
            )
        },
        "memory_graph_edge_document_physical_key_drift",
        &mut failures,
    );
    let expected_backlink_keys = expected
        .backlink_memberships
        .iter()
        .map(|membership| {
            (
                membership.backlink_key.as_str(),
                membership.document_key.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for image in &closure.backlinks {
        let Some(backlink) = image.after.as_ref().or(image.before.as_ref()) else {
            failures.push("memory_graph_backlink_document_physical_key_drift".to_string());
            continue;
        };
        let logical_key = memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id);
        let canonical_key = scoped_memory_graph_storage_key(
            memory_space_id,
            mounted_subject_id,
            &format!("backlink:{logical_key}"),
        );
        let after_membership_drift = image.after.as_ref().is_some()
            && expected_backlink_keys.get(logical_key.as_str()).copied()
                != Some(image.physical_key.as_str());
        if image.physical_key != canonical_key || after_membership_drift {
            failures.push("memory_graph_backlink_document_physical_key_drift".to_string());
        }
    }

    GovernedPostImageValidation::from_failures(failures)
}

fn append_graph_owner_bindings(
    owner_ref: GovernedMemoryOwnerRef,
    owner_revision: u64,
    visible: bool,
    memberships_by_owner: &BTreeMap<GovernedMemoryOwnerRef, Vec<&MemoryGraphNodeMembership>>,
    owner_refs: &mut BTreeSet<GovernedMemoryOwnerRef>,
    owner_bindings: &mut Vec<MemoryGraphOwnerBinding>,
    failures: &mut Vec<String>,
) {
    let Some(memberships) = memberships_by_owner.get(&owner_ref) else {
        return;
    };
    if !owner_refs.insert(owner_ref.clone()) {
        failures.push("memory_graph_owner_duplicate".to_string());
    }
    for membership in memberships {
        if membership.owner_revision != owner_revision {
            failures.push("memory_graph_owner_revision_drift".to_string());
        }
        owner_bindings.push(MemoryGraphOwnerBinding {
            node_id: membership.node_id.clone(),
            owner_ref: owner_ref.clone(),
            owner_revision,
            visible,
        });
    }
}

fn memory_graph_before_image(closure: &MemoryGraphPostImageClosure) -> MemoryGraphPostImageClosure {
    let before_owner_refs = closure
        .node_memberships
        .iter()
        .filter_map(|image| image.before.as_ref())
        .map(|membership| &membership.owner_ref)
        .collect::<BTreeSet<_>>();
    MemoryGraphPostImageClosure {
        memory_space_id: closure.memory_space_id.clone(),
        long_term_owner_id: closure.long_term_owner_id.clone(),
        mounted_subject_id: closure.mounted_subject_id.clone(),
        allow_missing_before_owners: false,
        validate_transition_successors: false,
        long_term_owners: closure
            .long_term_owners
            .iter()
            .filter(|image| {
                image
                    .before
                    .as_ref()
                    .is_some_and(|owner| before_owner_refs.contains(&owner.owner_ref))
            })
            .map(long_term_graph_before_image)
            .collect(),
        evidence_document_owners: closure
            .evidence_document_owners
            .iter()
            .filter(|image| {
                image.before.as_ref().is_some_and(|owner| {
                    before_owner_refs.contains(&GovernedMemoryOwnerRef::new(
                        GovernedMemoryOwnerPlane::EvidenceDocument,
                        owner.document_id.clone(),
                    ))
                })
            })
            .map(graph_before_image)
            .collect(),
        manifest: graph_before_image(&closure.manifest),
        revision: graph_before_image(&closure.revision),
        node_memberships: graph_before_images(&closure.node_memberships),
        edge_memberships: graph_before_images(&closure.edge_memberships),
        backlink_memberships: graph_before_images(&closure.backlink_memberships),
        indexes: graph_before_images(&closure.indexes),
        nodes: graph_before_images(&closure.nodes),
        edges: graph_before_images(&closure.edges),
        backlinks: graph_before_images(&closure.backlinks),
    }
}

fn graph_missing_before_owner_ids(
    closure: &MemoryGraphPostImageClosure,
) -> BTreeSet<GovernedMemoryOwnerRef> {
    let mut available_owner_refs = closure
        .long_term_owners
        .iter()
        .filter_map(|image| image.before.as_ref())
        .map(|owner| owner.owner_ref.clone())
        .collect::<BTreeSet<_>>();
    available_owner_refs.extend(
        closure
            .evidence_document_owners
            .iter()
            .filter_map(|image| image.before.as_ref())
            .map(|owner| {
                GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    owner.document_id.clone(),
                )
            }),
    );
    closure
        .node_memberships
        .iter()
        .filter_map(|image| {
            let before = image.before.as_ref()?;
            (!available_owner_refs.contains(&before.owner_ref) && image.after.is_none())
                .then(|| before.owner_ref.clone())
        })
        .collect()
}

fn graph_before_image<T: Clone>(image: &GovernedDocumentImage<T>) -> GovernedDocumentImage<T> {
    GovernedDocumentImage {
        physical_key: image.physical_key.clone(),
        before: None,
        after: image.before.clone(),
    }
}

fn long_term_graph_before_image(
    image: &LongTermMemoryVersionMaterialImage,
) -> LongTermMemoryVersionMaterialImage {
    LongTermMemoryVersionMaterialImage {
        before_physical_key: None,
        before: None,
        after_physical_key: image.before_physical_key.clone(),
        after: image.before.clone(),
    }
}

fn graph_before_images<T: Clone>(
    images: &[GovernedDocumentImage<T>],
) -> Vec<GovernedDocumentImage<T>> {
    images
        .iter()
        .filter(|image| image.before.is_some())
        .map(graph_before_image)
        .collect()
}

fn validate_memory_graph_deleted_post_image(
    closure: &MemoryGraphPostImageClosure,
    failures: &mut Vec<String>,
) {
    let memory_space_id = closure.memory_space_id.trim();
    let mounted_subject_id = closure.mounted_subject_id.trim();
    let Some(manifest) = closure.manifest.before.as_ref() else {
        failures.push("memory_graph_manifest_delete_physical_key_drift".to_string());
        return;
    };
    if closure.manifest.physical_key
        != memory_graph_scope_manifest_key(memory_space_id, mounted_subject_id)
    {
        failures.push("memory_graph_manifest_delete_physical_key_drift".to_string());
    }
    let all_deleted = closure.revision.after.is_none()
        && closure
            .node_memberships
            .iter()
            .all(|image| image.after.is_none())
        && closure
            .edge_memberships
            .iter()
            .all(|image| image.after.is_none())
        && closure
            .backlink_memberships
            .iter()
            .all(|image| image.after.is_none())
        && closure.indexes.iter().all(|image| image.after.is_none())
        && closure.nodes.iter().all(|image| image.after.is_none())
        && closure.edges.iter().all(|image| image.after.is_none())
        && closure.backlinks.iter().all(|image| image.after.is_none());
    if !all_deleted {
        failures.push("memory_graph_delete_exact_closure_drift".to_string());
    }
    if closure
        .revision
        .before
        .as_ref()
        .is_none_or(|revision| closure.revision.physical_key != revision.revision_key)
    {
        failures.push("memory_graph_revision_physical_key_drift".to_string());
    }
    let exact_dependency_sets = deleted_dependency_keys(&closure.node_memberships)
        == dependency_storage_keys(&manifest.node_memberships)
        && deleted_keys_are_unique(&closure.node_memberships)
        && deleted_dependency_keys(&closure.edge_memberships)
            == dependency_storage_keys(&manifest.edge_memberships)
        && deleted_keys_are_unique(&closure.edge_memberships)
        && deleted_dependency_keys(&closure.backlink_memberships)
            == dependency_storage_keys(&manifest.backlink_memberships)
        && deleted_keys_are_unique(&closure.backlink_memberships)
        && deleted_dependency_keys(&closure.indexes)
            == dependency_storage_keys(&manifest.recall_indexes)
        && deleted_keys_are_unique(&closure.indexes)
        && closure.revision.physical_key == manifest.revision.storage_key;
    let exact_document_sets = deleted_dependency_keys(&closure.nodes)
        == deleted_document_keys(&closure.node_memberships, |value| {
            value.document_key.as_str()
        })
        && deleted_keys_are_unique(&closure.nodes)
        && deleted_dependency_keys(&closure.edges)
            == deleted_document_keys(&closure.edge_memberships, |value| {
                value.document_key.as_str()
            })
        && deleted_keys_are_unique(&closure.edges)
        && deleted_dependency_keys(&closure.backlinks)
            == deleted_document_keys(&closure.backlink_memberships, |value| {
                value.document_key.as_str()
            })
        && deleted_keys_are_unique(&closure.backlinks);
    if !exact_dependency_sets || !exact_document_sets {
        failures.push("memory_graph_delete_exact_closure_drift".to_string());
    }
    validate_deleted_graph_typed_keys(
        &closure.node_memberships,
        |value| value.membership_key.as_str(),
        "memory_graph_node_membership_physical_key_drift",
        failures,
    );
    validate_deleted_graph_typed_keys(
        &closure.edge_memberships,
        |value| value.membership_key.as_str(),
        "memory_graph_edge_membership_physical_key_drift",
        failures,
    );
    validate_deleted_graph_typed_keys(
        &closure.backlink_memberships,
        |value| value.membership_key.as_str(),
        "memory_graph_backlink_membership_physical_key_drift",
        failures,
    );
    validate_deleted_graph_typed_keys(
        &closure.indexes,
        |value| value.index_key.as_str(),
        "memory_graph_index_physical_key_drift",
        failures,
    );
    for image in &closure.nodes {
        if image.before.as_ref().is_none_or(|node| {
            image.physical_key
                != scoped_memory_graph_storage_key(
                    memory_space_id,
                    mounted_subject_id,
                    &format!("node:{}", node.node_id),
                )
        }) {
            failures.push("memory_graph_node_document_physical_key_drift".to_string());
        }
    }
    for image in &closure.edges {
        if image.before.as_ref().is_none_or(|edge| {
            image.physical_key
                != scoped_memory_graph_storage_key(
                    memory_space_id,
                    mounted_subject_id,
                    &format!("edge:{}", edge.edge_id),
                )
        }) {
            failures.push("memory_graph_edge_document_physical_key_drift".to_string());
        }
    }
    for image in &closure.backlinks {
        if image.before.as_ref().is_none_or(|backlink| {
            let logical_key = memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id);
            image.physical_key
                != scoped_memory_graph_storage_key(
                    memory_space_id,
                    mounted_subject_id,
                    &format!("backlink:{logical_key}"),
                )
        }) {
            failures.push("memory_graph_backlink_document_physical_key_drift".to_string());
        }
    }
}

fn dependency_storage_keys(dependencies: &[MemoryGraphDependencyRef]) -> BTreeSet<String> {
    dependencies
        .iter()
        .map(|dependency| dependency.storage_key.clone())
        .collect()
}

fn deleted_dependency_keys<T>(images: &[GovernedDocumentImage<T>]) -> BTreeSet<String> {
    images
        .iter()
        .map(|image| image.physical_key.clone())
        .collect()
}

fn deleted_keys_are_unique<T>(images: &[GovernedDocumentImage<T>]) -> bool {
    deleted_dependency_keys(images).len() == images.len()
}

fn deleted_document_keys<T>(
    images: &[GovernedDocumentImage<T>],
    document_key: impl Fn(&T) -> &str,
) -> BTreeSet<String> {
    images
        .iter()
        .filter_map(|image| image.before.as_ref())
        .map(|value| document_key(value).to_string())
        .collect()
}

fn validate_deleted_graph_typed_keys<T>(
    images: &[GovernedDocumentImage<T>],
    physical_key: impl Fn(&T) -> &str,
    failure: &str,
    failures: &mut Vec<String>,
) {
    for image in images {
        if image
            .before
            .as_ref()
            .is_none_or(|value| image.physical_key != physical_key(value))
        {
            failures.push(failure.to_string());
        }
    }
}

fn collect_graph_post_image_docs<T: Clone>(
    images: &[GovernedDocumentImage<T>],
    logical_id: impl Fn(&T) -> &str,
    duplicate_failure: &str,
    failures: &mut Vec<String>,
) -> Vec<T> {
    let mut seen = BTreeSet::new();
    let mut values = Vec::new();
    for image in images {
        let Some(value) = image.after.as_ref() else {
            continue;
        };
        if !seen.insert(logical_id(value).to_string()) {
            failures.push(duplicate_failure.to_string());
        }
        values.push(value.clone());
    }
    values
}

fn validate_graph_typed_images<T: Clone + PartialEq>(
    images: &[GovernedDocumentImage<T>],
    expected: &[T],
    physical_key: impl Fn(&T) -> &str,
    physical_key_failure: &str,
    closure_failure: &str,
    failures: &mut Vec<String>,
) {
    let actual = images
        .iter()
        .filter_map(|image| image.after.clone())
        .collect::<Vec<_>>();
    if actual != expected {
        failures.push(closure_failure.to_string());
    }
    for image in images {
        let value = image.after.as_ref().or(image.before.as_ref());
        if value.is_some_and(|value| image.physical_key != physical_key(value)) {
            failures.push(physical_key_failure.to_string());
        }
    }
}

fn validate_graph_document_keys<T>(
    images: &[GovernedDocumentImage<T>],
    logical_id: impl Fn(&T) -> &str,
    expected_keys: &BTreeMap<&str, &str>,
    canonical_physical_key: impl Fn(&T) -> String,
    failure: &str,
    failures: &mut Vec<String>,
) {
    for image in images {
        let Some(value) = image.after.as_ref().or(image.before.as_ref()) else {
            failures.push(failure.to_string());
            continue;
        };
        let after_membership_drift = image.after.as_ref().is_some()
            && expected_keys.get(logical_id(value)).copied() != Some(image.physical_key.as_str());
        if image.physical_key != canonical_physical_key(value) || after_membership_drift {
            failures.push(failure.to_string());
        }
    }
}

fn validate_graph_effect_keys<T>(
    images: &[GovernedDocumentImage<T>],
    expected_keys: &BTreeSet<String>,
    failure: &str,
    failures: &mut Vec<String>,
) {
    if images
        .iter()
        .map(|image| image.physical_key.clone())
        .collect::<BTreeSet<_>>()
        != *expected_keys
    {
        failures.push(failure.to_string());
    }
}

fn validate_graph_effect_closure(
    closure: &MemoryGraphPostImageClosure,
    manifest: &MemoryGraphScopeManifest,
    failures: &mut Vec<String>,
) {
    let mut expected_node_membership_effects = dependency_storage_keys(&manifest.node_memberships);
    let mut expected_edge_membership_effects = dependency_storage_keys(&manifest.edge_memberships);
    let mut expected_backlink_membership_effects =
        dependency_storage_keys(&manifest.backlink_memberships);
    let mut expected_index_effects = dependency_storage_keys(&manifest.recall_indexes);
    if let Some(before_manifest) = closure.manifest.before.as_ref() {
        expected_node_membership_effects
            .extend(dependency_storage_keys(&before_manifest.node_memberships));
        expected_edge_membership_effects
            .extend(dependency_storage_keys(&before_manifest.edge_memberships));
        expected_backlink_membership_effects.extend(dependency_storage_keys(
            &before_manifest.backlink_memberships,
        ));
        expected_index_effects.extend(dependency_storage_keys(&before_manifest.recall_indexes));
    }
    validate_graph_effect_keys(
        &closure.node_memberships,
        &expected_node_membership_effects,
        "memory_graph_node_membership_effect_closure_drift",
        failures,
    );
    validate_graph_effect_keys(
        &closure.edge_memberships,
        &expected_edge_membership_effects,
        "memory_graph_edge_membership_effect_closure_drift",
        failures,
    );
    validate_graph_effect_keys(
        &closure.backlink_memberships,
        &expected_backlink_membership_effects,
        "memory_graph_backlink_membership_effect_closure_drift",
        failures,
    );
    validate_graph_effect_keys(
        &closure.indexes,
        &expected_index_effects,
        "memory_graph_index_effect_closure_drift",
        failures,
    );

    let expected_node_document_effects =
        graph_document_effect_keys(&closure.node_memberships, |value| {
            value.document_key.as_str()
        });
    let expected_edge_document_effects =
        graph_document_effect_keys(&closure.edge_memberships, |value| {
            value.document_key.as_str()
        });
    let expected_backlink_document_effects =
        graph_document_effect_keys(&closure.backlink_memberships, |value| {
            value.document_key.as_str()
        });
    validate_graph_effect_keys(
        &closure.nodes,
        &expected_node_document_effects,
        "memory_graph_node_document_effect_closure_drift",
        failures,
    );
    validate_graph_effect_keys(
        &closure.edges,
        &expected_edge_document_effects,
        "memory_graph_edge_document_effect_closure_drift",
        failures,
    );
    validate_graph_effect_keys(
        &closure.backlinks,
        &expected_backlink_document_effects,
        "memory_graph_backlink_document_effect_closure_drift",
        failures,
    );
}

fn graph_document_effect_keys<T>(
    images: &[GovernedDocumentImage<T>],
    document_key: impl Fn(&T) -> &str,
) -> BTreeSet<String> {
    images
        .iter()
        .flat_map(|image| image.before.iter().chain(image.after.iter()))
        .map(|value| document_key(value).to_string())
        .collect()
}

pub fn memory_graph_scope_digest(memory_space_id: &str, mounted_subject_id: &str) -> String {
    memory_graph_sha256(
        "memory_graph_scope_v4",
        &[
            MEMORY_GRAPH_SCHEMA_VERSION.to_string().as_bytes(),
            memory_space_id.trim().as_bytes(),
            mounted_subject_id.trim().as_bytes(),
        ],
    )
}

pub fn scoped_memory_graph_storage_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
    logical_key: &str,
) -> String {
    let scope = memory_graph_scope_digest(memory_space_id, mounted_subject_id);
    let document = memory_graph_sha256(
        "memory_graph_storage_key_v4",
        &[logical_key.trim().as_bytes()],
    );
    format!("scope:{scope}:doc:{document}")
}

pub fn memory_graph_scope_manifest_key(memory_space_id: &str, mounted_subject_id: &str) -> String {
    scoped_memory_graph_storage_key(
        memory_space_id,
        mounted_subject_id,
        MEMORY_GRAPH_SCOPE_MANIFEST_LOGICAL_KEY,
    )
}

pub fn memory_graph_integrity_incident_token(
    scope_digest: &str,
    manifest_generation: Option<u64>,
    graph_revision: Option<&str>,
    failures: &[String],
) -> String {
    let mut failures = failures.to_vec();
    failures.sort();
    failures.dedup();
    let generation = manifest_generation
        .map(|value| value.to_string())
        .unwrap_or_default();
    let failure_set = serde_json::to_string(&failures).unwrap_or_else(|_| "[]".to_string());
    format!(
        "graph_incident:{}",
        memory_graph_sha256(
            "memory_graph_integrity_incident_v4",
            &[
                scope_digest.as_bytes(),
                generation.as_bytes(),
                graph_revision.unwrap_or_default().as_bytes(),
                failure_set.as_bytes(),
            ],
        )
    )
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphRecallCandidateScore {
    pub candidate_id: String,
    pub lexical_score: u32,
    pub entity_alias_score: u32,
    pub temporal_anchor_score: u32,
    pub session_alias_score: u32,
    pub graph_neighborhood_score: u32,
    pub temporal_validity_score: u32,
    pub temporal_reasoning_score: u32,
    pub evidence_quality_score: u32,
    pub multi_evidence_coverage_score: u32,
    pub source_authority_score: u32,
    pub facet_exact_score: u32,
    pub facet_expanded_score: u32,
    pub facet_authority_score: u32,
    pub facet_diversity_score: u32,
    pub facet_temporal_score: u32,
    pub facet_stale_penalty: u32,
    pub privacy_profile_eligibility_score: u32,
    pub stale_superseded_penalty: u32,
    pub total_score: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphFacetPropagationContext {
    pub exact_anchor_ids: Vec<String>,
    pub expanded_anchor_ids: Vec<String>,
    pub covered_evidence_groups: Vec<String>,
    pub candidate_evidence_groups: BTreeMap<String, Vec<String>>,
    pub candidate_observed_at: BTreeMap<String, u64>,
}

impl GraphFacetPropagationContext {
    fn is_empty(&self) -> bool {
        self.exact_anchor_ids.is_empty()
            && self.expanded_anchor_ids.is_empty()
            && self.covered_evidence_groups.is_empty()
            && self.candidate_evidence_groups.is_empty()
            && self.candidate_observed_at.is_empty()
    }
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
    pub reranked_candidate_ids: Vec<String>,
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
    pub schema_version: u32,
    pub owner: String,
    pub index_id: String,
    pub index_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub scope_digest: String,
    pub owner_ref: GovernedMemoryOwnerRef,
    pub owner_candidate_id: String,
    pub owner_revision: u64,
    pub source_anchor_node_ids: Vec<String>,
    pub manifest_generation: u64,
    pub graph_revision: String,
    pub node_memberships: Vec<MemoryGraphDependencyRef>,
    pub edge_memberships: Vec<MemoryGraphDependencyRef>,
    pub backlink_memberships: Vec<MemoryGraphDependencyRef>,
    pub node_count: usize,
    pub edge_count: usize,
    pub backlink_count: usize,
    pub dependency_digest: String,
}

pub fn memory_graph_backlink_key(source_kind: &str, source_id: &str) -> String {
    memory_graph_sha256(
        "memory_graph_backlink_key_v4",
        &[source_kind.trim().as_bytes(), source_id.trim().as_bytes()],
    )
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
    temporal_memory_graph_gate_report(&nodes, &edges, &evidence_backlinks)
}

fn temporal_memory_graph_gate_report(
    nodes: &[MemoryGraphNode],
    edges: &[MemoryGraphEdge],
    evidence_backlinks: &[EvidenceBacklink],
) -> TemporalMemoryGraphGateReport {
    let mut failures = Vec::new();
    if nodes.is_empty() {
        failures.push("memory_graph_nodes_empty".to_string());
    }
    for node in nodes {
        let validation = node.validate_contract();
        if !validation.accepted {
            failures.push(format!("node:{}:{}", node.node_id, validation.reason));
        }
    }
    for edge in edges {
        let validation = edge.validate_contract();
        if !validation.accepted {
            failures.push(format!("edge:{}:{}", edge.edge_id, validation.reason));
        }
    }
    let valid_backlink_sources = evidence_backlinks
        .iter()
        .filter(|backlink| !backlink.fingerprint.is_empty())
        .map(|backlink| backlink.source_id.as_str())
        .collect::<BTreeSet<_>>();
    for evidence_ref in nodes
        .iter()
        .flat_map(|node| node.evidence_refs.iter())
        .chain(edges.iter().flat_map(|edge| edge.evidence_refs.iter()))
    {
        if !valid_backlink_sources.contains(evidence_ref.as_str()) {
            failures.push(format!("missing_evidence_backlink:{evidence_ref}"));
        }
    }
    for backlink in evidence_backlinks {
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
    let gate = temporal_memory_graph_write_gate(&nodes, &edges, &backlinks);

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

fn temporal_memory_graph_write_gate(
    nodes: &[MemoryGraphNode],
    edges: &[MemoryGraphEdge],
    backlinks: &[EvidenceBacklink],
) -> TemporalMemoryGraphGateReport {
    let mut gate = temporal_memory_graph_gate_report(nodes, edges, backlinks);
    let node_ids = nodes
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<BTreeSet<_>>();
    for edge in edges {
        if !node_ids.contains(edge.from_node_id.as_str()) {
            gate.failures.push(format!(
                "edge:{}:memory_graph_edge_from_missing",
                edge.edge_id
            ));
        }
        if !node_ids.contains(edge.to_node_id.as_str()) {
            gate.failures.push(format!(
                "edge:{}:memory_graph_edge_to_missing",
                edge.edge_id
            ));
        }
    }
    gate.failures.sort();
    gate.failures.dedup();
    gate.high_confidence_projection_allowed = gate.failures.is_empty();
    gate
}

pub fn memory_graph_recall_index_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
    owner_candidate_id: &str,
) -> String {
    scoped_memory_graph_storage_key(
        memory_space_id,
        mounted_subject_id,
        &format!("graph_index:{owner_candidate_id}"),
    )
}

pub fn build_memory_graph_persistence_plan(
    memory_space_id: impl Into<String>,
    mounted_subject_id: impl Into<String>,
    manifest_generation: u64,
    nodes: Vec<MemoryGraphNode>,
    edges: Vec<MemoryGraphEdge>,
    backlinks: Vec<EvidenceBacklink>,
    owner_bindings: Vec<MemoryGraphOwnerBinding>,
) -> MemoryGraphPersistencePlan {
    let memory_space_id = memory_space_id.into().trim().to_string();
    let mounted_subject_id = mounted_subject_id.into().trim().to_string();
    let mut failures = temporal_memory_graph_write_gate(&nodes, &edges, &backlinks).failures;
    if memory_space_id.is_empty() {
        failures.push("memory_graph_memory_space_id_empty".to_string());
    }
    if mounted_subject_id.is_empty() {
        failures.push("memory_graph_mounted_subject_id_empty".to_string());
    }
    if manifest_generation == 0 {
        failures.push("memory_graph_manifest_generation_invalid".to_string());
    }
    append_duplicate_graph_id_failures(
        nodes.iter().map(|node| node.node_id.as_str()),
        "memory_graph_node_id_duplicate",
        &mut failures,
    );
    append_duplicate_graph_id_failures(
        edges.iter().map(|edge| edge.edge_id.as_str()),
        "memory_graph_edge_id_duplicate",
        &mut failures,
    );
    append_duplicate_graph_id_failures(
        backlinks
            .iter()
            .map(|backlink| memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id)),
        "memory_graph_backlink_id_duplicate",
        &mut failures,
    );

    append_duplicate_graph_id_failures(
        owner_bindings.iter().map(|owner| owner.node_id.as_str()),
        "memory_graph_owner_node_id_duplicate",
        &mut failures,
    );
    let owners_by_node = owner_bindings
        .iter()
        .map(|owner| (owner.node_id.as_str(), owner))
        .collect::<BTreeMap<_, _>>();
    let mut owner_revisions = BTreeMap::<&GovernedMemoryOwnerRef, u64>::new();
    for owner in &owner_bindings {
        if owner_revisions
            .insert(&owner.owner_ref, owner.owner_revision)
            .is_some_and(|revision| revision != owner.owner_revision)
        {
            failures.push("memory_graph_owner_revision_inconsistent".to_string());
        }
    }
    for node in &nodes {
        match owners_by_node.get(node.node_id.as_str()) {
            None => failures.push("memory_graph_persistent_node_owner_missing".to_string()),
            Some(owner) if !owner.owner_ref.is_valid() || owner.node_id.trim().is_empty() => {
                failures.push("memory_graph_persistent_node_owner_identity_invalid".to_string())
            }
            Some(owner) if !owner.visible => {
                failures.push("memory_graph_persistent_node_owner_not_visible".to_string())
            }
            Some(owner) if owner.owner_revision == 0 => {
                failures.push("memory_graph_persistent_node_owner_revision_invalid".to_string())
            }
            Some(_) => {}
        }
    }
    failures.sort();
    failures.dedup();
    if !failures.is_empty() {
        return MemoryGraphPersistencePlan {
            failures,
            ..MemoryGraphPersistencePlan::default()
        };
    }

    let scope_digest = memory_graph_scope_digest(&memory_space_id, &mounted_subject_id);
    let mut revision_nodes = nodes.clone();
    revision_nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    let mut revision_edges = edges.clone();
    revision_edges.sort_by(|left, right| left.edge_id.cmp(&right.edge_id));
    let mut revision_backlinks = backlinks.clone();
    revision_backlinks.sort_by(|left, right| {
        memory_graph_backlink_key(&left.source_kind, &left.source_id).cmp(
            &memory_graph_backlink_key(&right.source_kind, &right.source_id),
        )
    });
    let mut revision_owners = owner_bindings.clone();
    revision_owners.sort_by(|left, right| {
        left.owner_ref
            .cmp(&right.owner_ref)
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    let graph_revision = format!(
        "graph_revision:{}",
        memory_graph_serialized_digest(&(
            MEMORY_GRAPH_SCHEMA_VERSION,
            &scope_digest,
            manifest_generation,
            &revision_nodes,
            &revision_edges,
            &revision_backlinks,
            &revision_owners,
        ))
    );

    let node_membership_keys = nodes
        .iter()
        .map(|node| {
            (
                node.node_id.clone(),
                scoped_memory_graph_storage_key(
                    &memory_space_id,
                    &mounted_subject_id,
                    &format!("node_membership:{}", node.node_id),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let edge_membership_keys = edges
        .iter()
        .map(|edge| {
            (
                edge.edge_id.clone(),
                scoped_memory_graph_storage_key(
                    &memory_space_id,
                    &mounted_subject_id,
                    &format!("edge_membership:{}", edge.edge_id),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let backlink_membership_keys = backlinks
        .iter()
        .map(|backlink| {
            let backlink_key =
                memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id);
            (
                backlink_key.clone(),
                scoped_memory_graph_storage_key(
                    &memory_space_id,
                    &mounted_subject_id,
                    &format!("backlink_membership:{backlink_key}"),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let nodes_by_id = nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let edges_by_id = edges
        .iter()
        .map(|edge| (edge.edge_id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    let mut recall_adjacency = BTreeMap::<&str, Vec<(&str, &str)>>::new();
    for edge in &edges {
        if !graph_edge_allows_recall_expansion(edge.kind) {
            continue;
        }
        recall_adjacency
            .entry(edge.from_node_id.as_str())
            .or_default()
            .push((edge.to_node_id.as_str(), edge.edge_id.as_str()));
        recall_adjacency
            .entry(edge.to_node_id.as_str())
            .or_default()
            .push((edge.from_node_id.as_str(), edge.edge_id.as_str()));
    }
    for neighbors in recall_adjacency.values_mut() {
        neighbors.sort();
        neighbors.dedup();
    }
    let mut backlink_keys_by_evidence = BTreeMap::<&str, Vec<String>>::new();
    for backlink in &backlinks {
        let backlink_key = memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id);
        backlink_keys_by_evidence
            .entry(backlink.source_id.as_str())
            .or_default()
            .push(
                backlink_membership_keys
                    .get(&backlink_key)
                    .expect("validated backlink membership key")
                    .clone(),
            );
    }
    for keys in backlink_keys_by_evidence.values_mut() {
        keys.sort();
        keys.dedup();
    }
    let backlink_keys_for_evidence = |evidence_refs: &[String]| {
        let mut keys = evidence_refs
            .iter()
            .flat_map(|evidence_ref| {
                backlink_keys_by_evidence
                    .get(evidence_ref.as_str())
                    .into_iter()
                    .flatten()
                    .cloned()
            })
            .collect::<Vec<_>>();
        keys.sort();
        keys.dedup();
        keys
    };

    #[derive(Clone)]
    struct RawIndexDependencies {
        index_id: String,
        index_key: String,
        owner_ref: GovernedMemoryOwnerRef,
        owner_candidate_id: String,
        owner_revision: u64,
        source_anchor_node_ids: Vec<String>,
        node_membership_keys: Vec<String>,
        edge_membership_keys: Vec<String>,
        backlink_membership_keys: Vec<String>,
    }

    let mut raw_indexes_by_owner = BTreeMap::<GovernedMemoryOwnerRef, RawIndexDependencies>::new();
    for node in &nodes {
        let owner = owners_by_node
            .get(node.node_id.as_str())
            .expect("validated graph owner");
        let owner_candidate_id = governed_memory_recall_candidate_id(&owner.owner_ref);
        let (node_ids, edge_ids) =
            graph_recall_dependency_ids(&node.node_id, &nodes_by_id, &recall_adjacency);
        let mut evidence_refs = node_ids
            .iter()
            .filter_map(|node_id| nodes_by_id.get(node_id.as_str()).copied())
            .flat_map(|node| node.evidence_refs.iter().cloned())
            .collect::<Vec<_>>();
        evidence_refs.extend(
            edge_ids
                .iter()
                .filter_map(|edge_id| edges_by_id.get(edge_id.as_str()).copied())
                .flat_map(|edge| edge.evidence_refs.iter().cloned()),
        );
        evidence_refs.sort();
        evidence_refs.dedup();
        let dependency_backlinks = backlink_keys_for_evidence(&evidence_refs);
        let raw = raw_indexes_by_owner
            .entry(owner.owner_ref.clone())
            .or_insert_with(|| RawIndexDependencies {
                index_key: memory_graph_recall_index_key(
                    &memory_space_id,
                    &mounted_subject_id,
                    &owner_candidate_id,
                ),
                index_id: format!("graph_index:{owner_candidate_id}"),
                owner_ref: owner.owner_ref.clone(),
                owner_candidate_id: owner_candidate_id.clone(),
                owner_revision: owner.owner_revision,
                source_anchor_node_ids: Vec::new(),
                node_membership_keys: Vec::new(),
                edge_membership_keys: Vec::new(),
                backlink_membership_keys: Vec::new(),
            });
        raw.source_anchor_node_ids.push(node.node_id.clone());
        raw.node_membership_keys.extend(
            node_ids
                .iter()
                .filter_map(|node_id| node_membership_keys.get(node_id).cloned()),
        );
        raw.edge_membership_keys.extend(
            edge_ids
                .iter()
                .filter_map(|edge_id| edge_membership_keys.get(edge_id).cloned()),
        );
        raw.backlink_membership_keys.extend(dependency_backlinks);
    }
    let mut raw_indexes = raw_indexes_by_owner.into_values().collect::<Vec<_>>();
    for raw in &mut raw_indexes {
        raw.source_anchor_node_ids.sort();
        raw.source_anchor_node_ids.dedup();
        raw.node_membership_keys.sort();
        raw.node_membership_keys.dedup();
        raw.edge_membership_keys.sort();
        raw.edge_membership_keys.dedup();
        raw.backlink_membership_keys.sort();
        raw.backlink_membership_keys.dedup();
    }

    let mut node_memberships = nodes
        .iter()
        .map(|node| {
            let backlink_keys = backlink_keys_for_evidence(&node.evidence_refs);
            let mut membership = MemoryGraphNodeMembership {
                schema_version: MEMORY_GRAPH_SCHEMA_VERSION,
                memory_space_id: memory_space_id.clone(),
                mounted_subject_id: mounted_subject_id.clone(),
                scope_digest: scope_digest.clone(),
                manifest_generation,
                graph_revision: graph_revision.clone(),
                membership_key: node_membership_keys
                    .get(&node.node_id)
                    .expect("validated node membership key")
                    .clone(),
                node_id: node.node_id.clone(),
                document_key: scoped_memory_graph_storage_key(
                    &memory_space_id,
                    &mounted_subject_id,
                    &format!("node:{}", node.node_id),
                ),
                document_digest: memory_graph_serialized_digest(node),
                owner_ref: owners_by_node
                    .get(node.node_id.as_str())
                    .expect("validated graph owner")
                    .owner_ref
                    .clone(),
                owner_revision: owners_by_node
                    .get(node.node_id.as_str())
                    .expect("validated graph owner")
                    .owner_revision,
                index_key: memory_graph_recall_index_key(
                    &memory_space_id,
                    &mounted_subject_id,
                    &governed_memory_recall_candidate_id(
                        &owners_by_node
                            .get(node.node_id.as_str())
                            .expect("validated graph owner")
                            .owner_ref,
                    ),
                ),
                backlink_membership_keys: backlink_keys,
                dependency_digest: String::new(),
            };
            membership.dependency_digest = memory_graph_node_membership_digest(&membership);
            membership
        })
        .collect::<Vec<_>>();
    node_memberships.sort_by(|left, right| left.membership_key.cmp(&right.membership_key));

    let mut edge_memberships = edges
        .iter()
        .map(|edge| {
            let backlink_keys = backlink_keys_for_evidence(&edge.evidence_refs);
            let mut membership = MemoryGraphEdgeMembership {
                schema_version: MEMORY_GRAPH_SCHEMA_VERSION,
                memory_space_id: memory_space_id.clone(),
                mounted_subject_id: mounted_subject_id.clone(),
                scope_digest: scope_digest.clone(),
                manifest_generation,
                graph_revision: graph_revision.clone(),
                membership_key: edge_membership_keys
                    .get(&edge.edge_id)
                    .expect("validated edge membership key")
                    .clone(),
                edge_id: edge.edge_id.clone(),
                document_key: scoped_memory_graph_storage_key(
                    &memory_space_id,
                    &mounted_subject_id,
                    &format!("edge:{}", edge.edge_id),
                ),
                document_digest: memory_graph_serialized_digest(edge),
                from_node_membership_key: node_membership_keys
                    .get(&edge.from_node_id)
                    .expect("validated edge source membership")
                    .clone(),
                to_node_membership_key: node_membership_keys
                    .get(&edge.to_node_id)
                    .expect("validated edge target membership")
                    .clone(),
                backlink_membership_keys: backlink_keys,
                dependency_digest: String::new(),
            };
            membership.dependency_digest = memory_graph_edge_membership_digest(&membership);
            membership
        })
        .collect::<Vec<_>>();
    edge_memberships.sort_by(|left, right| left.membership_key.cmp(&right.membership_key));

    let mut backlink_node_keys = BTreeMap::<&str, Vec<String>>::new();
    for membership in &node_memberships {
        for backlink_key in &membership.backlink_membership_keys {
            backlink_node_keys
                .entry(backlink_key.as_str())
                .or_default()
                .push(membership.membership_key.clone());
        }
    }
    let mut backlink_edge_keys = BTreeMap::<&str, Vec<String>>::new();
    for membership in &edge_memberships {
        for backlink_key in &membership.backlink_membership_keys {
            backlink_edge_keys
                .entry(backlink_key.as_str())
                .or_default()
                .push(membership.membership_key.clone());
        }
    }
    let mut backlink_index_keys = BTreeMap::<&str, Vec<String>>::new();
    for index in &raw_indexes {
        for backlink_key in &index.backlink_membership_keys {
            backlink_index_keys
                .entry(backlink_key.as_str())
                .or_default()
                .push(index.index_key.clone());
        }
    }
    for keys in backlink_node_keys.values_mut() {
        keys.sort();
        keys.dedup();
    }
    for keys in backlink_edge_keys.values_mut() {
        keys.sort();
        keys.dedup();
    }
    for keys in backlink_index_keys.values_mut() {
        keys.sort();
        keys.dedup();
    }

    let mut backlink_memberships = backlinks
        .iter()
        .map(|backlink| {
            let backlink_key =
                memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id);
            let membership_key = backlink_membership_keys
                .get(&backlink_key)
                .expect("validated backlink membership")
                .clone();
            let node_keys = backlink_node_keys
                .get(membership_key.as_str())
                .cloned()
                .unwrap_or_default();
            let edge_keys = backlink_edge_keys
                .get(membership_key.as_str())
                .cloned()
                .unwrap_or_default();
            let index_keys = backlink_index_keys
                .get(membership_key.as_str())
                .cloned()
                .unwrap_or_default();
            let mut membership = MemoryGraphBacklinkMembership {
                schema_version: MEMORY_GRAPH_SCHEMA_VERSION,
                memory_space_id: memory_space_id.clone(),
                mounted_subject_id: mounted_subject_id.clone(),
                scope_digest: scope_digest.clone(),
                manifest_generation,
                graph_revision: graph_revision.clone(),
                membership_key,
                backlink_key: backlink_key.clone(),
                document_key: scoped_memory_graph_storage_key(
                    &memory_space_id,
                    &mounted_subject_id,
                    &format!("backlink:{backlink_key}"),
                ),
                document_digest: memory_graph_serialized_digest(backlink),
                node_membership_keys: node_keys,
                edge_membership_keys: edge_keys,
                index_keys,
                dependency_digest: String::new(),
            };
            membership.dependency_digest = memory_graph_backlink_membership_digest(&membership);
            membership
        })
        .collect::<Vec<_>>();
    backlink_memberships.sort_by(|left, right| left.membership_key.cmp(&right.membership_key));

    let node_refs = node_memberships
        .iter()
        .map(|membership| {
            (
                membership.membership_key.clone(),
                MemoryGraphDependencyRef {
                    storage_key: membership.membership_key.clone(),
                    dependency_digest: membership.dependency_digest.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let edge_refs = edge_memberships
        .iter()
        .map(|membership| {
            (
                membership.membership_key.clone(),
                MemoryGraphDependencyRef {
                    storage_key: membership.membership_key.clone(),
                    dependency_digest: membership.dependency_digest.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let backlink_refs = backlink_memberships
        .iter()
        .map(|membership| {
            (
                membership.membership_key.clone(),
                MemoryGraphDependencyRef {
                    storage_key: membership.membership_key.clone(),
                    dependency_digest: membership.dependency_digest.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut recall_indexes = raw_indexes
        .into_iter()
        .map(|raw| {
            let mut index = MemoryGraphRecallIndexDoc {
                schema_version: MEMORY_GRAPH_SCHEMA_VERSION,
                owner: "bm-sdk::MemoryRuntime".to_string(),
                index_id: raw.index_id,
                index_key: raw.index_key,
                memory_space_id: memory_space_id.clone(),
                mounted_subject_id: mounted_subject_id.clone(),
                scope_digest: scope_digest.clone(),
                owner_ref: raw.owner_ref,
                owner_candidate_id: raw.owner_candidate_id,
                owner_revision: raw.owner_revision,
                source_anchor_node_ids: raw.source_anchor_node_ids,
                manifest_generation,
                graph_revision: graph_revision.clone(),
                node_memberships: raw
                    .node_membership_keys
                    .iter()
                    .filter_map(|key| node_refs.get(key).cloned())
                    .collect(),
                edge_memberships: raw
                    .edge_membership_keys
                    .iter()
                    .filter_map(|key| edge_refs.get(key).cloned())
                    .collect(),
                backlink_memberships: raw
                    .backlink_membership_keys
                    .iter()
                    .filter_map(|key| backlink_refs.get(key).cloned())
                    .collect(),
                node_count: raw.node_membership_keys.len(),
                edge_count: raw.edge_membership_keys.len(),
                backlink_count: raw.backlink_membership_keys.len(),
                dependency_digest: String::new(),
            };
            index.node_memberships.sort();
            index.edge_memberships.sort();
            index.backlink_memberships.sort();
            index.dependency_digest = memory_graph_recall_index_digest(&index);
            index
        })
        .collect::<Vec<_>>();
    recall_indexes.sort_by(|left, right| left.index_key.cmp(&right.index_key));

    let revision_key = scoped_memory_graph_storage_key(
        &memory_space_id,
        &mounted_subject_id,
        MEMORY_GRAPH_REVISION_LOGICAL_KEY,
    );
    let mut revision = MemoryGraphRevisionDoc {
        schema_version: MEMORY_GRAPH_SCHEMA_VERSION,
        memory_space_id: memory_space_id.clone(),
        mounted_subject_id: mounted_subject_id.clone(),
        scope_digest: scope_digest.clone(),
        manifest_generation,
        graph_revision: graph_revision.clone(),
        revision_key: revision_key.clone(),
        node_count: nodes.len(),
        edge_count: edges.len(),
        backlink_count: backlinks.len(),
        index_count: recall_indexes.len(),
        dependency_digest: String::new(),
    };
    revision.dependency_digest = memory_graph_revision_digest(&revision);
    let revision_ref = MemoryGraphDependencyRef {
        storage_key: revision_key,
        dependency_digest: revision.dependency_digest.clone(),
    };

    let mut manifest = MemoryGraphScopeManifest {
        schema_version: MEMORY_GRAPH_SCHEMA_VERSION,
        memory_space_id,
        mounted_subject_id,
        scope_digest,
        manifest_generation,
        graph_revision,
        node_count: nodes.len(),
        edge_count: edges.len(),
        backlink_count: backlinks.len(),
        index_count: recall_indexes.len(),
        node_memberships: node_refs.into_values().collect(),
        edge_memberships: edge_refs.into_values().collect(),
        backlink_memberships: backlink_refs.into_values().collect(),
        recall_indexes: recall_indexes
            .iter()
            .map(|index| MemoryGraphDependencyRef {
                storage_key: index.index_key.clone(),
                dependency_digest: index.dependency_digest.clone(),
            })
            .collect(),
        revision: revision_ref,
        dependency_digest: String::new(),
    };
    manifest.node_memberships.sort();
    manifest.edge_memberships.sort();
    manifest.backlink_memberships.sort();
    manifest.recall_indexes.sort();
    manifest.dependency_digest = memory_graph_manifest_digest(&manifest);

    MemoryGraphPersistencePlan {
        accepted: true,
        failures: Vec::new(),
        scope_manifest: Some(manifest),
        revision: Some(revision),
        node_memberships,
        edge_memberships,
        backlink_memberships,
        recall_indexes,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn validate_memory_graph_read_chain(
    manifest: &MemoryGraphScopeManifest,
    indexes: &[MemoryGraphRecallIndexDoc],
    node_memberships: &[MemoryGraphNodeMembership],
    edge_memberships: &[MemoryGraphEdgeMembership],
    backlink_memberships: &[MemoryGraphBacklinkMembership],
    nodes: &[MemoryGraphNode],
    edges: &[MemoryGraphEdge],
    backlinks: &[EvidenceBacklink],
    owner_bindings: &[MemoryGraphOwnerBinding],
) -> MemoryGraphReadChainValidation {
    let mut failures = validate_memory_graph_scope_manifest(manifest).failures;
    let manifest_node_refs = dependency_ref_map(&manifest.node_memberships);
    let manifest_edge_refs = dependency_ref_map(&manifest.edge_memberships);
    let manifest_backlink_refs = dependency_ref_map(&manifest.backlink_memberships);
    let manifest_index_refs = dependency_ref_map(&manifest.recall_indexes);
    let node_membership_map = node_memberships
        .iter()
        .map(|membership| (membership.membership_key.as_str(), membership))
        .collect::<BTreeMap<_, _>>();
    let node_memberships_by_id = node_memberships
        .iter()
        .map(|membership| (membership.node_id.as_str(), membership))
        .collect::<BTreeMap<_, _>>();
    let edge_membership_map = edge_memberships
        .iter()
        .map(|membership| (membership.membership_key.as_str(), membership))
        .collect::<BTreeMap<_, _>>();
    let backlink_membership_map = backlink_memberships
        .iter()
        .map(|membership| (membership.membership_key.as_str(), membership))
        .collect::<BTreeMap<_, _>>();
    let nodes = nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let edges = edges
        .iter()
        .map(|edge| (edge.edge_id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    let backlinks = backlinks
        .iter()
        .map(|backlink| {
            (
                memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id),
                backlink,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let owners = owner_bindings
        .iter()
        .map(|owner| ((&owner.owner_ref, owner.node_id.as_str()), owner))
        .collect::<BTreeMap<_, _>>();
    let expected_backlink_nodes = node_memberships.iter().fold(
        BTreeMap::<&str, BTreeSet<String>>::new(),
        |mut expected, membership| {
            for backlink_key in &membership.backlink_membership_keys {
                expected
                    .entry(backlink_key.as_str())
                    .or_default()
                    .insert(membership.membership_key.clone());
            }
            expected
        },
    );
    let expected_backlink_edges = edge_memberships.iter().fold(
        BTreeMap::<&str, BTreeSet<String>>::new(),
        |mut expected, membership| {
            for backlink_key in &membership.backlink_membership_keys {
                expected
                    .entry(backlink_key.as_str())
                    .or_default()
                    .insert(membership.membership_key.clone());
            }
            expected
        },
    );
    let expected_backlink_indexes = indexes.iter().fold(
        BTreeMap::<&str, BTreeSet<String>>::new(),
        |mut expected, index| {
            for backlink in &index.backlink_memberships {
                expected
                    .entry(backlink.storage_key.as_str())
                    .or_default()
                    .insert(index.index_key.clone());
            }
            expected
        },
    );
    let full_node_closure = node_membership_map.len() == manifest.node_memberships.len();
    let full_edge_closure = edge_membership_map.len() == manifest.edge_memberships.len();
    let full_index_closure = indexes.len() == manifest.recall_indexes.len();
    for index in indexes {
        append_graph_scope_binding_failures(index, manifest, "memory_graph_index", &mut failures);
        if index.node_count != index.node_memberships.len()
            || index.edge_count != index.edge_memberships.len()
            || index.backlink_count != index.backlink_memberships.len()
        {
            failures.push("memory_graph_index_count_drift".to_string());
        }
        if !index.owner_ref.is_valid() || index.owner_revision == 0 {
            failures.push("memory_graph_index_owner_binding_invalid".to_string());
        }
        let expected_owner_candidate_id = governed_memory_recall_candidate_id(&index.owner_ref);
        if index.owner_candidate_id != expected_owner_candidate_id {
            failures.push("memory_graph_index_owner_candidate_id_drift".to_string());
        }
        if index.index_key
            != memory_graph_recall_index_key(
                &index.memory_space_id,
                &index.mounted_subject_id,
                &expected_owner_candidate_id,
            )
        {
            failures.push("memory_graph_index_key_drift".to_string());
        }
        let mut canonical_anchor_nodes = index.source_anchor_node_ids.clone();
        canonical_anchor_nodes.sort();
        canonical_anchor_nodes.dedup();
        if canonical_anchor_nodes.is_empty()
            || canonical_anchor_nodes != index.source_anchor_node_ids
        {
            failures.push("memory_graph_index_source_anchor_nodes_invalid".to_string());
        }
        let index_digest = memory_graph_recall_index_digest(index);
        if index.dependency_digest != index_digest {
            failures.push("memory_graph_index_dependency_digest_drift".to_string());
        }
        if !dependency_ref_matches(
            &manifest_index_refs,
            &index.index_key,
            &index.dependency_digest,
        ) {
            failures.push("memory_graph_index_manifest_dependency_drift".to_string());
        }
        for source_anchor_node_id in &index.source_anchor_node_ids {
            match owners.get(&(&index.owner_ref, source_anchor_node_id.as_str())) {
                None => failures.push("memory_graph_owner_missing".to_string()),
                Some(owner) if !owner.visible => {
                    failures.push("memory_graph_owner_privacy_scope_restricted".to_string())
                }
                Some(owner) if owner.owner_revision != index.owner_revision => {
                    failures.push("memory_graph_owner_revision_drift".to_string())
                }
                Some(_) => {}
            }
            if node_memberships_by_id
                .get(source_anchor_node_id.as_str())
                .is_none_or(|membership| {
                    index
                        .node_memberships
                        .iter()
                        .all(|dependency| dependency.storage_key != membership.membership_key)
                })
            {
                failures.push("memory_graph_index_source_anchor_membership_missing".to_string());
            }
        }
        validate_index_membership_refs(
            &index.node_memberships,
            &manifest_node_refs,
            &node_membership_map,
            "memory_graph_node_membership_missing",
            &mut failures,
        );
        validate_index_membership_refs(
            &index.edge_memberships,
            &manifest_edge_refs,
            &edge_membership_map,
            "memory_graph_edge_membership_missing",
            &mut failures,
        );
        validate_index_membership_refs(
            &index.backlink_memberships,
            &manifest_backlink_refs,
            &backlink_membership_map,
            "memory_graph_backlink_membership_missing",
            &mut failures,
        );
    }

    for membership in node_memberships {
        append_graph_scope_binding_failures(
            membership,
            manifest,
            "memory_graph_node_membership",
            &mut failures,
        );
        if !membership.owner_ref.is_valid() || membership.owner_revision == 0 {
            failures.push("memory_graph_node_membership_owner_binding_invalid".to_string());
        }
        let expected_index_key = memory_graph_recall_index_key(
            &membership.memory_space_id,
            &membership.mounted_subject_id,
            &governed_memory_recall_candidate_id(&membership.owner_ref),
        );
        if membership.index_key != expected_index_key
            || !manifest_index_refs.contains_key(expected_index_key.as_str())
        {
            failures.push("memory_graph_node_membership_index_binding_drift".to_string());
        }
        if membership.dependency_digest != memory_graph_node_membership_digest(membership) {
            failures.push("memory_graph_node_membership_dependency_digest_drift".to_string());
        }
        if !dependency_ref_matches(
            &manifest_node_refs,
            &membership.membership_key,
            &membership.dependency_digest,
        ) {
            failures.push("memory_graph_node_membership_manifest_dependency_drift".to_string());
        }
        match nodes.get(membership.node_id.as_str()) {
            None => failures.push("memory_graph_node_document_missing".to_string()),
            Some(node) if memory_graph_serialized_digest(*node) != membership.document_digest => {
                failures.push("memory_graph_node_document_dependency_digest_drift".to_string())
            }
            Some(_) => {}
        }
        match owners.get(&(&membership.owner_ref, membership.node_id.as_str())) {
            None => failures.push("memory_graph_owner_missing".to_string()),
            Some(owner) if !owner.visible => {
                failures.push("memory_graph_owner_privacy_scope_restricted".to_string())
            }
            Some(owner) if owner.owner_revision != membership.owner_revision => {
                failures.push("memory_graph_owner_revision_drift".to_string())
            }
            Some(_) => {}
        }
    }

    for membership in edge_memberships {
        append_graph_scope_binding_failures(
            membership,
            manifest,
            "memory_graph_edge_membership",
            &mut failures,
        );
        if membership.dependency_digest != memory_graph_edge_membership_digest(membership) {
            failures.push("memory_graph_edge_membership_dependency_digest_drift".to_string());
        }
        if !dependency_ref_matches(
            &manifest_edge_refs,
            &membership.membership_key,
            &membership.dependency_digest,
        ) {
            failures.push("memory_graph_edge_membership_manifest_dependency_drift".to_string());
        }
        if !node_membership_map.contains_key(membership.from_node_membership_key.as_str())
            || !node_membership_map.contains_key(membership.to_node_membership_key.as_str())
        {
            failures.push("memory_graph_edge_node_membership_missing".to_string());
        }
        match edges.get(membership.edge_id.as_str()) {
            None => failures.push("memory_graph_edge_document_missing".to_string()),
            Some(edge) if memory_graph_serialized_digest(*edge) != membership.document_digest => {
                failures.push("memory_graph_edge_document_dependency_digest_drift".to_string())
            }
            Some(_) => {}
        }
    }

    for membership in backlink_memberships {
        append_graph_scope_binding_failures(
            membership,
            manifest,
            "memory_graph_backlink_membership",
            &mut failures,
        );
        if membership.dependency_digest != memory_graph_backlink_membership_digest(membership) {
            failures.push("memory_graph_backlink_membership_dependency_digest_drift".to_string());
        }
        if !dependency_ref_matches(
            &manifest_backlink_refs,
            &membership.membership_key,
            &membership.dependency_digest,
        ) {
            failures.push("memory_graph_backlink_membership_manifest_dependency_drift".to_string());
        }
        match backlinks.get(&membership.backlink_key) {
            None => failures.push("memory_graph_backlink_document_missing".to_string()),
            Some(backlink)
                if memory_graph_serialized_digest(*backlink) != membership.document_digest =>
            {
                failures.push("memory_graph_backlink_document_dependency_digest_drift".to_string())
            }
            Some(_) => {}
        }
        validate_backlink_reverse_dependencies(
            &membership.node_membership_keys,
            expected_backlink_nodes.get(membership.membership_key.as_str()),
            &manifest_node_refs,
            full_node_closure,
            "memory_graph_backlink_reverse_node_closure_drift",
            &mut failures,
        );
        validate_backlink_reverse_dependencies(
            &membership.edge_membership_keys,
            expected_backlink_edges.get(membership.membership_key.as_str()),
            &manifest_edge_refs,
            full_edge_closure,
            "memory_graph_backlink_reverse_edge_closure_drift",
            &mut failures,
        );
        validate_backlink_reverse_dependencies(
            &membership.index_keys,
            expected_backlink_indexes.get(membership.membership_key.as_str()),
            &manifest_index_refs,
            full_index_closure,
            "memory_graph_backlink_reverse_index_closure_drift",
            &mut failures,
        );
    }

    failures.sort();
    failures.dedup();
    MemoryGraphReadChainValidation {
        verified: failures.is_empty(),
        failures,
    }
}

fn validate_backlink_reverse_dependencies(
    actual: &[String],
    expected_loaded: Option<&BTreeSet<String>>,
    manifest: &BTreeMap<&str, &MemoryGraphDependencyRef>,
    full_closure: bool,
    failure: &str,
    failures: &mut Vec<String>,
) {
    let actual_set = actual.iter().cloned().collect::<BTreeSet<_>>();
    let expected_loaded = expected_loaded.cloned().unwrap_or_default();
    let canonical = actual_set.iter().cloned().collect::<Vec<_>>();
    if canonical != actual
        || actual_set
            .iter()
            .any(|storage_key| !manifest.contains_key(storage_key.as_str()))
        || !expected_loaded.is_subset(&actual_set)
        || (full_closure && actual_set != expected_loaded)
    {
        failures.push(failure.to_string());
    }
}

pub fn validate_memory_graph_scope_manifest(
    manifest: &MemoryGraphScopeManifest,
) -> MemoryGraphReadChainValidation {
    let mut failures = Vec::new();
    if manifest.schema_version != MEMORY_GRAPH_SCHEMA_VERSION {
        failures.push("memory_graph_schema_version_unsupported".to_string());
    }
    if manifest.memory_space_id.trim().is_empty() || manifest.mounted_subject_id.trim().is_empty() {
        failures.push("memory_graph_manifest_scope_missing".to_string());
    }
    if manifest.scope_digest
        != memory_graph_scope_digest(&manifest.memory_space_id, &manifest.mounted_subject_id)
    {
        failures.push("memory_graph_manifest_scope_digest_drift".to_string());
    }
    if manifest.manifest_generation == 0 || manifest.graph_revision.trim().is_empty() {
        failures.push("memory_graph_manifest_revision_missing".to_string());
    }
    if manifest.node_count != manifest.node_memberships.len()
        || manifest.edge_count != manifest.edge_memberships.len()
        || manifest.backlink_count != manifest.backlink_memberships.len()
        || manifest.index_count != manifest.recall_indexes.len()
    {
        failures.push("memory_graph_manifest_count_drift".to_string());
    }
    for refs in [
        manifest.node_memberships.as_slice(),
        manifest.edge_memberships.as_slice(),
        manifest.backlink_memberships.as_slice(),
        manifest.recall_indexes.as_slice(),
    ] {
        if refs.iter().any(|item| {
            item.storage_key.trim().is_empty() || item.dependency_digest.trim().is_empty()
        }) {
            failures.push("memory_graph_manifest_dependency_missing".to_string());
        }
        let unique = refs
            .iter()
            .map(|item| item.storage_key.as_str())
            .collect::<BTreeSet<_>>();
        if unique.len() != refs.len() {
            failures.push("memory_graph_manifest_dependency_duplicate".to_string());
        }
    }
    if manifest.revision.storage_key.trim().is_empty()
        || manifest.revision.dependency_digest.trim().is_empty()
    {
        failures.push("memory_graph_manifest_revision_dependency_missing".to_string());
    }
    if manifest.dependency_digest != memory_graph_manifest_digest(manifest) {
        failures.push("memory_graph_manifest_dependency_digest_drift".to_string());
    }
    failures.sort();
    failures.dedup();
    MemoryGraphReadChainValidation {
        verified: failures.is_empty(),
        failures,
    }
}

pub fn validate_memory_graph_revision_doc(
    manifest: &MemoryGraphScopeManifest,
    revision: &MemoryGraphRevisionDoc,
) -> MemoryGraphReadChainValidation {
    let mut failures = Vec::new();
    append_graph_scope_binding_failures(revision, manifest, "memory_graph_revision", &mut failures);
    if revision.node_count != manifest.node_count
        || revision.edge_count != manifest.edge_count
        || revision.backlink_count != manifest.backlink_count
        || revision.index_count != manifest.index_count
    {
        failures.push("memory_graph_revision_count_drift".to_string());
    }
    if revision.revision_key != manifest.revision.storage_key {
        failures.push("memory_graph_revision_key_drift".to_string());
    }
    if revision.dependency_digest != memory_graph_revision_digest(revision)
        || revision.dependency_digest != manifest.revision.dependency_digest
    {
        failures.push("memory_graph_revision_dependency_digest_drift".to_string());
    }
    failures.sort();
    failures.dedup();
    MemoryGraphReadChainValidation {
        verified: failures.is_empty(),
        failures,
    }
}

fn append_duplicate_graph_id_failures<I, S>(values: I, failure: &str, failures: &mut Vec<String>)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value.as_ref().to_string()) {
            failures.push(failure.to_string());
        }
    }
}

fn graph_recall_dependency_ids(
    source_node_id: &str,
    nodes_by_id: &BTreeMap<&str, &MemoryGraphNode>,
    recall_adjacency: &BTreeMap<&str, Vec<(&str, &str)>>,
) -> (Vec<String>, Vec<String>) {
    let mut node_ids = BTreeSet::from([source_node_id.to_string()]);
    let mut edge_ids = BTreeSet::new();
    let direct_neighbors = recall_adjacency
        .get(source_node_id)
        .into_iter()
        .flatten()
        .map(|(neighbor_id, edge_id)| {
            node_ids.insert((*neighbor_id).to_string());
            edge_ids.insert((*edge_id).to_string());
            *neighbor_id
        })
        .collect::<BTreeSet<_>>();
    for neighbor in direct_neighbors {
        if let Some(second_hop) = recall_adjacency.get(neighbor) {
            for (second_hop_id, edge_id) in second_hop {
                node_ids.insert((*second_hop_id).to_string());
                edge_ids.insert((*edge_id).to_string());
            }
        }
    }
    node_ids.retain(|node_id| nodes_by_id.contains_key(node_id.as_str()));
    (
        node_ids.into_iter().collect(),
        edge_ids.into_iter().collect(),
    )
}

fn memory_graph_serialized_digest<T: Serialize>(value: &T) -> String {
    let serialized = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    memory_graph_sha256(
        "memory_graph_serialized_dependency_v4",
        &[serialized.as_bytes()],
    )
}

fn memory_graph_node_membership_digest(membership: &MemoryGraphNodeMembership) -> String {
    let mut value = membership.clone();
    value.dependency_digest.clear();
    memory_graph_serialized_digest(&value)
}

fn memory_graph_edge_membership_digest(membership: &MemoryGraphEdgeMembership) -> String {
    let mut value = membership.clone();
    value.dependency_digest.clear();
    memory_graph_serialized_digest(&value)
}

fn memory_graph_backlink_membership_digest(membership: &MemoryGraphBacklinkMembership) -> String {
    let mut value = membership.clone();
    value.dependency_digest.clear();
    memory_graph_serialized_digest(&value)
}

fn memory_graph_recall_index_digest(index: &MemoryGraphRecallIndexDoc) -> String {
    let mut value = index.clone();
    value.dependency_digest.clear();
    memory_graph_serialized_digest(&value)
}

fn memory_graph_revision_digest(revision: &MemoryGraphRevisionDoc) -> String {
    let mut value = revision.clone();
    value.dependency_digest.clear();
    memory_graph_serialized_digest(&value)
}

fn memory_graph_manifest_digest(manifest: &MemoryGraphScopeManifest) -> String {
    let mut value = manifest.clone();
    value.dependency_digest.clear();
    memory_graph_serialized_digest(&value)
}

fn dependency_ref_map(
    refs: &[MemoryGraphDependencyRef],
) -> BTreeMap<&str, &MemoryGraphDependencyRef> {
    refs.iter()
        .map(|item| (item.storage_key.as_str(), item))
        .collect()
}

fn dependency_ref_matches(
    refs: &BTreeMap<&str, &MemoryGraphDependencyRef>,
    storage_key: &str,
    dependency_digest: &str,
) -> bool {
    refs.get(storage_key)
        .is_some_and(|item| item.dependency_digest == dependency_digest)
}

trait MemoryGraphScopeBinding {
    fn schema_version(&self) -> u32;
    fn memory_space_id(&self) -> &str;
    fn mounted_subject_id(&self) -> &str;
    fn scope_digest(&self) -> &str;
    fn manifest_generation(&self) -> u64;
    fn graph_revision(&self) -> &str;
}

macro_rules! impl_memory_graph_scope_binding {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl MemoryGraphScopeBinding for $ty {
                fn schema_version(&self) -> u32 { self.schema_version }
                fn memory_space_id(&self) -> &str { &self.memory_space_id }
                fn mounted_subject_id(&self) -> &str { &self.mounted_subject_id }
                fn scope_digest(&self) -> &str { &self.scope_digest }
                fn manifest_generation(&self) -> u64 { self.manifest_generation }
                fn graph_revision(&self) -> &str { &self.graph_revision }
            }
        )+
    };
}

impl_memory_graph_scope_binding!(
    MemoryGraphRecallIndexDoc,
    MemoryGraphNodeMembership,
    MemoryGraphEdgeMembership,
    MemoryGraphBacklinkMembership,
    MemoryGraphRevisionDoc,
);

fn append_graph_scope_binding_failures(
    binding: &impl MemoryGraphScopeBinding,
    manifest: &MemoryGraphScopeManifest,
    prefix: &str,
    failures: &mut Vec<String>,
) {
    if binding.schema_version() != MEMORY_GRAPH_SCHEMA_VERSION {
        failures.push(format!("{prefix}_schema_version_unsupported"));
    }
    if binding.memory_space_id() != manifest.memory_space_id
        || binding.mounted_subject_id() != manifest.mounted_subject_id
        || binding.scope_digest() != manifest.scope_digest
    {
        failures.push(format!("{prefix}_scope_drift"));
    }
    if binding.manifest_generation() != manifest.manifest_generation {
        failures.push(format!("{prefix}_manifest_generation_drift"));
    }
    if binding.graph_revision() != manifest.graph_revision {
        failures.push(format!("{prefix}_graph_revision_drift"));
    }
}

fn validate_index_membership_refs<'a, T>(
    refs: &[MemoryGraphDependencyRef],
    manifest_refs: &BTreeMap<&str, &MemoryGraphDependencyRef>,
    loaded: &BTreeMap<&'a str, &'a T>,
    missing_failure: &str,
    failures: &mut Vec<String>,
) {
    for dependency in refs {
        if !dependency_ref_matches(
            manifest_refs,
            &dependency.storage_key,
            &dependency.dependency_digest,
        ) {
            failures.push("memory_graph_index_membership_dependency_drift".to_string());
        }
        if !loaded.contains_key(dependency.storage_key.as_str()) {
            failures.push(missing_failure.to_string());
        }
    }
}

fn memory_graph_sha256(domain: &str, parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update((domain.len() as u64).to_be_bytes());
    hasher.update(domain.as_bytes());
    hasher.update((parts.len() as u64).to_be_bytes());
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    format!("sha256:{:x}", hasher.finalize())
}

pub fn rerank_recall_with_temporal_graph(
    query: impl Into<String>,
    candidate_ids: Vec<String>,
    graph: &TemporalMemoryGraphBuildReport,
    expansion_budget: GraphRecallExpansionBudget,
) -> GraphRecallRerankReport {
    rerank_recall_with_temporal_graph_and_facets(
        query,
        candidate_ids,
        graph,
        expansion_budget,
        &GraphFacetPropagationContext::default(),
    )
}

pub fn rerank_recall_with_temporal_graph_and_facets(
    query: impl Into<String>,
    candidate_ids: Vec<String>,
    graph: &TemporalMemoryGraphBuildReport,
    expansion_budget: GraphRecallExpansionBudget,
    facet_context: &GraphFacetPropagationContext,
) -> GraphRecallRerankReport {
    let query = query.into();
    let stale_false_positive_count = candidate_ids
        .iter()
        .filter(|candidate_id| graph_node_is_superseded(candidate_id, graph))
        .count() as u32;
    let expansion = expand_recall_candidates(
        &query,
        &candidate_ids,
        graph,
        expansion_budget,
        facet_context,
    );

    let mut score_breakdown = expansion
        .expanded_candidate_ids
        .iter()
        .map(|candidate_id| {
            graph_recall_candidate_score_with_facets(&query, candidate_id, graph, facet_context)
        })
        .collect::<Vec<_>>();
    score_breakdown.sort_by(|left, right| {
        right
            .total_score
            .cmp(&left.total_score)
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });
    let reranked_candidate_ids = score_breakdown
        .iter()
        .map(|score| score.candidate_id.clone())
        .collect::<Vec<_>>();

    GraphRecallRerankReport {
        query,
        candidate_ids,
        expanded_candidate_ids: expansion.expanded_candidate_ids,
        graph_neighbor_ids: expansion.graph_neighbor_ids,
        reranked_candidate_ids,
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
    facet_context: &GraphFacetPropagationContext,
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
            let neighbors = graph_expansion_neighbors(query, source_id, graph, facet_context);
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
            graph_expansion_neighbors(query, node_id, graph, facet_context)
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
    facet_context: &GraphFacetPropagationContext,
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
        graph_expansion_neighbor_score(query, right, graph, right_node, facet_context)
            .cmp(&graph_expansion_neighbor_score(
                query,
                left,
                graph,
                left_node,
                facet_context,
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
    _node: Option<&MemoryGraphNode>,
    facet_context: &GraphFacetPropagationContext,
) -> u32 {
    graph_recall_candidate_score_with_facets(query, node_id, graph, facet_context).total_score
}

fn graph_facet_exact_score(
    node_id: &str,
    graph: &TemporalMemoryGraphBuildReport,
    facet_context: &GraphFacetPropagationContext,
) -> u32 {
    if facet_context
        .exact_anchor_ids
        .iter()
        .any(|anchor| anchor == node_id)
    {
        700
    } else if graph_node_connected_to_any(node_id, &facet_context.exact_anchor_ids, graph) {
        350
    } else {
        0
    }
}

fn graph_facet_expanded_score(
    node_id: &str,
    graph: &TemporalMemoryGraphBuildReport,
    facet_context: &GraphFacetPropagationContext,
) -> u32 {
    if facet_context
        .expanded_anchor_ids
        .iter()
        .any(|anchor| anchor == node_id)
    {
        350
    } else if graph_node_connected_to_any(node_id, &facet_context.expanded_anchor_ids, graph) {
        175
    } else {
        0
    }
}

fn graph_node_connected_to_any(
    node_id: &str,
    anchors: &[String],
    graph: &TemporalMemoryGraphBuildReport,
) -> bool {
    graph.edges.iter().any(|edge| {
        anchors.iter().any(|anchor| {
            (edge.from_node_id == node_id && edge.to_node_id == *anchor)
                || (edge.to_node_id == node_id && edge.from_node_id == *anchor)
        })
    })
}

fn graph_facet_evidence_groups(
    node_id: &str,
    graph: &TemporalMemoryGraphBuildReport,
    facet_context: &GraphFacetPropagationContext,
) -> Vec<String> {
    let mut groups = Vec::new();
    if let Some(candidate_groups) = facet_context.candidate_evidence_groups.get(node_id) {
        for group in candidate_groups {
            push_unique(&mut groups, group.clone());
        }
    }
    for anchor_id in facet_context
        .exact_anchor_ids
        .iter()
        .chain(facet_context.expanded_anchor_ids.iter())
    {
        if !graph_node_connected_to_any(node_id, std::slice::from_ref(anchor_id), graph) {
            continue;
        }
        if let Some(candidate_groups) = facet_context.candidate_evidence_groups.get(anchor_id) {
            for group in candidate_groups {
                push_unique(&mut groups, group.clone());
            }
        }
    }
    groups
}

fn graph_facet_authority_score(
    node_id: &str,
    graph: &TemporalMemoryGraphBuildReport,
    facet_context: &GraphFacetPropagationContext,
    base_source_authority_score: u32,
    has_facet_groups: bool,
) -> u32 {
    if !has_facet_groups
        && !facet_context
            .exact_anchor_ids
            .iter()
            .chain(facet_context.expanded_anchor_ids.iter())
            .any(|anchor| anchor == node_id)
    {
        return 0;
    }
    let graph_authority = graph
        .nodes
        .iter()
        .find(|node| node.node_id == node_id)
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
    graph_authority.max(base_source_authority_score) / 2
}

fn graph_facet_diversity_score(
    facet_groups: &[String],
    facet_context: &GraphFacetPropagationContext,
) -> u32 {
    if facet_groups.is_empty() {
        return 0;
    }
    let covered = facet_groups
        .iter()
        .filter(|group| {
            facet_context
                .covered_evidence_groups
                .iter()
                .any(|covered| covered == *group)
        })
        .count() as u32;
    if covered == 0 {
        120
    } else {
        180u32.saturating_add(covered.saturating_sub(1).saturating_mul(60))
    }
}

fn graph_facet_temporal_score(
    node_id: &str,
    graph: &TemporalMemoryGraphBuildReport,
    facet_context: &GraphFacetPropagationContext,
) -> u32 {
    let observed_at = graph_facet_observed_at(node_id, graph, facet_context);
    if observed_at == 0 {
        return 0;
    }
    80u32.saturating_add((observed_at.min(10_000) as u32) / 100)
}

fn graph_facet_observed_at(
    node_id: &str,
    graph: &TemporalMemoryGraphBuildReport,
    facet_context: &GraphFacetPropagationContext,
) -> u64 {
    let mut observed_at = facet_context
        .candidate_observed_at
        .get(node_id)
        .copied()
        .unwrap_or(0);
    for anchor_id in facet_context
        .exact_anchor_ids
        .iter()
        .chain(facet_context.expanded_anchor_ids.iter())
    {
        if !graph_node_connected_to_any(node_id, std::slice::from_ref(anchor_id), graph) {
            continue;
        }
        observed_at = observed_at.max(
            facet_context
                .candidate_observed_at
                .get(anchor_id)
                .copied()
                .unwrap_or(0),
        );
    }
    observed_at
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
    graph_recall_candidate_score_with_facets(
        query,
        node_id,
        graph,
        &GraphFacetPropagationContext::default(),
    )
}

fn graph_recall_candidate_score_with_facets(
    query: &str,
    node_id: &str,
    graph: &TemporalMemoryGraphBuildReport,
    facet_context: &GraphFacetPropagationContext,
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
                            .map(|_| 1)
                    })
                    .max()
            })
        })
        .unwrap_or(0);
    let lexical_score = lexical_graph_score(query, node);
    let graph_neighborhood_score = (connected_edges.len() as u32).saturating_mul(100);
    let temporal_validity_score = observed_rank.min(10_000) as u32;
    let entity_alias_score = graph_entity_alias_score(query, node);
    let temporal_anchor_score = graph_temporal_anchor_score(query, node);
    let session_alias_score = graph_session_alias_score(query, node);
    let temporal_reasoning_score = graph_temporal_reasoning_score(node_id, graph);
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
    let multi_evidence_coverage_score = node
        .map(|node| graph_multi_evidence_coverage_score(graph_evidence_group_count(node)))
        .unwrap_or(0);
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
    let facet_exact_score = graph_facet_exact_score(node_id, graph, facet_context);
    let facet_expanded_score = graph_facet_expanded_score(node_id, graph, facet_context);
    let facet_groups = graph_facet_evidence_groups(node_id, graph, facet_context);
    let facet_authority_score = graph_facet_authority_score(
        node_id,
        graph,
        facet_context,
        source_authority_score,
        !facet_groups.is_empty(),
    );
    let facet_diversity_score = graph_facet_diversity_score(&facet_groups, facet_context);
    let facet_temporal_score = graph_facet_temporal_score(node_id, graph, facet_context);
    let privacy_profile_eligibility_score = if node
        .map(|node| node.validate_contract().accepted)
        .unwrap_or(false)
    {
        100
    } else {
        0
    };
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
    let facet_stale_penalty =
        if !facet_context.is_empty() && graph_node_is_superseded(node_id, graph) {
            2_500
        } else {
            0
        };
    let total_score = lexical_score
        .saturating_add(entity_alias_score)
        .saturating_add(temporal_anchor_score)
        .saturating_add(session_alias_score)
        .saturating_add(graph_neighborhood_score)
        .saturating_add(temporal_validity_score)
        .saturating_add(temporal_reasoning_score)
        .saturating_add(evidence_quality_score)
        .saturating_add(multi_evidence_coverage_score)
        .saturating_add(source_authority_score)
        .saturating_add(facet_exact_score)
        .saturating_add(facet_expanded_score)
        .saturating_add(facet_authority_score)
        .saturating_add(facet_diversity_score)
        .saturating_add(facet_temporal_score)
        .saturating_add(privacy_profile_eligibility_score)
        .saturating_add(supersedes_bonus)
        .saturating_sub(facet_stale_penalty)
        .saturating_sub(stale_superseded_penalty);

    GraphRecallCandidateScore {
        candidate_id: node_id.to_string(),
        lexical_score,
        entity_alias_score,
        temporal_anchor_score,
        session_alias_score,
        graph_neighborhood_score,
        temporal_validity_score,
        temporal_reasoning_score,
        evidence_quality_score,
        multi_evidence_coverage_score,
        source_authority_score,
        facet_exact_score,
        facet_expanded_score,
        facet_authority_score,
        facet_diversity_score,
        facet_temporal_score,
        facet_stale_penalty,
        privacy_profile_eligibility_score,
        stale_superseded_penalty,
        total_score,
    }
}

fn lexical_graph_score(query: &str, node: Option<&MemoryGraphNode>) -> u32 {
    let Some(node) = node else {
        return 0;
    };
    let haystack = graph_node_retrieval_text(node);
    collect_retrieval_terms(query, 2, 32, &[2, 3])
        .into_iter()
        .filter(|term| haystack.contains(term))
        .count() as u32
        * 25
}

fn graph_node_retrieval_text(node: &MemoryGraphNode) -> String {
    let mut parts = vec![node.node_id.as_str(), node.label.as_str()];
    parts.extend(node.evidence_refs.iter().map(String::as_str));
    let normalized = normalize_retrieval_text(&parts.join(" "));
    let compact = normalized.replace(' ', "");
    if compact.is_empty() {
        normalized
    } else {
        format!("{normalized} {compact}")
    }
}

fn graph_entity_alias_score(query: &str, node: Option<&MemoryGraphNode>) -> u32 {
    let Some(node) = node else {
        return 0;
    };
    let haystack = graph_node_retrieval_text(node);
    collect_retrieval_terms(query, 2, 32, &[2, 3])
        .into_iter()
        .filter(|term| !graph_anchor_is_noise(term))
        .filter(|term| !graph_anchor_is_temporal(term))
        .filter(|term| haystack.contains(term))
        .fold(0u32, |score, term| {
            score.saturating_add(if term.len() >= 4 { 80 } else { 40 })
        })
        .min(400)
}

fn graph_temporal_anchor_score(query: &str, node: Option<&MemoryGraphNode>) -> u32 {
    let Some(node) = node else {
        return 0;
    };
    let haystack = graph_node_retrieval_text(node);
    collect_retrieval_terms(query, 2, 32, &[2, 3])
        .into_iter()
        .filter(|term| graph_anchor_is_temporal(term))
        .filter(|term| haystack.contains(term))
        .fold(0u32, |score, term| {
            score.saturating_add(if term.len() >= 4 { 120 } else { 45 })
        })
        .min(360)
}

fn graph_session_alias_score(query: &str, node: Option<&MemoryGraphNode>) -> u32 {
    let Some(node) = node else {
        return 0;
    };
    let haystack = graph_node_retrieval_text(node);
    let normalized_query = normalize_retrieval_text(query);
    let compact_query = normalized_query.replace(' ', "");
    let mut score = 0u32;
    for term in collect_retrieval_terms(query, 2, 32, &[2, 3]) {
        if (term.starts_with("session") || term.starts_with('d')) && haystack.contains(&term) {
            score = score.saturating_add(120);
        }
    }
    if !compact_query.is_empty() && haystack.contains(&compact_query) {
        score = score.saturating_add(160);
    }
    score.min(360)
}

fn graph_temporal_reasoning_score(node_id: &str, graph: &TemporalMemoryGraphBuildReport) -> u32 {
    graph
        .edges
        .iter()
        .filter(|edge| edge.from_node_id == node_id || edge.to_node_id == node_id)
        .map(|edge| {
            let mut score = edge.validity.observed_at.min(10_000) as u32 / 10;
            if edge.validity.valid_until.is_none() && edge.validity.superseded_by.is_none() {
                score = score.saturating_add(80);
            }
            if matches!(edge.kind, MemoryGraphEdgeKind::Supersedes) && edge.from_node_id == node_id
            {
                score = score.saturating_add(120);
            }
            score
        })
        .max()
        .unwrap_or(0)
}

fn graph_evidence_group_count(node: &MemoryGraphNode) -> u32 {
    let mut groups: Vec<String> = Vec::new();
    for evidence_ref in &node.evidence_refs {
        let group = canonical_recall_evidence_group(evidence_ref);
        if !group.is_empty() && !groups.iter().any(|existing| existing == &group) {
            groups.push(group);
        }
    }
    groups.len() as u32
}

fn graph_multi_evidence_coverage_score(group_count: u32) -> u32 {
    if group_count == 0 {
        0
    } else {
        120u32.saturating_add(group_count.saturating_sub(1).saturating_mul(40))
    }
}

fn graph_anchor_is_temporal(term: &str) -> bool {
    let digits = term.chars().filter(|ch| ch.is_ascii_digit()).count();
    (term.len() == 4 && digits == 4 && ("1900"..="2100").contains(&term))
        || (term.len() == 2
            && digits == 2
            && term
                .parse::<u8>()
                .is_ok_and(|value| (1..=31).contains(&value)))
}

fn graph_anchor_is_noise(term: &str) -> bool {
    matches!(
        term,
        "what"
            | "when"
            | "where"
            | "which"
            | "that"
            | "this"
            | "with"
            | "from"
            | "into"
            | "was"
            | "were"
            | "the"
            | "and"
            | "for"
            | "target"
            | "session"
            | "evidence"
            | "packet"
    )
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
    let graph_observed_at = graph_reference_observed_at(graph);
    graph.edges.iter().any(|edge| {
        let touches_node = edge.from_node_id == node_id || edge.to_node_id == node_id;
        (edge.kind == MemoryGraphEdgeKind::Supersedes && edge.to_node_id == node_id)
            || (touches_node
                && edge
                    .validity
                    .superseded_by
                    .as_deref()
                    .is_some_and(|superseded_by| {
                        !superseded_by.trim().is_empty() && superseded_by != node_id
                    }))
            || (touches_node
                && edge
                    .validity
                    .valid_until
                    .is_some_and(|valid_until| valid_until < graph_observed_at))
    })
}

fn graph_reference_observed_at(graph: &TemporalMemoryGraphBuildReport) -> u64 {
    graph
        .edges
        .iter()
        .map(|edge| edge.validity.observed_at)
        .max()
        .unwrap_or(0)
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProceduralMemoryPromotionInput {
    pub task_id: String,
    pub learning_id: String,
    pub learning_digest: String,
    pub trigger: String,
    pub procedure: String,
    pub constraints: Vec<String>,
    pub failure_modes: Vec<String>,
    pub counterfactual_fix: String,
    pub evidence_refs: Vec<String>,
    pub quality_score: u8,
    pub repeated_evidence_count: usize,
    pub capability_affinity: Vec<String>,
    pub privacy_class: MemoryPrivacyClass,
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
    if !(RuntimeSkillCreationRef::TaskLearningPromotion {
        learning_id: input.learning_id.clone(),
        learning_digest: input.learning_digest.clone(),
    })
    .validate_contract()
    {
        blocked_reasons.push("procedural_learning_authority_invalid".to_string());
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

#[cfg(test)]
mod graph_read_chain_tests {
    use super::*;

    #[test]
    fn read_chain_rebuilds_backlink_reverse_node_closure_instead_of_trusting_digests() {
        let evidence_ref = "evidence:reverse-closure";
        let nodes = vec![
            MemoryGraphNode {
                node_id: "node:reverse-a".to_string(),
                kind: MemoryGraphNodeKind::MemoryRecord,
                label: "Reverse A".to_string(),
                evidence_refs: vec![evidence_ref.to_string()],
            },
            MemoryGraphNode {
                node_id: "node:reverse-b".to_string(),
                kind: MemoryGraphNodeKind::MemoryRecord,
                label: "Reverse B".to_string(),
                evidence_refs: vec![evidence_ref.to_string()],
            },
        ];
        let edges = vec![MemoryGraphEdge {
            edge_id: "edge:reverse-a-b".to_string(),
            kind: MemoryGraphEdgeKind::Supports,
            from_node_id: nodes[0].node_id.clone(),
            to_node_id: nodes[1].node_id.clone(),
            validity: TemporalValidity {
                valid_from: 10,
                valid_until: None,
                observed_at: 10,
                superseded_by: None,
            },
            evidence_refs: vec![evidence_ref.to_string()],
        }];
        let backlinks = vec![EvidenceBacklink {
            source_kind: "conversation_transcript".to_string(),
            source_id: evidence_ref.to_string(),
            fingerprint: "fp:reverse-closure".to_string(),
        }];
        let owners = nodes
            .iter()
            .map(|node| MemoryGraphOwnerBinding {
                node_id: node.node_id.clone(),
                owner_ref: GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::LongTerm,
                    format!("owner:{}", node.node_id),
                ),
                owner_revision: 1,
                visible: true,
            })
            .collect::<Vec<_>>();
        let mut plan = build_memory_graph_persistence_plan(
            "space:reverse",
            "subject:reverse",
            1,
            nodes.clone(),
            edges.clone(),
            backlinks.clone(),
            owners.clone(),
        );
        assert!(plan.accepted, "{:?}", plan.failures);

        let backlink_membership = plan
            .backlink_memberships
            .first_mut()
            .expect("backlink membership");
        backlink_membership.node_membership_keys.clear();
        backlink_membership.dependency_digest =
            memory_graph_backlink_membership_digest(backlink_membership);
        let backlink_key = backlink_membership.membership_key.clone();
        let backlink_digest = backlink_membership.dependency_digest.clone();
        for index in &mut plan.recall_indexes {
            for dependency in &mut index.backlink_memberships {
                if dependency.storage_key == backlink_key {
                    dependency.dependency_digest = backlink_digest.clone();
                }
            }
            index.dependency_digest = memory_graph_recall_index_digest(index);
        }
        let index_digests = plan
            .recall_indexes
            .iter()
            .map(|index| (index.index_key.clone(), index.dependency_digest.clone()))
            .collect::<BTreeMap<_, _>>();
        let manifest = plan.scope_manifest.as_mut().expect("scope manifest");
        manifest
            .backlink_memberships
            .iter_mut()
            .find(|dependency| dependency.storage_key == backlink_key)
            .expect("manifest backlink dependency")
            .dependency_digest = backlink_digest;
        for dependency in &mut manifest.recall_indexes {
            dependency.dependency_digest = index_digests
                .get(&dependency.storage_key)
                .expect("index dependency")
                .clone();
        }
        manifest.dependency_digest = memory_graph_manifest_digest(manifest);

        let validation = validate_memory_graph_read_chain(
            manifest,
            &plan.recall_indexes,
            &plan.node_memberships,
            &plan.edge_memberships,
            &plan.backlink_memberships,
            &nodes,
            &edges,
            &backlinks,
            &owners,
        );
        assert!(!validation.verified);
        assert!(validation
            .failures
            .contains(&"memory_graph_backlink_reverse_node_closure_drift".to_string()));
    }
}
