//! PFI2 canonical tool evidence. Authentication and registry membership belong
//! to the SDK; this owner separates admissible content from execution metadata.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{domain_digest, is_canonical, is_canonical_text};
use crate::skills::AgentToolRegistryRef;

pub const MAX_PROCEDURAL_EVIDENCE_ITEMS: usize = 256;
pub const MAX_PROCEDURAL_METHOD_BYTES: usize = 16_384;
const MAX_EVIDENCE_IDENTIFIER_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralSourceSensitivity {
    NonPrivate,
    Private,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionOutcome {
    Succeeded,
    Failed,
    Rejected,
    NotExecuted,
    Cancelled,
    Partial,
    Unknown,
}

/// No free text, raw arguments, result/error text, or query/result commitment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionFactV1 {
    pub observation_id: String,
    pub call_id: String,
    pub outcome: ToolExecutionOutcome,
    pub source_sensitivity: ProceduralSourceSensitivity,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
}

impl ToolExecutionFactV1 {
    pub fn validate_contract(&self) -> bool {
        identifier(&self.observation_id)
            && identifier(&self.call_id)
            && match (self.started_at, self.completed_at) {
                (Some(start), Some(end)) => start > 0 && end >= start,
                (Some(start), None) => start > 0,
                (None, Some(_)) => false,
                (None, None) => true,
            }
    }

    pub fn validates_canonical_call(
        &self,
        tool_id: &str,
        observations: &[crate::memory::ToolObservationDigest],
        observed_at: u64,
    ) -> bool {
        self.validate_contract()
            && observed_at > 0
            && self.started_at.is_none_or(|time| time <= observed_at)
            && self.completed_at.is_none_or(|time| time <= observed_at)
            && observations.iter().any(|canonical| {
                canonical.observation_id == self.observation_id
                    && canonical.call_id == self.call_id
                    && canonical.tool_name == tool_id
            })
    }
}

/// Method authorship is supplied by the authenticated producer binding, not by
/// this input. References are exact observation IDs within the submitted turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolMethodEvidenceV1 {
    pub method_id: String,
    pub task_signature: String,
    pub body: String,
    pub execution_refs: Vec<String>,
    pub source_sensitivity: ProceduralSourceSensitivity,
    pub external_content: bool,
}

impl ToolMethodEvidenceV1 {
    pub fn validate_contract(&self) -> bool {
        identifier(&self.method_id)
            && identifier(&self.task_signature)
            && self.body.len() <= MAX_PROCEDURAL_METHOD_BYTES
            && is_canonical_text(&self.body)
            && self.execution_refs.len() <= MAX_PROCEDURAL_EVIDENCE_ITEMS
            && self.execution_refs.iter().all(|value| identifier(value))
            && self.execution_refs.windows(2).all(|pair| pair[0] < pair[1])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolUsageFeedbackV3 {
    pub registry_ref: AgentToolRegistryRef,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub execution_facts: Vec<ToolExecutionFactV1>,
    pub method_evidence: Vec<ToolMethodEvidenceV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralEvidenceRejection {
    NonCanonical,
    CapacityExceeded,
    ExecutionReferenceInvalid,
    PrivateMethod,
    UnknownMethodSource,
    ExternalMethodSource,
    WeakMethod,
    RawMethodPayload,
    MethodOwnerContentConflict,
}

/// Safe correlation uses an input ordinal, never the rejected method ID, body,
/// or body digest (which could themselves disclose private material).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralMethodDispositionV1 {
    pub input_index: u32,
    pub rejection: ProceduralEvidenceRejection,
}

/// Only the privacy-admitted image may become durable learning evidence.
/// This is NOT a method-quality acceptance or RuntimeSkill promotion decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmittedAgentToolEvidenceV1 {
    pub registry_ref: AgentToolRegistryRef,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub execution_facts: Vec<ToolExecutionFactV1>,
    pub method_evidence: Vec<ToolMethodEvidenceV1>,
    pub submitted_method_count: u32,
    pub rejected_methods: Vec<ProceduralMethodDispositionV1>,
}

impl AgentToolUsageFeedbackV3 {
    /// The authenticated producer supplies classifications; the canonical turn
    /// can only tighten their external-source boundary, never grant permission.
    pub fn admit_for_turn(
        &self,
        observations: &[crate::memory::ToolObservationDigest],
        external_content_used: bool,
        observed_at: u64,
    ) -> std::result::Result<AdmittedAgentToolEvidenceV1, ProceduralEvidenceRejection> {
        if crate::memory::validate_tool_observation_identities(observations).is_err()
            || !self
                .execution_facts
                .iter()
                .all(|fact| fact.validates_canonical_call(&self.tool_id, observations, observed_at))
        {
            return Err(ProceduralEvidenceRejection::ExecutionReferenceInvalid);
        }
        let mut constrained = self.clone();
        for method in &mut constrained.method_evidence {
            method.external_content |= external_content_used
                || method.execution_refs.iter().any(|reference| {
                    observations.iter().any(|canonical| {
                        &canonical.observation_id == reference && canonical.external_content
                    })
                });
        }
        constrained.admit_content()
    }

    pub fn admit_content(
        &self,
    ) -> std::result::Result<AdmittedAgentToolEvidenceV1, ProceduralEvidenceRejection> {
        use ProceduralEvidenceRejection as Rejection;
        if self.execution_facts.len() + self.method_evidence.len() > MAX_PROCEDURAL_EVIDENCE_ITEMS {
            return Err(Rejection::CapacityExceeded);
        }
        if !identifier(&self.registry_ref.registry_id)
            || !identifier(&self.registry_ref.fingerprint)
            || !self.registry_ref.scope.validate_contract()
            || !identifier(&self.tool_id)
            || !identifier(&self.schema_fingerprint)
            || (self.execution_facts.is_empty() && self.method_evidence.is_empty())
            || !self
                .execution_facts
                .iter()
                .all(ToolExecutionFactV1::validate_contract)
            || !self
                .method_evidence
                .iter()
                .all(ToolMethodEvidenceV1::validate_contract)
            || !self
                .execution_facts
                .windows(2)
                .all(|pair| pair[0].observation_id < pair[1].observation_id)
            || !self
                .method_evidence
                .windows(2)
                .all(|pair| pair[0].method_id < pair[1].method_id)
        {
            return Err(Rejection::NonCanonical);
        }
        let calls = self
            .execution_facts
            .iter()
            .map(|fact| &fact.call_id)
            .collect::<BTreeSet<_>>();
        if calls.len() != self.execution_facts.len() {
            return Err(Rejection::NonCanonical);
        }
        let mut admitted = AdmittedAgentToolEvidenceV1 {
            registry_ref: self.registry_ref.clone(),
            tool_id: self.tool_id.clone(),
            schema_fingerprint: self.schema_fingerprint.clone(),
            execution_facts: self.execution_facts.clone(),
            method_evidence: Vec::new(),
            submitted_method_count: self.method_evidence.len() as u32,
            rejected_methods: Vec::new(),
        };
        for (index, method) in self.method_evidence.iter().enumerate() {
            let mut sensitivity = method.source_sensitivity;
            for reference in &method.execution_refs {
                let fact = self
                    .execution_facts
                    .iter()
                    .find(|fact| &fact.observation_id == reference)
                    .ok_or(Rejection::ExecutionReferenceInvalid)?;
                sensitivity = sensitivity.ceiling(fact.source_sensitivity);
            }
            let rejection = match sensitivity {
                ProceduralSourceSensitivity::Private => Some(Rejection::PrivateMethod),
                ProceduralSourceSensitivity::Unknown => Some(Rejection::UnknownMethodSource),
                ProceduralSourceSensitivity::NonPrivate if method.external_content => {
                    Some(Rejection::ExternalMethodSource)
                }
                ProceduralSourceSensitivity::NonPrivate => {
                    crate::skills::validate_runtime_skill_method_shape(
                        &method.task_signature,
                        &method.body,
                    )
                    .err()
                    .map(|reason| match reason {
                        crate::skills::RuntimeSkillWriteReason::RawPayloadOrLog => {
                            Rejection::RawMethodPayload
                        }
                        _ => Rejection::WeakMethod,
                    })
                }
            };
            if let Some(rejection) = rejection {
                admitted
                    .rejected_methods
                    .push(ProceduralMethodDispositionV1 {
                        input_index: index as u32,
                        rejection,
                    });
            } else {
                admitted.method_evidence.push(method.clone());
            }
        }
        Ok(admitted)
    }
}

/// Batch admission preserves input group and method ordinals across filtering.
pub fn admit_agent_tool_feedback_for_turn(
    feedback: &[AgentToolUsageFeedbackV3],
    observations: &[crate::memory::ToolObservationDigest],
    external_content_used: bool,
    observed_at: u64,
) -> std::result::Result<Vec<AdmittedAgentToolEvidenceV1>, ProceduralEvidenceRejection> {
    if feedback.len() > MAX_PROCEDURAL_EVIDENCE_ITEMS
        || feedback
            .iter()
            .map(|input| {
                input
                    .execution_facts
                    .len()
                    .saturating_add(input.method_evidence.len())
            })
            .try_fold(0_usize, usize::checked_add)
            .is_none_or(|count| count > MAX_PROCEDURAL_EVIDENCE_ITEMS)
    {
        return Err(ProceduralEvidenceRejection::CapacityExceeded);
    }
    let mut admitted = feedback
        .iter()
        .map(|input| input.admit_for_turn(observations, external_content_used, observed_at))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let conflicts = method_owner_conflicts(&admitted)?;
    for group in &mut admitted {
        let mut kept = Vec::new();
        let mut methods = std::mem::take(&mut group.method_evidence).into_iter();
        for ordinal in 0..group.submitted_method_count {
            if group
                .rejected_methods
                .iter()
                .any(|item| item.input_index == ordinal)
            {
                continue;
            }
            let method = methods
                .next()
                .ok_or(ProceduralEvidenceRejection::NonCanonical)?;
            if conflicts.contains(&method_owner_key(group, &method)?) {
                group.rejected_methods.push(ProceduralMethodDispositionV1 {
                    input_index: ordinal,
                    rejection: ProceduralEvidenceRejection::MethodOwnerContentConflict,
                });
            } else {
                kept.push(method);
            }
        }
        group.method_evidence = kept;
        group.rejected_methods.sort_by_key(|item| item.input_index);
    }
    Ok(admitted)
}

fn method_owner_key(
    group: &AdmittedAgentToolEvidenceV1,
    method: &ToolMethodEvidenceV1,
) -> std::result::Result<Vec<u8>, ProceduralEvidenceRejection> {
    serde_json::to_vec(&(
        &group.registry_ref.registry_id,
        &group.registry_ref.scope,
        &group.tool_id,
        &group.schema_fingerprint,
        &method.task_signature,
    ))
    .map_err(|_| ProceduralEvidenceRejection::NonCanonical)
}

fn method_owner_conflicts(
    groups: &[AdmittedAgentToolEvidenceV1],
) -> std::result::Result<BTreeSet<Vec<u8>>, ProceduralEvidenceRejection> {
    let mut methods = std::collections::BTreeMap::<Vec<u8>, &str>::new();
    let mut conflicts = BTreeSet::new();
    for group in groups {
        for method in &group.method_evidence {
            let key = method_owner_key(group, method)?;
            if methods
                .insert(key.clone(), &method.body)
                .is_some_and(|previous| previous != method.body)
            {
                conflicts.insert(key);
            }
        }
    }
    Ok(conflicts)
}

pub(super) fn admitted_method_owners_are_consistent(
    groups: &[AdmittedAgentToolEvidenceV1],
) -> bool {
    method_owner_conflicts(groups).is_ok_and(|conflicts| conflicts.is_empty())
}

impl ProceduralSourceSensitivity {
    /// A caller cannot make a derived method public by relabeling the method.
    pub fn ceiling(self, source: Self) -> Self {
        match (self, source) {
            (Self::Private, _) | (_, Self::Private) => Self::Private,
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            _ => Self::NonPrivate,
        }
    }
}

impl AdmittedAgentToolEvidenceV1 {
    pub fn validates_canonical_turn(
        &self,
        observations: &[crate::memory::ToolObservationDigest],
        external_content_used: bool,
        observed_at: u64,
    ) -> bool {
        self.validate_contract()
            && crate::memory::validate_tool_observation_identities(observations).is_ok()
            && self
                .execution_facts
                .iter()
                .all(|fact| fact.validates_canonical_call(&self.tool_id, observations, observed_at))
            && self.method_evidence.iter().all(|method| {
                !external_content_used
                    && !method.execution_refs.iter().any(|reference| {
                        observations.iter().any(|canonical| {
                            &canonical.observation_id == reference && canonical.external_content
                        })
                    })
            })
    }

    pub fn validate_contract(&self) -> bool {
        let input = AgentToolUsageFeedbackV3 {
            registry_ref: self.registry_ref.clone(),
            tool_id: self.tool_id.clone(),
            schema_fingerprint: self.schema_fingerprint.clone(),
            execution_facts: self.execution_facts.clone(),
            method_evidence: self.method_evidence.clone(),
        };
        let rejected_only = self.execution_facts.is_empty()
            && self.method_evidence.is_empty()
            && !self.rejected_methods.is_empty()
            && identifier(&self.registry_ref.registry_id)
            && identifier(&self.registry_ref.fingerprint)
            && self.registry_ref.scope.validate_contract()
            && identifier(&self.tool_id)
            && identifier(&self.schema_fingerprint);
        (rejected_only
            || input
                .admit_content()
                .is_ok_and(|admitted| admitted.rejected_methods.is_empty()))
            && self.method_evidence.len() + self.rejected_methods.len()
                == self.submitted_method_count as usize
            && self.execution_facts.len() + self.submitted_method_count as usize
                <= MAX_PROCEDURAL_EVIDENCE_ITEMS
            && self.rejected_methods.len() <= MAX_PROCEDURAL_EVIDENCE_ITEMS
            && self.rejected_methods.iter().all(|item| {
                item.input_index < self.submitted_method_count
                    && matches!(
                        item.rejection,
                        ProceduralEvidenceRejection::PrivateMethod
                            | ProceduralEvidenceRejection::UnknownMethodSource
                            | ProceduralEvidenceRejection::ExternalMethodSource
                            | ProceduralEvidenceRejection::WeakMethod
                            | ProceduralEvidenceRejection::RawMethodPayload
                            | ProceduralEvidenceRejection::MethodOwnerContentConflict
                    )
            })
            && self
                .rejected_methods
                .windows(2)
                .all(|pair| pair[0].input_index < pair[1].input_index)
    }

    pub fn canonical_digest(&self) -> crate::Result<String> {
        let encoded = serde_json::to_vec(self)
            .map_err(|error| crate::Error::config("admitted_tool_evidence", error.to_string()))?;
        Ok(domain_digest("admitted_tool_evidence_v1", &[&encoded]))
    }
}

/// Counts describe execution only; they do not encode task success or method
/// confidence. Absence of a denominator stays unavailable.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionCountsV1 {
    pub succeeded: u64,
    pub failed: u64,
    pub rejected: u64,
    pub not_executed: u64,
    pub cancelled: u64,
    pub partial: u64,
    pub unknown: u64,
}

impl ToolExecutionCountsV1 {
    pub fn total_count(&self) -> Option<u64> {
        [
            self.succeeded,
            self.failed,
            self.rejected,
            self.not_executed,
            self.cancelled,
            self.partial,
            self.unknown,
        ]
        .into_iter()
        .try_fold(0_u64, u64::checked_add)
    }

    pub fn record(&mut self, outcome: ToolExecutionOutcome) -> Option<()> {
        let counter = match outcome {
            ToolExecutionOutcome::Succeeded => &mut self.succeeded,
            ToolExecutionOutcome::Failed => &mut self.failed,
            ToolExecutionOutcome::Rejected => &mut self.rejected,
            ToolExecutionOutcome::NotExecuted => &mut self.not_executed,
            ToolExecutionOutcome::Cancelled => &mut self.cancelled,
            ToolExecutionOutcome::Partial => &mut self.partial,
            ToolExecutionOutcome::Unknown => &mut self.unknown,
        };
        *counter = counter.checked_add(1)?;
        Some(())
    }

    /// Exact numerator/denominator for the limited-sample execution success rate.
    pub fn execution_success_sample(&self) -> Option<(u64, u64)> {
        let denominator = self.succeeded.checked_add(self.failed)?;
        (denominator != 0).then_some((self.succeeded, denominator))
    }
}

pub(super) fn identifier(value: &str) -> bool {
    value.len() <= MAX_EVIDENCE_IDENTIFIER_BYTES && is_canonical(value)
}
