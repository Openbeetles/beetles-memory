//! Cross-owner validity, eligibility, premise and lineage contracts for governed memory.
//!
//! These types are authority-neutral contracts. Durable ownership stays with the existing
//! long-term, evidence, graph and runtime-skill owners.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{GovernedMemoryOwnerRef, GovernedOwnerRevisionRef, MemoryPrivacyClass};
use crate::skills::{
    RuntimeSkillDeliveryDropReason, RuntimeSkillEvidenceKind, RuntimeSkillOwningScope,
    RuntimeSkillPremise, RuntimeSkillPremiseObservation, RuntimeSkillPremiseRequirement,
    RuntimeSkillProjectionMaterial, RuntimeSkillProjectionPolicy,
    RuntimeSkillProjectionRenderOutcome, RuntimeSkillProjectionRenderReceipt,
    RuntimeSkillRecallAuthority, RuntimeSkillRecallPlan, RuntimeSkillSafeEvidenceRef,
};
use crate::{Error, Result};

pub const GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION: u32 = 1;
pub const MAX_GOVERNED_ELIGIBILITY_REASONS: usize = 12;
pub const MAX_GOVERNED_PREMISE_DECISION_REF_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedContractValidation {
    pub accepted: bool,
    pub failures: Vec<GovernedContractFailure>,
}

impl GovernedContractValidation {
    fn from_failures(failures: Vec<GovernedContractFailure>) -> Self {
        Self {
            accepted: failures.is_empty(),
            failures,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedContractFailure {
    OwnerRevisionRefInvalid,
    ValidityIntervalInvalid,
    ValiditySelfLoop,
    ValidityTerminationMismatch,
    ValiditySuccessorMismatch,
    EligibilityReasonMissing,
    EligibilityReasonUnexpected,
    EligibilityReasonDuplicate,
    EligibilityReasonLimitExceeded,
    EligibilityPrimaryReasonMismatch,
    EligibilityReportSchemaMismatch,
    EligibilityDecisionDuplicate,
    EligibilityQueryMismatch,
    EligibilityCountMismatch,
    DynamicStateReportSchemaMismatch,
    DynamicStateOwnerMismatch,
    DynamicStateValidityMismatch,
    DynamicStateDecisionMismatch,
    PremiseGateInvalid,
    PremiseReportSchemaMismatch,
    PremiseReportItemInvalid,
    PremiseReportCountMismatch,
    DeliveryReportMismatch,
    ContentDigestMismatch,
    HeadManifestClosureMismatch,
    InvalidationActorMissing,
    InvalidationEvidenceMissing,
    InvalidationEvidenceOwnerInvalid,
    InvalidationAuditReasonMissing,
    LineageDuplicateRevision,
    LineageCycle,
    LineageGap,
    LineageScopeMismatch,
    LineagePrivacyMismatch,
    LineageDepthExceeded,
    LineageFailureMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedOwnerTermination {
    Revised,
    Corrected,
    Superseded,
    Invalidated,
    Deleted,
    Forgotten,
}

impl GovernedOwnerTermination {
    pub const fn is_terminal_without_successor(self) -> bool {
        matches!(self, Self::Invalidated | Self::Deleted | Self::Forgotten)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedOwnerValidity {
    pub valid_from: u64,
    pub valid_until: Option<u64>,
    pub observed_at: u64,
    pub predecessor: Option<GovernedOwnerRevisionRef>,
    pub successor: Option<GovernedOwnerRevisionRef>,
    pub termination: Option<GovernedOwnerTermination>,
}

impl GovernedOwnerValidity {
    pub fn validate_for(&self, current: &GovernedOwnerRevisionRef) -> GovernedContractValidation {
        let mut failures = Vec::new();
        if !current.is_valid()
            || self
                .predecessor
                .as_ref()
                .is_some_and(|value| !value.is_valid())
            || self
                .successor
                .as_ref()
                .is_some_and(|value| !value.is_valid())
        {
            failures.push(GovernedContractFailure::OwnerRevisionRefInvalid);
        }
        if self
            .valid_until
            .is_some_and(|valid_until| valid_until <= self.valid_from)
        {
            failures.push(GovernedContractFailure::ValidityIntervalInvalid);
        }
        if self.predecessor.as_ref() == Some(current) || self.successor.as_ref() == Some(current) {
            failures.push(GovernedContractFailure::ValiditySelfLoop);
        }
        match self.termination {
            None => {
                if self.valid_until.is_some() || self.successor.is_some() {
                    failures.push(GovernedContractFailure::ValidityTerminationMismatch);
                }
            }
            Some(termination) => {
                if self.valid_until.is_none() {
                    failures.push(GovernedContractFailure::ValidityTerminationMismatch);
                }
                match termination {
                    GovernedOwnerTermination::Revised | GovernedOwnerTermination::Corrected => {
                        let exact_successor = self.successor.as_ref().is_some_and(|successor| {
                            successor.owner_ref == current.owner_ref
                                && current
                                    .owner_revision
                                    .checked_add(1)
                                    .is_some_and(|revision| successor.owner_revision == revision)
                        });
                        if !exact_successor {
                            failures.push(GovernedContractFailure::ValiditySuccessorMismatch);
                        }
                    }
                    GovernedOwnerTermination::Superseded => {
                        let cross_owner_successor =
                            self.successor.as_ref().is_some_and(|successor| {
                                successor.owner_ref.owner_plane == current.owner_ref.owner_plane
                                    && successor.owner_ref.owner_id != current.owner_ref.owner_id
                                    && successor.owner_revision == 1
                            });
                        if !cross_owner_successor {
                            failures.push(GovernedContractFailure::ValiditySuccessorMismatch);
                        }
                    }
                    termination if termination.is_terminal_without_successor() => {
                        if self.successor.is_some() {
                            failures.push(GovernedContractFailure::ValiditySuccessorMismatch);
                        }
                    }
                    _ => {}
                }
            }
        }
        GovernedContractValidation::from_failures(failures)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedRecallEligibility {
    EligibleCurrent,
    EligibleHistoricalAsOf,
    Excluded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedRecallEligibilityReason {
    PrivacyBlocked,
    Forgotten,
    Deleted,
    Invalidated,
    Superseded,
    Obsolete,
    Stale,
    PremiseBlocked,
    ProfileBlocked,
    BudgetBlocked,
    Tombstoned,
    Redacted,
}

impl GovernedRecallEligibilityReason {
    const fn precedence(self) -> u8 {
        match self {
            Self::PrivacyBlocked => 1,
            Self::Forgotten => 2,
            Self::Deleted => 3,
            Self::Invalidated => 4,
            Self::Superseded => 5,
            Self::Obsolete => 6,
            Self::Stale => 7,
            Self::PremiseBlocked => 8,
            Self::ProfileBlocked => 9,
            Self::BudgetBlocked => 10,
            Self::Tombstoned => 11,
            Self::Redacted => 12,
        }
    }
}

pub fn primary_governed_recall_reason(
    reasons: &[GovernedRecallEligibilityReason],
) -> Option<GovernedRecallEligibilityReason> {
    reasons
        .iter()
        .copied()
        .min_by_key(|reason| reason.precedence())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedProfileBudgetDrop {
    None,
    Profile,
    Budget,
    ProfileAndBudget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedRecallTemporalQuery {
    Current { query_time: u64 },
    HistoricalAsOf { query_time: u64, as_of_time: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedRecallDisclosure {
    Allowed,
    PrivacyBlocked,
    Redacted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum GovernedRequiredPremiseGate {
    NotApplicable,
    Satisfied {
        decision_ref: String,
    },
    Blocked {
        decision_ref: String,
        decision: PremiseEvaluationDecision,
    },
}

impl GovernedRequiredPremiseGate {
    fn validate_contract(&self) -> GovernedContractValidation {
        let valid_ref = |decision_ref: &str| {
            !decision_ref.trim().is_empty()
                && decision_ref == decision_ref.trim()
                && decision_ref.len() <= MAX_GOVERNED_PREMISE_DECISION_REF_BYTES
        };
        let accepted = match self {
            Self::NotApplicable => true,
            Self::Satisfied { decision_ref } => valid_ref(decision_ref),
            Self::Blocked {
                decision_ref,
                decision,
            } => valid_ref(decision_ref) && *decision != PremiseEvaluationDecision::Satisfied,
        };
        GovernedContractValidation::from_failures(
            (!accepted)
                .then_some(GovernedContractFailure::PremiseGateInvalid)
                .into_iter()
                .collect(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedRecallAuthorityGates {
    pub disclosure: GovernedRecallDisclosure,
    pub required_premise: GovernedRequiredPremiseGate,
    pub profile_budget_drop: GovernedProfileBudgetDrop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedRecallLifecycleFacts {
    owner_revision_ref: GovernedOwnerRevisionRef,
    validity: GovernedOwnerValidity,
    stale: bool,
    obsolete: bool,
    tombstoned: bool,
    historical_model_allowed: bool,
}

impl GovernedRecallLifecycleFacts {
    pub(crate) fn new(
        owner_revision_ref: GovernedOwnerRevisionRef,
        validity: GovernedOwnerValidity,
        stale: bool,
        obsolete: bool,
        tombstoned: bool,
        historical_model_allowed: bool,
    ) -> Result<Self> {
        if !validity.validate_for(&owner_revision_ref).accepted {
            return Err(Error::config(
                "governed_recall_lifecycle_facts",
                "owner validity is not canonical",
            ));
        }
        Ok(Self {
            owner_revision_ref,
            validity,
            stale,
            obsolete,
            tombstoned,
            historical_model_allowed,
        })
    }
}

pub fn decide_governed_recall_eligibility(
    lifecycle: &GovernedRecallLifecycleFacts,
    query: GovernedRecallTemporalQuery,
    gates: GovernedRecallAuthorityGates,
) -> Result<GovernedRecallEligibilityDecision> {
    if !lifecycle
        .validity
        .validate_for(&lifecycle.owner_revision_ref)
        .accepted
    {
        return Err(Error::config(
            "governed_recall_eligibility",
            "owner lifecycle facts are invalid",
        ));
    }
    if !gates.required_premise.validate_contract().accepted {
        return Err(Error::config(
            "governed_recall_eligibility",
            "required premise gate is invalid",
        ));
    }
    let (query_time, effective_time, historical) = match query {
        GovernedRecallTemporalQuery::Current { query_time } => (query_time, query_time, false),
        GovernedRecallTemporalQuery::HistoricalAsOf {
            query_time,
            as_of_time,
        } => {
            if as_of_time > query_time {
                return Err(Error::config(
                    "governed_recall_eligibility",
                    "historical as-of time cannot be later than query time",
                ));
            }
            (query_time, as_of_time, true)
        }
    };
    if query_time == 0
        || effective_time < lifecycle.validity.valid_from
        || historical
            && lifecycle
                .validity
                .valid_until
                .is_some_and(|valid_until| effective_time >= valid_until)
    {
        return Err(Error::config(
            "governed_recall_eligibility",
            "query does not address an eligible owner validity interval",
        ));
    }

    let mut reasons = Vec::new();
    if historical && !lifecycle.historical_model_allowed {
        reasons.push(GovernedRecallEligibilityReason::Obsolete);
    }
    match gates.disclosure {
        GovernedRecallDisclosure::Allowed => {}
        GovernedRecallDisclosure::PrivacyBlocked => {
            reasons.push(GovernedRecallEligibilityReason::PrivacyBlocked);
        }
        GovernedRecallDisclosure::Redacted => {
            reasons.push(GovernedRecallEligibilityReason::Redacted);
        }
    }
    match lifecycle.validity.termination {
        Some(GovernedOwnerTermination::Invalidated) => {
            reasons.push(GovernedRecallEligibilityReason::Invalidated);
        }
        Some(GovernedOwnerTermination::Deleted) => {
            reasons.push(GovernedRecallEligibilityReason::Deleted);
        }
        Some(GovernedOwnerTermination::Forgotten) => {
            reasons.push(GovernedRecallEligibilityReason::Forgotten);
        }
        Some(GovernedOwnerTermination::Corrected) => {
            reasons.push(GovernedRecallEligibilityReason::Obsolete);
        }
        Some(GovernedOwnerTermination::Superseded) if !historical => {
            reasons.push(GovernedRecallEligibilityReason::Superseded);
            reasons.push(GovernedRecallEligibilityReason::Obsolete);
        }
        Some(GovernedOwnerTermination::Revised) if !historical => {
            reasons.push(GovernedRecallEligibilityReason::Obsolete);
        }
        _ => {}
    }
    if lifecycle.stale {
        reasons.push(GovernedRecallEligibilityReason::Stale);
    }
    if lifecycle.obsolete {
        reasons.push(GovernedRecallEligibilityReason::Obsolete);
    }
    let premise_decision_ref = match gates.required_premise {
        GovernedRequiredPremiseGate::NotApplicable => None,
        GovernedRequiredPremiseGate::Satisfied { decision_ref } => Some(decision_ref),
        GovernedRequiredPremiseGate::Blocked {
            decision_ref,
            decision,
        } => {
            if decision == PremiseEvaluationDecision::PrivacyBlocked {
                reasons.push(GovernedRecallEligibilityReason::PrivacyBlocked);
            }
            reasons.push(GovernedRecallEligibilityReason::PremiseBlocked);
            Some(decision_ref)
        }
    };
    match gates.profile_budget_drop {
        GovernedProfileBudgetDrop::None => {}
        GovernedProfileBudgetDrop::Profile => {
            reasons.push(GovernedRecallEligibilityReason::ProfileBlocked);
        }
        GovernedProfileBudgetDrop::Budget => {
            reasons.push(GovernedRecallEligibilityReason::BudgetBlocked);
        }
        GovernedProfileBudgetDrop::ProfileAndBudget => {
            reasons.push(GovernedRecallEligibilityReason::ProfileBlocked);
            reasons.push(GovernedRecallEligibilityReason::BudgetBlocked);
        }
    }
    let has_causal_terminal = lifecycle.validity.termination.is_some();
    if lifecycle.tombstoned && !has_causal_terminal {
        reasons.push(GovernedRecallEligibilityReason::Tombstoned);
    }
    if reasons.contains(&GovernedRecallEligibilityReason::PrivacyBlocked) {
        reasons.retain(|reason| *reason != GovernedRecallEligibilityReason::Redacted);
    }
    reasons.sort_by_key(|reason| reason.precedence());
    reasons.dedup();
    let eligibility = if reasons.is_empty() {
        if historical {
            GovernedRecallEligibility::EligibleHistoricalAsOf
        } else {
            GovernedRecallEligibility::EligibleCurrent
        }
    } else {
        GovernedRecallEligibility::Excluded
    };
    let decision = GovernedRecallEligibilityDecision {
        eligibility,
        primary_reason: primary_governed_recall_reason(&reasons),
        reasons,
        owner_revision_ref: lifecycle.owner_revision_ref.clone(),
        query_time,
        effective_time,
        premise_decision_ref,
        profile_budget_drop: gates.profile_budget_drop,
    };
    if !decision.validate_contract().accepted {
        return Err(Error::config(
            "governed_recall_eligibility",
            "derived decision failed its canonical contract",
        ));
    }
    Ok(decision)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedRecallEligibilityDecision {
    pub eligibility: GovernedRecallEligibility,
    pub primary_reason: Option<GovernedRecallEligibilityReason>,
    pub reasons: Vec<GovernedRecallEligibilityReason>,
    pub owner_revision_ref: GovernedOwnerRevisionRef,
    pub query_time: u64,
    pub effective_time: u64,
    pub premise_decision_ref: Option<String>,
    pub profile_budget_drop: GovernedProfileBudgetDrop,
}

impl GovernedRecallEligibilityDecision {
    pub fn validate_contract(&self) -> GovernedContractValidation {
        let mut failures = Vec::new();
        if !self.owner_revision_ref.is_valid() {
            failures.push(GovernedContractFailure::OwnerRevisionRefInvalid);
        }
        if self.reasons.len() > MAX_GOVERNED_ELIGIBILITY_REASONS {
            failures.push(GovernedContractFailure::EligibilityReasonLimitExceeded);
        }
        let unique = self.reasons.iter().collect::<BTreeSet<_>>();
        if unique.len() != self.reasons.len() {
            failures.push(GovernedContractFailure::EligibilityReasonDuplicate);
        }
        match self.eligibility {
            GovernedRecallEligibility::Excluded if self.reasons.is_empty() => {
                failures.push(GovernedContractFailure::EligibilityReasonMissing);
            }
            GovernedRecallEligibility::EligibleCurrent
            | GovernedRecallEligibility::EligibleHistoricalAsOf
                if !self.reasons.is_empty() =>
            {
                failures.push(GovernedContractFailure::EligibilityReasonUnexpected);
            }
            _ => {}
        }
        if self.primary_reason != primary_governed_recall_reason(&self.reasons) {
            failures.push(GovernedContractFailure::EligibilityPrimaryReasonMismatch);
        }
        if self
            .premise_decision_ref
            .as_ref()
            .is_some_and(|decision_ref| {
                decision_ref.trim().is_empty()
                    || decision_ref != decision_ref.trim()
                    || decision_ref.len() > MAX_GOVERNED_PREMISE_DECISION_REF_BYTES
            })
            || (self
                .reasons
                .contains(&GovernedRecallEligibilityReason::PremiseBlocked)
                && self.premise_decision_ref.is_none())
        {
            failures.push(GovernedContractFailure::PremiseGateInvalid);
        }
        GovernedContractValidation::from_failures(failures)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedRecallEligibilityReport {
    pub schema_version: u32,
    pub decisions: Vec<GovernedRecallEligibilityDecision>,
    pub eligibility_counts: BTreeMap<GovernedRecallEligibility, u64>,
    pub reason_counts: BTreeMap<GovernedRecallEligibilityReason, u64>,
    pub as_of_time: Option<u64>,
    pub profile_budget_drop_count: u64,
}

impl GovernedRecallEligibilityReport {
    pub fn validate_contract(&self) -> GovernedContractValidation {
        let mut failures = Vec::new();
        if self.schema_version != GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION {
            failures.push(GovernedContractFailure::EligibilityReportSchemaMismatch);
        }
        let mut owner_revisions = BTreeSet::new();
        if self.decisions.iter().any(|decision| {
            !decision.validate_contract().accepted
                || !owner_revisions.insert(decision.owner_revision_ref.clone())
        }) {
            failures.push(GovernedContractFailure::EligibilityDecisionDuplicate);
        }
        let eligibility_counts = count_eligibility(&self.decisions);
        let reason_counts = count_eligibility_reasons(&self.decisions);
        let profile_budget_drop_count = self
            .decisions
            .iter()
            .filter(|decision| decision.profile_budget_drop != GovernedProfileBudgetDrop::None)
            .count() as u64;
        if self.eligibility_counts != eligibility_counts
            || self.reason_counts != reason_counts
            || self.profile_budget_drop_count != profile_budget_drop_count
        {
            failures.push(GovernedContractFailure::EligibilityCountMismatch);
        }
        if let Some(as_of_time) = self.as_of_time {
            if self.decisions.iter().any(|decision| {
                decision.effective_time != as_of_time
                    || decision.eligibility == GovernedRecallEligibility::EligibleCurrent
            }) {
                failures.push(GovernedContractFailure::EligibilityQueryMismatch);
            }
        } else if self.decisions.iter().any(|decision| {
            decision.effective_time != decision.query_time
                || decision.eligibility == GovernedRecallEligibility::EligibleHistoricalAsOf
        }) {
            failures.push(GovernedContractFailure::EligibilityQueryMismatch);
        }
        GovernedContractValidation::from_failures(failures)
    }
}

pub fn build_governed_recall_eligibility_report(
    decisions: impl IntoIterator<Item = GovernedRecallEligibilityDecision>,
    query: GovernedRecallTemporalQuery,
    max_decisions: usize,
) -> Result<GovernedRecallEligibilityReport> {
    let decisions = decisions.into_iter().collect::<Vec<_>>();
    if max_decisions == 0 || decisions.len() > max_decisions {
        return Err(Error::config(
            "governed_recall_eligibility_report",
            "decision count exceeds the request-pinned report budget",
        ));
    }
    let mut owner_revisions = BTreeSet::new();
    let query_matches = decisions.iter().all(|decision| {
        decision.validate_contract().accepted
            && owner_revisions.insert(decision.owner_revision_ref.clone())
            && match query {
                GovernedRecallTemporalQuery::Current { query_time } => {
                    decision.query_time == query_time
                        && decision.effective_time == query_time
                        && decision.eligibility != GovernedRecallEligibility::EligibleHistoricalAsOf
                }
                GovernedRecallTemporalQuery::HistoricalAsOf {
                    query_time,
                    as_of_time,
                } => {
                    decision.query_time == query_time
                        && decision.effective_time == as_of_time
                        && decision.eligibility != GovernedRecallEligibility::EligibleCurrent
                }
            }
    });
    if !query_matches {
        return Err(Error::config(
            "governed_recall_eligibility_report",
            "decision owner or temporal query contract drift",
        ));
    }
    let report = GovernedRecallEligibilityReport {
        schema_version: GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION,
        eligibility_counts: count_eligibility(&decisions),
        reason_counts: count_eligibility_reasons(&decisions),
        as_of_time: match query {
            GovernedRecallTemporalQuery::Current { .. } => None,
            GovernedRecallTemporalQuery::HistoricalAsOf { as_of_time, .. } => Some(as_of_time),
        },
        profile_budget_drop_count: decisions
            .iter()
            .filter(|decision| decision.profile_budget_drop != GovernedProfileBudgetDrop::None)
            .count() as u64,
        decisions,
    };
    if !report.validate_contract().accepted {
        return Err(Error::config(
            "governed_recall_eligibility_report",
            "derived report failed its canonical contract",
        ));
    }
    Ok(report)
}

fn count_eligibility(
    decisions: &[GovernedRecallEligibilityDecision],
) -> BTreeMap<GovernedRecallEligibility, u64> {
    let mut counts = BTreeMap::new();
    for decision in decisions {
        *counts.entry(decision.eligibility).or_default() += 1;
    }
    counts
}

fn count_eligibility_reasons(
    decisions: &[GovernedRecallEligibilityDecision],
) -> BTreeMap<GovernedRecallEligibilityReason, u64> {
    let mut counts = BTreeMap::new();
    for reason in decisions
        .iter()
        .flat_map(|decision| decision.reasons.iter().copied())
    {
        *counts.entry(reason).or_default() += 1;
    }
    counts
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryUpdateLineageFailure {
    Cycle,
    Gap,
    ScopeMismatch,
    PrivacyMismatch,
    DepthExceeded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedUpdateLineageItem {
    pub owner_revision_ref: GovernedOwnerRevisionRef,
    pub predecessor: Option<GovernedOwnerRevisionRef>,
    pub successor: Option<GovernedOwnerRevisionRef>,
    pub scope_digest: String,
    pub privacy_class: MemoryPrivacyClass,
    pub content_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryUpdateLineageReport {
    pub schema_version: u32,
    pub items: Vec<GovernedUpdateLineageItem>,
    pub failures: Vec<MemoryUpdateLineageFailure>,
    pub manifest_revision: u64,
    pub max_lineage_depth: usize,
    pub complete: bool,
}

impl MemoryUpdateLineageReport {
    pub fn validate_contract(&self) -> GovernedContractValidation {
        let mut detected =
            detect_memory_update_lineage_failures(&self.items, self.max_lineage_depth);
        if self.schema_version != GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION
            || self.manifest_revision == 0
            || self.max_lineage_depth == 0
        {
            detected.insert(GovernedContractFailure::LineageFailureMismatch);
        }
        let declared = self
            .failures
            .iter()
            .copied()
            .map(lineage_failure_to_contract_failure)
            .collect::<BTreeSet<_>>();
        let reportable = detected
            .iter()
            .copied()
            .filter(|failure| {
                matches!(
                    failure,
                    GovernedContractFailure::LineageCycle
                        | GovernedContractFailure::LineageGap
                        | GovernedContractFailure::LineageScopeMismatch
                        | GovernedContractFailure::LineagePrivacyMismatch
                        | GovernedContractFailure::LineageDepthExceeded
                )
            })
            .collect::<BTreeSet<_>>();
        if declared != reportable || self.complete != reportable.is_empty() {
            detected.insert(GovernedContractFailure::LineageFailureMismatch);
        }
        GovernedContractValidation::from_failures(detected.into_iter().collect())
    }
}

pub fn build_memory_update_lineage_report(
    mut items: Vec<GovernedUpdateLineageItem>,
    manifest_revision: u64,
    max_lineage_depth: usize,
) -> Result<MemoryUpdateLineageReport> {
    if manifest_revision == 0 || max_lineage_depth == 0 {
        return Err(Error::config(
            "memory_update_lineage_report",
            "manifest revision and request-pinned lineage depth must be positive",
        ));
    }
    let detected = detect_memory_update_lineage_failures(&items, max_lineage_depth);
    if detected.is_empty() {
        items = order_complete_lineage(items)?;
    } else {
        items.sort_by(|left, right| left.owner_revision_ref.cmp(&right.owner_revision_ref));
    }
    if detected.iter().any(|failure| {
        !matches!(
            failure,
            GovernedContractFailure::LineageCycle
                | GovernedContractFailure::LineageGap
                | GovernedContractFailure::LineageScopeMismatch
                | GovernedContractFailure::LineagePrivacyMismatch
                | GovernedContractFailure::LineageDepthExceeded
        )
    }) {
        return Err(Error::config(
            "memory_update_lineage_report",
            "lineage items contain a non-reportable contract failure",
        ));
    }
    let failures = detected
        .iter()
        .filter_map(|failure| match failure {
            GovernedContractFailure::LineageCycle => Some(MemoryUpdateLineageFailure::Cycle),
            GovernedContractFailure::LineageGap => Some(MemoryUpdateLineageFailure::Gap),
            GovernedContractFailure::LineageScopeMismatch => {
                Some(MemoryUpdateLineageFailure::ScopeMismatch)
            }
            GovernedContractFailure::LineagePrivacyMismatch => {
                Some(MemoryUpdateLineageFailure::PrivacyMismatch)
            }
            GovernedContractFailure::LineageDepthExceeded => {
                Some(MemoryUpdateLineageFailure::DepthExceeded)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let report = MemoryUpdateLineageReport {
        schema_version: GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION,
        items,
        complete: failures.is_empty(),
        failures,
        manifest_revision,
        max_lineage_depth,
    };
    let validation = report.validate_contract();
    if validation.failures.iter().copied().collect::<BTreeSet<_>>() != detected {
        return Err(Error::config(
            "memory_update_lineage_report",
            "derived lineage report failed its canonical contract",
        ));
    }
    Ok(report)
}

fn detect_memory_update_lineage_failures(
    items: &[GovernedUpdateLineageItem],
    max_lineage_depth: usize,
) -> BTreeSet<GovernedContractFailure> {
    let mut detected = BTreeSet::new();
    let mut refs = BTreeSet::new();
    for item in items {
        if !item.owner_revision_ref.is_valid() {
            detected.insert(GovernedContractFailure::OwnerRevisionRefInvalid);
        }
        if !is_canonical_lineage_digest(&item.scope_digest)
            || !is_canonical_lineage_digest(&item.content_digest)
        {
            detected.insert(GovernedContractFailure::LineageFailureMismatch);
        }
        if !refs.insert(item.owner_revision_ref.clone()) {
            detected.insert(GovernedContractFailure::LineageDuplicateRevision);
        }
        if item.predecessor.as_ref() == Some(&item.owner_revision_ref)
            || item.successor.as_ref() == Some(&item.owner_revision_ref)
        {
            detected.insert(GovernedContractFailure::LineageCycle);
        }
    }
    let successors = items
        .iter()
        .map(|item| (item.owner_revision_ref.clone(), item.successor.clone()))
        .collect::<BTreeMap<_, _>>();
    for start in successors.keys() {
        let mut seen = BTreeSet::new();
        let mut cursor = Some(start.clone());
        while let Some(current) = cursor {
            if !seen.insert(current.clone()) {
                detected.insert(GovernedContractFailure::LineageCycle);
                break;
            }
            cursor = successors.get(&current).and_then(Clone::clone);
        }
    }
    if items.len() > max_lineage_depth {
        detected.insert(GovernedContractFailure::LineageDepthExceeded);
    }
    for item in items {
        if let Some(predecessor_ref) = item.predecessor.as_ref() {
            if let Some(predecessor) = items
                .iter()
                .find(|candidate| &candidate.owner_revision_ref == predecessor_ref)
            {
                if predecessor.successor.as_ref() != Some(&item.owner_revision_ref) {
                    detected.insert(GovernedContractFailure::LineageGap);
                }
                if predecessor.scope_digest != item.scope_digest {
                    detected.insert(GovernedContractFailure::LineageScopeMismatch);
                }
                if predecessor.privacy_class != item.privacy_class {
                    detected.insert(GovernedContractFailure::LineagePrivacyMismatch);
                }
            } else {
                detected.insert(GovernedContractFailure::LineageGap);
            }
        }
        if let Some(successor_ref) = item.successor.as_ref() {
            if let Some(successor) = items
                .iter()
                .find(|candidate| &candidate.owner_revision_ref == successor_ref)
            {
                if successor.predecessor.as_ref() != Some(&item.owner_revision_ref) {
                    detected.insert(GovernedContractFailure::LineageGap);
                }
                if successor.scope_digest != item.scope_digest {
                    detected.insert(GovernedContractFailure::LineageScopeMismatch);
                }
                if successor.privacy_class != item.privacy_class {
                    detected.insert(GovernedContractFailure::LineagePrivacyMismatch);
                }
            } else {
                detected.insert(GovernedContractFailure::LineageGap);
            }
        }
    }
    if !items.is_empty()
        && (items
            .iter()
            .filter(|item| item.predecessor.is_none())
            .count()
            != 1
            || items.iter().filter(|item| item.successor.is_none()).count() != 1)
    {
        detected.insert(GovernedContractFailure::LineageGap);
    }
    detected
}

fn is_canonical_lineage_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn order_complete_lineage(
    items: Vec<GovernedUpdateLineageItem>,
) -> Result<Vec<GovernedUpdateLineageItem>> {
    let by_ref = items
        .into_iter()
        .map(|item| (item.owner_revision_ref.clone(), item))
        .collect::<BTreeMap<_, _>>();
    let mut cursor = by_ref
        .values()
        .find(|item| item.predecessor.is_none())
        .map(|item| item.owner_revision_ref.clone());
    let mut ordered = Vec::with_capacity(by_ref.len());
    while let Some(current) = cursor {
        let item = by_ref.get(&current).ok_or_else(|| {
            Error::config(
                "memory_update_lineage_report",
                "complete lineage cursor is missing",
            )
        })?;
        cursor = item.successor.clone();
        ordered.push(item.clone());
    }
    if ordered.len() != by_ref.len() {
        return Err(Error::config(
            "memory_update_lineage_report",
            "complete lineage does not cover every item",
        ));
    }
    Ok(ordered)
}

fn lineage_failure_to_contract_failure(
    failure: MemoryUpdateLineageFailure,
) -> GovernedContractFailure {
    match failure {
        MemoryUpdateLineageFailure::Cycle => GovernedContractFailure::LineageCycle,
        MemoryUpdateLineageFailure::Gap => GovernedContractFailure::LineageGap,
        MemoryUpdateLineageFailure::ScopeMismatch => GovernedContractFailure::LineageScopeMismatch,
        MemoryUpdateLineageFailure::PrivacyMismatch => {
            GovernedContractFailure::LineagePrivacyMismatch
        }
        MemoryUpdateLineageFailure::DepthExceeded => GovernedContractFailure::LineageDepthExceeded,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PremiseEvaluationDecision {
    Satisfied,
    Unsatisfied,
    Unknown,
    Expired,
    PrivacyBlocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PremiseTypedSource {
    RegisteredCapability,
    OpaquePresenceAttestation,
    GovernedEnvironmentEvidence,
    TaskLearning,
    TaskRun,
    TaskArtifact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PremiseEvaluationItem {
    pub premise_index: usize,
    pub source: PremiseTypedSource,
    pub decision: PremiseEvaluationDecision,
    pub required: bool,
    pub governed_evidence_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PremiseEvaluationReport {
    pub schema_version: u32,
    pub query_time: u64,
    pub items: Vec<PremiseEvaluationItem>,
    pub decision_counts: BTreeMap<PremiseEvaluationDecision, u64>,
    pub required_failure_count: u64,
}

impl PremiseEvaluationReport {
    pub fn validate_contract(&self) -> GovernedContractValidation {
        let mut failures = Vec::new();
        if self.schema_version != GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION || self.query_time == 0 {
            failures.push(GovernedContractFailure::PremiseReportSchemaMismatch);
        }
        if self
            .items
            .iter()
            .enumerate()
            .any(|(index, item)| item.premise_index != index)
        {
            failures.push(GovernedContractFailure::PremiseReportItemInvalid);
        }
        let mut decision_counts = BTreeMap::new();
        let mut required_failure_count = 0u64;
        for item in &self.items {
            *decision_counts.entry(item.decision).or_insert(0) += 1;
            if item.required && item.decision != PremiseEvaluationDecision::Satisfied {
                required_failure_count = required_failure_count.saturating_add(1);
            }
        }
        if self.decision_counts != decision_counts
            || self.required_failure_count != required_failure_count
        {
            failures.push(GovernedContractFailure::PremiseReportCountMismatch);
        }
        GovernedContractValidation::from_failures(failures)
    }
}

pub fn build_runtime_skill_premise_evaluation_report(
    requirements: &[RuntimeSkillPremiseRequirement],
    observations: &[RuntimeSkillPremiseObservation],
    owning_scope: &RuntimeSkillOwningScope,
    authority: &RuntimeSkillRecallAuthority,
    query_time: u64,
    max_evidence_reads: usize,
) -> Result<PremiseEvaluationReport> {
    if query_time == 0
        || max_evidence_reads == 0
        || observations.len() > max_evidence_reads
        || observations
            .iter()
            .any(|observation| !observation.validate_contract())
        || observations
            .windows(2)
            .any(|pair| pair[0].canonical_identity() >= pair[1].canonical_identity())
    {
        return Err(Error::config(
            "runtime_skill_premise_report",
            "query time, typed observations, or evidence-read budget is invalid",
        ));
    }

    let mut items = Vec::with_capacity(requirements.len());
    for (premise_index, requirement) in requirements.iter().enumerate() {
        let source = requirement.premise.typed_source();
        let privacy_allowed = authority.privacy_allows(owning_scope, requirement.privacy_class);
        let decision = if !privacy_allowed {
            PremiseEvaluationDecision::PrivacyBlocked
        } else if requirement
            .valid_until
            .is_some_and(|valid_until| query_time >= valid_until)
        {
            PremiseEvaluationDecision::Expired
        } else if query_time < requirement.valid_from {
            PremiseEvaluationDecision::Unknown
        } else {
            let premise_decision = evaluate_runtime_skill_premise(
                &requirement.premise,
                observations,
                authority.governed_environment_evidence_allowed(),
                authority.task_evidence_allowed(),
            );
            let governed_evidence_decision = evaluate_governed_evidence_refs(
                &requirement.governed_evidence_refs,
                observations,
                authority.governed_environment_evidence_allowed(),
            );
            combine_premise_decisions(premise_decision, governed_evidence_decision)
        };
        items.push(PremiseEvaluationItem {
            premise_index,
            source,
            decision,
            required: requirement.required,
            governed_evidence_count: u32::try_from(requirement.governed_evidence_refs.len())
                .unwrap_or(u32::MAX),
        });
    }

    let mut decision_counts = BTreeMap::new();
    let mut required_failure_count = 0u64;
    for item in &items {
        *decision_counts.entry(item.decision).or_insert(0) += 1;
        if item.required && item.decision != PremiseEvaluationDecision::Satisfied {
            required_failure_count = required_failure_count.saturating_add(1);
        }
    }
    let report = PremiseEvaluationReport {
        schema_version: GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION,
        query_time,
        items,
        decision_counts,
        required_failure_count,
    };
    if !report.validate_contract().accepted {
        return Err(Error::config(
            "runtime_skill_premise_report",
            "derived premise report failed its canonical contract",
        ));
    }
    Ok(report)
}

fn evaluate_governed_evidence_refs(
    evidence_refs: &[GovernedOwnerRevisionRef],
    observations: &[RuntimeSkillPremiseObservation],
    governed_environment_evidence_allowed: bool,
) -> PremiseEvaluationDecision {
    if evidence_refs.is_empty() {
        return PremiseEvaluationDecision::Satisfied;
    }
    if !governed_environment_evidence_allowed {
        return PremiseEvaluationDecision::Unknown;
    }
    let mut decision = PremiseEvaluationDecision::Satisfied;
    for evidence_ref in evidence_refs {
        let observed = observations
            .iter()
            .find_map(|observation| match observation {
                RuntimeSkillPremiseObservation::GovernedEnvironmentEvidence {
                    evidence_revision_ref,
                    present,
                } if evidence_revision_ref == evidence_ref => Some(*present),
                _ => None,
            });
        match observed {
            None => return PremiseEvaluationDecision::Unknown,
            Some(false) => decision = PremiseEvaluationDecision::Unsatisfied,
            Some(true) => {}
        }
    }
    decision
}

const fn combine_premise_decisions(
    premise: PremiseEvaluationDecision,
    governed_evidence: PremiseEvaluationDecision,
) -> PremiseEvaluationDecision {
    if matches!(premise, PremiseEvaluationDecision::Unknown)
        || matches!(governed_evidence, PremiseEvaluationDecision::Unknown)
    {
        PremiseEvaluationDecision::Unknown
    } else if matches!(premise, PremiseEvaluationDecision::Unsatisfied)
        || matches!(governed_evidence, PremiseEvaluationDecision::Unsatisfied)
    {
        PremiseEvaluationDecision::Unsatisfied
    } else {
        PremiseEvaluationDecision::Satisfied
    }
}

fn evaluate_runtime_skill_premise(
    premise: &RuntimeSkillPremise,
    observations: &[RuntimeSkillPremiseObservation],
    governed_environment_evidence_allowed: bool,
    task_evidence_allowed: bool,
) -> PremiseEvaluationDecision {
    match premise {
        RuntimeSkillPremise::RegisteredCapability {
            capability_id,
            version_constraint,
        } => observations
            .iter()
            .find_map(|observation| match observation {
                RuntimeSkillPremiseObservation::RegisteredCapability {
                    capability_id: observed_id,
                    version,
                } if observed_id == capability_id => Some(
                    if version_constraint
                        .min_inclusive
                        .is_none_or(|minimum| *version >= minimum)
                        && version_constraint
                            .max_exclusive
                            .is_none_or(|maximum| *version < maximum)
                    {
                        PremiseEvaluationDecision::Satisfied
                    } else {
                        PremiseEvaluationDecision::Unsatisfied
                    },
                ),
                _ => None,
            })
            .unwrap_or(PremiseEvaluationDecision::Unknown),
        RuntimeSkillPremise::GovernedEnvironmentEvidence {
            evidence_revision_ref,
        } => {
            if !governed_environment_evidence_allowed {
                return PremiseEvaluationDecision::Unknown;
            }
            observations
                .iter()
                .find_map(|observation| match observation {
                    RuntimeSkillPremiseObservation::GovernedEnvironmentEvidence {
                        evidence_revision_ref: observed_ref,
                        present,
                    } if observed_ref == evidence_revision_ref => Some(if *present {
                        PremiseEvaluationDecision::Satisfied
                    } else {
                        PremiseEvaluationDecision::Unsatisfied
                    }),
                    _ => None,
                })
                .unwrap_or(PremiseEvaluationDecision::Unknown)
        }
        RuntimeSkillPremise::OpaquePresenceAttestation { handle_ref } => observations
            .iter()
            .find_map(|observation| match observation {
                RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
                    handle_ref: observed_ref,
                    present,
                } if observed_ref == handle_ref => Some(if *present {
                    PremiseEvaluationDecision::Satisfied
                } else {
                    PremiseEvaluationDecision::Unsatisfied
                }),
                _ => None,
            })
            .unwrap_or(PremiseEvaluationDecision::Unknown),
        RuntimeSkillPremise::TaskEvidence {
            source,
            evidence_kind,
            safe_ref,
        } => {
            if !task_evidence_allowed {
                return PremiseEvaluationDecision::Unknown;
            }
            observations
                .iter()
                .find_map(|observation| match observation {
                    RuntimeSkillPremiseObservation::TaskEvidence {
                        source: observed_source,
                        evidence_kind: observed_kind,
                        safe_ref: observed_ref,
                        present,
                    } if observed_source == source
                        && observed_kind == evidence_kind
                        && observed_ref == safe_ref =>
                    {
                        Some(if *present {
                            PremiseEvaluationDecision::Satisfied
                        } else {
                            PremiseEvaluationDecision::Unsatisfied
                        })
                    }
                    _ => None,
                })
                .unwrap_or(PremiseEvaluationDecision::Unknown)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicStateResolutionReport {
    pub schema_version: u32,
    pub predecessor: Option<GovernedOwnerRevisionRef>,
    pub successor: Option<GovernedOwnerRevisionRef>,
    pub validity: GovernedOwnerValidity,
    pub current_decision: GovernedRecallEligibilityDecision,
    pub as_of_decision: Option<GovernedRecallEligibilityDecision>,
    pub conflict_count: u64,
    pub unknown_count: u64,
}

impl DynamicStateResolutionReport {
    pub fn validate_contract(&self) -> GovernedContractValidation {
        let mut failures = Vec::new();
        if self.schema_version != GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION {
            failures.push(GovernedContractFailure::DynamicStateReportSchemaMismatch);
        }
        if self.predecessor != self.validity.predecessor
            || self.successor != self.validity.successor
        {
            failures.push(GovernedContractFailure::DynamicStateValidityMismatch);
        }
        if !self
            .validity
            .validate_for(&self.current_decision.owner_revision_ref)
            .accepted
        {
            failures.push(GovernedContractFailure::DynamicStateValidityMismatch);
        }
        if !self.current_decision.validate_contract().accepted
            || self.current_decision.query_time == 0
            || self.current_decision.effective_time != self.current_decision.query_time
            || self.current_decision.eligibility
                == GovernedRecallEligibility::EligibleHistoricalAsOf
        {
            failures.push(GovernedContractFailure::DynamicStateDecisionMismatch);
        }
        if let Some(decision) = self.as_of_decision.as_ref() {
            if decision.owner_revision_ref != self.current_decision.owner_revision_ref {
                failures.push(GovernedContractFailure::DynamicStateOwnerMismatch);
            }
            if !decision.validate_contract().accepted
                || decision.query_time == 0
                || decision.query_time != self.current_decision.query_time
                || decision.effective_time > decision.query_time
                || decision.effective_time < self.validity.valid_from
                || self
                    .validity
                    .valid_until
                    .is_some_and(|valid_until| decision.effective_time >= valid_until)
                || decision.eligibility == GovernedRecallEligibility::EligibleCurrent
            {
                failures.push(GovernedContractFailure::DynamicStateDecisionMismatch);
            }
        }
        failures.sort();
        failures.dedup();
        GovernedContractValidation::from_failures(failures)
    }
}

pub fn build_current_dynamic_state_resolution_report(
    lifecycle: &GovernedRecallLifecycleFacts,
    query_time: u64,
    gates: GovernedRecallAuthorityGates,
) -> Result<DynamicStateResolutionReport> {
    let current_decision = decide_governed_recall_eligibility(
        lifecycle,
        GovernedRecallTemporalQuery::Current { query_time },
        gates,
    )?;
    let report = DynamicStateResolutionReport {
        schema_version: GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION,
        predecessor: lifecycle.validity.predecessor.clone(),
        successor: lifecycle.validity.successor.clone(),
        validity: lifecycle.validity.clone(),
        current_decision,
        as_of_decision: None,
        conflict_count: 0,
        unknown_count: 0,
    };
    if !report.validate_contract().accepted
        || report.as_of_decision.is_some()
        || report.conflict_count != 0
        || report.unknown_count != 0
    {
        return Err(Error::config(
            "dynamic_state_resolution_report",
            "derived current dynamic-state report failed its canonical contract",
        ));
    }
    Ok(report)
}

pub fn build_historical_dynamic_state_resolution_report(
    lifecycle: &GovernedRecallLifecycleFacts,
    query_time: u64,
    as_of_time: u64,
    gates: GovernedRecallAuthorityGates,
) -> Result<DynamicStateResolutionReport> {
    let current_decision = decide_governed_recall_eligibility(
        lifecycle,
        GovernedRecallTemporalQuery::Current { query_time },
        gates.clone(),
    )?;
    let as_of_decision = decide_governed_recall_eligibility(
        lifecycle,
        GovernedRecallTemporalQuery::HistoricalAsOf {
            query_time,
            as_of_time,
        },
        gates,
    )?;
    let report = DynamicStateResolutionReport {
        schema_version: GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION,
        predecessor: lifecycle.validity.predecessor.clone(),
        successor: lifecycle.validity.successor.clone(),
        validity: lifecycle.validity.clone(),
        current_decision,
        as_of_decision: Some(as_of_decision),
        conflict_count: 0,
        unknown_count: 0,
    };
    if !report.validate_contract().accepted
        || report.as_of_decision.as_ref().is_none_or(|decision| {
            decision.query_time != query_time || decision.effective_time != as_of_time
        })
        || report.conflict_count != 0
        || report.unknown_count != 0
    {
        return Err(Error::config(
            "dynamic_state_resolution_report",
            "derived historical dynamic-state report failed its canonical contract",
        ));
    }
    Ok(report)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralMemoryDeliveryReport {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub owning_scope: RuntimeSkillOwningScope,
    pub manifest_revision: u64,
    pub owner_revision_ref: GovernedOwnerRevisionRef,
    pub query_time: u64,
    pub projection_policy: RuntimeSkillProjectionPolicy,
    pub premise_report: PremiseEvaluationReport,
    pub matched: bool,
    pub selected: bool,
    pub rendered: bool,
    pub drop_reasons: Vec<RuntimeSkillDeliveryDropReason>,
    pub safe_evidence_refs: Vec<RuntimeSkillSafeEvidenceRef>,
}

impl ProceduralMemoryDeliveryReport {
    pub fn validate_contract(&self, plan: &RuntimeSkillRecallPlan) -> GovernedContractValidation {
        let accepted = self.premise_report.validate_contract().accepted
            && build_procedural_memory_delivery_report(plan)
                .is_ok_and(|expected| self == &expected);
        GovernedContractValidation::from_failures(
            (!accepted)
                .then_some(GovernedContractFailure::DeliveryReportMismatch)
                .into_iter()
                .collect(),
        )
    }

    pub fn validate_finalized_contract(
        &self,
        plan: &RuntimeSkillRecallPlan,
        material: Option<&RuntimeSkillProjectionMaterial>,
        receipt: &RuntimeSkillProjectionRenderReceipt,
    ) -> GovernedContractValidation {
        let accepted = finalize_procedural_memory_delivery_report(plan, material, receipt)
            .is_ok_and(|expected| self == &expected);
        GovernedContractValidation::from_failures(
            (!accepted)
                .then_some(GovernedContractFailure::DeliveryReportMismatch)
                .into_iter()
                .collect(),
        )
    }
}

pub fn build_procedural_memory_delivery_report(
    plan: &RuntimeSkillRecallPlan,
) -> Result<ProceduralMemoryDeliveryReport> {
    if !plan.premise_report().validate_contract().accepted {
        return Err(Error::config(
            "procedural_memory_delivery_report",
            "premise report is not canonical",
        ));
    }
    let mut safe_evidence_refs = if plan.selected() {
        plan.evidence_bindings()
            .iter()
            .map(|binding| RuntimeSkillSafeEvidenceRef {
                kind: binding.kind,
                safe_ref: binding.safe_ref.clone(),
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    safe_evidence_refs.sort();
    safe_evidence_refs.dedup();
    let report = ProceduralMemoryDeliveryReport {
        schema_version: GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION,
        memory_space_id: plan.memory_space_id().to_string(),
        owning_scope: plan.owning_scope().clone(),
        manifest_revision: plan.manifest_revision(),
        owner_revision_ref: GovernedOwnerRevisionRef {
            owner_ref: plan.owner_binding().owner_ref.clone(),
            owner_revision: plan.owner_binding().owner_revision,
        },
        query_time: plan.query_time(),
        projection_policy: plan.projection_policy().clone(),
        premise_report: plan.premise_report().clone(),
        matched: plan.matched(),
        selected: plan.selected(),
        rendered: false,
        drop_reasons: plan.drop_reasons().to_vec(),
        safe_evidence_refs,
    };
    Ok(report)
}

pub fn finalize_procedural_memory_delivery_report(
    plan: &RuntimeSkillRecallPlan,
    material: Option<&RuntimeSkillProjectionMaterial>,
    receipt: &RuntimeSkillProjectionRenderReceipt,
) -> Result<ProceduralMemoryDeliveryReport> {
    let mut report = build_procedural_memory_delivery_report(plan)?;
    match (plan.selected(), material, receipt.outcome()) {
        (false, None, RuntimeSkillProjectionRenderOutcome::NotRequested) => Ok(report),
        (true, Some(material), RuntimeSkillProjectionRenderOutcome::NotRequested)
            if material.validates_plan(plan) =>
        {
            Ok(report)
        }
        (true, Some(material), RuntimeSkillProjectionRenderOutcome::Rendered)
            if material.validates_plan(plan) && receipt.matches_material(material) =>
        {
            report.rendered = true;
            Ok(report)
        }
        (true, Some(material), RuntimeSkillProjectionRenderOutcome::DroppedBudget)
            if material.validates_plan(plan) && receipt.matches_material(material) =>
        {
            report
                .drop_reasons
                .push(RuntimeSkillDeliveryDropReason::RenderBudgetExceeded);
            report.drop_reasons.sort();
            report.drop_reasons.dedup();
            Ok(report)
        }
        _ => Err(Error::config(
            "procedural_memory_delivery_report",
            "render finalization must exactly bind selection, projection material, and actual receipt",
        )),
    }
}

pub fn build_public_safe_procedural_memory_delivery_report(
    plan: &RuntimeSkillRecallPlan,
) -> Result<Option<ProceduralMemoryDeliveryReport>> {
    if plan
        .drop_reasons()
        .contains(&RuntimeSkillDeliveryDropReason::PrivacyBlocked)
    {
        return Ok(None);
    }
    build_procedural_memory_delivery_report(plan).map(Some)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForgettingOperation {
    Delete,
    ForgetByQuery,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgettingDecisionReport {
    pub schema_version: u32,
    pub operation: ForgettingOperation,
    pub affected_owner_count: u64,
    pub affected_facet_count: u64,
    pub affected_graph_count: u64,
    pub tombstone_count: u64,
    pub audit_event_count: u64,
    pub closure_complete: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef};

    fn lifecycle(
        termination: Option<GovernedOwnerTermination>,
        tombstoned: bool,
    ) -> GovernedRecallLifecycleFacts {
        let owner_revision_ref = GovernedOwnerRevisionRef::try_new(
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "owner-1"),
            1,
        )
        .expect("owner revision");
        let successor = match termination {
            Some(GovernedOwnerTermination::Superseded) => Some(
                GovernedOwnerRevisionRef::try_new(
                    GovernedMemoryOwnerRef::new(
                        GovernedMemoryOwnerPlane::LongTerm,
                        "replacement-1",
                    ),
                    1,
                )
                .expect("successor revision"),
            ),
            Some(GovernedOwnerTermination::Revised | GovernedOwnerTermination::Corrected) => Some(
                GovernedOwnerRevisionRef::try_new(
                    GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "owner-1"),
                    2,
                )
                .expect("same-owner successor revision"),
            ),
            _ => None,
        };
        GovernedRecallLifecycleFacts::new(
            owner_revision_ref,
            GovernedOwnerValidity {
                valid_from: 10,
                valid_until: termination.map(|_| 20),
                observed_at: 12,
                predecessor: None,
                successor,
                termination,
            },
            false,
            false,
            tombstoned,
            true,
        )
        .expect("lifecycle")
    }

    fn allowed_gates() -> GovernedRecallAuthorityGates {
        GovernedRecallAuthorityGates {
            disclosure: GovernedRecallDisclosure::Allowed,
            required_premise: GovernedRequiredPremiseGate::NotApplicable,
            profile_budget_drop: GovernedProfileBudgetDrop::None,
        }
    }

    #[test]
    fn historical_superseded_tombstone_does_not_hide_the_valid_predecessor_interval() {
        let decision = decide_governed_recall_eligibility(
            &lifecycle(Some(GovernedOwnerTermination::Superseded), true),
            GovernedRecallTemporalQuery::HistoricalAsOf {
                query_time: 30,
                as_of_time: 15,
            },
            allowed_gates(),
        )
        .expect("historical decision");
        assert_eq!(
            decision.eligibility,
            GovernedRecallEligibility::EligibleHistoricalAsOf
        );
    }

    #[test]
    fn historical_model_rejects_corrected_invalidated_deleted_and_forgotten_material() {
        for (termination, expected_reason) in [
            (
                GovernedOwnerTermination::Corrected,
                GovernedRecallEligibilityReason::Obsolete,
            ),
            (
                GovernedOwnerTermination::Invalidated,
                GovernedRecallEligibilityReason::Invalidated,
            ),
            (
                GovernedOwnerTermination::Deleted,
                GovernedRecallEligibilityReason::Deleted,
            ),
            (
                GovernedOwnerTermination::Forgotten,
                GovernedRecallEligibilityReason::Forgotten,
            ),
        ] {
            let mut lifecycle = lifecycle(Some(termination), true);
            lifecycle.historical_model_allowed = false;
            let report = build_historical_dynamic_state_resolution_report(
                &lifecycle,
                30,
                15,
                allowed_gates(),
            )
            .expect("historical exclusion report");
            let decision = report.as_of_decision.expect("historical decision");
            assert_eq!(decision.eligibility, GovernedRecallEligibility::Excluded);
            assert_eq!(decision.primary_reason, Some(expected_reason));
        }
    }

    #[test]
    fn premise_gate_rejects_blank_refs_and_blocked_satisfied_decisions() {
        for required_premise in [
            GovernedRequiredPremiseGate::Satisfied {
                decision_ref: " ".into(),
            },
            GovernedRequiredPremiseGate::Blocked {
                decision_ref: "premise-1".into(),
                decision: PremiseEvaluationDecision::Satisfied,
            },
        ] {
            assert!(decide_governed_recall_eligibility(
                &lifecycle(None, false),
                GovernedRecallTemporalQuery::Current { query_time: 30 },
                GovernedRecallAuthorityGates {
                    required_premise,
                    ..allowed_gates()
                },
            )
            .is_err());
        }
    }

    #[test]
    fn current_dynamic_state_builder_derives_the_only_canonical_resolution_report() {
        let report = build_current_dynamic_state_resolution_report(
            &lifecycle(None, false),
            30,
            allowed_gates(),
        )
        .expect("canonical current dynamic-state report");

        assert!(report.validate_contract().accepted);
        assert_eq!(
            report.current_decision.eligibility,
            GovernedRecallEligibility::EligibleCurrent
        );
        assert_eq!(report.current_decision.query_time, 30);
        assert_eq!(report.current_decision.effective_time, 30);
        assert_eq!(report.predecessor, report.validity.predecessor);
        assert_eq!(report.successor, report.validity.successor);
        assert!(report.as_of_decision.is_none());
        assert_eq!(report.conflict_count, 0);
        assert_eq!(report.unknown_count, 0);

        let mut unknown_field = serde_json::to_value(&report).expect("serialize report");
        unknown_field
            .as_object_mut()
            .expect("report object")
            .insert("caller_claimed_conflicts".into(), serde_json::json!(1));
        assert!(
            serde_json::from_value::<DynamicStateResolutionReport>(unknown_field).is_err(),
            "dynamic-state reports must reject caller-owned extension fields"
        );
    }

    #[test]
    fn dynamic_state_contract_keeps_typed_as_of_and_resolution_counts_extensible() {
        let lifecycle = lifecycle(Some(GovernedOwnerTermination::Superseded), true);
        let current_decision = decide_governed_recall_eligibility(
            &lifecycle,
            GovernedRecallTemporalQuery::Current { query_time: 30 },
            allowed_gates(),
        )
        .expect("current decision");
        let as_of_decision = decide_governed_recall_eligibility(
            &lifecycle,
            GovernedRecallTemporalQuery::HistoricalAsOf {
                query_time: 30,
                as_of_time: 15,
            },
            allowed_gates(),
        )
        .expect("historical decision");
        let report = DynamicStateResolutionReport {
            schema_version: GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION,
            predecessor: lifecycle.validity.predecessor.clone(),
            successor: lifecycle.validity.successor.clone(),
            validity: lifecycle.validity,
            current_decision,
            as_of_decision: Some(as_of_decision),
            conflict_count: 2,
            unknown_count: 1,
        };

        assert!(
            report.validate_contract().accepted,
            "the generic report contract must not freeze WP1 current-only output"
        );

        let mut outside_half_open_interval = report.clone();
        outside_half_open_interval
            .as_of_decision
            .as_mut()
            .expect("as-of decision")
            .effective_time = 20;
        assert_eq!(
            outside_half_open_interval
                .validate_contract()
                .failures
                .as_slice(),
            &[GovernedContractFailure::DynamicStateDecisionMismatch]
        );
    }

    #[test]
    fn historical_dynamic_state_builder_derives_current_and_as_of_from_one_lifecycle() {
        let report = build_historical_dynamic_state_resolution_report(
            &lifecycle(Some(GovernedOwnerTermination::Superseded), true),
            30,
            15,
            allowed_gates(),
        )
        .expect("canonical historical dynamic-state report");

        assert!(report.validate_contract().accepted);
        assert_eq!(
            report.current_decision.eligibility,
            GovernedRecallEligibility::Excluded
        );
        assert_eq!(
            report
                .as_of_decision
                .as_ref()
                .expect("as-of decision")
                .eligibility,
            GovernedRecallEligibility::EligibleHistoricalAsOf
        );
        assert_eq!(report.current_decision.query_time, 30);
        assert_eq!(
            report
                .as_of_decision
                .as_ref()
                .expect("as-of decision")
                .effective_time,
            15
        );
        assert_eq!(report.conflict_count, 0);
        assert_eq!(report.unknown_count, 0);

        assert!(build_historical_dynamic_state_resolution_report(
            &lifecycle(Some(GovernedOwnerTermination::Superseded), true),
            30,
            20,
            allowed_gates(),
        )
        .is_err());
        assert!(build_historical_dynamic_state_resolution_report(
            &lifecycle(Some(GovernedOwnerTermination::Superseded), true),
            30,
            31,
            allowed_gates(),
        )
        .is_err());
    }
}
