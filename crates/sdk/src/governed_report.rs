use std::collections::{BTreeMap, BTreeSet};

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    GovernedRecallEligibility, GovernedRecallEligibilityReason, MemoryUpdateLineageFailure,
    PremiseEvaluationDecision, PremiseTypedSource,
};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    MemoryRecallTemporalOperation, ProceduralMemoryDeliveryView, RuntimeSkillRecallTransport,
};

pub const GOVERNED_RECALL_PUBLIC_REPORT_SCHEMA_V1: &str = "beetle-memory.governed-recall-public.v1";
pub const GOVERNED_RECALL_OPERATOR_REPORT_SCHEMA_V1: &str =
    "beetle-memory.governed-recall-operator.v1";

const OWNER_REVISION_PREFIX: &str = "governed_owner_revision:sha256:";
const PRIVATE_ADMISSION_PREFIX: &str = "recall_private_admission:sha256:";
const AUTHORITY_PREFIX: &str = "recall_operation_authority:sha256:";
const STORE_RECEIPT_PREFIX: &str = "recall_store_snapshot:sha256:";
const OPERATOR_REPORT_PREFIX: &str = "governed_recall_operator_report:sha256:";

fn is_lower_hex_digest(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn is_runtime_budget_report_id(value: &str) -> bool {
    is_lower_hex_digest(value, "rtb-v2-")
}

pub(crate) fn domain_separated_sha256(domain: &str, parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(
        u64::try_from(domain.len())
            .expect("in-memory domain length fits u64")
            .to_be_bytes(),
    );
    hasher.update(domain.as_bytes());
    for part in parts {
        hasher.update(
            u64::try_from(part.len())
                .expect("in-memory digest part length fits u64")
                .to_be_bytes(),
        );
        hasher.update(part);
    }
    format!("{:x}", hasher.finalize())
}

macro_rules! opaque_safe_ref {
    ($name:ident, $prefix:expr, $domain:expr) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub(crate) fn derive(parts: &[&[u8]]) -> Self {
                Self(format!(
                    "{}{}",
                    $prefix,
                    domain_separated_sha256($domain, parts)
                ))
            }

            fn is_valid(&self) -> bool {
                is_lower_hex_digest(&self.0, $prefix)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                if !is_lower_hex_digest(&value, $prefix) {
                    return Err(D::Error::custom(concat!(
                        stringify!($name),
                        " has an invalid domain or digest"
                    )));
                }
                Ok(Self(value))
            }
        }
    };
}

opaque_safe_ref!(
    GovernedOwnerRevisionSafeRef,
    OWNER_REVISION_PREFIX,
    "beetle_memory_governed_owner_revision_safe_ref_v1"
);
opaque_safe_ref!(
    RecallPrivateAdmissionCommitmentRef,
    PRIVATE_ADMISSION_PREFIX,
    "beetle_memory_recall_private_admission_commitment_v1"
);
opaque_safe_ref!(
    RecallOperationAuthorityRef,
    AUTHORITY_PREFIX,
    "beetle_memory_recall_operation_authority_v1"
);
opaque_safe_ref!(
    RecallStoreSnapshotReceiptRef,
    STORE_RECEIPT_PREFIX,
    "beetle_memory_recall_store_snapshot_receipt_v1"
);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedRecallBudgetSafeCeilingsV1 {
    pub(crate) max_validity_joins: u64,
    pub(crate) max_lineage_depth: u64,
    pub(crate) max_as_of_candidates: u64,
    pub(crate) max_obsolete_decisions: u64,
    pub(crate) max_procedural_candidates: u64,
    pub(crate) max_premises_per_skill: u64,
    pub(crate) max_premise_evidence_reads: u64,
    pub(crate) effective_selected_candidates: u64,
}

impl GovernedRecallBudgetSafeCeilingsV1 {
    fn all_positive(&self) -> bool {
        [
            self.max_validity_joins,
            self.max_lineage_depth,
            self.max_as_of_candidates,
            self.max_obsolete_decisions,
            self.max_procedural_candidates,
            self.max_premises_per_skill,
            self.max_premise_evidence_reads,
            self.effective_selected_candidates,
        ]
        .into_iter()
        .all(|value| value > 0)
    }

    fn max_lineage_items(&self) -> Option<u64> {
        self.max_as_of_candidates
            .checked_mul(self.max_lineage_depth)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallOperationAuthoritySafeViewV1 {
    pub(crate) authority_ref: RecallOperationAuthorityRef,
    pub(crate) private_admission_commitment: RecallPrivateAdmissionCommitmentRef,
    pub(crate) temporal_operation: MemoryRecallTemporalOperation,
    pub(crate) profile: ProfileId,
    pub(crate) capability_catalog_schema: String,
    pub(crate) capability_catalog_identity: String,
    pub(crate) budget_report_identity: String,
    pub(crate) governed_budget_ceilings: GovernedRecallBudgetSafeCeilingsV1,
    pub(crate) runtime_skill_transport: RuntimeSkillRecallTransport,
}

impl RecallOperationAuthoritySafeViewV1 {
    pub fn authority_ref(&self) -> &RecallOperationAuthorityRef {
        &self.authority_ref
    }

    pub const fn temporal_operation(&self) -> MemoryRecallTemporalOperation {
        self.temporal_operation
    }

    pub const fn profile(&self) -> ProfileId {
        self.profile
    }

    pub fn governed_budget_ceilings(&self) -> &GovernedRecallBudgetSafeCeilingsV1 {
        &self.governed_budget_ceilings
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        private_admission_commitment: RecallPrivateAdmissionCommitmentRef,
        temporal_operation: MemoryRecallTemporalOperation,
        profile: ProfileId,
        capability_catalog_schema: String,
        capability_catalog_identity: String,
        budget_report_identity: String,
        governed_budget_ceilings: GovernedRecallBudgetSafeCeilingsV1,
        runtime_skill_transport: RuntimeSkillRecallTransport,
    ) -> Self {
        let mut view = Self {
            authority_ref: RecallOperationAuthorityRef::derive(&[b"uninitialized"]),
            private_admission_commitment,
            temporal_operation,
            profile,
            capability_catalog_schema,
            capability_catalog_identity,
            budget_report_identity,
            governed_budget_ceilings,
            runtime_skill_transport,
        };
        view.authority_ref = view.derived_authority_ref();
        view
    }

    fn derived_authority_ref(&self) -> RecallOperationAuthorityRef {
        let bytes = serde_json::to_vec(&RecallOperationAuthoritySafeCanonicalPayload {
            private_admission_commitment: &self.private_admission_commitment,
            temporal_operation: self.temporal_operation,
            profile: self.profile,
            capability_catalog_schema: &self.capability_catalog_schema,
            capability_catalog_identity: &self.capability_catalog_identity,
            budget_report_identity: &self.budget_report_identity,
            governed_budget_ceilings: &self.governed_budget_ceilings,
            runtime_skill_transport: self.runtime_skill_transport,
        })
        .expect("safe authority canonical payload serialization is infallible");
        RecallOperationAuthorityRef::derive(&[bytes.as_slice()])
    }

    fn is_valid(&self) -> bool {
        self.authority_ref.is_valid()
            && self.private_admission_commitment.is_valid()
            && self.authority_ref == self.derived_authority_ref()
            && self.capability_catalog_schema == crate::PLATFORM_CAPABILITY_SNAPSHOT_SCHEMA
            && is_lower_hex_digest(
                &self.capability_catalog_identity,
                "capability_catalog:sha256:",
            )
            && is_runtime_budget_report_id(&self.budget_report_identity)
            && self.governed_budget_ceilings.all_positive()
    }
}

#[derive(Serialize)]
struct RecallOperationAuthoritySafeCanonicalPayload<'a> {
    private_admission_commitment: &'a RecallPrivateAdmissionCommitmentRef,
    temporal_operation: MemoryRecallTemporalOperation,
    profile: ProfileId,
    capability_catalog_schema: &'a str,
    capability_catalog_identity: &'a str,
    budget_report_identity: &'a str,
    governed_budget_ceilings: &'a GovernedRecallBudgetSafeCeilingsV1,
    runtime_skill_transport: RuntimeSkillRecallTransport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedCandidateSafeBindingV1 {
    pub(crate) candidate_ref: GovernedOwnerRevisionSafeRef,
    pub(crate) eligibility: GovernedRecallEligibility,
    pub(crate) primary_reason: Option<GovernedRecallEligibilityReason>,
    pub(crate) suppression_reasons: Vec<GovernedRecallEligibilityReason>,
    pub(crate) matched: bool,
    pub(crate) selected: bool,
    pub(crate) rendered: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedDynamicStateSafeDecisionV1 {
    pub(crate) owner_revision: GovernedOwnerRevisionSafeRef,
    pub(crate) current_eligibility: GovernedRecallEligibility,
    pub(crate) as_of_eligibility: Option<GovernedRecallEligibility>,
    pub(crate) conflict_count: u64,
    pub(crate) unknown_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedLineageSafeSummaryV1 {
    pub(crate) item_count: u64,
    pub(crate) cycle_count: u64,
    pub(crate) gap_count: u64,
    pub(crate) scope_mismatch_count: u64,
    pub(crate) privacy_mismatch_count: u64,
    pub(crate) depth_exceeded_count: u64,
    pub(crate) complete: bool,
}

impl GovernedLineageSafeSummaryV1 {
    fn failure_total(&self) -> Option<u64> {
        self.cycle_count
            .checked_add(self.gap_count)?
            .checked_add(self.scope_mismatch_count)?
            .checked_add(self.privacy_mismatch_count)?
            .checked_add(self.depth_exceeded_count)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedLineageSafeItemV1 {
    pub(crate) owner_revision: GovernedOwnerRevisionSafeRef,
    pub(crate) predecessor: Option<GovernedOwnerRevisionSafeRef>,
    pub(crate) successor: Option<GovernedOwnerRevisionSafeRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PremiseSafeSummaryV1 {
    pub(crate) source_counts: BTreeMap<PremiseTypedSource, u64>,
    pub(crate) decision_counts: BTreeMap<PremiseEvaluationDecision, u64>,
    pub(crate) required_failure_count: u64,
}

impl PremiseSafeSummaryV1 {
    pub fn source_counts(&self) -> &BTreeMap<PremiseTypedSource, u64> {
        &self.source_counts
    }

    pub fn decision_counts(&self) -> &BTreeMap<PremiseEvaluationDecision, u64> {
        &self.decision_counts
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedRecallIntegrityFailureV1 {
    AuthorityMismatch,
    StoreReceiptMismatch,
    EligibilityCountMismatch,
    CandidateBindingMismatch,
    LineageIncomplete,
    PremiseIncomplete,
    DeliveryMismatch,
    BudgetExceeded,
    PrivacySuppressionMismatch,
    CanonicalOrderMismatch,
    DigestMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedRecallPublicReportV1 {
    pub(crate) schema: String,
    pub(crate) authority: RecallOperationAuthoritySafeViewV1,
    pub(crate) eligibility_counts: BTreeMap<GovernedRecallEligibility, u64>,
    pub(crate) reason_counts: BTreeMap<GovernedRecallEligibilityReason, u64>,
    pub(crate) dynamic_state: Vec<GovernedDynamicStateSafeDecisionV1>,
    pub(crate) lineage: GovernedLineageSafeSummaryV1,
    pub(crate) premise: PremiseSafeSummaryV1,
    pub(crate) procedural_delivery: Vec<ProceduralMemoryDeliveryView>,
    pub(crate) validity_candidate_bindings: Vec<GovernedCandidateSafeBindingV1>,
    pub(crate) privacy_suppression_count: u64,
    pub(crate) profile_suppression_count: u64,
    pub(crate) budget_suppression_count: u64,
    pub(crate) integrity_failures: Vec<GovernedRecallIntegrityFailureV1>,
}

impl GovernedRecallPublicReportV1 {
    pub fn authority(&self) -> &RecallOperationAuthoritySafeViewV1 {
        &self.authority
    }

    pub fn eligibility_counts(&self) -> &BTreeMap<GovernedRecallEligibility, u64> {
        &self.eligibility_counts
    }

    pub fn reason_counts(&self) -> &BTreeMap<GovernedRecallEligibilityReason, u64> {
        &self.reason_counts
    }

    pub fn premise(&self) -> &PremiseSafeSummaryV1 {
        &self.premise
    }

    pub fn validate_contract(&self) -> Vec<GovernedRecallIntegrityFailureV1> {
        let mut failures = self.integrity_failures.clone();
        let ceilings = &self.authority.governed_budget_ceilings;
        if self
            .integrity_failures
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            failures.push(GovernedRecallIntegrityFailureV1::CanonicalOrderMismatch);
        }
        if self.schema != GOVERNED_RECALL_PUBLIC_REPORT_SCHEMA_V1 || !self.authority.is_valid() {
            failures.push(GovernedRecallIntegrityFailureV1::AuthorityMismatch);
        }
        if !exact_eligibility_map(&self.eligibility_counts)
            || !exact_reason_map(&self.reason_counts)
            || !counts_match_bindings(
                &self.validity_candidate_bindings,
                &self.eligibility_counts,
                &self.reason_counts,
            )
        {
            failures.push(GovernedRecallIntegrityFailureV1::EligibilityCountMismatch);
        }
        if checked_len(self.dynamic_state.len())
            .is_none_or(|count| count > ceilings.max_validity_joins)
            || checked_len(self.validity_candidate_bindings.len())
                .is_none_or(|count| count > ceilings.max_validity_joins)
            || self.dynamic_state.iter().any(|decision| {
                decision.conflict_count > ceilings.max_validity_joins
                    || decision.unknown_count > ceilings.max_validity_joins
            })
            || !candidate_bindings_are_valid(&self.validity_candidate_bindings)
        {
            failures.push(GovernedRecallIntegrityFailureV1::CandidateBindingMismatch);
        }
        let dynamic_refs = self
            .dynamic_state
            .iter()
            .map(|decision| &decision.owner_revision)
            .collect::<BTreeSet<_>>();
        let binding_refs = self
            .validity_candidate_bindings
            .iter()
            .map(|binding| &binding.candidate_ref)
            .collect::<BTreeSet<_>>();
        if dynamic_refs != binding_refs
            || dynamic_refs.len() != self.dynamic_state.len()
            || binding_refs.len() != self.validity_candidate_bindings.len()
        {
            failures.push(GovernedRecallIntegrityFailureV1::CandidateBindingMismatch);
        }
        let selected_count = checked_len(
            self.validity_candidate_bindings
                .iter()
                .filter(|binding| binding.selected)
                .count(),
        );
        let rendered_count = checked_len(
            self.validity_candidate_bindings
                .iter()
                .filter(|binding| binding.rendered)
                .count(),
        );
        let procedural_count = checked_len(self.procedural_delivery.len());
        let procedural_selected_count = checked_len(
            self.procedural_delivery
                .iter()
                .filter(|item| item.selected)
                .count(),
        );
        let procedural_rendered_count = checked_len(
            self.procedural_delivery
                .iter()
                .filter(|item| item.rendered)
                .count(),
        );
        if selected_count.is_none_or(|count| count > ceilings.effective_selected_candidates)
            || rendered_count.is_none_or(|count| count > ceilings.effective_selected_candidates)
            || procedural_count.is_none_or(|count| count > ceilings.max_procedural_candidates)
            || procedural_selected_count
                .is_none_or(|count| count > ceilings.max_procedural_candidates)
            || procedural_rendered_count
                .is_none_or(|count| count > ceilings.max_procedural_candidates)
        {
            failures.push(GovernedRecallIntegrityFailureV1::BudgetExceeded);
        }
        if !lineage_summary_is_valid(&self.lineage, ceilings)
            || self
                .dynamic_state
                .windows(2)
                .any(|pair| pair[0].owner_revision >= pair[1].owner_revision)
        {
            failures.push(GovernedRecallIntegrityFailureV1::LineageIncomplete);
        }
        if !premise_summary_is_valid(&self.premise, ceilings, self.authority.profile) {
            failures.push(GovernedRecallIntegrityFailureV1::PremiseIncomplete);
        }
        if !procedural_delivery_is_canonical(&self.procedural_delivery) {
            failures.push(GovernedRecallIntegrityFailureV1::DeliveryMismatch);
        }
        if self.privacy_suppression_count
            != *self
                .reason_counts
                .get(&GovernedRecallEligibilityReason::PrivacyBlocked)
                .unwrap_or(&0)
            || self.profile_suppression_count
                != *self
                    .reason_counts
                    .get(&GovernedRecallEligibilityReason::ProfileBlocked)
                    .unwrap_or(&0)
            || self.budget_suppression_count
                != *self
                    .reason_counts
                    .get(&GovernedRecallEligibilityReason::BudgetBlocked)
                    .unwrap_or(&0)
        {
            failures.push(GovernedRecallIntegrityFailureV1::PrivacySuppressionMismatch);
        }
        failures.sort();
        failures.dedup();
        failures
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedRecallOperatorReportPayloadV1 {
    pub(crate) schema: String,
    pub(crate) public_report: GovernedRecallPublicReportV1,
    pub(crate) store_snapshot_receipt: RecallStoreSnapshotReceiptRef,
    pub(crate) bounded_lineage_items: Vec<GovernedLineageSafeItemV1>,
    pub(crate) manifest_verified: bool,
    pub(crate) read_set_exact: bool,
    pub(crate) session_open_count: u64,
    pub(crate) receipt_count: u64,
}

impl GovernedRecallOperatorReportPayloadV1 {
    pub fn public_report(&self) -> &GovernedRecallPublicReportV1 {
        &self.public_report
    }

    pub fn store_snapshot_receipt(&self) -> &RecallStoreSnapshotReceiptRef {
        &self.store_snapshot_receipt
    }

    pub const fn manifest_verified(&self) -> bool {
        self.manifest_verified
    }

    pub const fn read_set_exact(&self) -> bool {
        self.read_set_exact
    }

    pub const fn session_open_count(&self) -> u64 {
        self.session_open_count
    }

    pub const fn receipt_count(&self) -> u64 {
        self.receipt_count
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedRecallOperatorReportV1 {
    pub(crate) payload: GovernedRecallOperatorReportPayloadV1,
    pub(crate) report_digest: String,
}

impl GovernedRecallOperatorReportV1 {
    pub fn payload(&self) -> &GovernedRecallOperatorReportPayloadV1 {
        &self.payload
    }

    pub fn report_digest(&self) -> &str {
        &self.report_digest
    }

    pub fn validate_contract(&self) -> Vec<GovernedRecallIntegrityFailureV1> {
        let mut failures = self.payload.public_report.validate_contract();
        let ceilings = self
            .payload
            .public_report
            .authority
            .governed_budget_ceilings();
        if self.payload.schema != GOVERNED_RECALL_OPERATOR_REPORT_SCHEMA_V1
            || !self.payload.store_snapshot_receipt.is_valid()
            || !self.payload.manifest_verified
            || !self.payload.read_set_exact
            || self.payload.session_open_count != 1
            || self.payload.receipt_count != 1
        {
            failures.push(GovernedRecallIntegrityFailureV1::StoreReceiptMismatch);
        }
        if checked_len(self.payload.bounded_lineage_items.len())
            .is_none_or(|count| count > ceilings.max_lineage_items().unwrap_or(0))
            || checked_len(self.payload.bounded_lineage_items.len())
                != Some(self.payload.public_report.lineage.item_count)
            || self
                .payload
                .bounded_lineage_items
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            failures.push(GovernedRecallIntegrityFailureV1::CanonicalOrderMismatch);
        }
        let expected_digest = operator_report_digest(&self.payload);
        if !is_lower_hex_digest(&self.report_digest, OPERATOR_REPORT_PREFIX)
            || self.report_digest != expected_digest
        {
            failures.push(GovernedRecallIntegrityFailureV1::DigestMismatch);
        }
        failures.sort();
        failures.dedup();
        failures
    }

    pub(crate) fn from_payload(payload: GovernedRecallOperatorReportPayloadV1) -> Self {
        let report_digest = operator_report_digest(&payload);
        Self {
            payload,
            report_digest,
        }
    }
}

fn operator_report_digest(payload: &GovernedRecallOperatorReportPayloadV1) -> String {
    let bytes = serde_json::to_vec(payload)
        .expect("governed operator report payload serialization is infallible");
    format!(
        "{}{}",
        OPERATOR_REPORT_PREFIX,
        domain_separated_sha256(
            "beetle_memory_governed_recall_operator_report_v1",
            &[bytes.as_slice()]
        )
    )
}

fn exact_eligibility_map(counts: &BTreeMap<GovernedRecallEligibility, u64>) -> bool {
    counts.keys().copied().collect::<BTreeSet<_>>()
        == [
            GovernedRecallEligibility::EligibleCurrent,
            GovernedRecallEligibility::EligibleHistoricalAsOf,
            GovernedRecallEligibility::Excluded,
        ]
        .into_iter()
        .collect()
}

fn exact_reason_map(counts: &BTreeMap<GovernedRecallEligibilityReason, u64>) -> bool {
    counts.keys().copied().collect::<BTreeSet<_>>()
        == [
            GovernedRecallEligibilityReason::PrivacyBlocked,
            GovernedRecallEligibilityReason::Forgotten,
            GovernedRecallEligibilityReason::Deleted,
            GovernedRecallEligibilityReason::Invalidated,
            GovernedRecallEligibilityReason::Superseded,
            GovernedRecallEligibilityReason::Obsolete,
            GovernedRecallEligibilityReason::Stale,
            GovernedRecallEligibilityReason::PremiseBlocked,
            GovernedRecallEligibilityReason::ProfileBlocked,
            GovernedRecallEligibilityReason::BudgetBlocked,
            GovernedRecallEligibilityReason::Tombstoned,
            GovernedRecallEligibilityReason::Redacted,
        ]
        .into_iter()
        .collect()
}

fn counts_match_bindings(
    bindings: &[GovernedCandidateSafeBindingV1],
    eligibility_counts: &BTreeMap<GovernedRecallEligibility, u64>,
    reason_counts: &BTreeMap<GovernedRecallEligibilityReason, u64>,
) -> bool {
    let mut observed_eligibility = eligibility_counts
        .keys()
        .copied()
        .map(|key| (key, 0u64))
        .collect::<BTreeMap<_, _>>();
    let mut observed_reasons = reason_counts
        .keys()
        .copied()
        .map(|key| (key, 0u64))
        .collect::<BTreeMap<_, _>>();
    for binding in bindings {
        let Some(count) = observed_eligibility.get_mut(&binding.eligibility) else {
            return false;
        };
        let Some(next) = count.checked_add(1) else {
            return false;
        };
        *count = next;
        for reason in &binding.suppression_reasons {
            let Some(count) = observed_reasons.get_mut(reason) else {
                return false;
            };
            let Some(next) = count.checked_add(1) else {
                return false;
            };
            *count = next;
        }
    }
    observed_eligibility == *eligibility_counts && observed_reasons == *reason_counts
}

fn candidate_bindings_are_valid(bindings: &[GovernedCandidateSafeBindingV1]) -> bool {
    let mut refs = BTreeSet::new();
    bindings.iter().all(|binding| {
        let reasons = binding
            .suppression_reasons
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        binding.candidate_ref.is_valid()
            && refs.insert(binding.candidate_ref.clone())
            && reasons.len() == binding.suppression_reasons.len()
            && binding
                .suppression_reasons
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            && binding.primary_reason == binding.suppression_reasons.first().copied()
            && (!binding.rendered || binding.selected)
            && (!binding.selected || binding.matched)
    })
}

fn lineage_summary_is_valid(
    lineage: &GovernedLineageSafeSummaryV1,
    ceilings: &GovernedRecallBudgetSafeCeilingsV1,
) -> bool {
    let Some(failure_total) = lineage.failure_total() else {
        return false;
    };
    lineage.item_count <= ceilings.max_lineage_items().unwrap_or(0)
        && lineage.complete == (failure_total == 0)
}

fn premise_summary_is_valid(
    premise: &PremiseSafeSummaryV1,
    ceilings: &GovernedRecallBudgetSafeCeilingsV1,
    profile: ProfileId,
) -> bool {
    let exact_sources = premise
        .source_counts
        .keys()
        .copied()
        .collect::<BTreeSet<_>>()
        == [
            PremiseTypedSource::RegisteredCapability,
            PremiseTypedSource::OpaquePresenceAttestation,
            PremiseTypedSource::GovernedEnvironmentEvidence,
            PremiseTypedSource::TaskLearning,
            PremiseTypedSource::TaskRun,
            PremiseTypedSource::TaskArtifact,
        ]
        .into_iter()
        .collect();
    let exact_decisions = premise
        .decision_counts
        .keys()
        .copied()
        .collect::<BTreeSet<_>>()
        == [
            PremiseEvaluationDecision::Satisfied,
            PremiseEvaluationDecision::Unsatisfied,
            PremiseEvaluationDecision::Unknown,
            PremiseEvaluationDecision::Expired,
            PremiseEvaluationDecision::PrivacyBlocked,
        ]
        .into_iter()
        .collect();
    let source_total = premise
        .source_counts
        .values()
        .try_fold(0u64, |total, count| total.checked_add(*count));
    let decision_total = premise
        .decision_counts
        .values()
        .try_fold(0u64, |total, count| total.checked_add(*count));
    let maximum = ceilings
        .max_procedural_candidates
        .checked_mul(ceilings.max_premises_per_skill);
    let esp_deep_sources_zero = !matches!(
        profile,
        ProfileId::EspStandaloneMemory | ProfileId::EspEmbeddedSdk
    ) || [
        PremiseTypedSource::GovernedEnvironmentEvidence,
        PremiseTypedSource::TaskLearning,
        PremiseTypedSource::TaskRun,
        PremiseTypedSource::TaskArtifact,
    ]
    .into_iter()
    .all(|source| premise.source_counts.get(&source) == Some(&0));
    exact_sources
        && exact_decisions
        && source_total.is_some()
        && source_total == decision_total
        && source_total <= maximum
        && premise.required_failure_count <= decision_total.unwrap_or(0)
        && esp_deep_sources_zero
}

fn procedural_delivery_is_canonical(items: &[ProceduralMemoryDeliveryView]) -> bool {
    items
        .windows(2)
        .all(|pair| pair[0].candidate_ref < pair[1].candidate_ref)
        && items.iter().all(|item| {
            is_lower_hex_digest(
                &item.candidate_ref,
                "runtime_skill_projection_candidate:sha256:",
            ) && (!item.rendered || item.selected)
                && (!item.selected || item.matched)
                && item.drop_reasons.windows(2).all(|pair| pair[0] < pair[1])
        })
}

pub(crate) fn empty_premise_source_counts() -> BTreeMap<PremiseTypedSource, u64> {
    [
        PremiseTypedSource::RegisteredCapability,
        PremiseTypedSource::OpaquePresenceAttestation,
        PremiseTypedSource::GovernedEnvironmentEvidence,
        PremiseTypedSource::TaskLearning,
        PremiseTypedSource::TaskRun,
        PremiseTypedSource::TaskArtifact,
    ]
    .into_iter()
    .map(|source| (source, 0))
    .collect()
}

pub(crate) fn empty_premise_decision_counts() -> BTreeMap<PremiseEvaluationDecision, u64> {
    [
        PremiseEvaluationDecision::Satisfied,
        PremiseEvaluationDecision::Unsatisfied,
        PremiseEvaluationDecision::Unknown,
        PremiseEvaluationDecision::Expired,
        PremiseEvaluationDecision::PrivacyBlocked,
    ]
    .into_iter()
    .map(|decision| (decision, 0))
    .collect()
}

pub(crate) fn empty_eligibility_counts() -> BTreeMap<GovernedRecallEligibility, u64> {
    [
        GovernedRecallEligibility::EligibleCurrent,
        GovernedRecallEligibility::EligibleHistoricalAsOf,
        GovernedRecallEligibility::Excluded,
    ]
    .into_iter()
    .map(|eligibility| (eligibility, 0))
    .collect()
}

pub(crate) fn empty_reason_counts() -> BTreeMap<GovernedRecallEligibilityReason, u64> {
    [
        GovernedRecallEligibilityReason::PrivacyBlocked,
        GovernedRecallEligibilityReason::Forgotten,
        GovernedRecallEligibilityReason::Deleted,
        GovernedRecallEligibilityReason::Invalidated,
        GovernedRecallEligibilityReason::Superseded,
        GovernedRecallEligibilityReason::Obsolete,
        GovernedRecallEligibilityReason::Stale,
        GovernedRecallEligibilityReason::PremiseBlocked,
        GovernedRecallEligibilityReason::ProfileBlocked,
        GovernedRecallEligibilityReason::BudgetBlocked,
        GovernedRecallEligibilityReason::Tombstoned,
        GovernedRecallEligibilityReason::Redacted,
    ]
    .into_iter()
    .map(|reason| (reason, 0))
    .collect()
}

pub(crate) fn lineage_failure_counts(
    failures: impl IntoIterator<Item = MemoryUpdateLineageFailure>,
) -> Option<(u64, u64, u64, u64, u64)> {
    let mut counts = (0u64, 0u64, 0u64, 0u64, 0u64);
    for failure in failures {
        match failure {
            MemoryUpdateLineageFailure::Cycle => counts.0 = counts.0.checked_add(1)?,
            MemoryUpdateLineageFailure::Gap => counts.1 = counts.1.checked_add(1)?,
            MemoryUpdateLineageFailure::ScopeMismatch => counts.2 = counts.2.checked_add(1)?,
            MemoryUpdateLineageFailure::PrivacyMismatch => counts.3 = counts.3.checked_add(1)?,
            MemoryUpdateLineageFailure::DepthExceeded => counts.4 = counts.4.checked_add(1)?,
        }
    }
    Some(counts)
}

fn checked_len(value: usize) -> Option<u64> {
    u64::try_from(value).ok()
}
