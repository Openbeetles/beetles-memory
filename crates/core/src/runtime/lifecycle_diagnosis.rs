use serde::{Deserialize, Serialize};

use crate::platform::MemoryOperatorSurfaceSummary;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLifecycleEvidence {
    pub key: String,
    pub value: String,
}

impl RuntimeLifecycleEvidence {
    pub fn new(key: impl Into<String>, value: impl ToString) -> Self {
        Self {
            key: key.into(),
            value: value.to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLifecycleFinding {
    pub kind: String,
    pub message: String,
}

impl RuntimeLifecycleFinding {
    fn observed(message: impl Into<String>) -> Self {
        Self {
            kind: "observed".to_string(),
            message: message.into(),
        }
    }

    fn correlated(message: impl Into<String>) -> Self {
        Self {
            kind: "correlated".to_string(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLifecycleRootCause {
    pub code: String,
    pub summary: String,
    pub confidence: String,
}

impl RuntimeLifecycleRootCause {
    fn new(
        code: impl Into<String>,
        summary: impl Into<String>,
        confidence: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            summary: summary.into(),
            confidence: confidence.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLifecycleRecommendedAction {
    pub code: String,
    pub summary: String,
}

impl RuntimeLifecycleRecommendedAction {
    fn new(code: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            summary: summary.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLifecycleDiagnosisReport {
    pub summary: String,
    pub evidence: Vec<RuntimeLifecycleEvidence>,
    pub findings: Vec<RuntimeLifecycleFinding>,
    pub root_causes: Vec<RuntimeLifecycleRootCause>,
    pub recommended_actions: Vec<RuntimeLifecycleRecommendedAction>,
    pub degraded_by: Vec<String>,
    pub safe_actions_available: Vec<String>,
}

pub fn build_runtime_lifecycle_diagnosis(
    surface: &MemoryOperatorSurfaceSummary,
) -> RuntimeLifecycleDiagnosisReport {
    let mut findings = Vec::new();
    let mut root_causes = Vec::new();
    let mut recommended_actions = Vec::new();
    let mut degraded_by = Vec::new();
    let mut safe_actions_available = vec![
        "inspect_memory_status".to_string(),
        "inspect_personality_governance".to_string(),
        "inspect_runtime_governance_gate".to_string(),
        "inspect_memory_forge".to_string(),
        "inspect_soul_growth_promotion_gate".to_string(),
        "inspect_soul_feedback_projection".to_string(),
        "inspect_humanization_spine".to_string(),
    ];
    if surface.inspect.continuity_snapshot_supported {
        safe_actions_available.push("inspect_continuity_snapshot".to_string());
    } else {
        degraded_by.push("continuity_snapshot_tooling_unavailable".to_string());
    }

    let evidence = vec![
        RuntimeLifecycleEvidence::new(
            "memory_system_kind",
            surface.inspect.memory_system_kind.as_str(),
        ),
        RuntimeLifecycleEvidence::new("runtime_skill_count", surface.inspect.runtime_skill_count),
        RuntimeLifecycleEvidence::new("long_term_count", surface.inspect.long_term_count),
        RuntimeLifecycleEvidence::new(
            "continuity_capsule_count",
            surface.inspect.continuity_capsule_count,
        ),
        RuntimeLifecycleEvidence::new(
            "continuity_snapshot_supported",
            surface.inspect.continuity_snapshot_supported,
        ),
        RuntimeLifecycleEvidence::new(
            "humanization_spine_present",
            surface.inspect.humanization_spine_present,
        ),
        RuntimeLifecycleEvidence::new(
            "subject_shell_grounded",
            surface.inspect.subject_shell_grounded,
        ),
        RuntimeLifecycleEvidence::new(
            "felt_significance_present",
            surface.inspect.felt_significance_present,
        ),
        RuntimeLifecycleEvidence::new("repair_needed", surface.repair.repair_needed),
        RuntimeLifecycleEvidence::new("primary_action", surface.repair.primary_action.as_str()),
        RuntimeLifecycleEvidence::new("board_review_due", surface.diff.board_review_due),
        RuntimeLifecycleEvidence::new(
            "relationship_needs_runtime_attention",
            surface.diff.relationship_needs_runtime_attention,
        ),
        RuntimeLifecycleEvidence::new("drift_flag_count", surface.diff.drift_flags.len()),
        RuntimeLifecycleEvidence::new("outstanding_count", surface.diff.outstanding.len()),
    ];

    let mut summary = "Memory runtime lifecycle snapshot is stable.".to_string();
    if surface.repair.repair_needed {
        summary = format!(
            "Memory runtime lifecycle recommends {}.",
            surface.repair.primary_action
        );
        findings.push(RuntimeLifecycleFinding::observed(
            "memory governance repair is currently required",
        ));
        root_causes.push(RuntimeLifecycleRootCause::new(
            "memory_runtime_repair_needed",
            "memory runtime governance has an active repair plan for the current board or relationship state",
            "high",
        ));
        recommended_actions.push(RuntimeLifecycleRecommendedAction::new(
            "inspect_memory_status",
            "inspect memory operator status, repair reasons, and active relationship targets",
        ));
    }

    if surface.diff.board_review_due
        || surface.diff.relationship_needs_runtime_attention
        || !surface.diff.drift_flags.is_empty()
        || !surface.diff.outstanding.is_empty()
    {
        findings.push(RuntimeLifecycleFinding::correlated(
            "memory governance drift or outstanding closure work is present",
        ));
        root_causes.push(RuntimeLifecycleRootCause::new(
            "memory_governance_drift",
            "review debt, relationship drift, or unresolved closure work is affecting memory runtime stability",
            "medium",
        ));
        recommended_actions.push(RuntimeLifecycleRecommendedAction::new(
            "inspect_memory_governance_drift",
            "review drift flags, outstanding closure items, and relationship runtime attention markers",
        ));
    }

    let humanization_spine_has_signal = surface.inspect.subject_shell_grounded
        || surface.inspect.felt_significance_present
        || surface.soul_governance_view.temperament_continuity_present
        || surface.soul_governance_view.subjective_projection_present
        || surface.soul_governance_view.active_inner_conflict_count > 0;
    if humanization_spine_has_signal && !surface.inspect.humanization_spine_present {
        findings.push(RuntimeLifecycleFinding::correlated(
            "humanization spine is partially present but not complete",
        ));
        root_causes.push(RuntimeLifecycleRootCause::new(
            "humanization_spine_incomplete",
            "subject shell grounding, felt significance, temperament continuity, or subjective projection is missing from the governed humanization spine",
            "medium",
        ));
        recommended_actions.push(RuntimeLifecycleRecommendedAction::new(
            "inspect_humanization_spine",
            "inspect subject-shell grounding and subjective projection before treating identity behavior as model wording",
        ));
    }

    if surface.inspect.long_term_count == 0
        && surface.inspect.continuity_capsule_count == 0
        && surface.inspect.runtime_skill_count == 0
    {
        findings.push(RuntimeLifecycleFinding::correlated(
            "persistent memory state is currently sparse",
        ));
        root_causes.push(RuntimeLifecycleRootCause::new(
            "sparse_persistent_memory",
            "persistent memory stores are sparse, so recall depth may be limited even when runtime lifecycle is healthy",
            "low",
        ));
        recommended_actions.push(RuntimeLifecycleRecommendedAction::new(
            "inspect_memory_population",
            "confirm whether the runtime is freshly initialized or persistence is unexpectedly empty",
        ));
    }

    RuntimeLifecycleDiagnosisReport {
        summary,
        evidence,
        findings,
        root_causes,
        recommended_actions,
        degraded_by,
        safe_actions_available,
    }
}
