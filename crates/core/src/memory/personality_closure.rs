//! Personality governance inspection and closure gate.
//! 人格治理检查与人格封板门。

use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::{
    audit_relationship_constitution, build_core_revision_timeline,
    compute_core_revision_governance_digest, CoreRevisionGovernanceDigest, CoreRevisionLedger,
    CoreRevisionTimelineEntry, RecentPersonaEvidence, RelationshipConstitution,
    RelationshipConstitutionAudit, RelationshipTopology, SelfAuthoredCore,
};

const PERSONALITY_CLOSURE_TEXT_MAX_CHARS: usize = 160;
const PERSONALITY_CLOSURE_OUTSTANDING_MAX: usize = 6;
const PERSONALITY_CLOSURE_EVENT_LIMIT: usize = 8;
const PERSONALITY_MIN_EVIDENCE_TURNS: usize = 4;
const PERSONALITY_RUNTIME_GATE_REASON_MAX_CHARS: usize = 220;
const PERSONALITY_REPAIR_SUMMARY_MAX_CHARS: usize = 160;

/// Absence of a loaded constitution never proves that no relationship applies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PersonalityRelationshipScope<'a> {
    #[default]
    Unverified,
    SubjectOnly,
    Active(&'a str),
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersonalityRelationshipApplicability {
    #[default]
    Unverified,
    NotApplicable,
    Required,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonalityGovernanceEvent {
    #[serde(default)]
    pub layer: String,
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub summary: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonalityClosureReport {
    pub relationship_applicability: PersonalityRelationshipApplicability,
    /// Initial evidence-led model formation only; does not authorize normal promotion.
    pub subject_bootstrap_ready: bool,
    pub subject_distillation_ready: bool,
    pub relationship_expression_ready: bool,
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub board_core_ready: bool,
    #[serde(default)]
    pub revision_governance_ready: bool,
    #[serde(default)]
    pub relationship_governance_ready: bool,
    #[serde(default)]
    pub evidence_loop_ready: bool,
    #[serde(default)]
    pub drift_control_ready: bool,
    #[serde(default)]
    pub observation_control_ready: bool,
    #[serde(default)]
    pub review_cadence_ready: bool,
    #[serde(default)]
    pub outstanding: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonalityGovernanceInspection {
    #[serde(default)]
    pub subject_id: String,
    #[serde(default)]
    pub relationship_scope_id: String,
    #[serde(default)]
    pub core_revision_governance: CoreRevisionGovernanceDigest,
    #[serde(default)]
    pub core_revision_timeline: Vec<CoreRevisionTimelineEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_audit: Option<RelationshipConstitutionAudit>,
    #[serde(default)]
    pub governance_events: Vec<PersonalityGovernanceEvent>,
    #[serde(default)]
    pub closure: PersonalityClosureReport,
    #[serde(default)]
    pub repair_plan: PersonalityGovernanceRepairPlan,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersonalityGovernanceRepairAction {
    RepairSelfAuthoredCore,
    RepairRelationshipConstitution,
    RepairOuterVoice,
    #[default]
    ObserveOnly,
}

impl PersonalityGovernanceRepairAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::RepairSelfAuthoredCore => "repair_self_authored_core",
            Self::RepairRelationshipConstitution => "repair_relationship_constitution",
            Self::RepairOuterVoice => "repair_outer_voice",
            Self::ObserveOnly => "observe_only",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonalityGovernanceRepairPlan {
    #[serde(default)]
    pub repair_needed: bool,
    #[serde(default)]
    pub primary_action: PersonalityGovernanceRepairAction,
    #[serde(default)]
    pub repair_self_authored_core: bool,
    #[serde(default)]
    pub repair_relationship_constitution: bool,
    #[serde(default)]
    pub repair_outer_voice: bool,
    #[serde(default)]
    pub observe_only: bool,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonalityRuntimeGovernanceGate {
    pub relationship_applicability: PersonalityRelationshipApplicability,
    pub allow_subject_bootstrap: bool,
    pub relationship_authority_ready: bool,
    pub allow_relationship_expression: bool,
    #[serde(default)]
    pub conservative_reply: bool,
    #[serde(default)]
    pub allow_dynamic_persona_priority: bool,
    #[serde(default)]
    pub allow_upward_distillation: bool,
    #[serde(default)]
    pub reason_summary: String,
    #[serde(default)]
    pub outstanding: Vec<String>,
    #[serde(default)]
    pub repair_plan: PersonalityGovernanceRepairPlan,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PersonalityGovernanceInspectionInput<'a> {
    pub relationship_scope: PersonalityRelationshipScope<'a>,
    pub mounted_subject_id: &'a str,
    pub channel: &'a str,
    pub chat_id: &'a str,
    pub now_secs: u64,
    pub self_authored_core: Option<&'a SelfAuthoredCore>,
    pub core_revision_ledger: Option<&'a CoreRevisionLedger>,
    pub relationship_constitution: Option<&'a RelationshipConstitution>,
    pub relationship_topology: Option<&'a RelationshipTopology>,
    pub subject_persona_evidence: Option<&'a RecentPersonaEvidence>,
    /// Exact active-relationship evidence. Never substitutes for subject evidence.
    pub recent_persona_evidence: Option<&'a RecentPersonaEvidence>,
}

pub fn inspect_personality_governance(
    input: PersonalityGovernanceInspectionInput<'_>,
) -> PersonalityGovernanceInspection {
    let (relationship_applicability, relationship_scope_id) = match input.relationship_scope {
        PersonalityRelationshipScope::SubjectOnly => (
            PersonalityRelationshipApplicability::NotApplicable,
            String::new(),
        ),
        PersonalityRelationshipScope::Active(id)
            if !id.is_empty() && id.trim() == id && !id.chars().any(char::is_control) =>
        {
            (
                PersonalityRelationshipApplicability::Required,
                id.to_owned(),
            )
        }
        _ => (
            PersonalityRelationshipApplicability::Unverified,
            String::new(),
        ),
    };
    let relationship_constitution = input.relationship_constitution.filter(|constitution| {
        relationship_applicability == PersonalityRelationshipApplicability::Required
            && constitution.scope_id == relationship_scope_id
    });
    let core_revision_governance = compute_core_revision_governance_digest(
        input.core_revision_ledger,
        input
            .self_authored_core
            .map(|core| core.last_reviewed_at)
            .unwrap_or(0),
        input
            .self_authored_core
            .map(|core| core.stability_score)
            .unwrap_or(0),
        input.now_secs,
    );
    let core_revision_timeline = input
        .core_revision_ledger
        .map(|ledger| build_core_revision_timeline(ledger, PERSONALITY_CLOSURE_EVENT_LIMIT))
        .unwrap_or_default();
    let relationship_audit = relationship_constitution.map(|constitution| {
        let topology_entry = input.relationship_topology.and_then(|topology| {
            topology
                .entries
                .iter()
                .find(|entry| entry.scope_id.trim() == relationship_scope_id)
        });
        audit_relationship_constitution(
            constitution,
            topology_entry,
            None,
            input.recent_persona_evidence,
            input.now_secs,
        )
    });
    let governance_events = build_governance_events(
        &core_revision_timeline,
        relationship_constitution,
        relationship_audit.as_ref(),
        input.recent_persona_evidence,
    );
    let closure = build_personality_closure_report(
        input,
        relationship_applicability,
        &core_revision_governance,
        relationship_constitution,
        relationship_audit.as_ref(),
    );
    let provisional = PersonalityGovernanceInspection {
        subject_id: input.mounted_subject_id.to_string(),
        relationship_scope_id,
        core_revision_governance,
        core_revision_timeline,
        relationship_audit,
        governance_events,
        closure,
        repair_plan: PersonalityGovernanceRepairPlan::default(),
    };
    let repair_plan = derive_personality_governance_repair_plan(&provisional);
    PersonalityGovernanceInspection {
        repair_plan,
        ..provisional
    }
}

pub fn derive_personality_runtime_governance_gate(
    input: PersonalityGovernanceInspectionInput<'_>,
) -> PersonalityRuntimeGovernanceGate {
    let inspection = inspect_personality_governance(input);
    derive_personality_runtime_governance_gate_from_inspection(&inspection)
}

pub fn derive_personality_runtime_governance_gate_from_inspection(
    inspection: &PersonalityGovernanceInspection,
) -> PersonalityRuntimeGovernanceGate {
    let conservative_reply = !inspection.closure.ready;
    let scope_verified = inspection.closure.relationship_applicability
        != PersonalityRelationshipApplicability::Unverified;
    PersonalityRuntimeGovernanceGate {
        relationship_applicability: inspection.closure.relationship_applicability,
        allow_subject_bootstrap: scope_verified && inspection.closure.subject_bootstrap_ready,
        relationship_authority_ready: inspection.closure.relationship_applicability
            == PersonalityRelationshipApplicability::Required
            && inspection.closure.relationship_governance_ready,
        allow_relationship_expression: inspection.closure.relationship_applicability
            == PersonalityRelationshipApplicability::Required
            && inspection.closure.relationship_expression_ready,
        conservative_reply,
        allow_dynamic_persona_priority: scope_verified && !conservative_reply,
        allow_upward_distillation: scope_verified && inspection.closure.subject_distillation_ready,
        reason_summary: build_personality_runtime_gate_reason_summary(inspection),
        outstanding: inspection.closure.outstanding.clone(),
        repair_plan: inspection.repair_plan.clone(),
    }
}

pub fn derive_personality_governance_repair_plan(
    inspection: &PersonalityGovernanceInspection,
) -> PersonalityGovernanceRepairPlan {
    let board_core_missing = !inspection.closure.board_core_ready;
    let revision_history_missing = !inspection.closure.revision_governance_ready;
    let board_review_due = inspection.core_revision_governance.review_due;
    let board_under_observation = inspection.core_revision_governance.observation_active;
    let board_conservative = inspection.core_revision_governance.conservative_mode;
    let relationship_required = inspection.closure.relationship_applicability
        == PersonalityRelationshipApplicability::Required;
    let relationship_missing =
        relationship_required && !inspection.closure.relationship_governance_ready;
    let relationship_audit = inspection.relationship_audit.as_ref();
    let relationship_material_drift =
        relationship_audit.is_some_and(|audit| audit.has_material_drift());
    let relationship_review_due = relationship_audit.is_some_and(|audit| audit.review_overdue);
    let expression_drift = relationship_audit.is_some_and(|audit| {
        (audit.response_mode_drift || audit.relationship_posture_drift)
            && !audit.priority_drift
            && !audit.reply_scope_drift
            && !audit.disclosure_drift
            && !audit.boundary_drift
            && !audit.review_overdue
    });
    let evidence_insufficient = !inspection.closure.evidence_loop_ready;
    let repair_self_authored_core =
        board_core_missing || revision_history_missing || board_review_due || board_conservative;
    let repair_relationship_constitution = relationship_required
        && (relationship_missing || relationship_material_drift || relationship_review_due);
    let repair_outer_voice = relationship_required
        && inspection.closure.relationship_governance_ready
        && expression_drift
        && !repair_self_authored_core
        && !repair_relationship_constitution;
    let observe_only =
        !repair_self_authored_core && !repair_relationship_constitution && !repair_outer_voice;

    let mut reasons = Vec::with_capacity(6);
    if board_core_missing {
        reasons.push("board_core_not_stable".to_string());
    }
    if revision_history_missing {
        reasons.push("revision_governance_history_missing".to_string());
    }
    if board_review_due {
        reasons.push("board_core_review_due".to_string());
    }
    if board_under_observation {
        reasons.push("board_core_still_under_observation".to_string());
    }
    if board_conservative && !board_review_due && !board_under_observation {
        reasons.push("board_core_under_conservative_governance".to_string());
    }
    if relationship_missing {
        reasons.push("relationship_constitution_missing".to_string());
    }
    if relationship_material_drift {
        reasons.push("relationship_drift_requires_realignment".to_string());
    }
    if relationship_review_due {
        reasons.push("relationship_review_due".to_string());
    }
    if repair_outer_voice {
        reasons.push("expression_drift_without_constitution_break".to_string());
    }
    if evidence_insufficient {
        reasons.push("recent_persona_evidence_insufficient".to_string());
    }
    if reasons.is_empty() && inspection.closure.ready {
        reasons.push("governance_settled".to_string());
    }
    reasons.truncate(PERSONALITY_CLOSURE_OUTSTANDING_MAX);

    let primary_action = if repair_self_authored_core {
        PersonalityGovernanceRepairAction::RepairSelfAuthoredCore
    } else if repair_relationship_constitution {
        PersonalityGovernanceRepairAction::RepairRelationshipConstitution
    } else if repair_outer_voice {
        PersonalityGovernanceRepairAction::RepairOuterVoice
    } else {
        PersonalityGovernanceRepairAction::ObserveOnly
    };

    let summary = truncate_content_to_max(
        reasons.join(", ").trim(),
        PERSONALITY_REPAIR_SUMMARY_MAX_CHARS,
    )
    .into_owned();

    PersonalityGovernanceRepairPlan {
        repair_needed: !observe_only || !inspection.closure.ready,
        primary_action,
        repair_self_authored_core,
        repair_relationship_constitution,
        repair_outer_voice,
        observe_only,
        summary,
        reasons,
    }
}

pub fn render_personality_governance_inspection_markdown(
    inspection: &PersonalityGovernanceInspection,
) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str("# Personality Governance Inspection\n\n");
    let _ = writeln!(out, "- Subject: {}", inspection.subject_id);
    let _ = writeln!(
        out,
        "- Relationship scope: {}",
        inspection.relationship_scope_id
    );
    let _ = writeln!(out, "- Closure ready: {}", inspection.closure.ready);
    let _ = writeln!(
        out,
        "- Initial subject model eligible: {} | normal subject distillation ready: {} | relationship expression ready: {}",
        inspection.closure.subject_bootstrap_ready,
        inspection.closure.subject_distillation_ready,
        inspection.closure.relationship_expression_ready
    );
    let _ = writeln!(
        out,
        "- Board core ready: {} | revision governance ready: {} | relationship governance ready: {}",
        inspection.closure.board_core_ready,
        inspection.closure.revision_governance_ready,
        inspection.closure.relationship_governance_ready
    );
    let _ = writeln!(
        out,
        "- Evidence loop ready: {} | drift control ready: {} | observation control ready: {} | review cadence ready: {}",
        inspection.closure.evidence_loop_ready,
        inspection.closure.drift_control_ready,
        inspection.closure.observation_control_ready,
        inspection.closure.review_cadence_ready
    );
    let _ = writeln!(
        out,
        "- Repair plan: {} | repair_needed={}",
        inspection.repair_plan.primary_action.label(),
        inspection.repair_plan.repair_needed
    );
    if !inspection.closure.outstanding.is_empty() {
        out.push_str("\n## Outstanding\n");
        for item in &inspection.closure.outstanding {
            let _ = writeln!(out, "- {}", item);
        }
    }
    out.push_str("\n## Repair Plan\n");
    let _ = writeln!(
        out,
        "- Primary action: {}",
        inspection.repair_plan.primary_action.label()
    );
    if !inspection.repair_plan.summary.trim().is_empty() {
        let _ = writeln!(out, "- Summary: {}", inspection.repair_plan.summary.trim());
    }
    if !inspection.repair_plan.reasons.is_empty() {
        let _ = writeln!(
            out,
            "- Reasons: {}",
            inspection.repair_plan.reasons.join(", ")
        );
    }
    out.push_str("\n## Governance Events\n");
    if inspection.governance_events.is_empty() {
        out.push_str("- No governance events available.\n");
    } else {
        for event in &inspection.governance_events {
            let _ = writeln!(out, "- [{} @ {}] {}", event.layer, event.at, event.summary);
        }
    }
    out
}

pub fn render_personality_runtime_governance_gate_block(
    gate: &PersonalityRuntimeGovernanceGate,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !gate.conservative_reply {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(512));
    out.push_str("## Personality Governance Gate\n");
    out.push_str(
        "Personality governance is not fully settled on this turn. Keep the reply constitution-first and conservative.\n",
    );
    out.push_str(
        "Do not let one-turn pressure, fresh relational drift, or unstable inner material rewrite the board-level stance.\n",
    );
    let _ = writeln!(
        out,
        "Preferred repair path: {}",
        gate.repair_plan.primary_action.label()
    );
    out.push_str(
        "If privacy or disclosure handling is uncertain, do not expose raw inward material; explain the boundary, but stable user-facing facts may still be answered directly.\n",
    );
    if !gate.reason_summary.trim().is_empty() {
        let _ = writeln!(
            out,
            "Current governance debt: {}",
            gate.reason_summary.trim()
        );
    }
    if !gate.outstanding.is_empty() {
        let _ = writeln!(out, "Outstanding: {}", gate.outstanding.join(", "));
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

fn build_personality_runtime_gate_reason_summary(
    inspection: &PersonalityGovernanceInspection,
) -> String {
    let mut reasons = Vec::with_capacity(4);
    if !inspection.closure.outstanding.is_empty() {
        reasons.push(inspection.closure.outstanding.join(", "));
    }
    let governance_pressure = inspection.core_revision_governance.pressure_summary();
    if !governance_pressure.trim().is_empty() {
        reasons.push(governance_pressure);
    }
    truncate_content_to_max(
        reasons.join("; ").trim(),
        PERSONALITY_RUNTIME_GATE_REASON_MAX_CHARS,
    )
    .into_owned()
}

fn build_governance_events(
    core_revision_timeline: &[CoreRevisionTimelineEntry],
    relationship_constitution: Option<&RelationshipConstitution>,
    relationship_audit: Option<&RelationshipConstitutionAudit>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
) -> Vec<PersonalityGovernanceEvent> {
    let mut events = core_revision_timeline
        .iter()
        .map(|entry| {
            let mut summary = format!("rev {} {}", entry.resulting_revision, entry.outcome.label());
            if let Some(kind) = entry.correction_kind {
                let _ = write!(
                    summary,
                    " {}={}",
                    kind.label(),
                    entry.corrects_revision.unwrap_or(0)
                );
            }
            if !entry.adjudication_reason.trim().is_empty() {
                let _ = write!(summary, " {}", entry.adjudication_reason.trim());
            }
            PersonalityGovernanceEvent {
                layer: "board_core".to_string(),
                at: entry.reviewed_at,
                summary: truncate_content_to_max(
                    summary.trim(),
                    PERSONALITY_CLOSURE_TEXT_MAX_CHARS,
                )
                .into_owned(),
            }
        })
        .collect::<Vec<_>>();
    if let Some(constitution) = relationship_constitution {
        let mut summary = format!(
            "alignment={} must_realign={} drift_score={} review_overdue={}",
            constitution.alignment.label(),
            constitution.must_realign,
            constitution.drift_score,
            constitution.review_overdue
        );
        if let Some(audit) = relationship_audit {
            if !audit.drift_flags.is_empty() {
                let _ = write!(summary, " flags={}", audit.drift_flags.join(","));
            }
        }
        events.push(PersonalityGovernanceEvent {
            layer: "relationship".to_string(),
            at: constitution.updated_at,
            summary: truncate_content_to_max(summary.trim(), PERSONALITY_CLOSURE_TEXT_MAX_CHARS)
                .into_owned(),
        });
    }
    if let Some(evidence) = recent_persona_evidence {
        let mut summary = format!(
            "sampled={} meaningful={} scope={} disclosure={}",
            evidence.sampled_turns,
            evidence.meaningful_turns,
            evidence.repeated_reply_scope.trim(),
            evidence.repeated_disclosure_action.trim()
        );
        if !evidence.volatility_flags.is_empty() {
            let _ = write!(
                summary,
                " volatility={}",
                evidence.volatility_flags.join(",")
            );
        }
        events.push(PersonalityGovernanceEvent {
            layer: "recent_evidence".to_string(),
            at: evidence.updated_at,
            summary: truncate_content_to_max(summary.trim(), PERSONALITY_CLOSURE_TEXT_MAX_CHARS)
                .into_owned(),
        });
    }
    events.sort_by(|left, right| {
        right
            .at
            .cmp(&left.at)
            .then_with(|| left.layer.cmp(&right.layer))
    });
    events.truncate(PERSONALITY_CLOSURE_EVENT_LIMIT);
    events
}

fn build_personality_closure_report(
    input: PersonalityGovernanceInspectionInput<'_>,
    relationship_applicability: PersonalityRelationshipApplicability,
    governance: &CoreRevisionGovernanceDigest,
    relationship_constitution: Option<&RelationshipConstitution>,
    relationship_audit: Option<&RelationshipConstitutionAudit>,
) -> PersonalityClosureReport {
    let PersonalityGovernanceInspectionInput {
        self_authored_core,
        core_revision_ledger,
        subject_persona_evidence,
        recent_persona_evidence,
        ..
    } = input;
    let board_core_ready = self_authored_core.is_some_and(|core| {
        core.revision > 0 && !core.identity_anchor.trim().is_empty() && core.stability_score > 0
    });
    let revision_governance_ready =
        core_revision_ledger.is_some_and(|ledger| ledger.is_meaningful());
    let relationship_governance_ready = relationship_constitution.is_some_and(|constitution| {
        constitution.is_meaningful() && constitution.board_revision > 0
    });
    let evidence_loop_ready = subject_persona_evidence.is_some_and(|evidence| {
        evidence.counterevidence_turns == 0
            && evidence.has_promotable_growth_signals()
            && evidence.repeated_relationship_posture.is_empty()
            && evidence.meaningful_turns >= PERSONALITY_MIN_EVIDENCE_TURNS
    });
    let relationship_evidence_ready = recent_persona_evidence.is_some_and(|evidence| {
        evidence.counterevidence_turns == 0
            && evidence.has_promotable_growth_signals()
            && evidence.meaningful_turns >= PERSONALITY_MIN_EVIDENCE_TURNS
    });
    // An absent core AND absent ledger is the only bootstrap state. A partial or
    // malformed existing pair must stay on the repair path, never initialize again.
    let subject_bootstrap_ready = self_authored_core.is_none()
        && core_revision_ledger.is_none()
        && evidence_loop_ready
        && subject_persona_evidence.is_some_and(|evidence| evidence.volatility_flags.is_empty())
        && relationship_applicability != PersonalityRelationshipApplicability::Unverified;
    let drift_control_ready = relationship_audit.is_some_and(|audit| !audit.has_material_drift());
    let observation_control_ready = !governance.observation_active;
    let review_cadence_ready = !governance.review_due
        && relationship_constitution.is_none_or(|constitution| !constitution.review_overdue);
    let subject_distillation_ready = board_core_ready
        && revision_governance_ready
        && evidence_loop_ready
        && observation_control_ready
        && !governance.review_due
        && !governance.conservative_mode
        && relationship_applicability != PersonalityRelationshipApplicability::Unverified;
    let relationship_expression_ready = subject_distillation_ready
        && relationship_evidence_ready
        && relationship_applicability == PersonalityRelationshipApplicability::Required
        && relationship_governance_ready
        && drift_control_ready
        && review_cadence_ready;
    let mut outstanding = Vec::with_capacity(PERSONALITY_CLOSURE_OUTSTANDING_MAX);
    if !board_core_ready {
        outstanding.push("board_core_not_stable".to_string());
    }
    if !revision_governance_ready {
        outstanding.push("revision_governance_history_missing".to_string());
    }
    if relationship_applicability == PersonalityRelationshipApplicability::Unverified {
        outstanding.push("relationship_applicability_unverified".to_string());
    }
    if relationship_applicability == PersonalityRelationshipApplicability::Required
        && !relationship_governance_ready
    {
        outstanding.push("relationship_constitution_missing".to_string());
    }
    if !evidence_loop_ready {
        outstanding.push("recent_persona_evidence_insufficient".to_string());
    }
    if relationship_applicability == PersonalityRelationshipApplicability::Required
        && !relationship_evidence_ready
    {
        outstanding.push("relationship_persona_evidence_insufficient".to_string());
    }
    if relationship_applicability == PersonalityRelationshipApplicability::Required
        && !drift_control_ready
    {
        outstanding.push("relationship_drift_not_under_control".to_string());
    }
    if !observation_control_ready {
        outstanding.push("board_core_still_under_observation".to_string());
    }
    if !review_cadence_ready {
        outstanding.push("governance_review_due".to_string());
    }
    outstanding.truncate(PERSONALITY_CLOSURE_OUTSTANDING_MAX);
    PersonalityClosureReport {
        relationship_applicability,
        subject_bootstrap_ready,
        subject_distillation_ready,
        relationship_expression_ready,
        ready: subject_distillation_ready
            && (relationship_applicability == PersonalityRelationshipApplicability::NotApplicable
                || relationship_expression_ready),
        board_core_ready,
        revision_governance_ready,
        relationship_governance_ready,
        evidence_loop_ready,
        drift_control_ready,
        observation_control_ready,
        review_cadence_ready,
        outstanding,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unseeded_subject_evidence_opens_only_the_initial_model_gate() {
        let evidence = RecentPersonaEvidence {
            sampled_turns: 4,
            meaningful_turns: 4,
            repeated_value_orientation: "truth_before_comfort".into(),
            updated_at: 99,
            ..Default::default()
        };
        fn inspect<'a>(
            scope: PersonalityRelationshipScope<'a>,
            subject_evidence: Option<&'a RecentPersonaEvidence>,
            relationship_evidence: Option<&'a RecentPersonaEvidence>,
        ) -> PersonalityGovernanceInspection {
            inspect_personality_governance(PersonalityGovernanceInspectionInput {
                mounted_subject_id: "agent:a",
                relationship_scope: scope,
                subject_persona_evidence: subject_evidence,
                recent_persona_evidence: relationship_evidence,
                now_secs: 100,
                ..Default::default()
            })
        }
        let subject = inspect(
            PersonalityRelationshipScope::SubjectOnly,
            Some(&evidence),
            None,
        );
        let gate = derive_personality_runtime_governance_gate_from_inspection(&subject);
        assert!(subject.closure.subject_bootstrap_ready);
        assert!(gate.allow_subject_bootstrap);
        assert!(gate.conservative_reply);
        assert!(!gate.allow_upward_distillation);
        assert!(!gate.allow_dynamic_persona_priority);
        assert!(!gate.allow_relationship_expression);
        assert!(!subject.closure.ready);
        let relationship_missing = inspect(
            PersonalityRelationshipScope::Active("relationship:a"),
            Some(&evidence),
            None,
        );
        let relationship_gate =
            derive_personality_runtime_governance_gate_from_inspection(&relationship_missing);
        assert!(relationship_gate.allow_subject_bootstrap);
        assert!(!relationship_gate.relationship_authority_ready);
        assert!(!relationship_gate.allow_relationship_expression);

        for inspection in [
            inspect(PersonalityRelationshipScope::SubjectOnly, None, None),
            inspect(
                PersonalityRelationshipScope::SubjectOnly,
                None,
                Some(&evidence),
            ),
            inspect(
                PersonalityRelationshipScope::Unverified,
                Some(&evidence),
                None,
            ),
        ] {
            assert!(
                !derive_personality_runtime_governance_gate_from_inspection(&inspection)
                    .allow_subject_bootstrap
            );
        }
        let mut counterevidence = evidence.clone();
        counterevidence.counterevidence_turns = 1;
        let mut insufficient = evidence.clone();
        insufficient.meaningful_turns = 3;
        let mut volatile = evidence.clone();
        volatile.volatility_flags = vec!["priority_varies".into()];
        let mut relational = evidence;
        relational.repeated_relationship_posture = "warm but bounded".into();
        for ineligible in [counterevidence, insufficient, volatile, relational] {
            let inspection = inspect(
                PersonalityRelationshipScope::SubjectOnly,
                Some(&ineligible),
                None,
            );
            assert!(!inspection.closure.subject_bootstrap_ready);
        }
    }

    #[test]
    fn existing_or_partial_core_state_cannot_reenter_bootstrap() {
        let evidence = RecentPersonaEvidence {
            sampled_turns: 4,
            meaningful_turns: 4,
            repeated_value_orientation: "truth_before_comfort".into(),
            ..Default::default()
        };
        let core = SelfAuthoredCore {
            revision: 1,
            ..Default::default()
        };
        let ledger = CoreRevisionLedger::default();
        for (core, ledger) in [
            (Some(&core), None),
            (None, Some(&ledger)),
            (Some(&core), Some(&ledger)),
        ] {
            let inspection = inspect_personality_governance(PersonalityGovernanceInspectionInput {
                mounted_subject_id: "agent:a",
                relationship_scope: PersonalityRelationshipScope::SubjectOnly,
                self_authored_core: core,
                core_revision_ledger: ledger,
                subject_persona_evidence: Some(&evidence),
                ..Default::default()
            });
            assert!(!inspection.closure.subject_bootstrap_ready);
        }
    }

    #[test]
    fn explicit_absence_is_distinct_from_missing_relationship_authority() {
        let subject_only = inspect_personality_governance(PersonalityGovernanceInspectionInput {
            mounted_subject_id: "agent-a",
            relationship_scope: PersonalityRelationshipScope::SubjectOnly,
            ..Default::default()
        });
        assert_eq!(
            subject_only.closure.relationship_applicability,
            PersonalityRelationshipApplicability::NotApplicable
        );
        assert!(!subject_only.repair_plan.repair_relationship_constitution);
        assert!(!subject_only.closure.relationship_expression_ready);
        let missing = inspect_personality_governance(PersonalityGovernanceInspectionInput {
            mounted_subject_id: "agent-a",
            relationship_scope: PersonalityRelationshipScope::Active("relationship:exact"),
            ..Default::default()
        });
        assert_eq!(missing.relationship_scope_id, "relationship:exact");
        assert_eq!(
            missing.closure.relationship_applicability,
            PersonalityRelationshipApplicability::Required
        );
        assert!(!missing.closure.ready);
        assert!(!missing.closure.relationship_expression_ready);
        assert!(missing.repair_plan.repair_relationship_constitution);
        let unknown =
            inspect_personality_governance(PersonalityGovernanceInspectionInput::default());
        assert_eq!(
            unknown.closure.relationship_applicability,
            PersonalityRelationshipApplicability::Unverified
        );
        assert!(!unknown.closure.ready);
    }

    #[test]
    fn subject_readiness_does_not_grant_relationship_expression() {
        let inspection = PersonalityGovernanceInspection {
            closure: PersonalityClosureReport {
                ready: true,
                subject_distillation_ready: true,
                relationship_expression_ready: false,
                relationship_applicability: PersonalityRelationshipApplicability::NotApplicable,
                ..Default::default()
            },
            ..Default::default()
        };
        let gate = derive_personality_runtime_governance_gate_from_inspection(&inspection);
        assert!(gate.allow_upward_distillation);
        assert!(!gate.allow_relationship_expression);
        let mut unverified = inspection;
        unverified.closure.relationship_applicability =
            PersonalityRelationshipApplicability::Unverified;
        let gate = derive_personality_runtime_governance_gate_from_inspection(&unverified);
        assert!(!gate.allow_upward_distillation);
        assert!(!gate.allow_relationship_expression);
    }

    #[test]
    fn runtime_gate_turns_conservative_when_closure_is_not_ready() {
        let mut inspection = PersonalityGovernanceInspection {
            subject_id: "board".to_string(),
            relationship_scope_id: "rel:qq:chat".to_string(),
            core_revision_governance: CoreRevisionGovernanceDigest::default(),
            relationship_audit: Some(RelationshipConstitutionAudit::default()),
            closure: PersonalityClosureReport {
                relationship_applicability: PersonalityRelationshipApplicability::Required,
                subject_bootstrap_ready: false,
                subject_distillation_ready: false,
                relationship_expression_ready: false,
                ready: false,
                board_core_ready: true,
                revision_governance_ready: true,
                relationship_governance_ready: true,
                evidence_loop_ready: false,
                drift_control_ready: true,
                observation_control_ready: true,
                review_cadence_ready: true,
                outstanding: vec!["recent_persona_evidence_insufficient".to_string()],
            },
            ..PersonalityGovernanceInspection::default()
        };
        inspection.repair_plan = derive_personality_governance_repair_plan(&inspection);

        let gate = derive_personality_runtime_governance_gate_from_inspection(&inspection);

        assert!(gate.conservative_reply);
        assert!(!gate.allow_dynamic_persona_priority);
        assert!(!gate.allow_upward_distillation);
        assert_eq!(
            gate.repair_plan.primary_action,
            PersonalityGovernanceRepairAction::ObserveOnly
        );
        assert!(gate
            .outstanding
            .contains(&"recent_persona_evidence_insufficient".to_string()));
    }

    #[test]
    fn runtime_gate_block_renders_only_for_conservative_mode() {
        let gate = PersonalityRuntimeGovernanceGate {
            relationship_applicability: PersonalityRelationshipApplicability::Required,
            relationship_authority_ready: true,
            allow_relationship_expression: false,
            conservative_reply: true,
            allow_subject_bootstrap: false,
            allow_dynamic_persona_priority: false,
            allow_upward_distillation: false,
            reason_summary: "recent_persona_evidence_insufficient".to_string(),
            outstanding: vec!["recent_persona_evidence_insufficient".to_string()],
            repair_plan: PersonalityGovernanceRepairPlan {
                primary_action: PersonalityGovernanceRepairAction::ObserveOnly,
                observe_only: true,
                summary: "recent_persona_evidence_insufficient".to_string(),
                reasons: vec!["recent_persona_evidence_insufficient".to_string()],
                ..PersonalityGovernanceRepairPlan::default()
            },
        };

        let rendered = render_personality_runtime_governance_gate_block(&gate, 512)
            .expect("conservative gate should render");
        assert!(rendered.contains("## Personality Governance Gate"));
        assert!(rendered.contains("constitution-first and conservative"));
        assert!(rendered.contains("Preferred repair path: observe_only"));

        let non_conservative = PersonalityRuntimeGovernanceGate {
            relationship_applicability: PersonalityRelationshipApplicability::Required,
            relationship_authority_ready: true,
            allow_relationship_expression: true,
            conservative_reply: false,
            allow_subject_bootstrap: false,
            allow_dynamic_persona_priority: true,
            allow_upward_distillation: true,
            reason_summary: String::new(),
            outstanding: Vec::new(),
            repair_plan: PersonalityGovernanceRepairPlan::default(),
        };
        assert!(render_personality_runtime_governance_gate_block(&non_conservative, 512).is_none());
    }

    #[test]
    fn repair_plan_prioritizes_board_core_repair_when_board_governance_is_unsettled() {
        let inspection = PersonalityGovernanceInspection {
            closure: PersonalityClosureReport {
                relationship_applicability: PersonalityRelationshipApplicability::Required,
                subject_bootstrap_ready: false,
                subject_distillation_ready: false,
                relationship_expression_ready: false,
                ready: false,
                board_core_ready: false,
                revision_governance_ready: false,
                relationship_governance_ready: true,
                evidence_loop_ready: true,
                drift_control_ready: true,
                observation_control_ready: false,
                review_cadence_ready: false,
                outstanding: vec![
                    "board_core_not_stable".to_string(),
                    "revision_governance_history_missing".to_string(),
                ],
            },
            core_revision_governance: CoreRevisionGovernanceDigest {
                review_due: true,
                observation_active: true,
                conservative_mode: true,
                ..CoreRevisionGovernanceDigest::default()
            },
            ..PersonalityGovernanceInspection::default()
        };

        let repair = derive_personality_governance_repair_plan(&inspection);

        assert_eq!(
            repair.primary_action,
            PersonalityGovernanceRepairAction::RepairSelfAuthoredCore
        );
        assert!(repair.repair_self_authored_core);
        assert!(!repair.repair_relationship_constitution);
        assert!(!repair.repair_outer_voice);
        assert!(!repair.observe_only);
    }

    #[test]
    fn repair_plan_distinguishes_relationship_and_expression_repair() {
        let relationship_repair =
            derive_personality_governance_repair_plan(&PersonalityGovernanceInspection {
                closure: PersonalityClosureReport {
                    relationship_applicability: PersonalityRelationshipApplicability::Required,
                    subject_bootstrap_ready: false,
                    subject_distillation_ready: false,
                    relationship_expression_ready: false,
                    ready: false,
                    board_core_ready: true,
                    revision_governance_ready: true,
                    relationship_governance_ready: true,
                    evidence_loop_ready: true,
                    drift_control_ready: false,
                    observation_control_ready: true,
                    review_cadence_ready: true,
                    outstanding: vec!["relationship_drift_not_under_control".to_string()],
                },
                relationship_audit: Some(RelationshipConstitutionAudit {
                    disclosure_drift: true,
                    drift_score: 48,
                    drift_flags: vec!["disclosure_drift".to_string()],
                    ..RelationshipConstitutionAudit::default()
                }),
                ..PersonalityGovernanceInspection::default()
            });
        assert_eq!(
            relationship_repair.primary_action,
            PersonalityGovernanceRepairAction::RepairRelationshipConstitution
        );
        assert!(relationship_repair.repair_relationship_constitution);

        let expression_repair =
            derive_personality_governance_repair_plan(&PersonalityGovernanceInspection {
                closure: PersonalityClosureReport {
                    relationship_applicability: PersonalityRelationshipApplicability::Required,
                    subject_bootstrap_ready: false,
                    subject_distillation_ready: false,
                    relationship_expression_ready: false,
                    ready: false,
                    board_core_ready: true,
                    revision_governance_ready: true,
                    relationship_governance_ready: true,
                    evidence_loop_ready: true,
                    drift_control_ready: true,
                    observation_control_ready: true,
                    review_cadence_ready: true,
                    outstanding: Vec::new(),
                },
                relationship_audit: Some(RelationshipConstitutionAudit {
                    response_mode_drift: true,
                    drift_score: 12,
                    drift_flags: vec!["response_mode_drift".to_string()],
                    ..RelationshipConstitutionAudit::default()
                }),
                ..PersonalityGovernanceInspection::default()
            });
        assert_eq!(
            expression_repair.primary_action,
            PersonalityGovernanceRepairAction::RepairOuterVoice
        );
        assert!(expression_repair.repair_outer_voice);
        assert!(!expression_repair.repair_relationship_constitution);
    }
}
