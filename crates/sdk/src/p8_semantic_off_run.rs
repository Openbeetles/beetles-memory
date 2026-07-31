use std::collections::BTreeSet;

use bm_core::memory::{
    GovernedOwnerTermination, GovernedRecallEligibilityReason, LongTermMemoryControlRevision,
    LongTermMemoryVersionMaterial,
};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

use crate::{
    governed_report::domain_separated_sha256, GovernedRecallOperatorReportV1, MemoryRecallRequest,
    RecallOperationAuthorityRef, RecallStoreSnapshotReceiptRef,
};

pub const P8_SEMANTIC_OFF_RUN_REPORT_SCHEMA: &str = "beetle-memory.p8.semantic-off-run-report.v1";
pub const P8_SEMANTIC_OFF_RUN_METHOD: &str = "sdk_p8_semantic_off_run_v1";

const SAFE_CANDIDATE_PREFIX: &str = "p8_semantic_safe_candidate:sha256:";
const OFF_RUN_DIGEST_PREFIX: &str = "p8_semantic_off_run:sha256:";
const NEGATIVE_PROOF_PREFIX: &str = "p8_semantic_negative_proof:sha256:";
const REPORT_DIGEST_PREFIX: &str = "p8_semantic_off_run_report:sha256:";
const FORGET_BINDING_PREFIX: &str = "p8_forgetting_pre_operation:sha256:";
const FORGET_RECEIPT_PREFIX: &str = "p8_forget_transaction:sha256:";
const FORGET_POST_IMAGE_RECEIPT_PREFIX: &str = "p8_forget_post_image:sha256:";
const NO_EXECUTION_CLOSURE_PREFIX: &str = "p8_no_execution_closure:sha256:";

fn is_lower_hex_digest(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn prefixed_digest(prefix: &str, domain: &str, parts: &[&[u8]]) -> String {
    format!("{}{}", prefix, domain_separated_sha256(domain, parts))
}

macro_rules! p8_opaque_ref {
    ($name:ident, $prefix:expr) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
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

p8_opaque_ref!(P8SemanticSafeCandidateRef, SAFE_CANDIDATE_PREFIX);
p8_opaque_ref!(P8SemanticOffRunDigest, OFF_RUN_DIGEST_PREFIX);
p8_opaque_ref!(P8SemanticNegativeProofRef, NEGATIVE_PROOF_PREFIX);
p8_opaque_ref!(P8SemanticOffRunReportDigest, REPORT_DIGEST_PREFIX);
p8_opaque_ref!(P8ForgettingPreOperationRef, FORGET_BINDING_PREFIX);
p8_opaque_ref!(P8ForgetTransactionReceiptRef, FORGET_RECEIPT_PREFIX);
p8_opaque_ref!(
    P8ForgetPostImageReceiptRef,
    FORGET_POST_IMAGE_RECEIPT_PREFIX
);
p8_opaque_ref!(P8NoExecutionClosureReceiptRef, NO_EXECUTION_CLOSURE_PREFIX);

#[derive(Clone, Debug)]
pub(crate) struct P8SemanticLongTermCounterfactualInput {
    binding: P8SemanticSafeCandidateBinding,
    owner_group_ref: P8SemanticSafeCandidateRef,
    predecessor: Option<P8SemanticSafeCandidateRef>,
    predecessor_reason: Option<GovernedRecallEligibilityReason>,
    successor: Option<P8SemanticSafeCandidateRef>,
}

impl P8SemanticLongTermCounterfactualInput {
    pub(crate) fn from_retained_materials(
        retained: Vec<(
            LongTermMemoryVersionMaterial,
            Option<LongTermMemoryControlRevision>,
        )>,
        max_inputs: usize,
    ) -> crate::Result<Vec<Self>> {
        if retained.len() > max_inputs {
            return Err(crate::Error::config(
                "p8_semantic_counterfactual_input",
                "retained counterfactual inputs exceed the request-pinned validity budget",
            ));
        }
        retained
            .into_iter()
            .filter(|(material, _)| material.privacy_class.projection_content_allowed())
            .map(|(material, control)| {
                let revision_ref = material.owner_revision_ref();
                let candidate_ref = safe_candidate_ref_for_owner_revision(&revision_ref)?;
                let predecessor = material
                    .origin
                    .predecessor
                    .as_ref()
                    .map(safe_candidate_ref_for_owner_revision)
                    .transpose()?;
                let predecessor_reason = material.origin.predecessor.as_ref().map(|predecessor| {
                    if predecessor.owner_ref == revision_ref.owner_ref {
                        GovernedRecallEligibilityReason::Obsolete
                    } else {
                        GovernedRecallEligibilityReason::Superseded
                    }
                });
                let successor = control
                    .as_ref()
                    .and_then(|value| value.transition.successor.as_ref())
                    .map(safe_candidate_ref_for_owner_revision)
                    .transpose()?;
                let primary_reason =
                    control
                        .as_ref()
                        .map(|value| match value.transition.termination {
                            GovernedOwnerTermination::Revised
                            | GovernedOwnerTermination::Corrected => {
                                GovernedRecallEligibilityReason::Obsolete
                            }
                            GovernedOwnerTermination::Superseded => {
                                GovernedRecallEligibilityReason::Superseded
                            }
                            GovernedOwnerTermination::Invalidated => {
                                GovernedRecallEligibilityReason::Invalidated
                            }
                            GovernedOwnerTermination::Deleted => {
                                GovernedRecallEligibilityReason::Deleted
                            }
                            GovernedOwnerTermination::Forgotten => {
                                GovernedRecallEligibilityReason::Forgotten
                            }
                        });
                let group_bytes = serde_json::to_vec(&material.owner_ref).map_err(|error| {
                    crate::Error::config("p8_semantic_counterfactual_input", error.to_string())
                })?;
                Ok(Self {
                    binding: P8SemanticSafeCandidateBinding {
                        candidate_ref,
                        candidate_kind: P8SemanticSafeCandidateKind::CounterfactualSafeOnly,
                        primary_reason,
                        suppression_reasons: primary_reason.into_iter().collect(),
                        selected: primary_reason.is_none(),
                        rendered: false,
                    },
                    owner_group_ref: P8SemanticSafeCandidateRef(prefixed_digest(
                        SAFE_CANDIDATE_PREFIX,
                        "beetle_memory_p8_counterfactual_owner_group_v1",
                        &[group_bytes.as_slice()],
                    )),
                    predecessor,
                    predecessor_reason,
                    successor,
                })
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8SemanticOffRunKey {
    TemporalValidityGateOff,
    UpdateLineageOff,
    ObsoleteSuppressionOff,
    InvalidatedSuppressionNegativeOff,
    ForgettingSuppressionNegativeOff,
    ProceduralWorkflowEvidenceOff,
    EnvironmentPremiseGateOff,
    DynamicStateConsolidationOff,
}

impl P8SemanticOffRunKey {
    pub const ALL: [Self; 8] = [
        Self::TemporalValidityGateOff,
        Self::UpdateLineageOff,
        Self::ObsoleteSuppressionOff,
        Self::InvalidatedSuppressionNegativeOff,
        Self::ForgettingSuppressionNegativeOff,
        Self::ProceduralWorkflowEvidenceOff,
        Self::EnvironmentPremiseGateOff,
        Self::DynamicStateConsolidationOff,
    ];

    const fn execution_mode(self) -> P8SemanticOffRunExecutionMode {
        match self {
            Self::InvalidatedSuppressionNegativeOff
            | Self::ForgettingSuppressionNegativeOff
            | Self::EnvironmentPremiseGateOff => {
                P8SemanticOffRunExecutionMode::CounterfactualSafeOnly
            }
            _ => P8SemanticOffRunExecutionMode::PairedProductionSafe,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8SemanticOffRunExecutionMode {
    PairedProductionSafe,
    CounterfactualSafeOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8SemanticOffRunFeatureState {
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8SemanticOffRunFailure {
    SchemaMismatch,
    MethodMismatch,
    BaselineInvalid,
    AuthorityMismatch,
    StoreReceiptMismatch,
    ExactObservationSetMismatch,
    ObservationInvalid,
    NegativeProofMismatch,
    ResourceDeltaInvalid,
    DigestMismatch,
}

#[derive(Clone, Debug)]
pub struct P8SemanticOffRunRequest {
    recall: MemoryRecallRequest,
    forgetting_pre_operation: Option<P8ForgettingPreOperationAuthority>,
}

impl P8SemanticOffRunRequest {
    pub fn new(recall: MemoryRecallRequest) -> Self {
        Self {
            recall,
            forgetting_pre_operation: None,
        }
    }

    pub fn with_forgetting_authority(
        recall: MemoryRecallRequest,
        forgetting_pre_operation: P8ForgettingPreOperationAuthority,
    ) -> Self {
        Self {
            recall,
            forgetting_pre_operation: Some(forgetting_pre_operation),
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (MemoryRecallRequest, Option<P8ForgettingPreOperationBinding>) {
        (
            self.recall,
            self.forgetting_pre_operation
                .map(P8ForgettingPreOperationAuthority::into_binding),
        )
    }
}

#[derive(Clone, Debug)]
pub struct P8ForgettingPreOperationAuthority {
    binding: P8ForgettingPreOperationBinding,
}

impl P8ForgettingPreOperationAuthority {
    pub fn binding_ref(&self) -> &P8ForgettingPreOperationRef {
        &self.binding.binding_ref
    }

    pub fn forgotten_candidate_refs(&self) -> &[P8SemanticSafeCandidateRef] {
        &self.binding.forgotten_candidate_refs
    }

    fn into_binding(self) -> P8ForgettingPreOperationBinding {
        self.binding
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8ForgettingPreOperationBinding {
    binding_ref: P8ForgettingPreOperationRef,
    forget_transaction_receipt: P8ForgetTransactionReceiptRef,
    post_forget_store_receipt: P8ForgetPostImageReceiptRef,
    forgotten_candidate_refs: Vec<P8SemanticSafeCandidateRef>,
    verified_absent_raw_address_count: u64,
}

impl P8ForgettingPreOperationBinding {
    pub fn binding_ref(&self) -> &P8ForgettingPreOperationRef {
        &self.binding_ref
    }

    pub fn forget_transaction_receipt(&self) -> &P8ForgetTransactionReceiptRef {
        &self.forget_transaction_receipt
    }

    pub fn post_forget_store_receipt(&self) -> &P8ForgetPostImageReceiptRef {
        &self.post_forget_store_receipt
    }

    pub fn forgotten_candidate_refs(&self) -> &[P8SemanticSafeCandidateRef] {
        &self.forgotten_candidate_refs
    }

    pub(crate) fn authority_from_forget_mutation(
        report: &crate::MemoryLongTermMutationReport,
        transaction: &crate::MemoryWriteTransactionReport,
        pre_forget_raw_addresses: &[(String, String)],
        post_forget_receipt: &crate::store_internal::StoreReadReceipt,
    ) -> crate::Result<P8ForgettingPreOperationAuthority> {
        if !report.accepted
            || report.dry_run
            || report.operation != "forget_by_query"
            || report.affected_records.is_empty()
            || pre_forget_raw_addresses.is_empty()
            || post_forget_receipt.state_digest.len() != 64
            || post_forget_receipt.json_doc_count != 0
            || post_forget_receipt.blob_count != 0
            || post_forget_receipt.entry_count != pre_forget_raw_addresses.len()
        {
            return Err(crate::Error::config(
                "p8_forgetting_pre_operation",
                "P8 forgetting binding requires one accepted non-dry-run Forget transaction",
            ));
        }
        let mut forgotten_candidate_refs = report
            .affected_records
            .iter()
            .map(|record| {
                let owner_revision_ref = bm_core::memory::GovernedOwnerRevisionRef::try_new(
                    bm_core::memory::GovernedMemoryOwnerRef::new(
                        bm_core::memory::GovernedMemoryOwnerPlane::LongTerm,
                        record.record_id.clone(),
                    ),
                    record.previous_owner_revision,
                )?;
                let owner_bytes = serde_json::to_vec(&owner_revision_ref).map_err(|error| {
                    crate::Error::config("p8_forgetting_pre_operation", error.to_string())
                })?;
                let owner_safe_ref =
                    crate::GovernedOwnerRevisionSafeRef::derive(&[owner_bytes.as_slice()]);
                let bytes = serde_json::to_vec(&("governed", owner_safe_ref)).map_err(|error| {
                    crate::Error::config("p8_forgetting_pre_operation", error.to_string())
                })?;
                Ok(P8SemanticSafeCandidateRef(prefixed_digest(
                    SAFE_CANDIDATE_PREFIX,
                    "beetle_memory_p8_semantic_safe_candidate_v1",
                    &[bytes.as_slice()],
                )))
            })
            .collect::<crate::Result<Vec<_>>>()?;
        forgotten_candidate_refs.sort();
        forgotten_candidate_refs.dedup();
        if transaction.operation != "long_term_control.mutation"
            || transaction.planned_mutations == 0
            || transaction.committed_mutations != transaction.planned_mutations
            || transaction.partial_write
            || transaction.changed_count != report.affected_records.len()
        {
            return Err(crate::Error::config(
                "p8_forgetting_pre_operation",
                "Forget transaction receipt is not an exact committed post-image",
            ));
        }
        let mut pre_forget_raw_addresses = pre_forget_raw_addresses.to_vec();
        pre_forget_raw_addresses.sort();
        pre_forget_raw_addresses.dedup();
        let verified_absent_raw_address_count = u64::try_from(pre_forget_raw_addresses.len())
            .map_err(|_| {
                crate::Error::config(
                    "p8_forgetting_pre_operation",
                    "verified absent raw address count overflowed",
                )
            })?;
        let transaction_bytes = serde_json::to_vec(&(
            report.operation,
            report.affected_records.len(),
            report.tombstones.len(),
            &forgotten_candidate_refs,
            &transaction.transaction_id,
            &transaction.operation,
            transaction.planned_mutations,
            transaction.committed_mutations,
            &transaction.event_ids,
            &transaction.budget_report.admission_report_id,
            transaction.changed_count,
            transaction.partial_write,
        ))
        .map_err(|error| crate::Error::config("p8_forgetting_pre_operation", error.to_string()))?;
        let forget_transaction_receipt = P8ForgetTransactionReceiptRef(prefixed_digest(
            FORGET_RECEIPT_PREFIX,
            "beetle_memory_p8_forget_transaction_receipt_v1",
            &[transaction_bytes.as_slice()],
        ));
        let post_image_bytes = serde_json::to_vec(&(
            &pre_forget_raw_addresses,
            &post_forget_receipt.state_digest,
            post_forget_receipt.json_doc_count,
            post_forget_receipt.blob_count,
            post_forget_receipt.event_count,
            post_forget_receipt.entry_count,
            post_forget_receipt.json_bytes,
            post_forget_receipt.blob_bytes,
            verified_absent_raw_address_count,
        ))
        .map_err(|error| crate::Error::config("p8_forgetting_pre_operation", error.to_string()))?;
        let post_forget_store_receipt = P8ForgetPostImageReceiptRef(prefixed_digest(
            FORGET_POST_IMAGE_RECEIPT_PREFIX,
            "beetle_memory_p8_forget_post_image_receipt_v1",
            &[post_image_bytes.as_slice()],
        ));
        let binding_bytes = serde_json::to_vec(&(
            &forget_transaction_receipt,
            &post_forget_store_receipt,
            &forgotten_candidate_refs,
            verified_absent_raw_address_count,
        ))
        .map_err(|error| crate::Error::config("p8_forgetting_pre_operation", error.to_string()))?;
        Ok(P8ForgettingPreOperationAuthority {
            binding: Self {
                binding_ref: P8ForgettingPreOperationRef(prefixed_digest(
                    FORGET_BINDING_PREFIX,
                    "beetle_memory_p8_forgetting_pre_operation_binding_v1",
                    &[binding_bytes.as_slice()],
                )),
                forget_transaction_receipt,
                post_forget_store_receipt,
                forgotten_candidate_refs,
                verified_absent_raw_address_count,
            },
        })
    }

    fn is_valid(&self) -> bool {
        if !self.binding_ref.is_valid() {
            return false;
        }
        if !self.forget_transaction_receipt.is_valid()
            || !self.post_forget_store_receipt.is_valid()
            || self.forgotten_candidate_refs.is_empty()
            || !self
                .forgotten_candidate_refs
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            || self
                .forgotten_candidate_refs
                .iter()
                .any(|candidate_ref| !candidate_ref.is_valid())
            || self.verified_absent_raw_address_count == 0
        {
            return false;
        }
        let binding_bytes = serde_json::to_vec(&(
            &self.forget_transaction_receipt,
            &self.post_forget_store_receipt,
            &self.forgotten_candidate_refs,
            self.verified_absent_raw_address_count,
        ))
        .expect("P8 forgetting binding canonical serialization is infallible");
        self.binding_ref
            == P8ForgettingPreOperationRef(prefixed_digest(
                FORGET_BINDING_PREFIX,
                "beetle_memory_p8_forgetting_pre_operation_binding_v1",
                &[binding_bytes.as_slice()],
            ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticSafeCandidateBinding {
    candidate_ref: P8SemanticSafeCandidateRef,
    candidate_kind: P8SemanticSafeCandidateKind,
    primary_reason: Option<GovernedRecallEligibilityReason>,
    suppression_reasons: Vec<GovernedRecallEligibilityReason>,
    selected: bool,
    rendered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8SemanticSafeCandidateKind {
    GovernedMemory,
    ProceduralMemory,
    CounterfactualSafeOnly,
}

impl P8SemanticSafeCandidateBinding {
    pub fn candidate_ref(&self) -> &P8SemanticSafeCandidateRef {
        &self.candidate_ref
    }

    pub const fn selected(&self) -> bool {
        self.selected
    }

    pub const fn candidate_kind(&self) -> P8SemanticSafeCandidateKind {
        self.candidate_kind
    }

    pub const fn primary_reason(&self) -> Option<GovernedRecallEligibilityReason> {
        self.primary_reason
    }

    pub fn suppression_reasons(&self) -> &[GovernedRecallEligibilityReason] {
        &self.suppression_reasons
    }

    pub const fn rendered(&self) -> bool {
        self.rendered
    }
}

pub(crate) struct P8NoExecutionCounterfactualAuthority {
    closure_receipt: P8NoExecutionClosureReceiptRef,
    provider_call_count: u64,
    procedure_execution_count: u64,
    tool_execution_count: u64,
    workflow_execution_count: u64,
}

impl P8NoExecutionCounterfactualAuthority {
    pub(crate) fn issue_for_capability_free_production_recall(
        authority_ref: &RecallOperationAuthorityRef,
        receipt: &crate::store_internal::StoreReadReceipt,
        session_open_count: u64,
        receipt_count: u64,
    ) -> crate::Result<Self> {
        if receipt.state_digest.len() != 64 || session_open_count != 1 || receipt_count != 1 {
            return Err(crate::Error::config(
                "p8_counterfactual_execution_authority",
                "counterfactual execution authority requires one exact production recall closure",
            ));
        }
        let evidence = (
            authority_ref,
            &receipt.state_digest,
            receipt.entry_count,
            receipt.json_doc_count,
            receipt.blob_count,
            receipt.event_count,
            session_open_count,
            receipt_count,
            "production_recall_capability_free_v1",
        );
        let evidence_bytes = serde_json::to_vec(&evidence).map_err(|error| {
            crate::Error::config("p8_counterfactual_execution_authority", error.to_string())
        })?;
        Ok(Self {
            closure_receipt: P8NoExecutionClosureReceiptRef(prefixed_digest(
                NO_EXECUTION_CLOSURE_PREFIX,
                "beetle_memory_p8_no_execution_closure_receipt_v1",
                &[evidence_bytes.as_slice()],
            )),
            provider_call_count: 0,
            procedure_execution_count: 0,
            tool_execution_count: 0,
            workflow_execution_count: 0,
        })
    }

    fn closure_receipt(&self) -> &P8NoExecutionClosureReceiptRef {
        &self.closure_receipt
    }

    const fn provider_call_count(&self) -> u64 {
        self.provider_call_count
    }

    const fn procedure_execution_count(&self) -> u64 {
        self.procedure_execution_count
    }

    const fn tool_execution_count(&self) -> u64 {
        self.tool_execution_count
    }

    const fn workflow_execution_count(&self) -> u64 {
        self.workflow_execution_count
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticNegativeProofReceipt {
    key: P8SemanticOffRunKey,
    authority_ref: RecallOperationAuthorityRef,
    store_snapshot_receipt: RecallStoreSnapshotReceiptRef,
    actual_selected_count: u64,
    actual_rendered_count: u64,
    provider_projection_count: u64,
    provider_call_count: u64,
    procedure_execution_count: u64,
    tool_execution_count: u64,
    workflow_execution_count: u64,
    capability_free_closure_receipt: P8NoExecutionClosureReceiptRef,
    forgetting_pre_operation: Option<P8ForgettingPreOperationBinding>,
    receipt_ref: P8SemanticNegativeProofRef,
}

impl P8SemanticNegativeProofReceipt {
    fn build(
        key: P8SemanticOffRunKey,
        authority_ref: RecallOperationAuthorityRef,
        store_snapshot_receipt: RecallStoreSnapshotReceiptRef,
        forgetting_pre_operation: Option<P8ForgettingPreOperationBinding>,
        execution_authority: &P8NoExecutionCounterfactualAuthority,
    ) -> Self {
        let mut value = Self {
            key,
            authority_ref,
            store_snapshot_receipt,
            actual_selected_count: 0,
            actual_rendered_count: 0,
            provider_projection_count: 0,
            provider_call_count: execution_authority.provider_call_count(),
            procedure_execution_count: execution_authority.procedure_execution_count(),
            tool_execution_count: execution_authority.tool_execution_count(),
            workflow_execution_count: execution_authority.workflow_execution_count(),
            capability_free_closure_receipt: execution_authority.closure_receipt().clone(),
            forgetting_pre_operation,
            receipt_ref: P8SemanticNegativeProofRef(prefixed_digest(
                NEGATIVE_PROOF_PREFIX,
                "beetle_memory_p8_semantic_negative_proof_v1",
                &[b"uninitialized"],
            )),
        };
        value.receipt_ref = value.derived_receipt_ref();
        value
    }

    fn derived_receipt_ref(&self) -> P8SemanticNegativeProofRef {
        let bytes = serde_json::to_vec(&(
            self.key,
            &self.authority_ref,
            &self.store_snapshot_receipt,
            self.actual_selected_count,
            self.actual_rendered_count,
            self.provider_projection_count,
            self.provider_call_count,
            self.procedure_execution_count,
            self.tool_execution_count,
            self.workflow_execution_count,
            &self.capability_free_closure_receipt,
            &self.forgetting_pre_operation,
        ))
        .expect("P8 negative-proof canonical payload serialization is infallible");
        P8SemanticNegativeProofRef(prefixed_digest(
            NEGATIVE_PROOF_PREFIX,
            "beetle_memory_p8_semantic_negative_proof_v1",
            &[bytes.as_slice()],
        ))
    }

    fn is_valid_for(
        &self,
        key: P8SemanticOffRunKey,
        authority_ref: &RecallOperationAuthorityRef,
        store_snapshot_receipt: &RecallStoreSnapshotReceiptRef,
    ) -> bool {
        self.key == key
            && &self.authority_ref == authority_ref
            && &self.store_snapshot_receipt == store_snapshot_receipt
            && self.actual_selected_count == 0
            && self.actual_rendered_count == 0
            && self.provider_projection_count == 0
            && self.provider_call_count == 0
            && self.procedure_execution_count == 0
            && self.tool_execution_count == 0
            && self.workflow_execution_count == 0
            && self.capability_free_closure_receipt.is_valid()
            && self
                .forgetting_pre_operation
                .as_ref()
                .is_none_or(P8ForgettingPreOperationBinding::is_valid)
            && self.receipt_ref.is_valid()
            && self.receipt_ref == self.derived_receipt_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticResourceMeasurement {
    elapsed_millis: u64,
    peak_rss_bytes: u64,
    elapsed_delta_millis: i64,
    peak_rss_delta_bytes: i64,
}

impl P8SemanticResourceMeasurement {
    const ZERO: Self = Self {
        elapsed_millis: 0,
        peak_rss_bytes: 0,
        elapsed_delta_millis: 0,
        peak_rss_delta_bytes: 0,
    };

    fn is_valid(&self) -> bool {
        i64::try_from(self.elapsed_millis)
            .ok()
            .is_some_and(|value| value == self.elapsed_delta_millis)
            && i64::try_from(self.peak_rss_bytes)
                .ok()
                .is_some_and(|value| value == self.peak_rss_delta_bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticOffRunObservation {
    key: P8SemanticOffRunKey,
    feature_state: P8SemanticOffRunFeatureState,
    execution_mode: P8SemanticOffRunExecutionMode,
    baseline_report_digest: String,
    off_run_digest: P8SemanticOffRunDigest,
    authority_ref: RecallOperationAuthorityRef,
    store_snapshot_receipt: RecallStoreSnapshotReceiptRef,
    baseline_bindings: Vec<P8SemanticSafeCandidateBinding>,
    off_run_bindings: Vec<P8SemanticSafeCandidateBinding>,
    premise_decision_digest: P8SemanticOffRunDigest,
    lineage_decision_digest: P8SemanticOffRunDigest,
    resource: P8SemanticResourceMeasurement,
    applicable: bool,
    executed: bool,
    negative_proof: Option<P8SemanticNegativeProofReceipt>,
    provider_call_count: u64,
    procedure_execution_count: u64,
    tool_execution_count: u64,
    workflow_execution_count: u64,
    capability_free_closure_receipt: P8NoExecutionClosureReceiptRef,
    failures: Vec<P8SemanticOffRunFailure>,
}

impl P8SemanticOffRunObservation {
    pub const fn key(&self) -> P8SemanticOffRunKey {
        self.key
    }

    pub const fn feature_state(&self) -> P8SemanticOffRunFeatureState {
        self.feature_state
    }

    pub const fn execution_mode(&self) -> P8SemanticOffRunExecutionMode {
        self.execution_mode
    }

    pub fn baseline_report_digest(&self) -> &str {
        &self.baseline_report_digest
    }

    pub fn off_run_digest(&self) -> &P8SemanticOffRunDigest {
        &self.off_run_digest
    }

    pub fn authority_ref(&self) -> &RecallOperationAuthorityRef {
        &self.authority_ref
    }

    pub fn store_snapshot_receipt(&self) -> &RecallStoreSnapshotReceiptRef {
        &self.store_snapshot_receipt
    }

    pub const fn applicable(&self) -> bool {
        self.applicable
    }

    pub const fn executed(&self) -> bool {
        self.executed
    }

    pub fn baseline_bindings(&self) -> &[P8SemanticSafeCandidateBinding] {
        &self.baseline_bindings
    }

    pub fn off_run_bindings(&self) -> &[P8SemanticSafeCandidateBinding] {
        &self.off_run_bindings
    }

    pub fn premise_decision_digest(&self) -> &P8SemanticOffRunDigest {
        &self.premise_decision_digest
    }

    pub fn lineage_decision_digest(&self) -> &P8SemanticOffRunDigest {
        &self.lineage_decision_digest
    }

    pub fn negative_proof(&self) -> Option<&P8SemanticNegativeProofReceipt> {
        self.negative_proof.as_ref()
    }

    pub const fn provider_call_count(&self) -> u64 {
        self.provider_call_count
    }

    pub const fn procedure_execution_count(&self) -> u64 {
        self.procedure_execution_count
    }

    pub const fn tool_execution_count(&self) -> u64 {
        self.tool_execution_count
    }

    pub const fn workflow_execution_count(&self) -> u64 {
        self.workflow_execution_count
    }

    fn validate_for(
        &self,
        baseline_digest: &str,
        authority_ref: &RecallOperationAuthorityRef,
        store_snapshot_receipt: &RecallStoreSnapshotReceiptRef,
    ) -> Vec<P8SemanticOffRunFailure> {
        let mut failures = self.failures.clone();
        if self.feature_state != P8SemanticOffRunFeatureState::Disabled
            || self.execution_mode != self.key.execution_mode()
            || self.executed != self.applicable
        {
            failures.push(P8SemanticOffRunFailure::ObservationInvalid);
        }
        if self.baseline_report_digest != baseline_digest || &self.authority_ref != authority_ref {
            failures.push(P8SemanticOffRunFailure::AuthorityMismatch);
        }
        if &self.store_snapshot_receipt != store_snapshot_receipt {
            failures.push(P8SemanticOffRunFailure::StoreReceiptMismatch);
        }
        if !bindings_are_canonical(&self.baseline_bindings)
            || !bindings_are_canonical(&self.off_run_bindings)
            || self.off_run_bindings.iter().any(|binding| {
                binding
                    .suppression_reasons
                    .contains(&GovernedRecallEligibilityReason::PrivacyBlocked)
                    && (binding.selected || binding.rendered)
            })
            || !self.off_run_digest.is_valid()
            || !self.premise_decision_digest.is_valid()
            || !self.lineage_decision_digest.is_valid()
            || self.provider_call_count != 0
            || self.procedure_execution_count != 0
            || self.tool_execution_count != 0
            || self.workflow_execution_count != 0
            || !self.capability_free_closure_receipt.is_valid()
        {
            failures.push(P8SemanticOffRunFailure::ObservationInvalid);
        }
        if !self.resource.is_valid() {
            failures.push(P8SemanticOffRunFailure::ResourceDeltaInvalid);
        }
        let negative_required = self.applicable
            && self.execution_mode == P8SemanticOffRunExecutionMode::CounterfactualSafeOnly;
        let negative_actual_zero = match self.key {
            P8SemanticOffRunKey::InvalidatedSuppressionNegativeOff => {
                baseline_reason_is_unselected(
                    &self.baseline_bindings,
                    GovernedRecallEligibilityReason::Invalidated,
                )
            }
            P8SemanticOffRunKey::EnvironmentPremiseGateOff => baseline_reason_is_unselected(
                &self.baseline_bindings,
                GovernedRecallEligibilityReason::PremiseBlocked,
            ),
            P8SemanticOffRunKey::ForgettingSuppressionNegativeOff => self
                .negative_proof
                .as_ref()
                .and_then(|proof| proof.forgetting_pre_operation.as_ref())
                .is_some_and(|binding| {
                    binding
                        .forgotten_candidate_refs
                        .iter()
                        .all(|candidate_ref| {
                            self.baseline_bindings
                                .iter()
                                .all(|baseline| &baseline.candidate_ref != candidate_ref)
                        })
                }),
            _ => true,
        };
        if negative_required
            != self.negative_proof.as_ref().is_some_and(|proof| {
                proof.is_valid_for(self.key, authority_ref, store_snapshot_receipt)
            })
            || (negative_required && !negative_actual_zero)
        {
            failures.push(P8SemanticOffRunFailure::NegativeProofMismatch);
        }
        failures.sort();
        failures.dedup();
        failures
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticOffRunReport {
    schema: String,
    method: String,
    baseline: GovernedRecallOperatorReportV1,
    authority_ref: RecallOperationAuthorityRef,
    store_snapshot_receipt: RecallStoreSnapshotReceiptRef,
    materialization_count: u64,
    observations: Vec<P8SemanticOffRunObservation>,
    report_digest: P8SemanticOffRunReportDigest,
}

impl P8SemanticOffRunReport {
    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn baseline(&self) -> &GovernedRecallOperatorReportV1 {
        &self.baseline
    }

    pub fn authority_ref(&self) -> &RecallOperationAuthorityRef {
        &self.authority_ref
    }

    pub fn store_snapshot_receipt(&self) -> &RecallStoreSnapshotReceiptRef {
        &self.store_snapshot_receipt
    }

    pub fn observations(&self) -> &[P8SemanticOffRunObservation] {
        &self.observations
    }

    pub fn baseline_candidate_bindings(&self) -> &[P8SemanticSafeCandidateBinding] {
        self.observations
            .first()
            .map(P8SemanticOffRunObservation::baseline_bindings)
            .unwrap_or_default()
    }

    pub const fn materialization_count(&self) -> u64 {
        self.materialization_count
    }

    pub fn report_digest(&self) -> &P8SemanticOffRunReportDigest {
        &self.report_digest
    }

    pub fn validate_contract(&self) -> Vec<P8SemanticOffRunFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_SEMANTIC_OFF_RUN_REPORT_SCHEMA {
            failures.push(P8SemanticOffRunFailure::SchemaMismatch);
        }
        if self.method != P8_SEMANTIC_OFF_RUN_METHOD {
            failures.push(P8SemanticOffRunFailure::MethodMismatch);
        }
        if !self.baseline.validate_contract().is_empty() {
            failures.push(P8SemanticOffRunFailure::BaselineInvalid);
        }
        if self
            .baseline
            .payload()
            .public_report()
            .authority()
            .authority_ref()
            != &self.authority_ref
        {
            failures.push(P8SemanticOffRunFailure::AuthorityMismatch);
        }
        if self.baseline.payload().store_snapshot_receipt() != &self.store_snapshot_receipt
            || self.baseline.payload().session_open_count() != 1
            || self.baseline.payload().receipt_count() != 1
            || self.materialization_count != 1
        {
            failures.push(P8SemanticOffRunFailure::StoreReceiptMismatch);
        }
        let keys = self
            .observations
            .iter()
            .map(P8SemanticOffRunObservation::key)
            .collect::<Vec<_>>();
        if keys != P8SemanticOffRunKey::ALL {
            failures.push(P8SemanticOffRunFailure::ExactObservationSetMismatch);
        }
        for observation in &self.observations {
            failures.extend(observation.validate_for(
                self.baseline.report_digest(),
                &self.authority_ref,
                &self.store_snapshot_receipt,
            ));
        }
        if !self.report_digest.is_valid() || self.report_digest != self.derived_report_digest() {
            failures.push(P8SemanticOffRunFailure::DigestMismatch);
        }
        failures.sort();
        failures.dedup();
        failures
    }

    pub(crate) fn from_single_production_closure(
        baseline: GovernedRecallOperatorReportV1,
        forgetting_pre_operation: Option<P8ForgettingPreOperationBinding>,
        long_term_counterfactual_inputs: Vec<P8SemanticLongTermCounterfactualInput>,
        execution_authority: P8NoExecutionCounterfactualAuthority,
    ) -> Self {
        let authority_ref = baseline
            .payload()
            .public_report()
            .authority()
            .authority_ref()
            .clone();
        let store_snapshot_receipt = baseline.payload().store_snapshot_receipt().clone();
        let baseline_bindings = safe_candidate_bindings(&baseline);
        let observation_context = P8ObservationBuildContext {
            baseline: &baseline,
            authority_ref: &authority_ref,
            store_snapshot_receipt: &store_snapshot_receipt,
            baseline_bindings: &baseline_bindings,
            forgetting_pre_operation: forgetting_pre_operation.as_ref(),
            long_term_counterfactual_inputs: &long_term_counterfactual_inputs,
            execution_authority: &execution_authority,
        };
        let observations = P8SemanticOffRunKey::ALL
            .into_iter()
            .map(|key| build_observation(key, &observation_context))
            .collect();
        let mut report = Self {
            schema: P8_SEMANTIC_OFF_RUN_REPORT_SCHEMA.into(),
            method: P8_SEMANTIC_OFF_RUN_METHOD.into(),
            baseline,
            authority_ref,
            store_snapshot_receipt,
            materialization_count: 1,
            observations,
            report_digest: P8SemanticOffRunReportDigest(prefixed_digest(
                REPORT_DIGEST_PREFIX,
                "beetle_memory_p8_semantic_off_run_report_v1",
                &[b"uninitialized"],
            )),
        };
        report.report_digest = report.derived_report_digest();
        report
    }

    fn derived_report_digest(&self) -> P8SemanticOffRunReportDigest {
        let bytes = serde_json::to_vec(&(
            &self.schema,
            &self.method,
            &self.baseline,
            &self.authority_ref,
            &self.store_snapshot_receipt,
            self.materialization_count,
            &self.observations,
        ))
        .expect("P8 off-run report canonical payload serialization is infallible");
        P8SemanticOffRunReportDigest(prefixed_digest(
            REPORT_DIGEST_PREFIX,
            "beetle_memory_p8_semantic_off_run_report_v1",
            &[bytes.as_slice()],
        ))
    }
}

struct P8ObservationBuildContext<'a> {
    baseline: &'a GovernedRecallOperatorReportV1,
    authority_ref: &'a RecallOperationAuthorityRef,
    store_snapshot_receipt: &'a RecallStoreSnapshotReceiptRef,
    baseline_bindings: &'a [P8SemanticSafeCandidateBinding],
    forgetting_pre_operation: Option<&'a P8ForgettingPreOperationBinding>,
    long_term_counterfactual_inputs: &'a [P8SemanticLongTermCounterfactualInput],
    execution_authority: &'a P8NoExecutionCounterfactualAuthority,
}

fn safe_candidate_bindings(
    baseline: &GovernedRecallOperatorReportV1,
) -> Vec<P8SemanticSafeCandidateBinding> {
    let public = baseline.payload().public_report();
    let mut bindings = public
        .validity_candidate_bindings
        .iter()
        .map(|binding| {
            let bytes = serde_json::to_vec(&("governed", &binding.candidate_ref))
                .expect("safe governed candidate serialization is infallible");
            P8SemanticSafeCandidateBinding {
                candidate_ref: P8SemanticSafeCandidateRef(prefixed_digest(
                    SAFE_CANDIDATE_PREFIX,
                    "beetle_memory_p8_semantic_safe_candidate_v1",
                    &[bytes.as_slice()],
                )),
                candidate_kind: P8SemanticSafeCandidateKind::GovernedMemory,
                primary_reason: binding.primary_reason,
                suppression_reasons: binding.suppression_reasons.clone(),
                selected: binding.selected,
                rendered: binding.rendered,
            }
        })
        .chain(public.procedural_delivery.iter().map(|binding| {
            let bytes = serde_json::to_vec(&("procedural", &binding.candidate_ref))
                .expect("safe procedural candidate serialization is infallible");
            let primary_reason = binding
                .drop_reasons
                .contains(&crate::RuntimeSkillDeliveryDropReason::RequiredPremiseBlocked)
                .then_some(GovernedRecallEligibilityReason::PremiseBlocked);
            P8SemanticSafeCandidateBinding {
                candidate_ref: P8SemanticSafeCandidateRef(prefixed_digest(
                    SAFE_CANDIDATE_PREFIX,
                    "beetle_memory_p8_semantic_safe_candidate_v1",
                    &[bytes.as_slice()],
                )),
                candidate_kind: P8SemanticSafeCandidateKind::ProceduralMemory,
                primary_reason,
                suppression_reasons: primary_reason.into_iter().collect(),
                selected: binding.selected,
                rendered: binding.rendered,
            }
        }))
        .collect::<Vec<_>>();
    bindings.sort();
    bindings.dedup();
    bindings
}

fn safe_candidate_ref_for_owner_revision(
    revision_ref: &bm_core::memory::GovernedOwnerRevisionRef,
) -> crate::Result<P8SemanticSafeCandidateRef> {
    let owner_bytes = serde_json::to_vec(revision_ref)
        .map_err(|error| crate::Error::config("p8_semantic_safe_candidate", error.to_string()))?;
    let owner_safe_ref = crate::GovernedOwnerRevisionSafeRef::derive(&[owner_bytes.as_slice()]);
    let candidate_bytes = serde_json::to_vec(&("governed", owner_safe_ref))
        .map_err(|error| crate::Error::config("p8_semantic_safe_candidate", error.to_string()))?;
    Ok(P8SemanticSafeCandidateRef(prefixed_digest(
        SAFE_CANDIDATE_PREFIX,
        "beetle_memory_p8_semantic_safe_candidate_v1",
        &[candidate_bytes.as_slice()],
    )))
}

fn build_observation(
    key: P8SemanticOffRunKey,
    context: &P8ObservationBuildContext<'_>,
) -> P8SemanticOffRunObservation {
    let baseline = context.baseline;
    let authority_ref = context.authority_ref;
    let store_snapshot_receipt = context.store_snapshot_receipt;
    let baseline_bindings = context.baseline_bindings;
    let forgetting_pre_operation = context.forgetting_pre_operation;
    let long_term_counterfactual_inputs = context.long_term_counterfactual_inputs;
    let execution_authority = context.execution_authority;
    let public = baseline.payload().public_report();
    let mut counterfactual_bindings =
        safe_counterfactual_bindings(key, long_term_counterfactual_inputs);
    if key == P8SemanticOffRunKey::ForgettingSuppressionNegativeOff {
        if let Some(binding) = forgetting_pre_operation {
            counterfactual_bindings.extend(binding.forgotten_candidate_refs.iter().cloned().map(
                |candidate_ref| P8SemanticSafeCandidateBinding {
                    candidate_ref,
                    candidate_kind: P8SemanticSafeCandidateKind::CounterfactualSafeOnly,
                    primary_reason: Some(GovernedRecallEligibilityReason::Forgotten),
                    suppression_reasons: Vec::new(),
                    selected: true,
                    rendered: false,
                },
            ));
        }
    }
    counterfactual_bindings.sort();
    counterfactual_bindings.dedup();
    let applicable = match key {
        P8SemanticOffRunKey::TemporalValidityGateOff => {
            has_reason(public, &[GovernedRecallEligibilityReason::Stale])
        }
        P8SemanticOffRunKey::UpdateLineageOff => !counterfactual_bindings.is_empty(),
        P8SemanticOffRunKey::ObsoleteSuppressionOff => {
            !counterfactual_bindings.is_empty()
                || has_reason(
                    public,
                    &[
                        GovernedRecallEligibilityReason::Obsolete,
                        GovernedRecallEligibilityReason::Superseded,
                    ],
                )
        }
        P8SemanticOffRunKey::InvalidatedSuppressionNegativeOff => {
            !counterfactual_bindings.is_empty()
                || has_reason(public, &[GovernedRecallEligibilityReason::Invalidated])
        }
        P8SemanticOffRunKey::ForgettingSuppressionNegativeOff => {
            forgetting_pre_operation.is_some_and(P8ForgettingPreOperationBinding::is_valid)
        }
        P8SemanticOffRunKey::ProceduralWorkflowEvidenceOff => {
            !public.procedural_delivery.is_empty()
        }
        P8SemanticOffRunKey::EnvironmentPremiseGateOff => {
            public.premise.required_failure_count > 0
                || has_reason(public, &[GovernedRecallEligibilityReason::PremiseBlocked])
        }
        P8SemanticOffRunKey::DynamicStateConsolidationOff => !counterfactual_bindings.is_empty(),
    };
    let mut off_run_bindings = baseline_bindings.to_vec();
    if applicable {
        match key {
            P8SemanticOffRunKey::ProceduralWorkflowEvidenceOff => {
                for binding in &mut off_run_bindings {
                    if binding.candidate_kind == P8SemanticSafeCandidateKind::ProceduralMemory {
                        binding.selected = false;
                        binding.rendered = false;
                    }
                }
            }
            P8SemanticOffRunKey::TemporalValidityGateOff => {
                for binding in &mut off_run_bindings {
                    if binding
                        .suppression_reasons
                        .contains(&GovernedRecallEligibilityReason::Stale)
                    {
                        disable_only_suppression_reasons(
                            binding,
                            &[GovernedRecallEligibilityReason::Stale],
                        );
                    }
                }
            }
            P8SemanticOffRunKey::ObsoleteSuppressionOff => {
                for binding in &mut off_run_bindings {
                    if binding.suppression_reasons.iter().any(|reason| {
                        matches!(
                            reason,
                            GovernedRecallEligibilityReason::Obsolete
                                | GovernedRecallEligibilityReason::Superseded
                        )
                    }) {
                        disable_only_suppression_reasons(
                            binding,
                            &[
                                GovernedRecallEligibilityReason::Obsolete,
                                GovernedRecallEligibilityReason::Superseded,
                            ],
                        );
                    }
                }
                off_run_bindings.extend(counterfactual_bindings);
            }
            P8SemanticOffRunKey::InvalidatedSuppressionNegativeOff => {
                for binding in &mut off_run_bindings {
                    if binding
                        .suppression_reasons
                        .contains(&GovernedRecallEligibilityReason::Invalidated)
                    {
                        disable_only_suppression_reasons(
                            binding,
                            &[GovernedRecallEligibilityReason::Invalidated],
                        );
                    }
                }
                off_run_bindings.extend(counterfactual_bindings.into_iter().filter(|candidate| {
                    !baseline_bindings
                        .iter()
                        .any(|baseline| baseline.candidate_ref == candidate.candidate_ref)
                }));
            }
            P8SemanticOffRunKey::EnvironmentPremiseGateOff => {
                for binding in &mut off_run_bindings {
                    if binding
                        .suppression_reasons
                        .contains(&GovernedRecallEligibilityReason::PremiseBlocked)
                    {
                        disable_only_suppression_reasons(
                            binding,
                            &[GovernedRecallEligibilityReason::PremiseBlocked],
                        );
                    }
                }
            }
            P8SemanticOffRunKey::UpdateLineageOff
            | P8SemanticOffRunKey::ForgettingSuppressionNegativeOff
            | P8SemanticOffRunKey::DynamicStateConsolidationOff => {
                off_run_bindings.extend(counterfactual_bindings);
            }
        }
    }
    off_run_bindings.sort();
    off_run_bindings.dedup();
    let applicable =
        applicable && bindings_have_delivery_delta(baseline_bindings, &off_run_bindings);
    let off_bytes =
        serde_json::to_vec(&(key, baseline.report_digest(), &off_run_bindings, applicable))
            .expect("P8 off-run canonical payload serialization is infallible");
    let premise_bytes = serde_json::to_vec(&(key, &public.premise, applicable))
        .expect("P8 premise decision serialization is infallible");
    let lineage_bytes = serde_json::to_vec(&(key, &public.lineage, applicable))
        .expect("P8 lineage decision serialization is infallible");
    let negative_proof = (applicable
        && key.execution_mode() == P8SemanticOffRunExecutionMode::CounterfactualSafeOnly)
        .then(|| {
            P8SemanticNegativeProofReceipt::build(
                key,
                authority_ref.clone(),
                store_snapshot_receipt.clone(),
                if key == P8SemanticOffRunKey::ForgettingSuppressionNegativeOff {
                    forgetting_pre_operation.cloned()
                } else {
                    None
                },
                execution_authority,
            )
        });
    P8SemanticOffRunObservation {
        key,
        feature_state: P8SemanticOffRunFeatureState::Disabled,
        execution_mode: key.execution_mode(),
        baseline_report_digest: baseline.report_digest().into(),
        off_run_digest: P8SemanticOffRunDigest(prefixed_digest(
            OFF_RUN_DIGEST_PREFIX,
            "beetle_memory_p8_semantic_off_run_v1",
            &[off_bytes.as_slice()],
        )),
        authority_ref: authority_ref.clone(),
        store_snapshot_receipt: store_snapshot_receipt.clone(),
        baseline_bindings: baseline_bindings.to_vec(),
        off_run_bindings,
        premise_decision_digest: P8SemanticOffRunDigest(prefixed_digest(
            OFF_RUN_DIGEST_PREFIX,
            "beetle_memory_p8_semantic_premise_decision_v1",
            &[premise_bytes.as_slice()],
        )),
        lineage_decision_digest: P8SemanticOffRunDigest(prefixed_digest(
            OFF_RUN_DIGEST_PREFIX,
            "beetle_memory_p8_semantic_lineage_decision_v1",
            &[lineage_bytes.as_slice()],
        )),
        resource: P8SemanticResourceMeasurement::ZERO,
        applicable,
        executed: applicable,
        negative_proof,
        provider_call_count: execution_authority.provider_call_count(),
        procedure_execution_count: execution_authority.procedure_execution_count(),
        tool_execution_count: execution_authority.tool_execution_count(),
        workflow_execution_count: execution_authority.workflow_execution_count(),
        capability_free_closure_receipt: execution_authority.closure_receipt().clone(),
        failures: Vec::new(),
    }
}

fn safe_counterfactual_bindings(
    key: P8SemanticOffRunKey,
    inputs: &[P8SemanticLongTermCounterfactualInput],
) -> Vec<P8SemanticSafeCandidateBinding> {
    let inputs = inputs.iter().collect::<Vec<_>>();
    match key {
        P8SemanticOffRunKey::UpdateLineageOff => inputs
            .into_iter()
            .filter(|input| input.predecessor.is_some() || input.successor.is_some())
            .map(|input| {
                let mut binding = input.binding.clone();
                let bytes = serde_json::to_vec(&(
                    key,
                    &binding.candidate_ref,
                    &input.predecessor,
                    &input.successor,
                ))
                .expect("P8 lineage counterfactual serialization is infallible");
                binding.candidate_ref = P8SemanticSafeCandidateRef(prefixed_digest(
                    SAFE_CANDIDATE_PREFIX,
                    "beetle_memory_p8_lineage_counterfactual_safe_candidate_v1",
                    &[bytes.as_slice()],
                ));
                binding.rendered = false;
                binding
            })
            .collect(),
        P8SemanticOffRunKey::ObsoleteSuppressionOff => inputs
            .into_iter()
            .flat_map(|input| {
                let retained = matches!(
                    input.binding.primary_reason,
                    Some(
                        GovernedRecallEligibilityReason::Obsolete
                            | GovernedRecallEligibilityReason::Superseded
                    )
                )
                .then(|| {
                    let mut binding = input.binding.clone();
                    disable_only_suppression_reasons(
                        &mut binding,
                        &[
                            GovernedRecallEligibilityReason::Obsolete,
                            GovernedRecallEligibilityReason::Superseded,
                        ],
                    );
                    binding
                });
                let predecessor = input.predecessor.clone().zip(input.predecessor_reason).map(
                    |(candidate_ref, primary_reason)| P8SemanticSafeCandidateBinding {
                        candidate_ref,
                        candidate_kind: P8SemanticSafeCandidateKind::CounterfactualSafeOnly,
                        primary_reason: Some(primary_reason),
                        suppression_reasons: Vec::new(),
                        selected: true,
                        rendered: false,
                    },
                );
                retained.into_iter().chain(predecessor)
            })
            .collect(),
        P8SemanticOffRunKey::InvalidatedSuppressionNegativeOff => inputs
            .into_iter()
            .filter(|input| {
                input.binding.primary_reason == Some(GovernedRecallEligibilityReason::Invalidated)
            })
            .map(|input| {
                let mut binding = input.binding.clone();
                let bytes = serde_json::to_vec(&(key, &binding.candidate_ref))
                    .expect("P8 invalidated counterfactual serialization is infallible");
                binding.candidate_ref = P8SemanticSafeCandidateRef(prefixed_digest(
                    SAFE_CANDIDATE_PREFIX,
                    "beetle_memory_p8_invalidated_counterfactual_safe_candidate_v1",
                    &[bytes.as_slice()],
                ));
                disable_only_suppression_reasons(
                    &mut binding,
                    &[GovernedRecallEligibilityReason::Invalidated],
                );
                binding
            })
            .collect(),
        P8SemanticOffRunKey::DynamicStateConsolidationOff => inputs
            .into_iter()
            .filter(|input| input.predecessor.is_some() || input.successor.is_some())
            .map(|input| {
                let bytes = serde_json::to_vec(&(
                    key,
                    &input.owner_group_ref,
                    &input.binding.candidate_ref,
                    &input.predecessor,
                    &input.successor,
                ))
                .expect("P8 dynamic counterfactual serialization is infallible");
                let mut binding = input.binding.clone();
                binding.candidate_ref = P8SemanticSafeCandidateRef(prefixed_digest(
                    SAFE_CANDIDATE_PREFIX,
                    "beetle_memory_p8_dynamic_counterfactual_safe_candidate_v1",
                    &[bytes.as_slice()],
                ));
                binding.rendered = false;
                binding
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn has_reason(
    public: &crate::GovernedRecallPublicReportV1,
    reasons: &[GovernedRecallEligibilityReason],
) -> bool {
    reasons
        .iter()
        .any(|reason| public.reason_counts.get(reason).copied().unwrap_or(0) > 0)
}

fn bindings_are_canonical(bindings: &[P8SemanticSafeCandidateBinding]) -> bool {
    bindings.iter().all(|binding| {
        binding.candidate_ref.is_valid()
            && (!binding.rendered || binding.selected)
            && binding
                .suppression_reasons
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            && (!binding.selected || binding.suppression_reasons.is_empty())
    }) && bindings.windows(2).all(|pair| pair[0] < pair[1])
        && bindings
            .iter()
            .map(|binding| &binding.candidate_ref)
            .collect::<BTreeSet<_>>()
            .len()
            == bindings.len()
}

fn bindings_have_delivery_delta(
    baseline: &[P8SemanticSafeCandidateBinding],
    off_run: &[P8SemanticSafeCandidateBinding],
) -> bool {
    let delivery = |bindings: &[P8SemanticSafeCandidateBinding]| {
        bindings
            .iter()
            .filter(|binding| binding.selected || binding.rendered)
            .map(|binding| {
                (
                    binding.candidate_ref.clone(),
                    binding.selected,
                    binding.rendered,
                )
            })
            .collect::<BTreeSet<_>>()
    };
    delivery(baseline) != delivery(off_run)
}

fn baseline_reason_is_unselected(
    bindings: &[P8SemanticSafeCandidateBinding],
    reason: GovernedRecallEligibilityReason,
) -> bool {
    let matching = bindings
        .iter()
        .filter(|binding| binding.suppression_reasons.contains(&reason))
        .collect::<Vec<_>>();
    !matching.is_empty()
        && matching
            .into_iter()
            .all(|binding| !binding.selected && !binding.rendered)
}

fn disable_only_suppression_reasons(
    binding: &mut P8SemanticSafeCandidateBinding,
    disabled: &[GovernedRecallEligibilityReason],
) {
    let disabled_primary = binding
        .primary_reason
        .filter(|reason| disabled.contains(reason));
    binding
        .suppression_reasons
        .retain(|reason| !disabled.contains(reason));
    binding.primary_reason = binding
        .suppression_reasons
        .first()
        .copied()
        .or(disabled_primary);
    binding.selected = binding.suppression_reasons.is_empty();
    binding.rendered = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn safe_ref(byte: char) -> P8SemanticSafeCandidateRef {
        P8SemanticSafeCandidateRef(format!(
            "{SAFE_CANDIDATE_PREFIX}{}",
            byte.to_string().repeat(64)
        ))
    }

    #[test]
    fn lineage_and_dynamic_counterfactuals_preserve_every_terminal_suppression() {
        for reason in [
            GovernedRecallEligibilityReason::Invalidated,
            GovernedRecallEligibilityReason::Deleted,
            GovernedRecallEligibilityReason::Forgotten,
        ] {
            let input = P8SemanticLongTermCounterfactualInput {
                binding: P8SemanticSafeCandidateBinding {
                    candidate_ref: safe_ref('a'),
                    candidate_kind: P8SemanticSafeCandidateKind::CounterfactualSafeOnly,
                    primary_reason: Some(reason),
                    suppression_reasons: vec![reason],
                    selected: false,
                    rendered: false,
                },
                owner_group_ref: safe_ref('b'),
                predecessor: Some(safe_ref('c')),
                predecessor_reason: Some(GovernedRecallEligibilityReason::Obsolete),
                successor: None,
            };
            for key in [
                P8SemanticOffRunKey::UpdateLineageOff,
                P8SemanticOffRunKey::DynamicStateConsolidationOff,
            ] {
                let bindings = safe_counterfactual_bindings(key, std::slice::from_ref(&input));
                assert_eq!(bindings.len(), 1);
                assert_eq!(bindings[0].primary_reason, Some(reason));
                assert_eq!(bindings[0].suppression_reasons, vec![reason]);
                assert!(!bindings[0].selected);
                assert!(!bindings[0].rendered);
            }
        }
    }
}
