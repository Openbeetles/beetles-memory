//! Immutable execution contributions retained by the existing application ledger.
//! This pure reducer accepts the currently authorized source set selected by the
//! SDK/Store owner; it does not confer authority or make a projection decision.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::evidence::identifier;
use super::{
    domain_digest, is_digest, ProceduralFeedbackIdentityV1, ProceduralProducerRevisionRefV1,
    ProceduralProducerScopeV1, ProceduralProducerToolV1, ToolExecutionCountsV1,
    ToolExecutionFactV1,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralContributionRefV1 {
    pub contribution_id: String,
    pub content_digest: String,
    pub source_job_id: String,
}

impl ProceduralContributionRefV1 {
    pub fn validate_contract(&self) -> bool {
        is_digest(&self.contribution_id)
            && is_digest(&self.content_digest)
            && identifier(&self.source_job_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionContributionV1 {
    pub source: ProceduralFeedbackIdentityV1,
    pub learning_evidence_digest: String,
    pub producer: ProceduralProducerRevisionRefV1,
    pub tool: ProceduralProducerToolV1,
    pub fact: ToolExecutionFactV1,
    pub contribution_id: String,
    pub content_digest: String,
}

/// Immutable usage provenance. The ordinal identifies the original admitted
/// item, not a new observation manufactured during a reconciliation run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillUsageContributionV1 {
    pub source: ProceduralFeedbackIdentityV1,
    pub learning_evidence_digest: String,
    pub producer: ProceduralProducerRevisionRefV1,
    pub locator: crate::skills::RuntimeSkillOwnerLocator,
    pub selected_content_digest: String,
    pub outcome: super::ProceduralExecutionOutcomeV1,
    pub feedback_ordinal: u32,
    pub observed_at: u64,
    pub contribution_id: String,
    pub content_digest: String,
}

impl RuntimeSkillUsageContributionV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        source: ProceduralFeedbackIdentityV1,
        learning_evidence_digest: String,
        producer: ProceduralProducerRevisionRefV1,
        feedback: &super::RuntimeSkillUsageFeedbackV1,
        feedback_ordinal: u32,
        observed_at: u64,
    ) -> std::result::Result<Self, ProceduralContributionErrorV1> {
        let mut value = Self {
            source,
            learning_evidence_digest,
            producer,
            locator: feedback.locator.clone(),
            selected_content_digest: feedback.selected_content_digest.clone(),
            outcome: feedback.outcome,
            feedback_ordinal,
            observed_at,
            contribution_id: String::new(),
            content_digest: String::new(),
        };
        value.contribution_id = value.identity()?;
        value.content_digest = value.digest()?;
        if !value.validate_contract() {
            return Err(ProceduralContributionErrorV1::NonCanonical);
        }
        Ok(value)
    }

    fn identity(&self) -> std::result::Result<String, ProceduralContributionErrorV1> {
        digest(
            "procedural_runtime_usage_identity_v1",
            &(
                &self.source,
                &self.producer.binding_key,
                self.feedback_ordinal,
            ),
        )
    }

    fn digest(&self) -> std::result::Result<String, ProceduralContributionErrorV1> {
        digest(
            "procedural_runtime_usage_digest_v1",
            &(
                &self.source,
                &self.learning_evidence_digest,
                &self.producer,
                &self.locator,
                &self.selected_content_digest,
                self.outcome,
                self.feedback_ordinal,
                self.observed_at,
            ),
        )
    }

    pub fn validate_contract(&self) -> bool {
        self.source.validate().is_ok()
            && is_digest(&self.learning_evidence_digest)
            && self.producer.validate_contract()
            && self.locator.validate_for(&self.source.memory_space_id)
            && is_digest(&self.selected_content_digest)
            && matches!(self.locator.owning_scope(), crate::skills::RuntimeSkillOwningScope::Subject { mounted_subject_id }
                if mounted_subject_id == &self.source.mounted_subject_id)
            && self.observed_at > 0
            && (self.feedback_ordinal as usize) < super::MAX_PROCEDURAL_EVIDENCE_ITEMS
            && self
                .identity()
                .is_ok_and(|value| value == self.contribution_id)
            && self
                .digest()
                .is_ok_and(|value| value == self.content_digest)
    }

    pub fn reference(
        &self,
    ) -> std::result::Result<ProceduralContributionRefV1, ProceduralContributionErrorV1> {
        if !self.validate_contract() {
            return Err(ProceduralContributionErrorV1::NonCanonical);
        }
        Ok(ProceduralContributionRefV1 {
            contribution_id: self.contribution_id.clone(),
            content_digest: self.content_digest.clone(),
            source_job_id: self
                .source
                .job_id(&self.learning_evidence_digest)
                .map_err(|_| ProceduralContributionErrorV1::NonCanonical)?,
        })
    }
}

/// Rebuild from the complete currently-authorized source set; never subtract
/// guessed counters or replace an observation time with the worker's clock.
pub fn reduce_runtime_skill_usage_contributions(
    memory_space_id: &str,
    subject_id: &str,
    owner_id: &str,
    contributions: &[RuntimeSkillUsageContributionV1],
    max_contributions: usize,
) -> std::result::Result<
    crate::skills::RuntimeSkillUsageOutcomeSummary,
    ProceduralContributionErrorV1,
> {
    use super::ProceduralExecutionOutcomeV1 as Outcome;
    use crate::skills::{RuntimeSkillUsageOutcome, RuntimeSkillUsageOutcomeSummary};
    use ProceduralContributionErrorV1 as E;
    if !identifier(memory_space_id) || !identifier(subject_id) || !identifier(owner_id) {
        return Err(E::NonCanonical);
    }
    if contributions.len() > max_contributions {
        return Err(E::CapacityBlocked);
    }
    let mut unique = BTreeMap::new();
    for contribution in contributions {
        if !contribution.validate_contract() {
            return Err(E::NonCanonical);
        }
        if contribution.source.memory_space_id != memory_space_id
            || contribution.source.mounted_subject_id != subject_id
            || contribution.locator.owner_id() != owner_id
        {
            return Err(E::ScopeMismatch);
        }
        if let Some(previous) = unique.insert(&contribution.contribution_id, contribution) {
            if previous != contribution {
                return Err(E::IdentityConflict);
            }
        }
    }
    let mut ordered = unique
        .into_values()
        .filter(|value| value.outcome != Outcome::NotExecuted)
        .collect::<Vec<_>>();
    ordered.sort_by(|a, b| {
        (
            a.observed_at,
            &a.source.channel_id,
            &a.source.chat_id,
            &a.source.conversation_id,
            &a.source.turn_id,
            a.feedback_ordinal,
        )
            .cmp(&(
                b.observed_at,
                &b.source.channel_id,
                &b.source.chat_id,
                &b.source.conversation_id,
                &b.source.turn_id,
                b.feedback_ordinal,
            ))
    });
    let mut summary = RuntimeSkillUsageOutcomeSummary::default();
    for contribution in ordered {
        summary.observation_count = summary
            .observation_count
            .checked_add(1)
            .ok_or(E::CounterOverflow)?;
        summary.last_outcome = Some(match contribution.outcome {
            Outcome::Succeeded => {
                summary.succeeded_count = summary
                    .succeeded_count
                    .checked_add(1)
                    .ok_or(E::CounterOverflow)?;
                RuntimeSkillUsageOutcome::Succeeded
            }
            Outcome::Mismatch => {
                summary.mismatch_count = summary
                    .mismatch_count
                    .checked_add(1)
                    .ok_or(E::CounterOverflow)?;
                RuntimeSkillUsageOutcome::Mismatch
            }
            Outcome::Failed | Outcome::Partial | Outcome::Cancelled => {
                RuntimeSkillUsageOutcome::Neutral
            }
            Outcome::NotExecuted => unreachable!("filtered above"),
        });
        summary.last_outcome_at = Some(contribution.observed_at);
        summary.contributions.push(contribution.reference()?);
    }
    summary
        .contributions
        .sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
    summary.retained_contributions = summary.contributions.clone();
    Ok(summary)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralContributionErrorV1 {
    NonCanonical,
    ScopeMismatch,
    IdentityConflict,
    CapacityBlocked,
    CounterOverflow,
}

/// One reduction owner for initial application and reconciliation. The caller
/// supplies the currently authorized reference set; this function grants no
/// source permission and does not merge chat identities to manufacture scope.
pub fn reduce_agent_tool_experience_contributions(
    scope: &super::ProceduralSubjectScopeV1,
    tool: &ProceduralProducerToolV1,
    body: &crate::skills::AgentToolExperienceBodyV1,
    contributions: &[ToolExecutionContributionV1],
    previous_status: Option<crate::skills::AgentToolExperienceStatus>,
    max_references: usize,
) -> std::result::Result<
    (
        crate::skills::AgentToolExperienceBodyV1,
        crate::skills::AgentToolExperienceStatus,
    ),
    ProceduralContributionErrorV1,
> {
    use crate::skills::{AgentToolExperienceBodyV1 as Body, AgentToolExperienceStatus as Status};
    use ProceduralContributionErrorV1 as E;
    if !scope.validate_contract() || !tool.validate_contract() {
        return Err(E::NonCanonical);
    }
    if body.contribution_count() > max_references || contributions.len() > max_references {
        return Err(E::CapacityBlocked);
    }
    let mut exact = BTreeMap::new();
    for contribution in contributions {
        if !contribution.validate_contract() {
            return Err(E::NonCanonical);
        }
        if contribution.source.memory_space_id != scope.memory_space_id
            || contribution.source.mounted_subject_id != scope.mounted_subject_id
            || contribution.tool != *tool
        {
            return Err(E::ScopeMismatch);
        }
        if let Some(previous) = exact.insert(&contribution.contribution_id, contribution) {
            if previous != contribution {
                return Err(E::IdentityConflict);
            }
        }
    }
    let get = |reference: &ProceduralContributionRefV1| -> std::result::Result<&ToolExecutionContributionV1, E> {
        let contribution = exact.get(&reference.contribution_id).ok_or(E::NonCanonical)?;
        if contribution.reference()? != *reference { return Err(E::IdentityConflict); }
        Ok(contribution)
    };
    let mut reduced = body.clone();
    let status = match &mut reduced {
        Body::Execution {
            counts,
            contributions,
        } => {
            *counts = ToolExecutionCountsV1::default();
            for reference in contributions {
                counts
                    .record(get(reference)?.fact.outcome)
                    .ok_or(E::CounterOverflow)?;
            }
            Status::Active
        }
        Body::Method { sources, .. } => {
            let mut calls = std::collections::BTreeSet::new();
            for source in sources {
                for reference in &source.execution_refs {
                    let contribution = get(reference)?;
                    if contribution.fact.outcome == super::ToolExecutionOutcome::Succeeded
                        && contribution.fact.source_sensitivity
                            == super::ProceduralSourceSensitivity::NonPrivate
                    {
                        calls.insert((
                            &contribution.source.channel_id,
                            &contribution.source.chat_id,
                            &contribution.source.conversation_id,
                            &contribution.source.turn_id,
                            &contribution.fact.call_id,
                        ));
                    }
                }
            }
            if calls.len() >= 2 {
                Status::Active
            } else {
                Status::Candidate
            }
        }
    };
    if !reduced.validate_contract() {
        return Err(E::NonCanonical);
    }
    Ok((
        reduced,
        match previous_status {
            Some(previous @ (Status::Stale | Status::Rejected)) => previous,
            _ => status,
        },
    ))
}

impl ToolExecutionContributionV1 {
    pub fn build(
        source: ProceduralFeedbackIdentityV1,
        learning_evidence_digest: String,
        producer: ProceduralProducerRevisionRefV1,
        tool: ProceduralProducerToolV1,
        fact: ToolExecutionFactV1,
    ) -> std::result::Result<Self, ProceduralContributionErrorV1> {
        let mut value = Self {
            source,
            learning_evidence_digest,
            producer,
            tool,
            fact,
            contribution_id: String::new(),
            content_digest: String::new(),
        };
        value.contribution_id = value.canonical_identity()?;
        value.content_digest = value.canonical_digest()?;
        if !value.validate_contract() {
            return Err(ProceduralContributionErrorV1::NonCanonical);
        }
        Ok(value)
    }

    pub fn source_scope(&self) -> ProceduralProducerScopeV1 {
        ProceduralProducerScopeV1 {
            memory_space_id: self.source.memory_space_id.clone(),
            mounted_subject_id: self.source.mounted_subject_id.clone(),
            channel_id: self.source.channel_id.clone(),
            chat_id: self.source.chat_id.clone(),
        }
    }

    pub fn canonical_identity(&self) -> std::result::Result<String, ProceduralContributionErrorV1> {
        // A transport retry and a producer credential/revision rotation do not
        // change the identity of an already declared call.
        digest(
            "procedural_execution_contribution_identity_v1",
            &(
                &self.source,
                &self.producer.binding_key,
                &self.tool,
                &self.fact.call_id,
                &self.fact.observation_id,
            ),
        )
    }

    pub fn canonical_digest(&self) -> std::result::Result<String, ProceduralContributionErrorV1> {
        digest(
            "procedural_execution_contribution_digest_v1",
            &(
                &self.source,
                &self.learning_evidence_digest,
                &self.producer,
                &self.tool,
                &self.fact,
            ),
        )
    }

    pub fn reference(
        &self,
    ) -> std::result::Result<ProceduralContributionRefV1, ProceduralContributionErrorV1> {
        if !self.validate_contract() {
            return Err(ProceduralContributionErrorV1::NonCanonical);
        }
        Ok(ProceduralContributionRefV1 {
            contribution_id: self.contribution_id.clone(),
            content_digest: self.content_digest.clone(),
            source_job_id: self
                .source
                .job_id(&self.learning_evidence_digest)
                .map_err(|_| ProceduralContributionErrorV1::NonCanonical)?,
        })
    }

    pub fn validate_contract(&self) -> bool {
        self.source.validate().is_ok()
            && self.source_scope().validate_contract()
            && identifier(&self.source.conversation_id)
            && identifier(&self.source.turn_id)
            && is_digest(&self.learning_evidence_digest)
            && self.producer.validate_contract()
            && self.tool.validate_contract()
            && self.fact.validate_contract()
            && self
                .canonical_identity()
                .is_ok_and(|value| value == self.contribution_id)
            && self
                .canonical_digest()
                .is_ok_and(|value| value == self.content_digest)
    }
}

/// Does not store a percentage: the denominator remains exact and unavailable
/// when there was no completed execution. Source-set authorization/fencing must
/// happen before this reducer and remain in the final transaction's CAS set.
pub fn reduce_tool_execution_contributions(
    scope: &ProceduralProducerScopeV1,
    tool: &ProceduralProducerToolV1,
    contributions: &[ToolExecutionContributionV1],
    max_contributions: usize,
) -> std::result::Result<ToolExecutionCountsV1, ProceduralContributionErrorV1> {
    use ProceduralContributionErrorV1 as E;
    if !scope.validate_contract() || !tool.validate_contract() {
        return Err(E::NonCanonical);
    }
    if contributions.len() > max_contributions {
        return Err(E::CapacityBlocked);
    }
    let mut identities = BTreeMap::new();
    let mut counts = ToolExecutionCountsV1::default();
    for contribution in contributions {
        if !contribution.validate_contract() {
            return Err(E::NonCanonical);
        }
        if contribution.source_scope() != *scope || contribution.tool != *tool {
            return Err(E::ScopeMismatch);
        }
        if let Some(previous) =
            identities.insert(&contribution.contribution_id, &contribution.content_digest)
        {
            if previous != &contribution.content_digest {
                return Err(E::IdentityConflict);
            }
            continue;
        }
        counts
            .record(contribution.fact.outcome)
            .ok_or(E::CounterOverflow)?;
    }
    Ok(counts)
}

fn digest<T: Serialize>(
    domain: &str,
    value: &T,
) -> std::result::Result<String, ProceduralContributionErrorV1> {
    let encoded =
        serde_json::to_vec(value).map_err(|_| ProceduralContributionErrorV1::NonCanonical)?;
    Ok(domain_digest(domain, &[&encoded]))
}
