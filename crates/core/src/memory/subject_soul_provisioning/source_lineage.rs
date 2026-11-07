//! Exact source lineage owned by the existing Soul generation manifest.
//! No reverse index or raw transcript text is retained here.
use super::*;

pub const SUBJECT_SOUL_MAX_SOURCE_LINEAGE: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "source", rename_all = "snake_case")]
pub enum SubjectSoulSourceRefV1 {
    Transcript(SubjectSoulTranscriptSourceRefV1),
    Perception(crate::memory::PerceptionEpisodeSourceRef),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulTranscriptSourceRefV1 {
    pub memory_space_id: String,
    pub subject_id: String,
    pub channel_id: String,
    pub conversation_id: String,
    pub turn_id: String,
    pub sequence: u64,
    pub canonical_turn_digest: String,
    pub transcript_content_digest: String,
}

impl SubjectSoulSourceRefV1 {
    pub fn from_transcript(
        record: &crate::memory::TranscriptTurnRecord,
    ) -> SubjectSoulContractResult<Self> {
        if !record.permits_post_turn_learning() {
            return Err(SubjectSoulContractError::repair(
                "withdrawn Transcript cannot grant Soul source authority",
            ));
        }
        let value = Self::Transcript(SubjectSoulTranscriptSourceRefV1 {
            memory_space_id: record.key.memory_space_id.clone(),
            subject_id: record.subject.clone(),
            channel_id: record.key.channel_id.clone(),
            conversation_id: record.key.conversation_id.clone(),
            turn_id: record.turn_id.clone(),
            sequence: record.sequence,
            canonical_turn_digest: record.canonical_turn_digest.clone(),
            transcript_content_digest: transcript_content_digest(record)?,
        });
        value.validate()?;
        Ok(value)
    }

    pub fn from_perception(
        source: crate::memory::PerceptionEpisodeSourceRef,
    ) -> SubjectSoulContractResult<Self> {
        let value = Self::Perception(source);
        value.validate()?;
        Ok(value)
    }

    /// The Transcript turn and every perception episode that influenced its
    /// assistant suffix form one causal closure for Soul learning.
    pub fn closure_from_transcript(
        record: &crate::memory::TranscriptTurnRecord,
    ) -> SubjectSoulContractResult<Vec<Self>> {
        record.validate_canonical_intake().map_err(|error| {
            SubjectSoulContractError::repair(format!(
                "Transcript perception source closure is invalid: {error}"
            ))
        })?;
        let mut sources = BTreeSet::new();
        sources.insert(Self::from_transcript(record)?);
        for usage in &record.perception_source_uses {
            sources.insert(Self::from_perception(usage.source.clone())?);
            if sources.len() > SUBJECT_SOUL_MAX_SOURCE_LINEAGE {
                return Err(SubjectSoulContractError {
                    key: SubjectSoulLifecycleErrorKey::CapacityExceeded,
                    reason: "Soul source lineage capacity exhausted; no lineage may be discarded"
                        .into(),
                });
            }
        }
        Ok(sources.into_iter().collect())
    }

    pub fn memory_space_id(&self) -> &str {
        match self {
            Self::Transcript(source) => &source.memory_space_id,
            Self::Perception(source) => &source.memory_space_id,
        }
    }

    pub fn subject_id(&self) -> &str {
        match self {
            Self::Transcript(source) => &source.subject_id,
            Self::Perception(source) => &source.subject_id,
        }
    }

    pub fn transcript(&self) -> Option<&SubjectSoulTranscriptSourceRefV1> {
        match self {
            Self::Transcript(source) => Some(source),
            Self::Perception(_) => None,
        }
    }

    pub fn perception(&self) -> Option<&crate::memory::PerceptionEpisodeSourceRef> {
        match self {
            Self::Transcript(_) => None,
            Self::Perception(source) => Some(source),
        }
    }

    pub fn validate(&self) -> SubjectSoulContractResult<()> {
        match self {
            Self::Transcript(source) => source.validate(),
            Self::Perception(source) => source.validate_contract().map_err(|error| {
                SubjectSoulContractError::repair(format!("invalid perception source: {error}"))
            }),
        }
    }

    pub fn validate_for_owner(&self, owner: &SubjectSoulOwnerV1) -> SubjectSoulContractResult<()> {
        for value in [self.memory_space_id(), self.subject_id()] {
            validate_component(value, "source_identity")?;
        }
        self.validate()?;
        if self.memory_space_id() != owner.memory_space_id || self.subject_id() != owner.subject_id
        {
            return Err(SubjectSoulContractError::repair(
                "source lineage differs from exact Soul owner",
            ));
        }
        Ok(())
    }

    pub fn matches_transcript(&self, record: &crate::memory::TranscriptTurnRecord) -> bool {
        Self::from_transcript(record).is_ok_and(|current| current == *self)
    }
    pub fn names_transcript(&self, record: &crate::memory::TranscriptTurnRecord) -> bool {
        let Self::Transcript(source) = self else {
            return false;
        };
        source.memory_space_id == record.key.memory_space_id
            && source.subject_id == record.subject
            && source.channel_id == record.key.channel_id
            && source.conversation_id == record.key.conversation_id
            && source.turn_id == record.turn_id
    }
}

impl SubjectSoulTranscriptSourceRefV1 {
    pub fn validate(&self) -> SubjectSoulContractResult<()> {
        for value in [
            &self.memory_space_id,
            &self.subject_id,
            &self.channel_id,
            &self.conversation_id,
            &self.turn_id,
        ] {
            validate_component(value, "source_identity")?;
        }
        if self.sequence == 0 {
            return Err(SubjectSoulContractError::repair(
                "source sequence must be positive",
            ));
        }
        validate_digest(
            self.canonical_turn_digest
                .strip_prefix("sha256:")
                .ok_or_else(|| {
                    SubjectSoulContractError::repair("source digest prefix is invalid")
                })?,
            "canonical_turn_digest",
        )?;
        validate_digest(
            self.transcript_content_digest
                .strip_prefix("sha256:")
                .ok_or_else(|| {
                    SubjectSoulContractError::repair("source digest prefix is invalid")
                })?,
            "transcript_content_digest",
        )
    }
}

fn transcript_content_digest(
    record: &crate::memory::TranscriptTurnRecord,
) -> SubjectSoulContractResult<String> {
    let mut content = record.clone();
    // Archive/restore changes presentation state, not the admitted evidence.
    content.lifecycle_state = crate::memory::TranscriptLifecycleState::Active;
    content.updated_at = 0;
    let bytes = serde_json::to_vec(&content)
        .map_err(|_| SubjectSoulContractError::repair("cannot encode canonical source"))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

pub fn merge_subject_soul_source_lineage_v1<'a>(
    owner: &SubjectSoulOwnerV1,
    inputs: impl IntoIterator<Item = &'a [SubjectSoulSourceRefV1]>,
) -> SubjectSoulContractResult<Vec<SubjectSoulSourceRefV1>> {
    let mut sources = BTreeSet::new();
    for input in inputs {
        for source in input {
            source.validate_for_owner(owner)?;
            sources.insert(source.clone());
            if sources.len() > SUBJECT_SOUL_MAX_SOURCE_LINEAGE {
                return Err(SubjectSoulContractError {
                    key: SubjectSoulLifecycleErrorKey::CapacityExceeded,
                    reason: "Soul source lineage capacity exhausted; no lineage may be discarded"
                        .into(),
                });
            }
        }
    }
    Ok(sources.into_iter().collect())
}

pub(super) fn validate_source_lineage(
    owner: &SubjectSoulOwnerV1,
    lineage: &[SubjectSoulSourceRefV1],
) -> SubjectSoulContractResult<()> {
    if merge_subject_soul_source_lineage_v1(owner, [lineage])? != lineage {
        return Err(SubjectSoulContractError::repair(
            "source lineage must be canonical and unique",
        ));
    }
    Ok(())
}

impl SubjectSoulOwnedDocumentV1 {
    /// Caller supplies the union of the current turn and all consumed private
    /// dependencies. Store additionally checks source CAS and prior lineage.
    pub fn with_source_lineage(
        mut self,
        sources: Vec<SubjectSoulSourceRefV1>,
    ) -> SubjectSoulContractResult<Self> {
        let owner = SubjectSoulOwnerV1 {
            memory_space_id: self.memory_space_id.clone(),
            subject_id: self.subject_id.clone(),
            soul_id: self.soul_id.clone(),
        };
        self.source_lineage =
            merge_subject_soul_source_lineage_v1(&owner, [&self.source_lineage[..], &sources[..]])?;
        self.refresh_digest()?;
        self.validate_contract()?;
        Ok(self)
    }
}

/// Durable denial is bound to an immutable revision, not the current head.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulDeniedRevisionV1 {
    pub generation: u64,
    pub revision: u64,
    pub material_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulPendingSourceReviewV1 {
    pub epoch: u64,
    pub withdrawal_operation_id: String,
    pub withdrawal_intent_digest: String,
    pub basis_head_digest: String,
    pub founding_material_digest: Option<String>,
    pub recorded_at: u64,
    pub content_digest: String,
}

impl SubjectSoulPendingSourceReviewV1 {
    fn digest(
        &self,
        owner: &SubjectSoulOwnerV1,
        generation: u64,
    ) -> SubjectSoulContractResult<String> {
        let mut value = self.clone();
        value.content_digest.clear();
        canonical_digest(
            "subject_soul_pending_source_review_v1",
            &(owner, generation, value),
        )
    }
}

/// The commitment is obtained from the verified manifest, never from the model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulSourceReviewBasisV1 {
    pub epoch: u64,
    pub pending_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulSourceReviewStateV1 {
    pub epoch: u64,
    pub pending: Option<SubjectSoulPendingSourceReviewV1>,
    pub withdrawn_sources: Vec<SubjectSoulSourceRefV1>,
    pub denied_revisions: Vec<SubjectSoulDeniedRevisionV1>,
}

impl SubjectSoulSourceReviewStateV1 {
    pub fn empty() -> Self {
        Self {
            epoch: 0,
            pending: None,
            withdrawn_sources: Vec::new(),
            denied_revisions: Vec::new(),
        }
    }

    pub fn requires_review(&self) -> bool {
        self.pending.is_some()
    }

    pub fn pending_basis(&self) -> Option<SubjectSoulSourceReviewBasisV1> {
        self.pending
            .as_ref()
            .map(|pending| SubjectSoulSourceReviewBasisV1 {
                epoch: pending.epoch,
                pending_digest: pending.content_digest.clone(),
            })
    }

    pub fn validates_basis(&self, basis: &SubjectSoulSourceReviewBasisV1) -> bool {
        self.pending_basis().as_ref() == Some(basis)
    }

    pub fn validate(
        &self,
        owner: &SubjectSoulOwnerV1,
        generation: u64,
    ) -> SubjectSoulContractResult<()> {
        validate_source_lineage(owner, &self.withdrawn_sources)?;
        if (self.epoch == 0) != self.withdrawn_sources.is_empty()
            || (self.epoch == 0 && (self.pending.is_some() || !self.denied_revisions.is_empty()))
            || self.denied_revisions.len() > SUBJECT_SOUL_MAX_SOURCE_LINEAGE
            || self
                .denied_revisions
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(SubjectSoulContractError::repair(
                "source review state is not canonical",
            ));
        }
        for denied in &self.denied_revisions {
            if denied.generation != generation || denied.revision == 0 {
                return Err(SubjectSoulContractError::repair(
                    "denied revision belongs to another generation",
                ));
            }
            validate_digest(&denied.material_digest, "denied_material_digest")?;
        }
        if let Some(pending) = &self.pending {
            if pending.epoch == 0
                || pending.epoch != self.epoch
                || pending.recorded_at == 0
                || pending.digest(owner, generation)? != pending.content_digest
            {
                return Err(SubjectSoulContractError::repair(
                    "pending source review commitment is invalid",
                ));
            }
            validate_component(&pending.withdrawal_operation_id, "withdrawal_operation_id")?;
            validate_digest(
                &pending.withdrawal_intent_digest,
                "withdrawal_intent_digest",
            )?;
            validate_digest(&pending.basis_head_digest, "basis_head_digest")?;
            validate_optional_digest(
                &pending.founding_material_digest,
                "founding_material_digest",
            )?;
        }
        Ok(())
    }

    pub fn denies_revision(&self, material: &SubjectSoulRevisionMaterialV1) -> bool {
        self.denied_revisions
            .contains(&SubjectSoulDeniedRevisionV1 {
                generation: material.generation,
                revision: material.revision,
                material_digest: material.content_digest.clone(),
            })
            || material
                .source_lineage
                .iter()
                .any(|source| self.withdrawn_sources.contains(source))
    }

    /// Runtime and disclosure reads use this for both current and as-of material.
    /// Operator-safe metadata may describe the denial without reading its body.
    pub fn validate_material_read(
        &self,
        material: &SubjectSoulRevisionMaterialV1,
    ) -> SubjectSoulContractResult<()> {
        if self.denies_revision(material) {
            return Err(SubjectSoulContractError {
                key: SubjectSoulLifecycleErrorKey::RevisionSourceWithdrawn,
                reason: "the exact Soul revision depends on a withdrawn source".into(),
            });
        }
        if self.requires_review() {
            return Err(source_review_required("Soul source review is pending"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulRetainedMaterialV1 {
    pub physical_key: String,
    pub material: SubjectSoulRevisionMaterialV1,
}

/// Lineage is the union of actual, verified observations and consumed derived
/// documents. It is supplied by the SDK reader, never by an LLM proposal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulRevisionEvidenceV1 {
    pub lawful_founding: Option<Box<SubjectSoulRevisionMaterialV1>>,
    pub source_lineage: Vec<SubjectSoulSourceRefV1>,
    pub source_review: Option<SubjectSoulSourceReviewBasisV1>,
}

impl SubjectSoulRevisionEvidenceV1 {
    pub fn empty() -> Self {
        Self {
            lawful_founding: None,
            source_lineage: Vec::new(),
            source_review: None,
        }
    }

    pub fn validate(&self, owner: &SubjectSoulOwnerV1) -> SubjectSoulContractResult<()> {
        validate_source_lineage(owner, &self.source_lineage)?;
        if let Some(founding) = &self.lawful_founding {
            founding.validate_contract()?;
            if self.source_review.is_none()
                || founding.memory_space_id != owner.memory_space_id
                || founding.subject_id != owner.subject_id
                || founding.soul_id != owner.soul_id
                || founding.provenance.origin != SubjectSoulRevisionOriginV1::HumanFoundingCharter
            {
                return Err(SubjectSoulContractError::repair(
                    "lawful founding must bind the exact review owner",
                ));
            }
        }
        if let Some(review) = &self.source_review {
            if review.epoch == 0 {
                return Err(SubjectSoulContractError::repair(
                    "review epoch must be positive",
                ));
            }
            validate_digest(&review.pending_digest, "pending_review_digest")?;
        }
        Ok(())
    }
}

fn source_review_required(reason: &str) -> SubjectSoulContractError {
    SubjectSoulContractError {
        key: SubjectSoulLifecycleErrorKey::SourceReviewRequired,
        reason: reason.into(),
    }
}

pub(super) fn validate_revision_evidence(
    owner: &SubjectSoulOwnerV1,
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
    evidence: &SubjectSoulRevisionEvidenceV1,
    refresh_plan: &SelfAuthoredCoreRefreshPlanV1,
) -> SubjectSoulContractResult<()> {
    evidence.validate(owner)?;
    if matches!(refresh_plan, SelfAuthoredCoreRefreshPlanV1::Adopt { .. })
        && evidence.source_lineage.is_empty()
    {
        return Err(SubjectSoulContractError::repair(
            "autonomous Soul adoption requires actual source lineage",
        ));
    }
    match basis {
        SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded { .. } => {
            if evidence.source_review.is_some() {
                return Err(source_review_required(
                    "pristine Soul has no source review basis",
                ));
            }
        }
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } => {
            let state = &snapshot.manifest.source_review;
            if evidence
                .source_lineage
                .iter()
                .any(|source| state.withdrawn_sources.contains(source))
            {
                return Err(source_review_required(
                    "withdrawn source cannot be readmitted during review or growth",
                ));
            }
            if state.requires_review() {
                validate_review_founding(snapshot, evidence)?;
                if !evidence
                    .source_review
                    .as_ref()
                    .is_some_and(|review| state.validates_basis(review))
                {
                    return Err(source_review_required(
                        "source review requires the exact pending commitment",
                    ));
                }
                if let SelfAuthoredCoreRefreshPlanV1::Adopt {
                    next_core,
                    next_ledger,
                    ..
                } = refresh_plan
                {
                    if let Some(founding) = &evidence.lawful_founding {
                        validate_subject_soul_review_boundaries_v1(founding, next_core)?;
                    }
                    let latest = next_ledger.entries.last().ok_or_else(|| {
                        SubjectSoulContractError::repair("review has no ledger decision")
                    })?;
                    if latest.correction_kind
                        != Some(super::super::CoreRevisionCorrectionKind::Correction)
                        || latest.corrects_revision != snapshot.head.current_revision
                    {
                        return Err(source_review_required("source review adoption requires an explicit correction ledger decision"));
                    }
                }
            } else if evidence.source_review.is_some() {
                return Err(source_review_required(
                    "source review basis is stale or no longer pending",
                ));
            }
        }
    }
    Ok(())
}

fn validate_review_founding(
    snapshot: &SubjectSoulVerifiedSnapshotV1,
    evidence: &SubjectSoulRevisionEvidenceV1,
) -> SubjectSoulContractResult<()> {
    let pending = snapshot
        .manifest
        .source_review
        .pending
        .as_ref()
        .ok_or_else(|| source_review_required("review is not pending"))?;
    if pending.founding_material_digest.as_deref()
        != evidence
            .lawful_founding
            .as_ref()
            .map(|material| material.content_digest.as_str())
        || evidence.lawful_founding.as_ref().is_some_and(|material| {
            material.generation != snapshot.head.generation
                || snapshot.manifest.source_review.denies_revision(material)
        })
    {
        return Err(source_review_required(
            "review requires the exact lawful founding material",
        ));
    }
    Ok(())
}

/// Deterministic conservative boundary preservation for a source correction.
/// This is not a permanent founding overlay on ordinary governed growth.
pub fn validate_subject_soul_review_boundaries_v1(
    founding: &SubjectSoulRevisionMaterialV1,
    core: &SelfAuthoredCore,
) -> SubjectSoulContractResult<()> {
    founding.validate_contract()?;
    if founding.provenance.origin != SubjectSoulRevisionOriginV1::HumanFoundingCharter
        || founding
            .core
            .non_negotiables
            .iter()
            .any(|clause| !core.non_negotiables.contains(clause))
        || [
            (&founding.core.boundary_doctrine, &core.boundary_doctrine),
            (&founding.core.truth_doctrine, &core.truth_doctrine),
            (
                &founding.core.self_preservation_doctrine,
                &core.self_preservation_doctrine,
            ),
            (&founding.core.repair_doctrine, &core.repair_doctrine),
            (&founding.core.change_protocol, &core.change_protocol),
        ]
        .into_iter()
        .any(|(prior, next)| !prior.is_empty() && prior != next)
    {
        return Err(source_review_required(
            "source correction cannot drop lawful founding boundaries",
        ));
    }
    Ok(())
}

/// Bind a governance decision made from a clean, unseeded model view to the
/// actual held revision. No withdrawn Core or ledger prose needs to enter the
/// model envelope, and the SDK never assigns revision or correction authority.
pub fn rebase_subject_soul_source_review_plan_v1(
    owner: &SubjectSoulOwnerV1,
    snapshot: &SubjectSoulVerifiedSnapshotV1,
    evidence: &SubjectSoulRevisionEvidenceV1,
    refresh_plan: &SelfAuthoredCoreRefreshPlanV1,
    recorded_at: u64,
) -> SubjectSoulContractResult<SelfAuthoredCoreRefreshPlanV1> {
    snapshot.validate_contract()?;
    validate_subject_soul_snapshot_owner(owner, snapshot)?;
    evidence.validate(owner)?;
    if recorded_at < snapshot.head.updated_at
        || recorded_at == 0
        || !evidence
            .source_review
            .as_ref()
            .is_some_and(|basis| snapshot.manifest.source_review.validates_basis(basis))
        || evidence.source_lineage.iter().any(|source| {
            snapshot
                .manifest
                .source_review
                .withdrawn_sources
                .contains(source)
        })
    {
        return Err(source_review_required(
            "review decision does not bind the pending lawful source image",
        ));
    }
    validate_review_founding(snapshot, evidence)?;
    let empty_ledger = CoreRevisionLedger::default();
    let clean_expected = compute_self_authored_core_expected_prior_v1(None, &empty_ledger)
        .map_err(|_| SubjectSoulContractError::repair("cannot compute clean review prior"))?;
    match refresh_plan {
        SelfAuthoredCoreRefreshPlanV1::Skipped => Ok(SelfAuthoredCoreRefreshPlanV1::Skipped),
        SelfAuthoredCoreRefreshPlanV1::ReviewedRejected { expected_prior, .. } => {
            if expected_prior != &clean_expected {
                return Err(source_review_required(
                    "review evaluated a non-clean model prior",
                ));
            }
            // A rejected clean bootstrap cannot mutate or release the held Soul.
            Ok(SelfAuthoredCoreRefreshPlanV1::Skipped)
        }
        SelfAuthoredCoreRefreshPlanV1::Adopt {
            expected_prior,
            next_core,
            next_ledger,
            origin,
            proposal_ref,
            source_refs,
        } => {
            if expected_prior != &clean_expected
                || *origin != SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap
                || next_core.revision != 1
                || next_core.supersedes_revision.is_some()
                || next_core.updated_at != recorded_at
                || next_core.last_reviewed_at != recorded_at
                || !next_core.is_meaningful()
                || evidence.source_lineage.is_empty()
            {
                return Err(source_review_required(
                    "review adoption must be governed from a clean model prior",
                ));
            }
            validate_self_authored_plan_metadata(
                &clean_expected,
                expected_prior,
                &empty_ledger,
                next_ledger,
                *origin,
                proposal_ref,
                source_refs,
                Some(1),
                true,
                recorded_at,
            )?;
            let observed_ledger = snapshot
                .current_revision_ledger
                .as_ref()
                .unwrap_or(&empty_ledger);
            let actual_expected = compute_self_authored_core_expected_prior_v1(
                snapshot.current_core.as_ref(),
                observed_ledger,
            )
            .map_err(|_| {
                SubjectSoulContractError::repair("cannot compute exact held review prior")
            })?;
            let revision = snapshot
                .head
                .current_revision
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| SubjectSoulContractError::repair("review revision overflow"))?;
            let mut core = next_core.clone();
            core.revision = revision;
            core.supersedes_revision = snapshot.head.current_revision;
            if let Some(founding) = &evidence.lawful_founding {
                for clause in &founding.core.non_negotiables {
                    if !core.non_negotiables.contains(clause) {
                        core.non_negotiables.push(clause.clone());
                    }
                }
                for (prior, next) in [
                    (
                        &founding.core.boundary_doctrine,
                        &mut core.boundary_doctrine,
                    ),
                    (&founding.core.truth_doctrine, &mut core.truth_doctrine),
                    (
                        &founding.core.self_preservation_doctrine,
                        &mut core.self_preservation_doctrine,
                    ),
                    (&founding.core.repair_doctrine, &mut core.repair_doctrine),
                    (&founding.core.change_protocol, &mut core.change_protocol),
                ] {
                    if !prior.is_empty() {
                        *next = prior.clone();
                    }
                }
                validate_subject_soul_review_boundaries_v1(founding, &core)?;
            }
            let mut decision = next_ledger.entries.last().cloned().ok_or_else(|| {
                SubjectSoulContractError::repair("review has no adoption decision")
            })?;
            decision.based_on_revision = snapshot.head.current_revision.unwrap_or(0);
            decision.resulting_revision = revision;
            decision.corrects_revision = snapshot.head.current_revision;
            decision.correction_kind = Some(crate::memory::CoreRevisionCorrectionKind::Correction);
            let result = SelfAuthoredCoreRefreshPlanV1::Adopt {
                expected_prior: actual_expected,
                next_core: core,
                next_ledger: append_core_revision_record(observed_ledger.clone(), decision),
                origin: if snapshot.head.current_revision.is_some() {
                    SubjectSoulRevisionOriginV1::SelfGovernedRevision
                } else {
                    SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap
                },
                proposal_ref: proposal_ref.clone(),
                source_refs: source_refs.clone(),
            };
            validate_revision_evidence(
                owner,
                &SubjectSoulSelfAuthoredRevisionBasisV1::Verified {
                    snapshot: Box::new(snapshot.clone()),
                },
                evidence,
                &result,
            )?;
            Ok(result)
        }
    }
}

pub(super) fn validate_layer_review_admission(
    owner: &SubjectSoulOwnerV1,
    state: &SubjectSoulSourceReviewStateV1,
    intent: &SubjectSoulGenerationLayerIntentV1,
) -> SubjectSoulContractResult<()> {
    let basis = match &intent.authority {
        SubjectSoulGenerationLayerAuthorityV1::SourceReviewEvidence { basis, .. } => Some(basis),
        _ => None,
    };
    if state.requires_review() != basis.is_some()
        || basis.is_some_and(|basis| !state.validates_basis(basis))
    {
        return Err(source_review_required(
            "layer admission requires the current review commitment",
        ));
    }
    for mutation in &intent.mutations {
        match mutation {
            SubjectSoulGenerationLayerMutationV1::Upsert {
                layer, document, ..
            } => {
                validate_source_lineage(owner, &document.source_lineage)?;
                if document
                    .source_lineage
                    .iter()
                    .any(|source| state.withdrawn_sources.contains(source))
                {
                    return Err(source_review_required(
                        "withdrawn lineage cannot reenter a Soul generation",
                    ));
                }
                if basis.is_some()
                    && (*layer != SubjectSoulGenerationLayerKindV1::TurnPersonaEvidence
                        || document.source_lineage.is_empty())
                {
                    return Err(source_review_required(
                        "pending review admits only lawful observed persona evidence",
                    ));
                }
            }
            SubjectSoulGenerationLayerMutationV1::Delete { .. } if basis.is_some() => {
                return Err(source_review_required(
                    "review evidence admission cannot delete Soul layers",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Ledger entries lack per-field typed provenance. Once a generation has any
/// withdrawn source, only its structured governance metadata is model-visible.
/// The authoritative ledger remains untouched for audit and exact CAS.
pub fn source_governance_ledger_view_v1(
    ledger: &CoreRevisionLedger,
    state: &SubjectSoulSourceReviewStateV1,
) -> CoreRevisionLedger {
    if state.withdrawn_sources.is_empty() {
        return ledger.clone();
    }
    CoreRevisionLedger {
        updated_at: ledger.updated_at,
        entries: ledger
            .entries
            .iter()
            .map(|record| CoreRevisionRecord {
                based_on_revision: record.based_on_revision,
                resulting_revision: record.resulting_revision,
                outcome: record.outcome,
                corrects_revision: record.corrects_revision,
                correction_kind: record.correction_kind,
                conflict_classes: record.conflict_classes.clone(),
                observation_due_at: record.observation_due_at,
                stability_score: record.stability_score,
                reviewed_at: record.reviewed_at,
                ..CoreRevisionRecord::default()
            })
            .collect(),
    }
}

/// Rebind only the new decision to the immutable authoritative ledger after
/// governance consumed its metadata-only view. This cannot clear source review.
pub fn rebase_subject_soul_safe_history_plan_v1(
    owner: &SubjectSoulOwnerV1,
    snapshot: &SubjectSoulVerifiedSnapshotV1,
    refresh_plan: &SelfAuthoredCoreRefreshPlanV1,
    recorded_at: u64,
) -> SubjectSoulContractResult<SelfAuthoredCoreRefreshPlanV1> {
    snapshot.validate_contract()?;
    validate_subject_soul_snapshot_owner(owner, snapshot)?;
    if snapshot.manifest.source_review.requires_review() || recorded_at < snapshot.head.updated_at {
        return Err(source_review_required(
            "safe history rebinding cannot resolve a pending source review",
        ));
    }
    let ledger = snapshot.current_revision_ledger.clone().unwrap_or_default();
    let safe = source_governance_ledger_view_v1(&ledger, &snapshot.manifest.source_review);
    let observed =
        compute_self_authored_core_expected_prior_v1(snapshot.current_core.as_ref(), &safe)
            .map_err(|_| {
                SubjectSoulContractError::repair("cannot compute safe governance history prior")
            })?;
    let actual =
        compute_self_authored_core_expected_prior_v1(snapshot.current_core.as_ref(), &ledger)
            .map_err(|_| {
                SubjectSoulContractError::repair(
                    "cannot compute authoritative governance history prior",
                )
            })?;
    let mut result = refresh_plan.clone();
    match &mut result {
        SelfAuthoredCoreRefreshPlanV1::Skipped => {}
        SelfAuthoredCoreRefreshPlanV1::ReviewedRejected {
            expected_prior,
            next_ledger,
            origin,
            proposal_ref,
            source_refs,
        } => {
            validate_self_authored_plan_metadata(
                &observed,
                expected_prior,
                &safe,
                next_ledger,
                *origin,
                proposal_ref,
                source_refs,
                snapshot.head.current_revision,
                false,
                recorded_at,
            )?;
            let decision = next_ledger.entries.last().cloned().ok_or_else(|| {
                SubjectSoulContractError::repair("safe review decision is absent")
            })?;
            *expected_prior = actual;
            *next_ledger = append_core_revision_record(ledger, decision);
        }
        SelfAuthoredCoreRefreshPlanV1::Adopt {
            expected_prior,
            next_core,
            next_ledger,
            origin,
            proposal_ref,
            source_refs,
        } => {
            validate_self_authored_plan_metadata(
                &observed,
                expected_prior,
                &safe,
                next_ledger,
                *origin,
                proposal_ref,
                source_refs,
                Some(next_core.revision),
                true,
                recorded_at,
            )?;
            let decision = next_ledger.entries.last().cloned().ok_or_else(|| {
                SubjectSoulContractError::repair("safe adoption decision is absent")
            })?;
            *expected_prior = actual;
            *next_ledger = append_core_revision_record(ledger, decision);
        }
    }
    Ok(result)
}

pub(super) fn ledger_lineage(
    owner: &SubjectSoulOwnerV1,
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
    evidence: &SubjectSoulRevisionEvidenceV1,
) -> SubjectSoulContractResult<Vec<SubjectSoulSourceRefV1>> {
    let prior = match basis {
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } => snapshot
            .current_revision_ledger_document
            .as_ref()
            .map(|document| &document.source_lineage[..])
            .unwrap_or(&[]),
        _ => &[],
    };
    merge_subject_soul_source_lineage_v1(owner, [prior, &evidence.source_lineage])
}

pub(super) fn revision_lineage(
    owner: &SubjectSoulOwnerV1,
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
    evidence: &SubjectSoulRevisionEvidenceV1,
) -> SubjectSoulContractResult<Vec<SubjectSoulSourceRefV1>> {
    let prior = match basis {
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot }
            if evidence.source_review.is_none() =>
        {
            snapshot
                .current_material
                .as_ref()
                .map(|material| &material.source_lineage[..])
                .unwrap_or(&[])
        }
        _ => &[],
    };
    merge_subject_soul_source_lineage_v1(owner, [prior, &evidence.source_lineage])
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulSourceWithdrawalIntentV1 {
    pub operation_id: String,
    pub actor_subject_id: String,
    pub withdrawn_sources: Vec<SubjectSoulSourceRefV1>,
}

/// Store must derive this intent from an authorized source before/post-image.
/// It is erasure authority, never an autonomous or SystemGovernor impersonation.
pub fn plan_subject_soul_source_withdrawal_v1(
    owner: &SubjectSoulOwnerV1,
    snapshot: &SubjectSoulVerifiedSnapshotV1,
    retained_materials: &[SubjectSoulRetainedMaterialV1],
    intent: &SubjectSoulSourceWithdrawalIntentV1,
    recorded_at: u64,
) -> SubjectSoulContractResult<SubjectSoulGenerationLayerDeltaPlanV1> {
    snapshot.validate_contract()?;
    validate_subject_soul_snapshot_owner(owner, snapshot)?;
    validate_component(&intent.operation_id, "operation_id")?;
    validate_component(&intent.actor_subject_id, "actor_subject_id")?;
    validate_source_lineage(owner, &intent.withdrawn_sources)?;
    if intent.withdrawn_sources.is_empty() || recorded_at < snapshot.head.updated_at {
        return Err(SubjectSoulContractError::repair(
            "source withdrawal requires exact sources and monotone time",
        ));
    }
    let expected_state = SubjectSoulExpectedStateV1::Exact {
        generation: snapshot.head.generation,
        revision: snapshot.head.current_revision,
        lifecycle_state: snapshot.head.state,
        head_digest: snapshot.head.head_digest.clone(),
        manifest_digest: snapshot.manifest.closure_digest.clone(),
    };
    let intent_digest = canonical_digest(
        "subject_soul_source_withdrawal_v1",
        &(owner, &expected_state, intent),
    )?;
    let delete_addresses = snapshot
        .manifest
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.owner_role,
                SubjectSoulManifestOwnerRoleV1::Private
                    | SubjectSoulManifestOwnerRoleV1::GenerationDerived
            ) && entry
                .source_lineage
                .iter()
                .any(|source| intent.withdrawn_sources.contains(source))
        })
        .map(|entry| SubjectSoulManifestAddressV1 {
            namespace: entry.namespace.clone(),
            physical_key: entry.physical_key.clone(),
        })
        .collect::<Vec<_>>();
    let keys = retained_materials
        .iter()
        .map(|retained| retained.physical_key.clone())
        .collect::<Vec<_>>();
    if keys != snapshot.head.retained_revision_refs {
        return Err(SubjectSoulContractError::repair(
            "withdrawal requires the exact retained material closure",
        ));
    }
    let mut affected = Vec::new();
    let mut founding_digests = BTreeSet::new();
    let mut revisions = BTreeSet::new();
    for material in snapshot
        .current_material
        .iter()
        .chain(retained_materials.iter().map(|retained| &retained.material))
    {
        material.validate_contract()?;
        if !revisions.insert(material.revision)
            || snapshot
                .head
                .current_revision
                .is_none_or(|revision| material.revision > revision)
        {
            return Err(SubjectSoulContractError::repair(
                "retained Soul material revisions are not an exact history",
            ));
        }
        if material.provenance.origin == SubjectSoulRevisionOriginV1::HumanFoundingCharter {
            founding_digests.insert(material.content_digest.clone());
        }
        if material.memory_space_id != owner.memory_space_id
            || material.subject_id != owner.subject_id
            || material.soul_id != owner.soul_id
            || material.generation != snapshot.head.generation
        {
            return Err(SubjectSoulContractError::repair(
                "withdrawal retained material owner mismatch",
            ));
        }
        if material
            .source_lineage
            .iter()
            .any(|source| intent.withdrawn_sources.contains(source))
        {
            affected.push(SubjectSoulDeniedRevisionV1 {
                generation: material.generation,
                revision: material.revision,
                material_digest: material.content_digest.clone(),
            });
        }
    }
    let new_denial = affected.iter().any(|revision| {
        !snapshot
            .manifest
            .source_review
            .denied_revisions
            .contains(revision)
    });
    let newly_withdrawn = !affected.is_empty()
        && intent.withdrawn_sources.iter().any(|source| {
            !snapshot
                .manifest
                .source_review
                .withdrawn_sources
                .contains(source)
        });
    if delete_addresses.is_empty() && !new_denial && !newly_withdrawn {
        return Ok(SubjectSoulGenerationLayerDeltaPlanV1::NoEffect {
            expected_state,
            intent_digest,
        });
    }
    if snapshot.head.state == SubjectSoulLifecycleStateV1::Deleted {
        return Err(SubjectSoulContractError::repair(
            "deleted Soul retained source-owned artifacts",
        ));
    }
    let mut post_manifest = snapshot.manifest.clone();
    post_manifest.entries.retain(|entry| {
        !delete_addresses.iter().any(|address| {
            address.namespace == entry.namespace && address.physical_key == entry.physical_key
        })
    });
    let review = &mut post_manifest.source_review;
    review.epoch = review
        .epoch
        .checked_add(1)
        .ok_or_else(|| SubjectSoulContractError::repair("source review epoch overflow"))?;
    review.withdrawn_sources = merge_subject_soul_source_lineage_v1(
        owner,
        [&review.withdrawn_sources[..], &intent.withdrawn_sources[..]],
    )?;
    review.denied_revisions.extend(affected);
    review.denied_revisions.sort();
    review.denied_revisions.dedup();
    if review.denied_revisions.len() > SUBJECT_SOUL_MAX_SOURCE_LINEAGE {
        return Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::CapacityExceeded,
            reason: "retained Soul denial capacity exhausted".into(),
        });
    }
    if founding_digests.len() > 1 {
        return Err(SubjectSoulContractError::repair(
            "one Soul generation cannot have multiple founding charters",
        ));
    }
    let mut pending = SubjectSoulPendingSourceReviewV1 {
        epoch: review.epoch,
        withdrawal_operation_id: intent.operation_id.clone(),
        withdrawal_intent_digest: intent_digest.clone(),
        basis_head_digest: snapshot.head.head_digest.clone(),
        founding_material_digest: founding_digests.into_iter().next(),
        recorded_at,
        content_digest: String::new(),
    };
    pending.content_digest = pending.digest(owner, snapshot.head.generation)?;
    review.pending = Some(pending);
    post_manifest.manifest_revision =
        next_subject_soul_manifest_revision(post_manifest.manifest_revision)?;
    post_manifest.refresh_digest()?;
    let mut post_head = snapshot.head.clone();
    post_head.scope_manifest_digest = post_manifest.closure_digest.clone();
    post_head.updated_at = recorded_at;
    post_head.refresh_digest()?;
    validate_subject_soul_post_image(
        &post_head,
        &post_manifest,
        snapshot.current_material.as_ref(),
        snapshot.current_core.as_ref(),
        snapshot
            .current_revision_ledger_document
            .as_ref()
            .map(|document| document.content_digest.as_str()),
    )?;
    Ok(SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        expected_state,
        intent_digest,
        post_head: Box::new(post_head),
        post_manifest: Box::new(post_manifest),
        upsert_documents: Vec::new(),
        delete_addresses,
    })
}

#[cfg(test)]
#[path = "source_lineage_tests.rs"]
mod tests;
