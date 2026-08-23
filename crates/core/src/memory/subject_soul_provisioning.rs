//! Typed Core contracts for subject Soul provisioning and lifecycle closure.
//!
//! This module owns canonical Soul intent/material shapes. Registry admission and
//! persistence remain SDK/Store responsibilities; callers cannot smuggle raw
//! profile blobs, revision authority, or a second relationship policy through
//! these types.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::{
    append_core_revision_record, compute_self_authored_core_expected_prior_v1, CoreRevisionLedger,
    CoreRevisionOutcome, CoreRevisionRecord, SelfAuthoredCore, SelfAuthoredCoreExpectedPriorV1,
    SelfAuthoredCoreRefreshPlanV1,
};
use crate::util::truncate_content_to_max;

pub const SUBJECT_SOUL_SCHEMA_VERSION: u32 = 1;
pub const SUBJECT_SOUL_MAX_CLAUSE_CHARS: usize = 220;
pub const SUBJECT_SOUL_MAX_CLAUSES_PER_FIELD: usize = 16;
pub const SUBJECT_SOUL_MAX_TOTAL_CHARS: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulLifecycleErrorKey {
    TargetNotMounted,
    SubjectNotFound,
    TargetMustBeActiveAgentPersona,
    AuthorityDenied,
    InvalidFoundingCharter,
    GenerationConflict,
    AlreadyInitialized,
    Archived,
    Deleted,
    OperationConflict,
    CapacityExceeded,
    RepairRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{key:?}: {reason}")]
pub struct SubjectSoulContractError {
    pub key: SubjectSoulLifecycleErrorKey,
    pub reason: String,
}

impl SubjectSoulContractError {
    fn invalid(reason: impl Into<String>) -> Self {
        Self {
            key: SubjectSoulLifecycleErrorKey::InvalidFoundingCharter,
            reason: reason.into(),
        }
    }

    fn repair(reason: impl Into<String>) -> Self {
        Self {
            key: SubjectSoulLifecycleErrorKey::RepairRequired,
            reason: reason.into(),
        }
    }
}

pub type SubjectSoulContractResult<T> = Result<T, SubjectSoulContractError>;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulFoundingCharterSeedV1 {
    #[serde(default)]
    pub identity_anchor: Option<String>,
    #[serde(default)]
    pub character_tendencies: Vec<String>,
    #[serde(default)]
    pub priority_constitution: Vec<String>,
    #[serde(default)]
    pub non_negotiables: Vec<String>,
    #[serde(default)]
    pub default_response_mode: Option<String>,
    #[serde(default)]
    pub default_initiative_posture: Option<String>,
    #[serde(default)]
    pub default_relationship_posture: Option<String>,
    #[serde(default)]
    pub boundary_doctrine: Option<String>,
    #[serde(default)]
    pub truth_seeking_commitment: Option<String>,
    #[serde(default)]
    pub self_preservation_doctrine: Option<String>,
    #[serde(default)]
    pub repair_doctrine: Option<String>,
    #[serde(default)]
    pub change_principle: Option<String>,
}

impl SubjectSoulFoundingCharterSeedV1 {
    pub fn canonicalize(mut self) -> SubjectSoulContractResult<Self> {
        canonicalize_optional(&mut self.identity_anchor, "identity_anchor")?;
        canonicalize_list(&mut self.character_tendencies, "character_tendencies")?;
        canonicalize_list(&mut self.priority_constitution, "priority_constitution")?;
        canonicalize_list(&mut self.non_negotiables, "non_negotiables")?;
        canonicalize_optional(&mut self.default_response_mode, "default_response_mode")?;
        canonicalize_optional(
            &mut self.default_initiative_posture,
            "default_initiative_posture",
        )?;
        canonicalize_optional(
            &mut self.default_relationship_posture,
            "default_relationship_posture",
        )?;
        canonicalize_optional(&mut self.boundary_doctrine, "boundary_doctrine")?;
        canonicalize_optional(
            &mut self.truth_seeking_commitment,
            "truth_seeking_commitment",
        )?;
        canonicalize_optional(
            &mut self.self_preservation_doctrine,
            "self_preservation_doctrine",
        )?;
        canonicalize_optional(&mut self.repair_doctrine, "repair_doctrine")?;
        canonicalize_optional(&mut self.change_principle, "change_principle")?;

        if self.total_chars() == 0 {
            return Err(SubjectSoulContractError::invalid(
                "founding charter must contain at least one non-empty clause",
            ));
        }
        if self.total_chars() > SUBJECT_SOUL_MAX_TOTAL_CHARS {
            return Err(SubjectSoulContractError::invalid(
                "founding charter exceeds the total character budget",
            ));
        }
        Ok(self)
    }

    pub fn validate_canonical(&self) -> SubjectSoulContractResult<()> {
        let canonical = self.clone().canonicalize()?;
        if canonical != *self {
            return Err(SubjectSoulContractError::invalid(
                "founding charter is not canonical",
            ));
        }
        Ok(())
    }

    fn total_chars(&self) -> usize {
        [
            self.identity_anchor.as_deref(),
            self.default_response_mode.as_deref(),
            self.default_initiative_posture.as_deref(),
            self.default_relationship_posture.as_deref(),
            self.boundary_doctrine.as_deref(),
            self.truth_seeking_commitment.as_deref(),
            self.self_preservation_doctrine.as_deref(),
            self.repair_doctrine.as_deref(),
            self.change_principle.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::chars)
        .map(Iterator::count)
        .sum::<usize>()
            + self
                .character_tendencies
                .iter()
                .chain(&self.priority_constitution)
                .chain(&self.non_negotiables)
                .map(|value| value.chars().count())
                .sum::<usize>()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulProvisionIntentV1 {
    Unseeded,
    Founding {
        operation_id: String,
        human_actor_subject_id: String,
        charter: Box<SubjectSoulFoundingCharterSeedV1>,
        source_asserted_at: Option<u64>,
    },
}

impl SubjectSoulProvisionIntentV1 {
    pub fn validate_canonical(&self) -> SubjectSoulContractResult<()> {
        if let Self::Founding {
            operation_id,
            human_actor_subject_id,
            charter,
            ..
        } = self
        {
            validate_component(operation_id, "operation_id")?;
            validate_component(human_actor_subject_id, "human_actor_subject_id")?;
            charter.validate_canonical()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulRevisionOriginV1 {
    HumanFoundingCharter,
    SelfAuthoredBootstrap,
    SelfGovernedRevision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulSourceAuthorityV1 {
    ActiveHumanUser,
    SoulSelfGovernance,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulRevisionProvenanceV1 {
    pub origin: SubjectSoulRevisionOriginV1,
    pub source_authority: SubjectSoulSourceAuthorityV1,
    pub source_subject_id: String,
    pub source_asserted_at: Option<u64>,
    pub recorded_at: u64,
    pub operation_ref: Option<String>,
    pub proposal_ref: Option<String>,
    pub source_refs: Vec<String>,
}

impl SubjectSoulRevisionProvenanceV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_component(&self.source_subject_id, "source_subject_id")?;
        validate_sorted_unique_components(&self.source_refs, "source_refs")?;
        if self.recorded_at == 0
            || self
                .source_asserted_at
                .is_some_and(|value| value > self.recorded_at)
        {
            return Err(SubjectSoulContractError::repair(
                "Soul revision provenance time binding is invalid",
            ));
        }
        match (self.origin, self.source_authority) {
            (
                SubjectSoulRevisionOriginV1::HumanFoundingCharter,
                SubjectSoulSourceAuthorityV1::ActiveHumanUser,
            ) => {
                validate_optional_component(&self.operation_ref, "operation_ref")?;
                if self.operation_ref.is_none() || self.proposal_ref.is_some() {
                    return Err(SubjectSoulContractError::repair(
                        "human founding provenance requires only operation_ref",
                    ));
                }
            }
            (
                SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap,
                SubjectSoulSourceAuthorityV1::SoulSelfGovernance,
            )
            | (
                SubjectSoulRevisionOriginV1::SelfGovernedRevision,
                SubjectSoulSourceAuthorityV1::SoulSelfGovernance,
            ) => {
                validate_optional_component(&self.proposal_ref, "proposal_ref")?;
                if self.proposal_ref.is_none() || self.operation_ref.is_some() {
                    return Err(SubjectSoulContractError::repair(
                        "self-governed provenance requires only proposal_ref",
                    ));
                }
            }
            _ => {
                return Err(SubjectSoulContractError::repair(
                    "Soul revision origin does not match its source authority",
                ));
            }
        }
        Ok(())
    }
}

pub fn compile_subject_soul_founding_core(
    seed: &SubjectSoulFoundingCharterSeedV1,
    recorded_at: u64,
) -> SubjectSoulContractResult<SelfAuthoredCore> {
    seed.validate_canonical()?;
    Ok(SelfAuthoredCore {
        revision: 1,
        supersedes_revision: None,
        stability_score: 0,
        last_reviewed_at: recorded_at,
        adopted_change_summary: Vec::new(),
        rejected_change_summary: Vec::new(),
        identity_anchor: seed.identity_anchor.clone().unwrap_or_default(),
        character_tendencies: seed.character_tendencies.clone(),
        non_negotiables: seed.non_negotiables.clone(),
        priority_constitution: seed.priority_constitution.clone(),
        default_response_mode: seed.default_response_mode.clone().unwrap_or_default(),
        default_task_scope: String::new(),
        default_initiative_posture: seed.default_initiative_posture.clone().unwrap_or_default(),
        default_relationship_posture: seed
            .default_relationship_posture
            .clone()
            .unwrap_or_default(),
        boundary_doctrine: seed.boundary_doctrine.clone().unwrap_or_default(),
        truth_doctrine: seed.truth_seeking_commitment.clone().unwrap_or_default(),
        self_preservation_doctrine: seed.self_preservation_doctrine.clone().unwrap_or_default(),
        repair_doctrine: seed.repair_doctrine.clone().unwrap_or_default(),
        change_protocol: seed.change_principle.clone().unwrap_or_default(),
        updated_at: recorded_at,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulConstitutionalViewV1 {
    pub core: SelfAuthoredCore,
    pub provenance: SubjectSoulRevisionProvenanceV1,
    pub material_digest: String,
}

impl SubjectSoulConstitutionalViewV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        if !self.core.is_meaningful() || self.core.revision == 0 {
            return Err(SubjectSoulContractError::repair(
                "constitutional view requires a meaningful revisioned Core",
            ));
        }
        self.provenance.validate_contract()?;
        validate_digest(&self.material_digest, "material_digest")
    }
}

pub fn render_subject_soul_constitutional_block(
    view: &SubjectSoulConstitutionalViewV1,
    max_len: usize,
) -> SubjectSoulContractResult<Option<String>> {
    view.validate_contract()?;
    if max_len < 96 {
        return Ok(None);
    }
    let title = match view.provenance.origin {
        SubjectSoulRevisionOriginV1::HumanFoundingCharter => {
            "## Human-Sourced Founding Constitution"
        }
        SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap
        | SubjectSoulRevisionOriginV1::SelfGovernedRevision => "## Self-Governed Soul Constitution",
    };
    let core = &view.core;
    let mut rendered = String::with_capacity(max_len.min(1_024));
    let _ = writeln!(rendered, "{title}");
    let _ = writeln!(
        rendered,
        "Source authority: {:?}; material: {}",
        view.provenance.source_authority, view.material_digest
    );
    let _ = writeln!(rendered, "Revision: {}", core.revision);
    if !core.identity_anchor.trim().is_empty() {
        let _ = writeln!(rendered, "Identity anchor: {}", core.identity_anchor.trim());
    }
    if !core.character_tendencies.is_empty() {
        let _ = writeln!(
            rendered,
            "Character tendencies: {}",
            core.character_tendencies.join(" | ")
        );
    }
    if !core.priority_constitution.is_empty() {
        let _ = writeln!(
            rendered,
            "Priority constitution: {}",
            core.priority_constitution.join(" > ")
        );
    }
    if !core.non_negotiables.is_empty() {
        let _ = writeln!(
            rendered,
            "Non-negotiables: {}",
            core.non_negotiables.join(" | ")
        );
    }
    for (label, value) in [
        ("Default response mode", core.default_response_mode.as_str()),
        (
            "Default initiative posture",
            core.default_initiative_posture.as_str(),
        ),
        (
            "Default relationship posture",
            core.default_relationship_posture.as_str(),
        ),
        ("Boundary doctrine", core.boundary_doctrine.as_str()),
        ("Truth doctrine", core.truth_doctrine.as_str()),
        (
            "Self-preservation doctrine",
            core.self_preservation_doctrine.as_str(),
        ),
        ("Repair doctrine", core.repair_doctrine.as_str()),
        ("Change protocol", core.change_protocol.as_str()),
    ] {
        if !value.trim().is_empty() {
            let _ = writeln!(rendered, "{label}: {}", value.trim());
        }
    }
    Ok(Some(
        truncate_content_to_max(rendered.trim_end(), max_len).into_owned(),
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulLifecycleStateV1 {
    Unseeded,
    Active,
    Archived,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulLifecycleHeadV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub subject_id: String,
    pub soul_id: String,
    pub generation: u64,
    pub state: SubjectSoulLifecycleStateV1,
    pub current_revision: Option<u64>,
    pub current_material_digest: Option<String>,
    pub current_ledger_digest: Option<String>,
    pub scope_manifest_digest: String,
    pub retained_revision_refs: Vec<String>,
    pub retained_tombstone_refs: Vec<String>,
    pub updated_at: u64,
    pub head_digest: String,
}

impl SubjectSoulLifecycleHeadV1 {
    pub fn refresh_digest(&mut self) -> SubjectSoulContractResult<()> {
        self.head_digest.clear();
        self.head_digest = canonical_digest("subject_soul_head_v1", self)?;
        Ok(())
    }

    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_owner(
            self.schema_version,
            &self.memory_space_id,
            &self.subject_id,
            &self.soul_id,
        )?;
        if self.generation == 0 {
            return Err(SubjectSoulContractError::repair(
                "Soul generation must be positive",
            ));
        }
        match self.state {
            SubjectSoulLifecycleStateV1::Unseeded | SubjectSoulLifecycleStateV1::Deleted => {
                if self.current_revision.is_some()
                    || self.current_material_digest.is_some()
                    || self.current_ledger_digest.is_some()
                {
                    return Err(SubjectSoulContractError::repair(
                        "unseeded/deleted head cannot reference current revision material",
                    ));
                }
            }
            SubjectSoulLifecycleStateV1::Active | SubjectSoulLifecycleStateV1::Archived => {
                if self.current_revision.is_none()
                    || self.current_material_digest.is_none()
                    || self.current_ledger_digest.is_none()
                {
                    return Err(SubjectSoulContractError::repair(
                        "active/archived head requires exact current revision closure",
                    ));
                }
            }
        }
        validate_digest(&self.scope_manifest_digest, "scope_manifest_digest")?;
        validate_sorted_unique_components(&self.retained_revision_refs, "retained_revision_refs")?;
        validate_sorted_unique_components(
            &self.retained_tombstone_refs,
            "retained_tombstone_refs",
        )?;
        validate_digest(&self.head_digest, "head_digest")?;
        let mut canonical = self.clone();
        canonical.head_digest.clear();
        if canonical_digest("subject_soul_head_v1", &canonical)? != self.head_digest {
            return Err(SubjectSoulContractError::repair("head digest mismatch"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulRevisionMaterialV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub subject_id: String,
    pub soul_id: String,
    pub generation: u64,
    pub revision: u64,
    pub supersedes_revision: Option<u64>,
    pub core: SelfAuthoredCore,
    pub provenance: SubjectSoulRevisionProvenanceV1,
    pub content_digest: String,
}

impl SubjectSoulRevisionMaterialV1 {
    pub fn refresh_digest(&mut self) -> SubjectSoulContractResult<()> {
        self.content_digest.clear();
        self.content_digest = canonical_digest("subject_soul_material_v1", self)?;
        Ok(())
    }

    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_owner(
            self.schema_version,
            &self.memory_space_id,
            &self.subject_id,
            &self.soul_id,
        )?;
        if self.generation == 0 || self.revision == 0 || self.core.revision != self.revision {
            return Err(SubjectSoulContractError::repair(
                "material generation/revision must match the Core post-image",
            ));
        }
        if self.core.supersedes_revision != self.supersedes_revision {
            return Err(SubjectSoulContractError::repair(
                "material supersedes_revision must match the Core post-image",
            ));
        }
        self.provenance.validate_contract()?;
        validate_digest(&self.content_digest, "content_digest")?;
        let mut canonical = self.clone();
        canonical.content_digest.clear();
        if canonical_digest("subject_soul_material_v1", &canonical)? != self.content_digest {
            return Err(SubjectSoulContractError::repair("material digest mismatch"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulManifestOwnerRoleV1 {
    SubjectGlobal,
    RelationshipProjection,
    GenerationDerived,
    Private,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulScopeManifestEntryV1 {
    pub namespace: String,
    pub physical_key: String,
    pub owner_role: SubjectSoulManifestOwnerRoleV1,
    pub generation: u64,
    pub revision: Option<u64>,
    pub content_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulScopeManifestV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub subject_id: String,
    pub soul_id: String,
    pub generation: u64,
    pub manifest_revision: u64,
    pub entries: Vec<SubjectSoulScopeManifestEntryV1>,
    pub closure_digest: String,
}

impl SubjectSoulScopeManifestV1 {
    pub fn refresh_digest(&mut self) -> SubjectSoulContractResult<()> {
        self.closure_digest.clear();
        self.closure_digest = canonical_digest("subject_soul_manifest_v1", self)?;
        Ok(())
    }

    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_owner(
            self.schema_version,
            &self.memory_space_id,
            &self.subject_id,
            &self.soul_id,
        )?;
        if self.generation == 0 || self.manifest_revision == 0 {
            return Err(SubjectSoulContractError::repair(
                "manifest generation and revision must be positive",
            ));
        }
        let mut previous: Option<(&str, &str)> = None;
        for entry in &self.entries {
            validate_component(&entry.namespace, "manifest.namespace")?;
            validate_component(&entry.physical_key, "manifest.physical_key")?;
            validate_digest(&entry.content_digest, "manifest.content_digest")?;
            if entry.generation != self.generation {
                return Err(SubjectSoulContractError::repair(
                    "manifest entry generation mismatch",
                ));
            }
            let current = (entry.namespace.as_str(), entry.physical_key.as_str());
            if previous.is_some_and(|value| value >= current) {
                return Err(SubjectSoulContractError::repair(
                    "manifest entries must be sorted and unique",
                ));
            }
            previous = Some(current);
        }
        validate_digest(&self.closure_digest, "closure_digest")?;
        let mut canonical = self.clone();
        canonical.closure_digest.clear();
        if canonical_digest("subject_soul_manifest_v1", &canonical)? != self.closure_digest {
            return Err(SubjectSoulContractError::repair("manifest digest mismatch"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulGenerationTombstoneV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub subject_id: String,
    pub soul_id: String,
    pub generation: u64,
    pub terminal_action: SubjectSoulTerminalActionV1,
    pub terminal_revision: Option<u64>,
    pub terminal_material_digest: Option<String>,
    pub actor_subject_id: String,
    pub reason_code: String,
    pub terminated_at: u64,
    pub prior_head_digest: String,
    pub next_generation: Option<u64>,
    pub tombstone_digest: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulTerminalActionV1 {
    Reset,
    Reseed,
    Delete,
}

impl SubjectSoulGenerationTombstoneV1 {
    pub fn refresh_digest(&mut self) -> SubjectSoulContractResult<()> {
        self.tombstone_digest.clear();
        self.tombstone_digest = canonical_digest("subject_soul_tombstone_v1", self)?;
        Ok(())
    }

    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_owner(
            self.schema_version,
            &self.memory_space_id,
            &self.subject_id,
            &self.soul_id,
        )?;
        validate_component(&self.reason_code, "reason_code")?;
        validate_component(&self.actor_subject_id, "actor_subject_id")?;
        validate_digest(&self.prior_head_digest, "prior_head_digest")?;
        match (
            self.terminal_revision,
            self.terminal_material_digest.as_ref(),
        ) {
            (Some(revision), Some(digest)) if revision > 0 => {
                validate_digest(digest, "terminal_material_digest")?;
            }
            (None, None) => {}
            _ => {
                return Err(SubjectSoulContractError::repair(
                    "terminal revision and material digest must be present together",
                ));
            }
        }
        match self.terminal_action {
            SubjectSoulTerminalActionV1::Reset | SubjectSoulTerminalActionV1::Reseed => {
                if self.next_generation != self.generation.checked_add(1) {
                    return Err(SubjectSoulContractError::repair(
                        "reset/reseed tombstone must point to the next generation",
                    ));
                }
            }
            SubjectSoulTerminalActionV1::Delete => {
                if self.next_generation.is_some() {
                    return Err(SubjectSoulContractError::repair(
                        "terminal delete cannot point to a reusable generation",
                    ));
                }
            }
        }
        validate_digest(&self.tombstone_digest, "tombstone_digest")?;
        let mut canonical = self.clone();
        canonical.tombstone_digest.clear();
        if canonical_digest("subject_soul_tombstone_v1", &canonical)? != self.tombstone_digest {
            return Err(SubjectSoulContractError::repair(
                "tombstone digest mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulExpectedStateV1 {
    PristineAbsent {
        closure_certificate_digest: String,
    },
    Exact {
        generation: u64,
        revision: Option<u64>,
        lifecycle_state: SubjectSoulLifecycleStateV1,
        head_digest: String,
        manifest_digest: String,
    },
}

impl SubjectSoulExpectedStateV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        match self {
            Self::PristineAbsent {
                closure_certificate_digest,
            } => validate_digest(closure_certificate_digest, "closure_certificate_digest"),
            Self::Exact {
                generation,
                revision,
                lifecycle_state,
                head_digest,
                manifest_digest,
            } => {
                if *generation == 0 {
                    return Err(SubjectSoulContractError::repair(
                        "expected generation must be positive",
                    ));
                }
                match lifecycle_state {
                    SubjectSoulLifecycleStateV1::Active | SubjectSoulLifecycleStateV1::Archived
                        if revision.is_none() =>
                    {
                        return Err(SubjectSoulContractError::repair(
                            "active expected state requires revision",
                        ));
                    }
                    SubjectSoulLifecycleStateV1::Unseeded
                    | SubjectSoulLifecycleStateV1::Deleted
                        if revision.is_some() =>
                    {
                        return Err(SubjectSoulContractError::repair(
                            "unseeded/deleted expected state cannot carry revision",
                        ));
                    }
                    _ => {}
                }
                validate_digest(head_digest, "head_digest")?;
                validate_digest(manifest_digest, "manifest_digest")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanSoulLifecycleConfirmationV1 {
    pub human_subject_id: String,
    pub target_subject_id: String,
    pub expected_generation: u64,
    pub action: SubjectSoulTerminalActionV1,
    pub reason_code: String,
    pub confirmed_at: u64,
    pub evidence_digest: String,
}

impl HumanSoulLifecycleConfirmationV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_component(&self.human_subject_id, "human_subject_id")?;
        validate_component(&self.target_subject_id, "target_subject_id")?;
        if self.human_subject_id == self.target_subject_id
            || self.expected_generation == 0
            || self.confirmed_at == 0
        {
            return Err(SubjectSoulContractError {
                key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
                reason:
                    "destructive confirmation must bind distinct subjects, generation, and time"
                        .to_string(),
            });
        }
        validate_component(&self.reason_code, "reason_code")?;
        validate_digest(&self.evidence_digest, "evidence_digest")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "authority", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulLifecycleAuthorityV1 {
    Provision {
        human_actor_subject_id: String,
    },
    SelfGovernance {
        capability_digest: String,
    },
    Maintenance {
        system_actor_subject_id: String,
    },
    Destructive {
        system_actor_subject_id: String,
        human_confirmation: HumanSoulLifecycleConfirmationV1,
    },
}

impl SubjectSoulLifecycleAuthorityV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        match self {
            Self::Provision {
                human_actor_subject_id,
            } => validate_component(human_actor_subject_id, "human_actor_subject_id"),
            Self::SelfGovernance { capability_digest } => {
                validate_digest(capability_digest, "capability_digest")
            }
            Self::Maintenance {
                system_actor_subject_id,
            } => validate_component(system_actor_subject_id, "system_actor_subject_id"),
            Self::Destructive {
                system_actor_subject_id,
                human_confirmation,
            } => {
                validate_component(system_actor_subject_id, "system_actor_subject_id")?;
                human_confirmation.validate_contract()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulLifecycleActionV1 {
    Archive,
    Restore,
    Reset {
        reason_code: String,
    },
    Reseed {
        charter: Box<SubjectSoulFoundingCharterSeedV1>,
        reason_code: String,
        source_asserted_at: Option<u64>,
    },
    Delete {
        reason_code: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulLifecycleMutationRequestV1 {
    pub operation_id: String,
    pub target_subject_id: String,
    pub expected_state: SubjectSoulExpectedStateV1,
    pub authority: SubjectSoulLifecycleAuthorityV1,
    pub action: SubjectSoulLifecycleActionV1,
}

impl SubjectSoulLifecycleMutationRequestV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_component(&self.operation_id, "operation_id")?;
        validate_component(&self.target_subject_id, "target_subject_id")?;
        self.expected_state.validate_contract()?;
        self.authority.validate_contract()?;
        if let SubjectSoulLifecycleAuthorityV1::Destructive {
            human_confirmation, ..
        } = &self.authority
        {
            let SubjectSoulExpectedStateV1::Exact { generation, .. } = self.expected_state else {
                return Err(SubjectSoulContractError {
                    key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
                    reason: "destructive lifecycle action requires exact current generation"
                        .to_string(),
                });
            };
            if human_confirmation.expected_generation != generation {
                return Err(SubjectSoulContractError {
                    key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
                    reason: "human confirmation generation does not match expected state"
                        .to_string(),
                });
            }
        }
        match (&self.action, &self.authority) {
            (
                SubjectSoulLifecycleActionV1::Archive | SubjectSoulLifecycleActionV1::Restore,
                SubjectSoulLifecycleAuthorityV1::SelfGovernance { .. }
                | SubjectSoulLifecycleAuthorityV1::Maintenance { .. },
            ) => Ok(()),
            (
                SubjectSoulLifecycleActionV1::Reset { reason_code }
                | SubjectSoulLifecycleActionV1::Delete { reason_code },
                SubjectSoulLifecycleAuthorityV1::Destructive {
                    human_confirmation, ..
                },
            ) => {
                validate_component(reason_code, "reason_code")?;
                validate_destructive_binding(
                    &self.target_subject_id,
                    reason_code,
                    &self.action,
                    human_confirmation,
                )
            }
            (
                SubjectSoulLifecycleActionV1::Reseed {
                    charter,
                    reason_code,
                    ..
                },
                SubjectSoulLifecycleAuthorityV1::Destructive {
                    human_confirmation, ..
                },
            ) => {
                charter.validate_canonical()?;
                validate_component(reason_code, "reason_code")?;
                validate_destructive_binding(
                    &self.target_subject_id,
                    reason_code,
                    &self.action,
                    human_confirmation,
                )
            }
            _ => Err(SubjectSoulContractError {
                key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
                reason: "lifecycle action does not match its typed authority".to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulOwnerV1 {
    pub memory_space_id: String,
    pub subject_id: String,
    pub soul_id: String,
}

impl SubjectSoulOwnerV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_owner(
            SUBJECT_SOUL_SCHEMA_VERSION,
            &self.memory_space_id,
            &self.subject_id,
            &self.soul_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulManifestAddressV1 {
    pub namespace: String,
    pub physical_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulOwnedDocumentV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub subject_id: String,
    pub soul_id: String,
    pub generation: u64,
    pub revision: Option<u64>,
    pub namespace: String,
    pub physical_key: String,
    pub body: serde_json::Value,
    pub content_digest: String,
}

impl SubjectSoulOwnedDocumentV1 {
    pub fn new<T: Serialize>(
        owner: &SubjectSoulOwnerV1,
        generation: u64,
        revision: Option<u64>,
        address: &SubjectSoulManifestAddressV1,
        body: &T,
    ) -> SubjectSoulContractResult<Self> {
        owner.validate_contract()?;
        address.validate_contract()?;
        if generation == 0 || revision == Some(0) {
            return Err(SubjectSoulContractError::repair(
                "owned Soul document generation/revision is invalid",
            ));
        }
        let body = serde_json::to_value(body).map_err(|_| {
            SubjectSoulContractError::repair("owned Soul document body is not serializable")
        })?;
        let mut document = Self {
            schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
            memory_space_id: owner.memory_space_id.clone(),
            subject_id: owner.subject_id.clone(),
            soul_id: owner.soul_id.clone(),
            generation,
            revision,
            namespace: address.namespace.clone(),
            physical_key: address.physical_key.clone(),
            body,
            content_digest: String::new(),
        };
        document.refresh_digest()?;
        document.validate_contract()?;
        Ok(document)
    }

    pub fn refresh_digest(&mut self) -> SubjectSoulContractResult<()> {
        self.content_digest.clear();
        self.content_digest = subject_soul_owned_document_digest(self)?;
        Ok(())
    }

    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_owner(
            self.schema_version,
            &self.memory_space_id,
            &self.subject_id,
            &self.soul_id,
        )?;
        if self.generation == 0 || self.revision == Some(0) {
            return Err(SubjectSoulContractError::repair(
                "owned Soul document generation/revision is invalid",
            ));
        }
        validate_component(&self.namespace, "owned_document.namespace")?;
        validate_component(&self.physical_key, "owned_document.physical_key")?;
        validate_digest(&self.content_digest, "owned_document.content_digest")?;
        if subject_soul_owned_document_digest(self)? != self.content_digest {
            return Err(SubjectSoulContractError::repair(
                "owned Soul document digest mismatch",
            ));
        }
        Ok(())
    }
}

impl SubjectSoulManifestAddressV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_component(&self.namespace, "manifest.namespace")?;
        validate_component(&self.physical_key, "manifest.physical_key")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulRevisionAddressBindingsV1 {
    pub material: SubjectSoulManifestAddressV1,
    pub core: SubjectSoulManifestAddressV1,
    pub revision_ledger: SubjectSoulManifestAddressV1,
}

impl SubjectSoulRevisionAddressBindingsV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        self.material.validate_contract()?;
        self.core.validate_contract()?;
        self.revision_ledger.validate_contract()?;
        let addresses = [
            (&self.material.namespace, &self.material.physical_key),
            (&self.core.namespace, &self.core.physical_key),
            (
                &self.revision_ledger.namespace,
                &self.revision_ledger.physical_key,
            ),
        ];
        if addresses.iter().collect::<BTreeSet<_>>().len() != addresses.len() {
            return Err(SubjectSoulContractError::repair(
                "Soul revision artifact addresses must be exact and distinct",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulVerifiedSnapshotV1 {
    pub head: SubjectSoulLifecycleHeadV1,
    pub manifest: SubjectSoulScopeManifestV1,
    pub current_material: Option<SubjectSoulRevisionMaterialV1>,
    pub current_core: Option<SelfAuthoredCore>,
    pub current_core_document: Option<SubjectSoulOwnedDocumentV1>,
    pub current_revision_ledger: Option<CoreRevisionLedger>,
    pub current_revision_ledger_document: Option<SubjectSoulOwnedDocumentV1>,
}

impl SubjectSoulVerifiedSnapshotV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_subject_soul_snapshot_documents(self)?;
        let ledger_digest = self
            .current_revision_ledger_document
            .as_ref()
            .map(|document| document.content_digest.as_str());
        validate_subject_soul_post_image(
            &self.head,
            &self.manifest,
            self.current_material.as_ref(),
            self.current_core.as_ref(),
            ledger_digest,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulProvisionPlanV1 {
    UnseededNoEffect {
        owner: SubjectSoulOwnerV1,
        expected_state: SubjectSoulExpectedStateV1,
    },
    Commit {
        expected_state: SubjectSoulExpectedStateV1,
        intent_digest: String,
        head: Box<SubjectSoulLifecycleHeadV1>,
        manifest: Box<SubjectSoulScopeManifestV1>,
        material: Box<SubjectSoulRevisionMaterialV1>,
        core: Box<SelfAuthoredCore>,
        core_document: Box<SubjectSoulOwnedDocumentV1>,
        revision_ledger: Box<CoreRevisionLedger>,
        revision_ledger_document: Box<SubjectSoulOwnedDocumentV1>,
        revision_ledger_digest: String,
    },
}

pub fn subject_soul_provision_intent_digest_v1(
    owner: &SubjectSoulOwnerV1,
    intent: &SubjectSoulProvisionIntentV1,
) -> SubjectSoulContractResult<String> {
    owner.validate_contract()?;
    intent.validate_canonical()?;
    canonical_digest("subject_soul_provision_intent_v1", &(owner, intent))
}

pub fn subject_soul_lifecycle_intent_digest_v1(
    owner: &SubjectSoulOwnerV1,
    request: &SubjectSoulLifecycleMutationRequestV1,
) -> SubjectSoulContractResult<String> {
    owner.validate_contract()?;
    request.validate_contract()?;
    if request.target_subject_id != owner.subject_id {
        return Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::TargetNotMounted,
            reason: "Soul lifecycle intent target does not match its owner".to_string(),
        });
    }
    canonical_digest("subject_soul_lifecycle_intent_v1", &(owner, request))
}

pub fn plan_subject_soul_provision_v1(
    owner: &SubjectSoulOwnerV1,
    intent: &SubjectSoulProvisionIntentV1,
    expected_state: &SubjectSoulExpectedStateV1,
    revision_addresses: Option<&SubjectSoulRevisionAddressBindingsV1>,
    recorded_at: u64,
) -> SubjectSoulContractResult<SubjectSoulProvisionPlanV1> {
    owner.validate_contract()?;
    intent.validate_canonical()?;
    expected_state.validate_contract()?;
    if !matches!(
        expected_state,
        SubjectSoulExpectedStateV1::PristineAbsent { .. }
    ) {
        return Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::AlreadyInitialized,
            reason: "Soul provisioning requires verified pristine absence".to_string(),
        });
    }
    let SubjectSoulProvisionIntentV1::Founding {
        operation_id,
        human_actor_subject_id,
        charter,
        source_asserted_at,
    } = intent
    else {
        if revision_addresses.is_some() {
            return Err(SubjectSoulContractError::repair(
                "unseeded provisioning cannot allocate revision artifacts",
            ));
        }
        return Ok(SubjectSoulProvisionPlanV1::UnseededNoEffect {
            owner: owner.clone(),
            expected_state: expected_state.clone(),
        });
    };
    if recorded_at == 0 || source_asserted_at.is_some_and(|value| value > recorded_at) {
        return Err(SubjectSoulContractError::repair(
            "founding charter time binding is invalid",
        ));
    }
    let revision_addresses = revision_addresses.ok_or_else(|| {
        SubjectSoulContractError::repair("founding charter requires exact revision addresses")
    })?;
    let built = build_subject_soul_founding_revision(
        owner,
        1,
        1,
        operation_id,
        human_actor_subject_id,
        charter,
        *source_asserted_at,
        revision_addresses,
        recorded_at,
    )?;
    validate_subject_soul_post_image(
        &built.head,
        &built.manifest,
        Some(&built.material),
        Some(&built.core),
        Some(&built.revision_ledger_digest),
    )?;
    let intent_digest = subject_soul_provision_intent_digest_v1(owner, intent)?;
    Ok(SubjectSoulProvisionPlanV1::Commit {
        expected_state: expected_state.clone(),
        intent_digest,
        head: Box::new(built.head),
        manifest: Box::new(built.manifest),
        material: Box::new(built.material),
        core: Box::new(built.core),
        core_document: Box::new(built.core_document),
        revision_ledger: Box::new(built.revision_ledger),
        revision_ledger_document: Box::new(built.revision_ledger_document),
        revision_ledger_digest: built.revision_ledger_digest,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulLifecyclePlanV1 {
    pub expected_state: SubjectSoulExpectedStateV1,
    pub intent_digest: String,
    pub post_head: Box<SubjectSoulLifecycleHeadV1>,
    pub post_manifest: Box<SubjectSoulScopeManifestV1>,
    pub new_material: Option<Box<SubjectSoulRevisionMaterialV1>>,
    pub new_core: Option<Box<SelfAuthoredCore>>,
    pub new_core_document: Option<Box<SubjectSoulOwnedDocumentV1>>,
    pub new_revision_ledger: Option<Box<CoreRevisionLedger>>,
    pub new_revision_ledger_document: Option<Box<SubjectSoulOwnedDocumentV1>>,
    pub new_revision_ledger_digest: Option<String>,
    pub tombstone: Option<Box<SubjectSoulGenerationTombstoneV1>>,
    pub purge_manifest_addresses: Vec<SubjectSoulManifestAddressV1>,
    pub purge_retained_revision_refs: Vec<String>,
}

pub fn plan_subject_soul_lifecycle_v1(
    owner: &SubjectSoulOwnerV1,
    request: &SubjectSoulLifecycleMutationRequestV1,
    previous: &SubjectSoulVerifiedSnapshotV1,
    revision_addresses: Option<&SubjectSoulRevisionAddressBindingsV1>,
    tombstone_physical_ref: Option<&str>,
    recorded_at: u64,
) -> SubjectSoulContractResult<SubjectSoulLifecyclePlanV1> {
    owner.validate_contract()?;
    request.validate_contract()?;
    previous.validate_contract()?;
    if recorded_at == 0 || request.target_subject_id != owner.subject_id {
        return Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::TargetNotMounted,
            reason: "Soul lifecycle planner target/time binding is invalid".to_string(),
        });
    }
    validate_subject_soul_snapshot_owner(owner, previous)?;
    validate_subject_soul_expected_snapshot(&request.expected_state, previous)?;
    let intent_digest = subject_soul_lifecycle_intent_digest_v1(owner, request)?;
    let mut post_head = previous.head.clone();
    let mut post_manifest = previous.manifest.clone();
    let mut new_material = None;
    let mut new_core = None;
    let mut new_core_document = None;
    let mut new_revision_ledger = None;
    let mut new_revision_ledger_document = None;
    let mut new_revision_ledger_digest = None;
    let mut tombstone = None;
    let mut purge_manifest_addresses = Vec::new();
    let mut purge_retained_revision_refs = Vec::new();

    match &request.action {
        SubjectSoulLifecycleActionV1::Archive => {
            if previous.head.state != SubjectSoulLifecycleStateV1::Active {
                return Err(SubjectSoulContractError {
                    key: SubjectSoulLifecycleErrorKey::Archived,
                    reason: "archive requires an active Soul".to_string(),
                });
            }
            ensure_no_lifecycle_allocation(revision_addresses, tombstone_physical_ref)?;
            purge_manifest_addresses =
                purge_subject_soul_relationship_projections(&mut post_manifest)?;
            post_head.state = SubjectSoulLifecycleStateV1::Archived;
        }
        SubjectSoulLifecycleActionV1::Restore => {
            if previous.head.state != SubjectSoulLifecycleStateV1::Archived {
                return Err(SubjectSoulContractError {
                    key: SubjectSoulLifecycleErrorKey::GenerationConflict,
                    reason: "restore requires an archived Soul".to_string(),
                });
            }
            ensure_no_lifecycle_allocation(revision_addresses, tombstone_physical_ref)?;
            if post_manifest.entries.iter().any(|entry| {
                entry.owner_role == SubjectSoulManifestOwnerRoleV1::RelationshipProjection
            }) {
                return Err(SubjectSoulContractError::repair(
                    "archived Soul must purge relationship projections before restore",
                ));
            }
            post_head.state = SubjectSoulLifecycleStateV1::Active;
        }
        SubjectSoulLifecycleActionV1::Reset { reason_code }
        | SubjectSoulLifecycleActionV1::Delete { reason_code } => {
            if previous.head.state == SubjectSoulLifecycleStateV1::Deleted {
                return Err(SubjectSoulContractError {
                    key: SubjectSoulLifecycleErrorKey::Deleted,
                    reason: "deleted Soul generation is terminal".to_string(),
                });
            }
            if revision_addresses.is_some() {
                return Err(SubjectSoulContractError::repair(
                    "reset/delete cannot allocate revision artifacts",
                ));
            }
            let tombstone_ref = required_tombstone_ref(tombstone_physical_ref)?;
            let (terminal_action, next_generation, next_state) = match request.action {
                SubjectSoulLifecycleActionV1::Reset { .. } => (
                    SubjectSoulTerminalActionV1::Reset,
                    Some(next_subject_soul_generation(previous.head.generation)?),
                    SubjectSoulLifecycleStateV1::Unseeded,
                ),
                SubjectSoulLifecycleActionV1::Delete { .. } => (
                    SubjectSoulTerminalActionV1::Delete,
                    None,
                    SubjectSoulLifecycleStateV1::Deleted,
                ),
                _ => unreachable!("matched reset/delete"),
            };
            let next_generation_value = next_generation.unwrap_or(previous.head.generation);
            tombstone = Some(Box::new(build_subject_soul_tombstone(
                owner,
                previous,
                terminal_action,
                destructive_system_actor(&request.authority)?,
                reason_code,
                recorded_at,
                next_generation,
            )?));
            post_manifest = empty_subject_soul_manifest(
                owner,
                next_generation_value,
                next_subject_soul_manifest_revision(previous.manifest.manifest_revision)?,
            )?;
            post_head.generation = next_generation_value;
            post_head.state = next_state;
            clear_subject_soul_current(&mut post_head);
            (purge_manifest_addresses, purge_retained_revision_refs) =
                subject_soul_destructive_purge_set(previous);
            retain_subject_soul_tombstone_only(&mut post_head, previous, tombstone_ref);
        }
        SubjectSoulLifecycleActionV1::Reseed {
            charter,
            reason_code,
            source_asserted_at,
        } => {
            if previous.head.state == SubjectSoulLifecycleStateV1::Deleted {
                return Err(SubjectSoulContractError {
                    key: SubjectSoulLifecycleErrorKey::Deleted,
                    reason: "deleted Soul cannot be reseeded".to_string(),
                });
            }
            let addresses = revision_addresses.ok_or_else(|| {
                SubjectSoulContractError::repair("reseed requires exact revision addresses")
            })?;
            addresses.validate_contract()?;
            let tombstone_ref = required_tombstone_ref(tombstone_physical_ref)?;
            let next_generation = next_subject_soul_generation(previous.head.generation)?;
            tombstone = Some(Box::new(build_subject_soul_tombstone(
                owner,
                previous,
                SubjectSoulTerminalActionV1::Reseed,
                destructive_system_actor(&request.authority)?,
                reason_code,
                recorded_at,
                Some(next_generation),
            )?));
            let human_actor = destructive_human_actor(&request.authority)?;
            let built = build_subject_soul_founding_revision(
                owner,
                next_generation,
                next_subject_soul_manifest_revision(previous.manifest.manifest_revision)?,
                &request.operation_id,
                human_actor,
                charter,
                *source_asserted_at,
                addresses,
                recorded_at,
            )?;
            post_head = built.head;
            post_manifest = built.manifest;
            (purge_manifest_addresses, purge_retained_revision_refs) =
                subject_soul_destructive_purge_set(previous);
            retain_subject_soul_tombstone_only(&mut post_head, previous, tombstone_ref);
            post_head.scope_manifest_digest = post_manifest.closure_digest.clone();
            post_head.refresh_digest()?;
            new_material = Some(Box::new(built.material));
            new_core = Some(Box::new(built.core));
            new_core_document = Some(Box::new(built.core_document));
            new_revision_ledger = Some(Box::new(built.revision_ledger));
            new_revision_ledger_document = Some(Box::new(built.revision_ledger_document));
            new_revision_ledger_digest = Some(built.revision_ledger_digest);
        }
    }
    post_head.updated_at = recorded_at;
    post_head.scope_manifest_digest = post_manifest.closure_digest.clone();
    post_head.refresh_digest()?;

    match request.action {
        SubjectSoulLifecycleActionV1::Archive | SubjectSoulLifecycleActionV1::Restore => {
            let ledger_digest = previous
                .current_revision_ledger_document
                .as_ref()
                .map(|document| document.content_digest.as_str());
            validate_subject_soul_post_image(
                &post_head,
                &post_manifest,
                previous.current_material.as_ref(),
                previous.current_core.as_ref(),
                ledger_digest,
            )?;
        }
        SubjectSoulLifecycleActionV1::Reset { .. }
        | SubjectSoulLifecycleActionV1::Delete { .. } => {
            validate_subject_soul_post_image(&post_head, &post_manifest, None, None, None)?;
        }
        SubjectSoulLifecycleActionV1::Reseed { .. } => {
            validate_subject_soul_post_image(
                &post_head,
                &post_manifest,
                new_material.as_deref(),
                new_core.as_deref(),
                new_revision_ledger_digest.as_deref(),
            )?;
        }
    }
    Ok(SubjectSoulLifecyclePlanV1 {
        expected_state: request.expected_state.clone(),
        intent_digest,
        post_head: Box::new(post_head),
        post_manifest: Box::new(post_manifest),
        new_material,
        new_core,
        new_core_document,
        new_revision_ledger,
        new_revision_ledger_document,
        new_revision_ledger_digest,
        tombstone,
        purge_manifest_addresses,
        purge_retained_revision_refs,
    })
}

fn purge_subject_soul_relationship_projections(
    manifest: &mut SubjectSoulScopeManifestV1,
) -> SubjectSoulContractResult<Vec<SubjectSoulManifestAddressV1>> {
    let mut purge_addresses = manifest
        .entries
        .iter()
        .filter(|entry| entry.owner_role == SubjectSoulManifestOwnerRoleV1::RelationshipProjection)
        .map(|entry| SubjectSoulManifestAddressV1 {
            namespace: entry.namespace.clone(),
            physical_key: entry.physical_key.clone(),
        })
        .collect::<Vec<_>>();
    if purge_addresses.is_empty() {
        return Ok(purge_addresses);
    }
    manifest
        .entries
        .retain(|entry| entry.owner_role != SubjectSoulManifestOwnerRoleV1::RelationshipProjection);
    manifest.manifest_revision = next_subject_soul_manifest_revision(manifest.manifest_revision)?;
    manifest.refresh_digest()?;
    purge_addresses.sort_by(|left, right| {
        (&left.namespace, &left.physical_key).cmp(&(&right.namespace, &right.physical_key))
    });
    purge_addresses.dedup();
    Ok(purge_addresses)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "basis", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulSelfAuthoredRevisionBasisV1 {
    ImplicitUnseeded {
        closure_certificate_digest: String,
    },
    Verified {
        snapshot: Box<SubjectSoulVerifiedSnapshotV1>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulSelfAuthoredPostImageAddressesV1 {
    Adopt {
        revision: Box<SubjectSoulRevisionAddressBindingsV1>,
    },
    ReviewedRejected {
        revision_ledger: SubjectSoulManifestAddressV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulSelfAuthoredCommitPlanV1 {
    NoEffect,
    ReviewedRejected {
        expected_state: SubjectSoulExpectedStateV1,
        intent_digest: String,
        post_head: Box<SubjectSoulLifecycleHeadV1>,
        post_manifest: Box<SubjectSoulScopeManifestV1>,
        revision_ledger: Box<CoreRevisionLedger>,
        revision_ledger_document: Box<SubjectSoulOwnedDocumentV1>,
        purge_manifest_addresses: Vec<SubjectSoulManifestAddressV1>,
    },
    Adopt {
        expected_state: SubjectSoulExpectedStateV1,
        intent_digest: String,
        post_head: Box<SubjectSoulLifecycleHeadV1>,
        post_manifest: Box<SubjectSoulScopeManifestV1>,
        material: Box<SubjectSoulRevisionMaterialV1>,
        core: Box<SelfAuthoredCore>,
        core_document: Box<SubjectSoulOwnedDocumentV1>,
        revision_ledger: Box<CoreRevisionLedger>,
        revision_ledger_document: Box<SubjectSoulOwnedDocumentV1>,
        purge_manifest_addresses: Vec<SubjectSoulManifestAddressV1>,
    },
}

pub fn subject_soul_self_authored_revision_intent_digest_v1(
    owner: &SubjectSoulOwnerV1,
    expected_state: &SubjectSoulExpectedStateV1,
    refresh_plan: &SelfAuthoredCoreRefreshPlanV1,
) -> SubjectSoulContractResult<String> {
    owner.validate_contract()?;
    expected_state.validate_contract()?;
    match expected_state {
        SubjectSoulExpectedStateV1::PristineAbsent { .. } => canonical_digest(
            "subject_soul_self_authored_revision_intent_v1",
            &(owner, refresh_plan),
        ),
        SubjectSoulExpectedStateV1::Exact { .. } => canonical_digest(
            "subject_soul_self_authored_revision_intent_v1",
            &(owner, expected_state, refresh_plan),
        ),
    }
}

pub fn plan_subject_soul_self_authored_revision_v1(
    owner: &SubjectSoulOwnerV1,
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
    refresh_plan: &SelfAuthoredCoreRefreshPlanV1,
    addresses: Option<&SubjectSoulSelfAuthoredPostImageAddressesV1>,
    recorded_at: u64,
) -> SubjectSoulContractResult<SubjectSoulSelfAuthoredCommitPlanV1> {
    owner.validate_contract()?;
    if recorded_at == 0 {
        return Err(SubjectSoulContractError::repair(
            "self-authored Soul planning requires a positive commit time",
        ));
    }
    validate_self_authored_revision_basis(owner, basis)?;
    if matches!(
        basis,
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot }
            if recorded_at < snapshot.head.updated_at
    ) {
        return Err(SubjectSoulContractError::repair(
            "self-authored Soul commit time cannot precede the verified head",
        ));
    }
    if matches!(refresh_plan, SelfAuthoredCoreRefreshPlanV1::Skipped) {
        if addresses.is_some() {
            return Err(SubjectSoulContractError::repair(
                "skipped self-authored planning cannot allocate artifacts",
            ));
        }
        return Ok(SubjectSoulSelfAuthoredCommitPlanV1::NoEffect);
    }

    let expected_state = self_authored_basis_expected_state(basis);
    let intent_digest =
        subject_soul_self_authored_revision_intent_digest_v1(owner, &expected_state, refresh_plan)?;
    let empty_ledger = CoreRevisionLedger::default();
    let (observed_core, observed_ledger) = match basis {
        SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded { .. } => (None, &empty_ledger),
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } => (
            snapshot.current_core.as_ref(),
            snapshot
                .current_revision_ledger
                .as_ref()
                .unwrap_or(&empty_ledger),
        ),
    };
    let computed_expected =
        compute_self_authored_core_expected_prior_v1(observed_core, observed_ledger).map_err(
            |error| {
                SubjectSoulContractError::repair(format!(
                    "self-authored expected-prior digest failed: {error}"
                ))
            },
        )?;

    match refresh_plan {
        SelfAuthoredCoreRefreshPlanV1::Skipped => unreachable!("handled above"),
        SelfAuthoredCoreRefreshPlanV1::ReviewedRejected {
            expected_prior,
            next_ledger,
            origin,
            proposal_ref,
            source_refs,
        } => plan_subject_soul_reviewed_rejection(
            owner,
            basis,
            &computed_expected,
            expected_prior,
            observed_ledger,
            next_ledger,
            *origin,
            proposal_ref,
            source_refs,
            addresses,
            expected_state,
            intent_digest,
            recorded_at,
        ),
        SelfAuthoredCoreRefreshPlanV1::Adopt {
            expected_prior,
            next_core,
            next_ledger,
            origin,
            proposal_ref,
            source_refs,
        } => plan_subject_soul_adopted_revision(
            owner,
            basis,
            &computed_expected,
            expected_prior,
            observed_ledger,
            next_core,
            next_ledger,
            *origin,
            proposal_ref,
            source_refs,
            addresses,
            expected_state,
            intent_digest,
            recorded_at,
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulGenerationLayerKindV1 {
    SelfModel,
    SelfContinuity,
    RelationshipPortfolio,
    RelationshipTopology,
    AutonomyStrategy,
    InnerLife,
    FeltSignificance,
    TemperamentContinuity,
    InnerConflict,
    MentalPrivacy,
    PrivateDocument,
    PrivateGarden,
    OuterVoice,
}

impl SubjectSoulGenerationLayerKindV1 {
    pub fn canonical_namespace(self) -> &'static str {
        match self {
            Self::SelfModel => "self_model",
            Self::SelfContinuity => "self_continuity",
            Self::RelationshipPortfolio => "relationship_portfolio",
            Self::RelationshipTopology => "relationship_topology",
            Self::AutonomyStrategy => "autonomy_strategy",
            Self::InnerLife => "inner_life",
            Self::FeltSignificance => "felt_significance",
            Self::TemperamentContinuity => "temperament_continuity",
            Self::InnerConflict => "inner_conflict",
            Self::MentalPrivacy => "mental_privacy",
            Self::PrivateDocument => "private_doc",
            Self::PrivateGarden => "private_garden",
            Self::OuterVoice => "outer_voice",
        }
    }

    pub fn owner_role(self) -> SubjectSoulManifestOwnerRoleV1 {
        match self {
            Self::RelationshipPortfolio
            | Self::RelationshipTopology
            | Self::AutonomyStrategy
            | Self::OuterVoice => SubjectSoulManifestOwnerRoleV1::GenerationDerived,
            Self::SelfModel
            | Self::SelfContinuity
            | Self::InnerLife
            | Self::FeltSignificance
            | Self::TemperamentContinuity
            | Self::InnerConflict
            | Self::MentalPrivacy
            | Self::PrivateDocument
            | Self::PrivateGarden => SubjectSoulManifestOwnerRoleV1::Private,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mutation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulGenerationLayerMutationV1 {
    Upsert {
        layer: SubjectSoulGenerationLayerKindV1,
        expected_previous_digest: Option<String>,
        document: Box<SubjectSoulOwnedDocumentV1>,
    },
    Delete {
        layer: SubjectSoulGenerationLayerKindV1,
        address: SubjectSoulManifestAddressV1,
        expected_content_digest: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulGenerationLayerDeltaPlanV1 {
    NoEffect {
        expected_state: SubjectSoulExpectedStateV1,
        intent_digest: String,
    },
    Commit {
        expected_state: SubjectSoulExpectedStateV1,
        intent_digest: String,
        post_head: Box<SubjectSoulLifecycleHeadV1>,
        post_manifest: Box<SubjectSoulScopeManifestV1>,
        upsert_documents: Vec<SubjectSoulOwnedDocumentV1>,
        delete_addresses: Vec<SubjectSoulManifestAddressV1>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "basis", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulGenerationLayerBasisV1 {
    ImplicitUnseeded {
        closure_certificate_digest: String,
    },
    Verified {
        snapshot: Box<SubjectSoulVerifiedSnapshotV1>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "authority", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulGenerationLayerAuthorityV1 {
    MountedAgentPersona { actor_subject_id: String },
    SystemGovernor { actor_subject_id: String },
}

impl SubjectSoulGenerationLayerAuthorityV1 {
    pub fn actor_subject_id(&self) -> &str {
        match self {
            Self::MountedAgentPersona { actor_subject_id }
            | Self::SystemGovernor { actor_subject_id } => actor_subject_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulGenerationLayerIntentV1 {
    pub operation_id: String,
    pub authority: SubjectSoulGenerationLayerAuthorityV1,
    pub mutations: Vec<SubjectSoulGenerationLayerMutationV1>,
}

/// Trusted autonomous-cycle identity. The SDK derives `operation_id` from the
/// durable self-runtime job; neither prompt text nor wall-clock time owns MOR
/// replay identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulAutonomousCycleIntentV1 {
    pub operation_id: String,
    pub actor_subject_id: String,
    #[serde(default)]
    pub layer_mutations: Vec<SubjectSoulGenerationLayerMutationV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulAutonomousCyclePostImageV1 {
    pub head: Box<SubjectSoulLifecycleHeadV1>,
    pub manifest: Box<SubjectSoulScopeManifestV1>,
    pub current_material: Option<Box<SubjectSoulRevisionMaterialV1>>,
    pub current_core: Option<Box<SelfAuthoredCore>>,
    pub current_core_document: Option<Box<SubjectSoulOwnedDocumentV1>>,
    pub current_revision_ledger: Option<Box<CoreRevisionLedger>>,
    pub current_revision_ledger_document: Option<Box<SubjectSoulOwnedDocumentV1>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "delta", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulAutonomousRevisionDeltaV1 {
    None,
    ReviewedRejected {
        revision_ledger_document: Box<SubjectSoulOwnedDocumentV1>,
        purge_manifest_addresses: Vec<SubjectSoulManifestAddressV1>,
    },
    Adopt {
        material: Box<SubjectSoulRevisionMaterialV1>,
        core_document: Box<SubjectSoulOwnedDocumentV1>,
        revision_ledger_document: Box<SubjectSoulOwnedDocumentV1>,
        purge_manifest_addresses: Vec<SubjectSoulManifestAddressV1>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulAutonomousCyclePlanV1 {
    NoEffect {
        expected_state: SubjectSoulExpectedStateV1,
        intent_digest: String,
    },
    Commit {
        expected_state: SubjectSoulExpectedStateV1,
        intent_digest: String,
        post_image: Box<SubjectSoulAutonomousCyclePostImageV1>,
        revision_delta: Box<SubjectSoulAutonomousRevisionDeltaV1>,
        layer_upserts: Vec<SubjectSoulOwnedDocumentV1>,
        layer_deletes: Vec<SubjectSoulManifestAddressV1>,
    },
}

pub fn subject_soul_autonomous_cycle_intent_digest_v1(
    owner: &SubjectSoulOwnerV1,
    expected_state: &SubjectSoulExpectedStateV1,
    intent: &SubjectSoulAutonomousCycleIntentV1,
    refresh_plan: &SelfAuthoredCoreRefreshPlanV1,
) -> SubjectSoulContractResult<String> {
    validate_subject_soul_autonomous_cycle_intent(owner, intent)?;
    expected_state.validate_contract()?;
    match expected_state {
        SubjectSoulExpectedStateV1::PristineAbsent { .. } => canonical_digest(
            "subject_soul_autonomous_cycle_intent_v1",
            &(owner, intent, refresh_plan),
        ),
        SubjectSoulExpectedStateV1::Exact { .. } => canonical_digest(
            "subject_soul_autonomous_cycle_intent_v1",
            &(owner, expected_state, intent, refresh_plan),
        ),
    }
}

/// Compiles one complete autonomous Soul cycle. Core owns the ordering of
/// layer evidence and board-level revision planning, so callers receive one
/// expected state, one intent digest, and one final head/manifest root.
pub fn plan_subject_soul_autonomous_cycle_v1(
    owner: &SubjectSoulOwnerV1,
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
    intent: &SubjectSoulAutonomousCycleIntentV1,
    refresh_plan: &SelfAuthoredCoreRefreshPlanV1,
    self_authored_addresses: Option<&SubjectSoulSelfAuthoredPostImageAddressesV1>,
    recorded_at: u64,
) -> SubjectSoulContractResult<SubjectSoulAutonomousCyclePlanV1> {
    owner.validate_contract()?;
    validate_self_authored_revision_basis(owner, basis)?;
    validate_subject_soul_autonomous_cycle_intent(owner, intent)?;
    if recorded_at == 0 {
        return Err(SubjectSoulContractError::repair(
            "autonomous Soul cycle requires a positive commit time",
        ));
    }
    let expected_state = self_authored_basis_expected_state(basis);
    let intent_digest = subject_soul_autonomous_cycle_intent_digest_v1(
        owner,
        &expected_state,
        intent,
        refresh_plan,
    )?;
    if matches!(refresh_plan, SelfAuthoredCoreRefreshPlanV1::Skipped)
        && intent.layer_mutations.is_empty()
    {
        if self_authored_addresses.is_some() {
            return Err(SubjectSoulContractError::repair(
                "no-effect autonomous cycle cannot allocate revision addresses",
            ));
        }
        return Ok(SubjectSoulAutonomousCyclePlanV1::NoEffect {
            expected_state,
            intent_digest,
        });
    }

    if matches!(refresh_plan, SelfAuthoredCoreRefreshPlanV1::Skipped) {
        if self_authored_addresses.is_some() {
            return Err(SubjectSoulContractError::repair(
                "layer-only autonomous cycle cannot allocate revision addresses",
            ));
        }
        let generation_intent = SubjectSoulGenerationLayerIntentV1 {
            operation_id: intent.operation_id.clone(),
            authority: SubjectSoulGenerationLayerAuthorityV1::MountedAgentPersona {
                actor_subject_id: intent.actor_subject_id.clone(),
            },
            mutations: intent.layer_mutations.clone(),
        };
        let generation_basis = subject_soul_generation_basis_from_autonomous_basis(basis);
        let layer_plan = plan_subject_soul_generation_layer_delta_v1(
            owner,
            &generation_basis,
            &generation_intent,
            recorded_at,
        )?;
        return autonomous_cycle_from_layer_only_plan(
            basis,
            expected_state,
            intent_digest,
            layer_plan,
        );
    }

    let revision_plan = plan_subject_soul_self_authored_revision_v1(
        owner,
        basis,
        refresh_plan,
        self_authored_addresses,
        recorded_at,
    )?;
    let (mut post_image, revision_delta, mut post_head, mut post_manifest) =
        autonomous_cycle_from_revision_plan(basis, revision_plan)?;
    let (layer_upserts, layer_deletes) = apply_autonomous_cycle_layer_mutations(
        owner,
        basis,
        &mut post_head,
        &mut post_manifest,
        &intent.layer_mutations,
    )?;
    post_image.head = Box::new(post_head);
    post_image.manifest = Box::new(post_manifest);
    validate_subject_soul_post_image(
        &post_image.head,
        &post_image.manifest,
        post_image.current_material.as_deref(),
        post_image.current_core.as_deref(),
        post_image
            .current_revision_ledger_document
            .as_deref()
            .map(|document| document.content_digest.as_str()),
    )?;
    Ok(SubjectSoulAutonomousCyclePlanV1::Commit {
        expected_state,
        intent_digest,
        post_image: Box::new(post_image),
        revision_delta: Box::new(revision_delta),
        layer_upserts,
        layer_deletes,
    })
}

pub fn subject_soul_generation_layer_intent_digest_v1(
    owner: &SubjectSoulOwnerV1,
    expected_state: &SubjectSoulExpectedStateV1,
    intent: &SubjectSoulGenerationLayerIntentV1,
) -> SubjectSoulContractResult<String> {
    validate_generation_layer_intent(owner, intent)?;
    expected_state.validate_contract()?;
    match expected_state {
        SubjectSoulExpectedStateV1::PristineAbsent { .. } => {
            canonical_digest("subject_soul_generation_layer_intent_v1", &(owner, intent))
        }
        SubjectSoulExpectedStateV1::Exact { .. } => canonical_digest(
            "subject_soul_generation_layer_intent_v1",
            &(owner, expected_state, intent),
        ),
    }
}

pub fn plan_subject_soul_generation_layer_delta_v1(
    owner: &SubjectSoulOwnerV1,
    basis: &SubjectSoulGenerationLayerBasisV1,
    intent: &SubjectSoulGenerationLayerIntentV1,
    recorded_at: u64,
) -> SubjectSoulContractResult<SubjectSoulGenerationLayerDeltaPlanV1> {
    owner.validate_contract()?;
    validate_generation_layer_intent(owner, intent)?;
    if recorded_at == 0 {
        return Err(SubjectSoulContractError::repair(
            "generation-owned Soul layer delta requires a positive commit time",
        ));
    }
    let (expected_state, mut post_head, mut post_manifest, next_manifest_revision) = match basis {
        SubjectSoulGenerationLayerBasisV1::ImplicitUnseeded {
            closure_certificate_digest,
        } => {
            validate_digest(closure_certificate_digest, "closure_certificate_digest")?;
            (
                SubjectSoulExpectedStateV1::PristineAbsent {
                    closure_certificate_digest: closure_certificate_digest.clone(),
                },
                SubjectSoulLifecycleHeadV1 {
                    schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
                    memory_space_id: owner.memory_space_id.clone(),
                    subject_id: owner.subject_id.clone(),
                    soul_id: owner.soul_id.clone(),
                    generation: 1,
                    state: SubjectSoulLifecycleStateV1::Unseeded,
                    current_revision: None,
                    current_material_digest: None,
                    current_ledger_digest: None,
                    scope_manifest_digest: String::new(),
                    retained_revision_refs: Vec::new(),
                    retained_tombstone_refs: Vec::new(),
                    updated_at: recorded_at,
                    head_digest: String::new(),
                },
                SubjectSoulScopeManifestV1 {
                    schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
                    memory_space_id: owner.memory_space_id.clone(),
                    subject_id: owner.subject_id.clone(),
                    soul_id: owner.soul_id.clone(),
                    generation: 1,
                    manifest_revision: 1,
                    entries: Vec::new(),
                    closure_digest: String::new(),
                },
                1,
            )
        }
        SubjectSoulGenerationLayerBasisV1::Verified { snapshot } => {
            snapshot.validate_contract()?;
            validate_subject_soul_snapshot_owner(owner, snapshot)?;
            match snapshot.head.state {
                SubjectSoulLifecycleStateV1::Unseeded | SubjectSoulLifecycleStateV1::Active => {}
                SubjectSoulLifecycleStateV1::Archived => {
                    return Err(SubjectSoulContractError {
                        key: SubjectSoulLifecycleErrorKey::Archived,
                        reason: "archived Soul cannot accept generation layer mutations"
                            .to_string(),
                    });
                }
                SubjectSoulLifecycleStateV1::Deleted => {
                    return Err(SubjectSoulContractError {
                        key: SubjectSoulLifecycleErrorKey::Deleted,
                        reason: "deleted Soul cannot accept generation layer mutations".to_string(),
                    });
                }
            }
            if recorded_at < snapshot.head.updated_at {
                return Err(SubjectSoulContractError::repair(
                    "generation layer commit time cannot precede the verified head",
                ));
            }
            (
                SubjectSoulExpectedStateV1::Exact {
                    generation: snapshot.head.generation,
                    revision: snapshot.head.current_revision,
                    lifecycle_state: snapshot.head.state,
                    head_digest: snapshot.head.head_digest.clone(),
                    manifest_digest: snapshot.manifest.closure_digest.clone(),
                },
                snapshot.head.clone(),
                snapshot.manifest.clone(),
                next_subject_soul_manifest_revision(snapshot.manifest.manifest_revision)?,
            )
        }
    };
    let intent_digest =
        subject_soul_generation_layer_intent_digest_v1(owner, &expected_state, intent)?;
    let mut seen = BTreeSet::new();
    let mut upsert_documents = Vec::new();
    let mut delete_addresses = Vec::new();
    for mutation in &intent.mutations {
        let (layer, address) = match mutation {
            SubjectSoulGenerationLayerMutationV1::Upsert {
                layer, document, ..
            } => (
                *layer,
                SubjectSoulManifestAddressV1 {
                    namespace: document.namespace.clone(),
                    physical_key: document.physical_key.clone(),
                },
            ),
            SubjectSoulGenerationLayerMutationV1::Delete { layer, address, .. } => {
                (*layer, address.clone())
            }
        };
        address.validate_contract()?;
        if address.namespace != layer.canonical_namespace() {
            return Err(SubjectSoulContractError::repair(
                "generation layer kind does not own the supplied namespace",
            ));
        }
        if !seen.insert((address.namespace.clone(), address.physical_key.clone())) {
            return Err(SubjectSoulContractError::repair(
                "generation layer delta contains a duplicate physical address",
            ));
        }
        let existing_index = post_manifest.entries.iter().position(|entry| {
            entry.namespace == address.namespace && entry.physical_key == address.physical_key
        });
        match mutation {
            SubjectSoulGenerationLayerMutationV1::Upsert {
                layer,
                expected_previous_digest,
                document,
            } => {
                document.validate_contract()?;
                let expected_revision = match post_head.state {
                    SubjectSoulLifecycleStateV1::Unseeded => None,
                    SubjectSoulLifecycleStateV1::Active => post_head.current_revision,
                    SubjectSoulLifecycleStateV1::Archived
                    | SubjectSoulLifecycleStateV1::Deleted => unreachable!("rejected above"),
                };
                if document.memory_space_id != owner.memory_space_id
                    || document.subject_id != owner.subject_id
                    || document.soul_id != owner.soul_id
                    || document.generation != post_head.generation
                    || document.revision != expected_revision
                {
                    return Err(SubjectSoulContractError::repair(
                        "generation layer document owner/generation/revision mismatch",
                    ));
                }
                validate_optional_digest(expected_previous_digest, "expected_previous_digest")?;
                match (existing_index, expected_previous_digest.as_deref()) {
                    (None, None) => {}
                    (Some(index), Some(expected))
                        if post_manifest.entries[index].owner_role == layer.owner_role()
                            && post_manifest.entries[index].content_digest == expected =>
                    {
                        post_manifest.entries.remove(index);
                    }
                    _ => {
                        return Err(SubjectSoulContractError {
                            key: SubjectSoulLifecycleErrorKey::GenerationConflict,
                            reason:
                                "generation layer expected digest does not match manifest membership"
                                    .to_string(),
                        });
                    }
                }
                if expected_previous_digest.as_deref() == Some(&document.content_digest) {
                    post_manifest.entries.push(SubjectSoulScopeManifestEntryV1 {
                        namespace: address.namespace,
                        physical_key: address.physical_key,
                        owner_role: layer.owner_role(),
                        generation: post_head.generation,
                        revision: document.revision,
                        content_digest: document.content_digest.clone(),
                    });
                    continue;
                }
                post_manifest.entries.push(SubjectSoulScopeManifestEntryV1 {
                    namespace: address.namespace,
                    physical_key: address.physical_key,
                    owner_role: layer.owner_role(),
                    generation: post_head.generation,
                    revision: document.revision,
                    content_digest: document.content_digest.clone(),
                });
                upsert_documents.push((**document).clone());
            }
            SubjectSoulGenerationLayerMutationV1::Delete {
                layer,
                expected_content_digest,
                ..
            } => {
                validate_digest(expected_content_digest, "expected_content_digest")?;
                let Some(index) = existing_index else {
                    return Err(SubjectSoulContractError {
                        key: SubjectSoulLifecycleErrorKey::GenerationConflict,
                        reason: "generation layer delete target is absent".to_string(),
                    });
                };
                if post_manifest.entries[index].owner_role != layer.owner_role()
                    || post_manifest.entries[index].content_digest != *expected_content_digest
                {
                    return Err(SubjectSoulContractError {
                        key: SubjectSoulLifecycleErrorKey::GenerationConflict,
                        reason: "generation layer delete does not match exact manifest ownership"
                            .to_string(),
                    });
                }
                post_manifest.entries.remove(index);
                delete_addresses.push(address);
            }
        }
    }
    if upsert_documents.is_empty() && delete_addresses.is_empty() {
        return Ok(SubjectSoulGenerationLayerDeltaPlanV1::NoEffect {
            expected_state,
            intent_digest,
        });
    }
    post_manifest.entries.sort();
    post_manifest.manifest_revision = next_manifest_revision;
    post_manifest.refresh_digest()?;
    post_head.scope_manifest_digest = post_manifest.closure_digest.clone();
    post_head.updated_at = recorded_at;
    post_head.refresh_digest()?;
    match basis {
        SubjectSoulGenerationLayerBasisV1::ImplicitUnseeded { .. } => {
            validate_subject_soul_post_image(&post_head, &post_manifest, None, None, None)?;
        }
        SubjectSoulGenerationLayerBasisV1::Verified { snapshot } => {
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
        }
    }
    upsert_documents.sort_by(|left, right| {
        (&left.namespace, &left.physical_key).cmp(&(&right.namespace, &right.physical_key))
    });
    delete_addresses.sort_by(|left, right| {
        (&left.namespace, &left.physical_key).cmp(&(&right.namespace, &right.physical_key))
    });
    Ok(SubjectSoulGenerationLayerDeltaPlanV1::Commit {
        expected_state,
        intent_digest,
        post_head: Box::new(post_head),
        post_manifest: Box::new(post_manifest),
        upsert_documents,
        delete_addresses,
    })
}

fn validate_subject_soul_autonomous_cycle_intent(
    owner: &SubjectSoulOwnerV1,
    intent: &SubjectSoulAutonomousCycleIntentV1,
) -> SubjectSoulContractResult<()> {
    validate_component(&intent.operation_id, "operation_id")?;
    validate_component(&intent.actor_subject_id, "actor_subject_id")?;
    if intent.actor_subject_id != owner.subject_id {
        return Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
            reason: "autonomous Soul cycle actor must match the mounted Soul owner".to_string(),
        });
    }
    Ok(())
}

fn subject_soul_generation_basis_from_autonomous_basis(
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
) -> SubjectSoulGenerationLayerBasisV1 {
    match basis {
        SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded {
            closure_certificate_digest,
        } => SubjectSoulGenerationLayerBasisV1::ImplicitUnseeded {
            closure_certificate_digest: closure_certificate_digest.clone(),
        },
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } => {
            SubjectSoulGenerationLayerBasisV1::Verified {
                snapshot: snapshot.clone(),
            }
        }
    }
}

fn autonomous_cycle_from_layer_only_plan(
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
    expected_state: SubjectSoulExpectedStateV1,
    intent_digest: String,
    layer_plan: SubjectSoulGenerationLayerDeltaPlanV1,
) -> SubjectSoulContractResult<SubjectSoulAutonomousCyclePlanV1> {
    match layer_plan {
        SubjectSoulGenerationLayerDeltaPlanV1::NoEffect { .. } => {
            Ok(SubjectSoulAutonomousCyclePlanV1::NoEffect {
                expected_state,
                intent_digest,
            })
        }
        SubjectSoulGenerationLayerDeltaPlanV1::Commit {
            post_head,
            post_manifest,
            upsert_documents,
            delete_addresses,
            ..
        } => {
            let (
                current_material,
                current_core,
                current_core_document,
                current_revision_ledger,
                current_revision_ledger_document,
            ) = match basis {
                SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded { .. } => {
                    (None, None, None, None, None)
                }
                SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } => (
                    snapshot.current_material.clone().map(Box::new),
                    snapshot.current_core.clone().map(Box::new),
                    snapshot.current_core_document.clone().map(Box::new),
                    snapshot.current_revision_ledger.clone().map(Box::new),
                    snapshot
                        .current_revision_ledger_document
                        .clone()
                        .map(Box::new),
                ),
            };
            Ok(SubjectSoulAutonomousCyclePlanV1::Commit {
                expected_state,
                intent_digest,
                post_image: Box::new(SubjectSoulAutonomousCyclePostImageV1 {
                    head: post_head,
                    manifest: post_manifest,
                    current_material,
                    current_core,
                    current_core_document,
                    current_revision_ledger,
                    current_revision_ledger_document,
                }),
                revision_delta: Box::new(SubjectSoulAutonomousRevisionDeltaV1::None),
                layer_upserts: upsert_documents,
                layer_deletes: delete_addresses,
            })
        }
    }
}

#[allow(clippy::type_complexity)]
fn autonomous_cycle_from_revision_plan(
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
    revision_plan: SubjectSoulSelfAuthoredCommitPlanV1,
) -> SubjectSoulContractResult<(
    SubjectSoulAutonomousCyclePostImageV1,
    SubjectSoulAutonomousRevisionDeltaV1,
    SubjectSoulLifecycleHeadV1,
    SubjectSoulScopeManifestV1,
)> {
    match revision_plan {
        SubjectSoulSelfAuthoredCommitPlanV1::NoEffect => Err(SubjectSoulContractError::repair(
            "non-skipped autonomous refresh unexpectedly produced no revision effect",
        )),
        SubjectSoulSelfAuthoredCommitPlanV1::ReviewedRejected {
            post_head,
            post_manifest,
            revision_ledger,
            revision_ledger_document,
            purge_manifest_addresses,
            ..
        } => {
            let SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } = basis else {
                return Err(SubjectSoulContractError::repair(
                    "reviewed rejection requires a verified active Soul basis",
                ));
            };
            let head = *post_head;
            let manifest = *post_manifest;
            let post_image = SubjectSoulAutonomousCyclePostImageV1 {
                head: Box::new(head.clone()),
                manifest: Box::new(manifest.clone()),
                current_material: snapshot.current_material.clone().map(Box::new),
                current_core: snapshot.current_core.clone().map(Box::new),
                current_core_document: snapshot.current_core_document.clone().map(Box::new),
                current_revision_ledger: Some(revision_ledger),
                current_revision_ledger_document: Some(revision_ledger_document.clone()),
            };
            Ok((
                post_image,
                SubjectSoulAutonomousRevisionDeltaV1::ReviewedRejected {
                    revision_ledger_document,
                    purge_manifest_addresses,
                },
                head,
                manifest,
            ))
        }
        SubjectSoulSelfAuthoredCommitPlanV1::Adopt {
            post_head,
            post_manifest,
            material,
            core,
            core_document,
            revision_ledger,
            revision_ledger_document,
            purge_manifest_addresses,
            ..
        } => {
            let head = *post_head;
            let manifest = *post_manifest;
            let post_image = SubjectSoulAutonomousCyclePostImageV1 {
                head: Box::new(head.clone()),
                manifest: Box::new(manifest.clone()),
                current_material: Some(material.clone()),
                current_core: Some(core),
                current_core_document: Some(core_document.clone()),
                current_revision_ledger: Some(revision_ledger),
                current_revision_ledger_document: Some(revision_ledger_document.clone()),
            };
            Ok((
                post_image,
                SubjectSoulAutonomousRevisionDeltaV1::Adopt {
                    material,
                    core_document,
                    revision_ledger_document,
                    purge_manifest_addresses,
                },
                head,
                manifest,
            ))
        }
    }
}

fn autonomous_cycle_input_binding(
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
) -> (u64, Option<u64>) {
    match basis {
        SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded { .. } => (1, None),
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } => {
            (snapshot.head.generation, snapshot.head.current_revision)
        }
    }
}

fn apply_autonomous_cycle_layer_mutations(
    owner: &SubjectSoulOwnerV1,
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
    post_head: &mut SubjectSoulLifecycleHeadV1,
    post_manifest: &mut SubjectSoulScopeManifestV1,
    mutations: &[SubjectSoulGenerationLayerMutationV1],
) -> SubjectSoulContractResult<(
    Vec<SubjectSoulOwnedDocumentV1>,
    Vec<SubjectSoulManifestAddressV1>,
)> {
    let (input_generation, input_revision) = autonomous_cycle_input_binding(basis);
    let final_revision = match post_head.state {
        SubjectSoulLifecycleStateV1::Unseeded => None,
        SubjectSoulLifecycleStateV1::Active => post_head.current_revision,
        SubjectSoulLifecycleStateV1::Archived => {
            return Err(SubjectSoulContractError {
                key: SubjectSoulLifecycleErrorKey::Archived,
                reason: "archived Soul cannot accept autonomous layer mutations".to_string(),
            })
        }
        SubjectSoulLifecycleStateV1::Deleted => {
            return Err(SubjectSoulContractError {
                key: SubjectSoulLifecycleErrorKey::Deleted,
                reason: "deleted Soul cannot accept autonomous layer mutations".to_string(),
            })
        }
    };
    let mut seen = BTreeSet::new();
    let mut upserts = Vec::new();
    let mut deletes = Vec::new();
    for mutation in mutations {
        let (layer, address) = match mutation {
            SubjectSoulGenerationLayerMutationV1::Upsert {
                layer, document, ..
            } => (
                *layer,
                SubjectSoulManifestAddressV1 {
                    namespace: document.namespace.clone(),
                    physical_key: document.physical_key.clone(),
                },
            ),
            SubjectSoulGenerationLayerMutationV1::Delete { layer, address, .. } => {
                (*layer, address.clone())
            }
        };
        address.validate_contract()?;
        if address.namespace != layer.canonical_namespace() {
            return Err(SubjectSoulContractError::repair(
                "autonomous layer kind does not own the supplied namespace",
            ));
        }
        if !seen.insert((address.namespace.clone(), address.physical_key.clone())) {
            return Err(SubjectSoulContractError::repair(
                "autonomous cycle contains a duplicate layer address",
            ));
        }
        let existing_index = post_manifest.entries.iter().position(|entry| {
            entry.namespace == address.namespace && entry.physical_key == address.physical_key
        });
        match mutation {
            SubjectSoulGenerationLayerMutationV1::Upsert {
                expected_previous_digest,
                document,
                ..
            } => {
                document.validate_contract()?;
                if document.memory_space_id != owner.memory_space_id
                    || document.subject_id != owner.subject_id
                    || document.soul_id != owner.soul_id
                    || document.generation != input_generation
                    || document.revision != input_revision
                {
                    return Err(SubjectSoulContractError::repair(
                        "autonomous layer input owner/generation/revision mismatch",
                    ));
                }
                validate_optional_digest(expected_previous_digest, "expected_previous_digest")?;
                match (existing_index, expected_previous_digest.as_deref()) {
                    (None, None) => {}
                    (Some(index), Some(expected))
                        if post_manifest.entries[index].owner_role == layer.owner_role()
                            && post_manifest.entries[index].content_digest == expected =>
                    {
                        post_manifest.entries.remove(index);
                    }
                    _ => {
                        return Err(SubjectSoulContractError {
                            key: SubjectSoulLifecycleErrorKey::GenerationConflict,
                            reason: "autonomous layer expected digest does not match the exact manifest entry"
                                .to_string(),
                        })
                    }
                }
                let rebound = SubjectSoulOwnedDocumentV1::new(
                    owner,
                    post_head.generation,
                    final_revision,
                    &address,
                    &document.body,
                )?;
                let unchanged =
                    expected_previous_digest.as_deref() == Some(rebound.content_digest.as_str());
                post_manifest.entries.push(SubjectSoulScopeManifestEntryV1 {
                    namespace: address.namespace,
                    physical_key: address.physical_key,
                    owner_role: layer.owner_role(),
                    generation: post_head.generation,
                    revision: rebound.revision,
                    content_digest: rebound.content_digest.clone(),
                });
                if !unchanged {
                    upserts.push(rebound);
                }
            }
            SubjectSoulGenerationLayerMutationV1::Delete {
                expected_content_digest,
                ..
            } => {
                validate_digest(expected_content_digest, "expected_content_digest")?;
                let Some(index) = existing_index else {
                    return Err(SubjectSoulContractError {
                        key: SubjectSoulLifecycleErrorKey::GenerationConflict,
                        reason: "autonomous layer delete target is absent".to_string(),
                    });
                };
                if post_manifest.entries[index].owner_role != layer.owner_role()
                    || post_manifest.entries[index].content_digest != *expected_content_digest
                {
                    return Err(SubjectSoulContractError {
                        key: SubjectSoulLifecycleErrorKey::GenerationConflict,
                        reason: "autonomous layer delete does not match exact manifest ownership"
                            .to_string(),
                    });
                }
                post_manifest.entries.remove(index);
                deletes.push(address);
            }
        }
    }
    if !mutations.is_empty() {
        post_manifest.entries.sort();
        post_manifest.refresh_digest()?;
        post_head.scope_manifest_digest = post_manifest.closure_digest.clone();
        post_head.refresh_digest()?;
    }
    upserts.sort_by(|left, right| {
        (&left.namespace, &left.physical_key).cmp(&(&right.namespace, &right.physical_key))
    });
    deletes.sort_by(|left, right| {
        (&left.namespace, &left.physical_key).cmp(&(&right.namespace, &right.physical_key))
    });
    Ok((upserts, deletes))
}

fn validate_generation_layer_intent(
    owner: &SubjectSoulOwnerV1,
    intent: &SubjectSoulGenerationLayerIntentV1,
) -> SubjectSoulContractResult<()> {
    owner.validate_contract()?;
    validate_component(&intent.operation_id, "operation_id")?;
    validate_component(intent.authority.actor_subject_id(), "actor_subject_id")?;
    match &intent.authority {
        SubjectSoulGenerationLayerAuthorityV1::MountedAgentPersona { actor_subject_id }
            if actor_subject_id != &owner.subject_id =>
        {
            return Err(SubjectSoulContractError {
                key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
                reason: "mounted AgentPersona layer authority must match the Soul owner"
                    .to_string(),
            });
        }
        SubjectSoulGenerationLayerAuthorityV1::SystemGovernor { actor_subject_id }
            if actor_subject_id == &owner.subject_id =>
        {
            return Err(SubjectSoulContractError {
                key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
                reason: "SystemGovernor layer authority cannot impersonate the Soul owner"
                    .to_string(),
            });
        }
        _ => {}
    }
    if intent.mutations.is_empty() {
        return Err(SubjectSoulContractError::repair(
            "generation layer intent requires at least one mutation",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "selector", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulReadSelectorV1 {
    Current,
    Exact {
        generation: u64,
        revision: u64,
        material_digest: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulReadViewV1 {
    OperatorSafe,
    RuntimePrivate,
    GovernedDisclosure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulReadRequestV1 {
    pub target_subject_id: String,
    pub selector: SubjectSoulReadSelectorV1,
    pub view: SubjectSoulReadViewV1,
}

impl SubjectSoulReadRequestV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_component(&self.target_subject_id, "target_subject_id")?;
        if let SubjectSoulReadSelectorV1::Exact {
            generation,
            revision,
            material_digest,
        } = &self.selector
        {
            if *generation == 0 || *revision == 0 {
                return Err(SubjectSoulContractError::repair(
                    "exact Soul selector requires positive generation/revision",
                ));
            }
            validate_digest(material_digest, "material_digest")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedSubjectSoulReadViewV1 {
    pub memory_space_id: String,
    pub subject_id: String,
    pub soul_id: String,
    pub state: SubjectSoulLifecycleStateV1,
    pub generation: u64,
    pub revision: Option<u64>,
    pub material_digest: Option<String>,
    pub origin: Option<SubjectSoulRevisionOriginV1>,
    pub requested_view: SubjectSoulReadViewV1,
    pub runtime_private_core: Option<SelfAuthoredCore>,
    pub governed_disclosure: Option<String>,
    pub head_digest: String,
    pub manifest_digest: String,
}

impl VerifiedSubjectSoulReadViewV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_component(&self.memory_space_id, "memory_space_id")?;
        validate_component(&self.subject_id, "subject_id")?;
        validate_component(&self.soul_id, "soul_id")?;
        if self.generation == 0 {
            return Err(SubjectSoulContractError::repair(
                "verified Soul view generation must be positive",
            ));
        }
        validate_digest(&self.head_digest, "head_digest")?;
        validate_digest(&self.manifest_digest, "manifest_digest")?;
        match self.state {
            SubjectSoulLifecycleStateV1::Active | SubjectSoulLifecycleStateV1::Archived => {
                if self.revision.is_none()
                    || self.material_digest.is_none()
                    || self.origin.is_none()
                {
                    return Err(SubjectSoulContractError::repair(
                        "active/archived view requires verified revision metadata",
                    ));
                }
            }
            SubjectSoulLifecycleStateV1::Unseeded | SubjectSoulLifecycleStateV1::Deleted => {
                if self.revision.is_some()
                    || self.material_digest.is_some()
                    || self.origin.is_some()
                    || self.runtime_private_core.is_some()
                    || self.governed_disclosure.is_some()
                {
                    return Err(SubjectSoulContractError::repair(
                        "unseeded/deleted view cannot expose Soul material",
                    ));
                }
            }
        }
        validate_optional_digest(&self.material_digest, "material_digest")?;
        match self.requested_view {
            SubjectSoulReadViewV1::OperatorSafe => {
                if self.runtime_private_core.is_some() || self.governed_disclosure.is_some() {
                    return Err(SubjectSoulContractError::repair(
                        "operator-safe view cannot carry Soul content",
                    ));
                }
            }
            SubjectSoulReadViewV1::RuntimePrivate => {
                if self.state == SubjectSoulLifecycleStateV1::Active
                    && self.runtime_private_core.is_none()
                {
                    return Err(SubjectSoulContractError::repair(
                        "active runtime-private view requires exact Core material",
                    ));
                }
                if self.governed_disclosure.is_some() {
                    return Err(SubjectSoulContractError::repair(
                        "runtime-private view cannot masquerade as disclosure",
                    ));
                }
            }
            SubjectSoulReadViewV1::GovernedDisclosure => {
                if self.runtime_private_core.is_some() {
                    return Err(SubjectSoulContractError::repair(
                        "governed disclosure cannot carry raw Core material",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulReadOutcomeV1 {
    ImplicitUnseeded {
        memory_space_id: String,
        subject_id: String,
        soul_id: String,
        generation: u64,
        closure_certificate_digest: String,
    },
    Verified {
        view: Box<VerifiedSubjectSoulReadViewV1>,
    },
    TerminatedGeneration {
        memory_space_id: String,
        subject_id: String,
        soul_id: String,
        terminal: Box<SubjectSoulTerminatedGenerationV1>,
    },
}

impl SubjectSoulReadOutcomeV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        match self {
            Self::ImplicitUnseeded {
                memory_space_id,
                subject_id,
                soul_id,
                generation,
                closure_certificate_digest,
            } => {
                validate_component(memory_space_id, "memory_space_id")?;
                validate_component(subject_id, "subject_id")?;
                validate_component(soul_id, "soul_id")?;
                if *generation != 1 {
                    return Err(SubjectSoulContractError::repair(
                        "implicit unseeded is exactly generation one",
                    ));
                }
                validate_digest(closure_certificate_digest, "closure_certificate_digest")
            }
            Self::Verified { view } => view.validate_contract(),
            Self::TerminatedGeneration {
                memory_space_id,
                subject_id,
                soul_id,
                terminal,
            } => {
                validate_component(memory_space_id, "memory_space_id")?;
                validate_component(subject_id, "subject_id")?;
                validate_component(soul_id, "soul_id")?;
                terminal.validate_contract()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulTerminatedGenerationV1 {
    pub generation: u64,
    pub terminal_revision: Option<u64>,
    pub terminal_material_digest: Option<String>,
    pub terminal_action: SubjectSoulTerminalActionV1,
    pub tombstone_digest: String,
    pub terminated_at: u64,
    pub current_generation: u64,
    pub current_state: SubjectSoulLifecycleStateV1,
}

impl SubjectSoulTerminatedGenerationV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        if self.generation == 0 || self.current_generation == 0 || self.terminated_at == 0 {
            return Err(SubjectSoulContractError::repair(
                "terminated generation metadata requires positive generation/time",
            ));
        }
        match (
            self.terminal_revision,
            self.terminal_material_digest.as_ref(),
        ) {
            (Some(revision), Some(digest)) if revision > 0 => {
                validate_digest(digest, "terminal_material_digest")?;
            }
            (None, None) => {}
            _ => {
                return Err(SubjectSoulContractError::repair(
                    "terminated revision/material metadata must be present together",
                ));
            }
        }
        match self.terminal_action {
            SubjectSoulTerminalActionV1::Reset | SubjectSoulTerminalActionV1::Reseed
                if self.current_generation <= self.generation =>
            {
                return Err(SubjectSoulContractError::repair(
                    "reset/reseed terminal metadata must precede the current generation",
                ));
            }
            SubjectSoulTerminalActionV1::Delete
                if self.current_generation != self.generation
                    || self.current_state != SubjectSoulLifecycleStateV1::Deleted =>
            {
                return Err(SubjectSoulContractError::repair(
                    "delete terminal metadata must bind the current deleted generation",
                ));
            }
            _ => {}
        }
        validate_digest(&self.tombstone_digest, "tombstone_digest")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulOperatorSafeExportV1 {
    pub subject_id: String,
    pub soul_id: String,
    pub state: SubjectSoulLifecycleStateV1,
    pub generation: u64,
    pub revision: Option<u64>,
    pub material_digest: Option<String>,
    pub origin: Option<SubjectSoulRevisionOriginV1>,
    pub terminated_generations: Vec<SubjectSoulTerminatedGenerationV1>,
}

impl SubjectSoulOperatorSafeExportV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_component(&self.subject_id, "subject_id")?;
        validate_component(&self.soul_id, "soul_id")?;
        if self.generation == 0 {
            return Err(SubjectSoulContractError::repair(
                "operator-safe export generation must be positive",
            ));
        }
        match self.state {
            SubjectSoulLifecycleStateV1::Active | SubjectSoulLifecycleStateV1::Archived => {
                if self.revision.is_none()
                    || self.material_digest.is_none()
                    || self.origin.is_none()
                {
                    return Err(SubjectSoulContractError::repair(
                        "active/archived safe export requires revision metadata",
                    ));
                }
            }
            SubjectSoulLifecycleStateV1::Unseeded | SubjectSoulLifecycleStateV1::Deleted => {
                if self.revision.is_some()
                    || self.material_digest.is_some()
                    || self.origin.is_some()
                {
                    return Err(SubjectSoulContractError::repair(
                        "unseeded/deleted safe export cannot claim current material",
                    ));
                }
            }
        }
        validate_optional_digest(&self.material_digest, "material_digest")?;
        if self
            .terminated_generations
            .windows(2)
            .any(|window| window[0].generation >= window[1].generation)
        {
            return Err(SubjectSoulContractError::repair(
                "terminated generations must be sorted and unique",
            ));
        }
        for terminal in &self.terminated_generations {
            terminal.validate_contract()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulMutationOutcomeV1 {
    UnseededNoEffect,
    Committed,
    Replayed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulMutationReportV1 {
    pub outcome: SubjectSoulMutationOutcomeV1,
    pub state_before: SubjectSoulLifecycleStateV1,
    pub state_after: SubjectSoulLifecycleStateV1,
    pub generation: u64,
    pub revision: Option<u64>,
    pub head_digest: Option<String>,
    pub transaction_id: Option<String>,
    pub durable_receipt_ref: Option<String>,
    pub replayed: bool,
    pub safe_event_ref: Option<String>,
}

impl SubjectSoulMutationReportV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        if self.generation == 0 {
            return Err(SubjectSoulContractError::repair(
                "report generation must be positive",
            ));
        }
        match self.outcome {
            SubjectSoulMutationOutcomeV1::UnseededNoEffect => {
                if self.state_before != SubjectSoulLifecycleStateV1::Unseeded
                    || self.state_after != SubjectSoulLifecycleStateV1::Unseeded
                    || self.revision.is_some()
                    || self.head_digest.is_some()
                    || self.transaction_id.is_some()
                    || self.durable_receipt_ref.is_some()
                    || self.safe_event_ref.is_some()
                    || self.replayed
                {
                    return Err(SubjectSoulContractError::repair(
                        "unseeded no-effect report cannot claim durable effects",
                    ));
                }
            }
            SubjectSoulMutationOutcomeV1::Committed => {
                if self.transaction_id.is_none()
                    || self.durable_receipt_ref.is_none()
                    || self.safe_event_ref.is_none()
                    || self.head_digest.is_none()
                    || self.replayed
                {
                    return Err(SubjectSoulContractError::repair(
                        "committed report requires transaction, receipt, and event refs",
                    ));
                }
            }
            SubjectSoulMutationOutcomeV1::Replayed => {
                if self.transaction_id.is_none()
                    || self.durable_receipt_ref.is_none()
                    || self.safe_event_ref.is_none()
                    || self.head_digest.is_none()
                    || !self.replayed
                {
                    return Err(SubjectSoulContractError::repair(
                        "replayed report must reference the durable committed effect",
                    ));
                }
            }
        }
        validate_optional_digest(&self.head_digest, "head_digest")?;
        validate_optional_component(&self.transaction_id, "transaction_id")?;
        validate_optional_component(&self.durable_receipt_ref, "durable_receipt_ref")?;
        validate_optional_component(&self.safe_event_ref, "safe_event_ref")?;
        match self.state_after {
            SubjectSoulLifecycleStateV1::Active | SubjectSoulLifecycleStateV1::Archived
                if self.revision.is_none() =>
            {
                return Err(SubjectSoulContractError::repair(
                    "active/archived report requires a verified revision",
                ));
            }
            SubjectSoulLifecycleStateV1::Unseeded | SubjectSoulLifecycleStateV1::Deleted
                if self.revision.is_some() =>
            {
                return Err(SubjectSoulContractError::repair(
                    "unseeded/deleted report cannot claim a current revision",
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn validate_subject_soul_post_image(
    head: &SubjectSoulLifecycleHeadV1,
    manifest: &SubjectSoulScopeManifestV1,
    material: Option<&SubjectSoulRevisionMaterialV1>,
    current_core: Option<&SelfAuthoredCore>,
    current_ledger_digest: Option<&str>,
) -> SubjectSoulContractResult<()> {
    head.validate_contract()?;
    manifest.validate_contract()?;
    if head.memory_space_id != manifest.memory_space_id
        || head.subject_id != manifest.subject_id
        || head.soul_id != manifest.soul_id
        || head.generation != manifest.generation
        || head.scope_manifest_digest != manifest.closure_digest
    {
        return Err(SubjectSoulContractError::repair(
            "head and manifest owner/generation mismatch",
        ));
    }
    match head.state {
        SubjectSoulLifecycleStateV1::Unseeded | SubjectSoulLifecycleStateV1::Deleted => {
            if material.is_some() || current_core.is_some() || current_ledger_digest.is_some() {
                return Err(SubjectSoulContractError::repair(
                    "unseeded/deleted post-image cannot retain current material",
                ));
            }
        }
        SubjectSoulLifecycleStateV1::Active | SubjectSoulLifecycleStateV1::Archived => {
            let material = material.ok_or_else(|| {
                SubjectSoulContractError::repair("active post-image is missing material")
            })?;
            let current_core = current_core.ok_or_else(|| {
                SubjectSoulContractError::repair("active post-image is missing current core")
            })?;
            material.validate_contract()?;
            let ledger_digest = current_ledger_digest.ok_or_else(|| {
                SubjectSoulContractError::repair("active post-image is missing ledger digest")
            })?;
            validate_digest(ledger_digest, "current_ledger_digest")?;
            if material.memory_space_id != head.memory_space_id
                || material.subject_id != head.subject_id
                || material.soul_id != head.soul_id
                || material.generation != head.generation
                || Some(material.revision) != head.current_revision
                || Some(&material.content_digest) != head.current_material_digest.as_ref()
                || current_core != &material.core
                || current_ledger_digest != head.current_ledger_digest.as_deref()
            {
                return Err(SubjectSoulContractError::repair(
                    "head/material/current Core post-image is not exact",
                ));
            }
            let material_is_owned = manifest.entries.iter().any(|entry| {
                entry.owner_role == SubjectSoulManifestOwnerRoleV1::SubjectGlobal
                    && entry.revision == Some(material.revision)
                    && entry.content_digest == material.content_digest
            });
            let ledger_is_owned = manifest.entries.iter().any(|entry| {
                entry.owner_role == SubjectSoulManifestOwnerRoleV1::SubjectGlobal
                    && entry.revision == Some(material.revision)
                    && entry.content_digest == ledger_digest
            });
            if !material_is_owned || !ledger_is_owned {
                return Err(SubjectSoulContractError::repair(
                    "manifest does not own exact current material and ledger closure",
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_relationship_source_post_image(
    source: &RelationshipSourceConstitutionV1,
    manifest: &RelationshipSourceScopeManifestV1,
) -> SubjectSoulContractResult<()> {
    source.validate_contract()?;
    manifest.validate_contract()?;
    if source.memory_space_id != manifest.memory_space_id
        || source.relationship_id != manifest.relationship_id
        || source.revision != manifest.current_revision
        || source.content_digest != manifest.current_digest
    {
        return Err(SubjectSoulContractError::repair(
            "relationship source and manifest post-image is not exact",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipDisclosureCeilingV1 {
    None,
    RefusalOnly,
    GovernedSummary,
    FullGovernedDisclosure,
}

impl RelationshipDisclosureCeilingV1 {
    pub fn most_restrictive(self, other: Self) -> Self {
        self.min(other)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipSourceClausesV1 {
    pub disclosure_ceiling: RelationshipDisclosureCeilingV1,
    pub access_constraints: Vec<RelationshipAccessConstraintV1>,
    pub truth_commitments: Vec<String>,
    pub mutual_boundary_commitments: Vec<String>,
    pub repair_commitments: Vec<String>,
}

impl RelationshipSourceClausesV1 {
    pub fn validate_canonical(&self) -> SubjectSoulContractResult<()> {
        if self
            .access_constraints
            .windows(2)
            .any(|window| window[0] >= window[1])
        {
            return Err(SubjectSoulContractError::invalid(
                "access_constraints must be sorted and unique",
            ));
        }
        validate_sorted_unique_components(&self.truth_commitments, "truth_commitments")?;
        validate_sorted_unique_components(
            &self.mutual_boundary_commitments,
            "mutual_boundary_commitments",
        )?;
        validate_sorted_unique_components(&self.repair_commitments, "repair_commitments")
    }

    pub fn most_restrictive_merge(&self, other: &Self) -> Self {
        Self {
            disclosure_ceiling: self
                .disclosure_ceiling
                .most_restrictive(other.disclosure_ceiling),
            access_constraints: union_sorted(&self.access_constraints, &other.access_constraints),
            truth_commitments: union_sorted(&self.truth_commitments, &other.truth_commitments),
            mutual_boundary_commitments: union_sorted(
                &self.mutual_boundary_commitments,
                &other.mutual_boundary_commitments,
            ),
            repair_commitments: union_sorted(&self.repair_commitments, &other.repair_commitments),
        }
    }

    pub fn tightens_or_equals(&self, previous: &Self) -> bool {
        self.disclosure_ceiling <= previous.disclosure_ceiling
            && is_ordered_superset(&self.access_constraints, &previous.access_constraints)
            && is_ordered_superset(&self.truth_commitments, &previous.truth_commitments)
            && is_ordered_superset(
                &self.mutual_boundary_commitments,
                &previous.mutual_boundary_commitments,
            )
            && is_ordered_superset(&self.repair_commitments, &previous.repair_commitments)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipAccessConstraintV1 {
    NoPrivateRaw,
    NoHistoricalRaw,
    NoToolAuthority,
    NoIdentityMutation,
    NoThirdPartyDisclosure,
    GovernedDisclosureOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipSourceAuthorityKindV1 {
    HumanRelationshipCommitment,
    SubjectSelfBoundary,
    SystemPolicyFloor,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipSourceProvenanceV1 {
    pub source: RelationshipSourceAuthorityKindV1,
    pub source_subject_id: String,
    pub source_asserted_at: Option<u64>,
    pub recorded_at: u64,
    pub evidence_digest: String,
}

impl RelationshipSourceProvenanceV1 {
    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_component(&self.source_subject_id, "provenance.source_subject_id")?;
        validate_digest(&self.evidence_digest, "evidence_digest")?;
        if self.recorded_at == 0
            || self
                .source_asserted_at
                .is_some_and(|value| value > self.recorded_at)
        {
            return Err(SubjectSoulContractError::repair(
                "relationship provenance time binding is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipSourceContributionV1 {
    pub contributor_subject_id: String,
    pub clauses: RelationshipSourceClausesV1,
    pub provenance: RelationshipSourceProvenanceV1,
    pub contribution_digest: String,
}

impl RelationshipSourceContributionV1 {
    pub fn refresh_digest(&mut self) -> SubjectSoulContractResult<()> {
        self.contribution_digest.clear();
        self.contribution_digest = canonical_digest("relationship_source_contribution_v1", self)?;
        Ok(())
    }

    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        validate_component(&self.contributor_subject_id, "contributor_subject_id")?;
        if self.provenance.source_subject_id != self.contributor_subject_id {
            return Err(SubjectSoulContractError::repair(
                "relationship contribution provenance actor mismatch",
            ));
        }
        self.clauses.validate_canonical()?;
        self.provenance.validate_contract()?;
        validate_digest(&self.contribution_digest, "contribution_digest")?;
        let mut canonical = self.clone();
        canonical.contribution_digest.clear();
        if canonical_digest("relationship_source_contribution_v1", &canonical)?
            != self.contribution_digest
        {
            return Err(SubjectSoulContractError::repair(
                "relationship contribution digest mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipSourceStateV1 {
    Active,
    Archived,
    Terminated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipSourceConstitutionV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub relationship_id: String,
    pub mounted_subject_id: String,
    pub counterparty_subject_ids: Vec<String>,
    pub revision: u64,
    pub supersedes_revision: Option<u64>,
    pub state: RelationshipSourceStateV1,
    pub clauses: RelationshipSourceClausesV1,
    pub contributions: Vec<RelationshipSourceContributionV1>,
    pub content_digest: String,
}

impl RelationshipSourceConstitutionV1 {
    pub fn refresh_digest(&mut self) -> SubjectSoulContractResult<()> {
        self.content_digest.clear();
        self.content_digest = canonical_digest("relationship_source_constitution_v1", self)?;
        Ok(())
    }

    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        if self.schema_version != SUBJECT_SOUL_SCHEMA_VERSION || self.revision == 0 {
            return Err(SubjectSoulContractError::repair(
                "relationship source schema/revision is invalid",
            ));
        }
        if (self.revision == 1 && self.supersedes_revision.is_some())
            || (self.revision > 1 && self.supersedes_revision != self.revision.checked_sub(1))
        {
            return Err(SubjectSoulContractError::repair(
                "relationship source revision lineage is not exact",
            ));
        }
        validate_component(&self.memory_space_id, "memory_space_id")?;
        validate_component(&self.relationship_id, "relationship_id")?;
        validate_component(&self.mounted_subject_id, "mounted_subject_id")?;
        validate_sorted_unique_components(
            &self.counterparty_subject_ids,
            "counterparty_subject_ids",
        )?;
        if self.counterparty_subject_ids.is_empty()
            || self
                .counterparty_subject_ids
                .iter()
                .any(|subject_id| subject_id == &self.mounted_subject_id)
        {
            return Err(SubjectSoulContractError::repair(
                "relationship source requires distinct canonical counterparties",
            ));
        }
        self.clauses.validate_canonical()?;
        if self.contributions.is_empty() {
            return Err(SubjectSoulContractError::repair(
                "relationship source requires typed contributions",
            ));
        }
        let mut previous_key: Option<(RelationshipSourceAuthorityKindV1, &str)> = None;
        let mut aggregate: Option<RelationshipSourceClausesV1> = None;
        for contribution in &self.contributions {
            contribution.validate_contract()?;
            let key = (
                contribution.provenance.source,
                contribution.contributor_subject_id.as_str(),
            );
            if previous_key.is_some_and(|previous| previous >= key) {
                return Err(SubjectSoulContractError::repair(
                    "relationship contributions must be sorted and unique by authority and actor",
                ));
            }
            match contribution.provenance.source {
                RelationshipSourceAuthorityKindV1::HumanRelationshipCommitment => {
                    if contribution.contributor_subject_id == self.mounted_subject_id
                        || !self
                            .counterparty_subject_ids
                            .contains(&contribution.contributor_subject_id)
                    {
                        return Err(SubjectSoulContractError::repair(
                            "human relationship contribution must belong to a counterparty",
                        ));
                    }
                }
                RelationshipSourceAuthorityKindV1::SubjectSelfBoundary => {
                    if contribution.contributor_subject_id != self.mounted_subject_id {
                        return Err(SubjectSoulContractError::repair(
                            "subject self-boundary must belong to the mounted subject",
                        ));
                    }
                }
                RelationshipSourceAuthorityKindV1::SystemPolicyFloor => {
                    if contribution.contributor_subject_id == self.mounted_subject_id
                        || self
                            .counterparty_subject_ids
                            .contains(&contribution.contributor_subject_id)
                    {
                        return Err(SubjectSoulContractError::repair(
                            "system policy floor cannot be authored by a relationship member",
                        ));
                    }
                }
            }
            aggregate = Some(match aggregate {
                Some(current) => current.most_restrictive_merge(&contribution.clauses),
                None => contribution.clauses.clone(),
            });
            previous_key = Some(key);
        }
        if aggregate.as_ref() != Some(&self.clauses) {
            return Err(SubjectSoulContractError::repair(
                "relationship clauses do not equal the deny-biased contribution aggregate",
            ));
        }
        validate_digest(&self.content_digest, "content_digest")?;
        let mut canonical = self.clone();
        canonical.content_digest.clear();
        if canonical_digest("relationship_source_constitution_v1", &canonical)?
            != self.content_digest
        {
            return Err(SubjectSoulContractError::repair(
                "relationship source digest mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipSourceScopeManifestV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub relationship_id: String,
    pub current_revision: u64,
    pub current_digest: String,
    pub retained_revision_refs: Vec<String>,
    pub closure_digest: String,
}

impl RelationshipSourceScopeManifestV1 {
    pub fn refresh_digest(&mut self) -> SubjectSoulContractResult<()> {
        self.closure_digest.clear();
        self.closure_digest = canonical_digest("relationship_source_manifest_v1", self)?;
        Ok(())
    }

    pub fn validate_contract(&self) -> SubjectSoulContractResult<()> {
        if self.schema_version != SUBJECT_SOUL_SCHEMA_VERSION || self.current_revision == 0 {
            return Err(SubjectSoulContractError::repair(
                "relationship manifest schema/revision is invalid",
            ));
        }
        validate_component(&self.memory_space_id, "memory_space_id")?;
        validate_component(&self.relationship_id, "relationship_id")?;
        validate_digest(&self.current_digest, "current_digest")?;
        validate_sorted_unique_components(&self.retained_revision_refs, "retained_revision_refs")?;
        validate_digest(&self.closure_digest, "closure_digest")?;
        let mut canonical = self.clone();
        canonical.closure_digest.clear();
        if canonical_digest("relationship_source_manifest_v1", &canonical)? != self.closure_digest {
            return Err(SubjectSoulContractError::repair(
                "relationship manifest digest mismatch",
            ));
        }
        Ok(())
    }
}

pub fn canonical_relationship_source_revision_ref_v1(
    memory_space_id: &str,
    relationship_id: &str,
    revision: u64,
) -> RelationshipSourceControlResultV1<String> {
    relationship_component(memory_space_id, "memory_space_id")?;
    relationship_component(relationship_id, "relationship_id")?;
    if revision == 0 {
        return Err(RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::RevisionConflict,
            "relationship source revision reference must be positive",
        ));
    }
    let mut hasher = Sha256::new();
    hash_relationship_ref_field(&mut hasher, b"relationship-source-revision");
    hash_relationship_ref_field(&mut hasher, memory_space_id.as_bytes());
    hash_relationship_ref_field(&mut hasher, relationship_id.as_bytes());
    hash_relationship_ref_field(&mut hasher, revision.to_string().as_bytes());
    Ok(format!("scope:sha256:{:x}", hasher.finalize()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipSourceControlErrorKeyV1 {
    TargetMismatch,
    MembershipMismatch,
    AuthorityDenied,
    InvalidClauseMutation,
    RevisionConflict,
    Archived,
    Terminated,
    OperationConflict,
    CapacityExceeded,
    RepairRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{key:?}: {reason}")]
pub struct RelationshipSourceControlContractErrorV1 {
    pub key: RelationshipSourceControlErrorKeyV1,
    pub reason: String,
}

impl RelationshipSourceControlContractErrorV1 {
    fn new(key: RelationshipSourceControlErrorKeyV1, reason: impl Into<String>) -> Self {
        Self {
            key,
            reason: reason.into(),
        }
    }
}

pub type RelationshipSourceControlResultV1<T> = Result<T, RelationshipSourceControlContractErrorV1>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "authority", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelationshipSourceControlAuthorityV1 {
    HumanUser {
        actor_subject_id: String,
    },
    MountedAgentPersona {
        actor_subject_id: String,
        self_governance_capability_digest: String,
    },
    SystemGovernor {
        actor_subject_id: String,
        policy_capability_digest: String,
    },
}

impl RelationshipSourceControlAuthorityV1 {
    pub fn actor_subject_id(&self) -> &str {
        match self {
            Self::HumanUser { actor_subject_id }
            | Self::MountedAgentPersona {
                actor_subject_id, ..
            }
            | Self::SystemGovernor {
                actor_subject_id, ..
            } => actor_subject_id,
        }
    }

    pub fn validate_contract(&self) -> RelationshipSourceControlResultV1<()> {
        relationship_component(self.actor_subject_id(), "actor_subject_id")?;
        match self {
            Self::HumanUser { .. } => Ok(()),
            Self::MountedAgentPersona {
                self_governance_capability_digest,
                ..
            } => relationship_digest(
                self_governance_capability_digest,
                "self_governance_capability_digest",
            ),
            Self::SystemGovernor {
                policy_capability_digest,
                ..
            } => relationship_digest(policy_capability_digest, "policy_capability_digest"),
        }
    }

    fn source_kind(&self) -> RelationshipSourceAuthorityKindV1 {
        match self {
            Self::HumanUser { .. } => {
                RelationshipSourceAuthorityKindV1::HumanRelationshipCommitment
            }
            Self::MountedAgentPersona { .. } => {
                RelationshipSourceAuthorityKindV1::SubjectSelfBoundary
            }
            Self::SystemGovernor { .. } => RelationshipSourceAuthorityKindV1::SystemPolicyFloor,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "expected", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelationshipSourceExpectedStateV1 {
    PristineAbsent {
        closure_certificate_digest: String,
    },
    Exact {
        revision: u64,
        state: RelationshipSourceStateV1,
        source_digest: String,
        manifest_digest: String,
    },
}

impl RelationshipSourceExpectedStateV1 {
    pub fn validate_contract(&self) -> RelationshipSourceControlResultV1<()> {
        match self {
            Self::PristineAbsent {
                closure_certificate_digest,
            } => relationship_digest(closure_certificate_digest, "closure_certificate_digest"),
            Self::Exact {
                revision,
                source_digest,
                manifest_digest,
                ..
            } => {
                if *revision == 0 {
                    return Err(RelationshipSourceControlContractErrorV1::new(
                        RelationshipSourceControlErrorKeyV1::RevisionConflict,
                        "expected relationship revision must be positive",
                    ));
                }
                relationship_digest(source_digest, "source_digest")?;
                relationship_digest(manifest_digest, "manifest_digest")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipSourceControlActionV1 {
    Create,
    UpdateContribution,
    Archive,
    Terminate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelationshipSourceControlIntentActionV1 {
    Create {
        clauses: RelationshipSourceClausesV1,
        source_asserted_at: Option<u64>,
        evidence_digest: String,
    },
    UpdateContribution {
        clauses: RelationshipSourceClausesV1,
        source_asserted_at: Option<u64>,
        evidence_digest: String,
    },
    Archive,
    Terminate,
}

impl RelationshipSourceControlIntentActionV1 {
    fn kind(&self) -> RelationshipSourceControlActionV1 {
        match self {
            Self::Create { .. } => RelationshipSourceControlActionV1::Create,
            Self::UpdateContribution { .. } => {
                RelationshipSourceControlActionV1::UpdateContribution
            }
            Self::Archive => RelationshipSourceControlActionV1::Archive,
            Self::Terminate => RelationshipSourceControlActionV1::Terminate,
        }
    }

    fn contribution_input(&self) -> Option<(&RelationshipSourceClausesV1, Option<u64>, &str)> {
        match self {
            Self::Create {
                clauses,
                source_asserted_at,
                evidence_digest,
            }
            | Self::UpdateContribution {
                clauses,
                source_asserted_at,
                evidence_digest,
            } => Some((clauses, *source_asserted_at, evidence_digest)),
            Self::Archive | Self::Terminate => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipSourceControlIntentV1 {
    pub operation_id: String,
    pub memory_space_id: String,
    pub relationship_id: String,
    pub mounted_subject_id: String,
    pub counterparty_subject_ids: Vec<String>,
    pub expected_state: RelationshipSourceExpectedStateV1,
    pub authority: RelationshipSourceControlAuthorityV1,
    pub action: RelationshipSourceControlIntentActionV1,
}

impl RelationshipSourceControlIntentV1 {
    pub fn validate_contract(&self) -> RelationshipSourceControlResultV1<()> {
        relationship_component(&self.operation_id, "operation_id")?;
        relationship_component(&self.memory_space_id, "memory_space_id")?;
        relationship_component(&self.relationship_id, "relationship_id")?;
        relationship_component(&self.mounted_subject_id, "mounted_subject_id")?;
        relationship_sorted_members(&self.counterparty_subject_ids)?;
        self.expected_state.validate_contract()?;
        self.authority.validate_contract()?;
        validate_relationship_control_membership(
            &self.mounted_subject_id,
            &self.counterparty_subject_ids,
            &self.authority,
        )?;
        match &self.action {
            RelationshipSourceControlIntentActionV1::Create {
                clauses,
                evidence_digest,
                ..
            } => {
                if !matches!(
                    self.expected_state,
                    RelationshipSourceExpectedStateV1::PristineAbsent { .. }
                ) {
                    return Err(RelationshipSourceControlContractErrorV1::new(
                        RelationshipSourceControlErrorKeyV1::RevisionConflict,
                        "create requires pristine relationship absence",
                    ));
                }
                map_relationship_contract(clauses.validate_canonical())?;
                relationship_digest(evidence_digest, "evidence_digest")
            }
            RelationshipSourceControlIntentActionV1::UpdateContribution {
                clauses,
                evidence_digest,
                ..
            } => {
                if !matches!(
                    self.expected_state,
                    RelationshipSourceExpectedStateV1::Exact {
                        state: RelationshipSourceStateV1::Active,
                        ..
                    }
                ) {
                    return Err(RelationshipSourceControlContractErrorV1::new(
                        RelationshipSourceControlErrorKeyV1::RevisionConflict,
                        "contribution update requires exact active roots",
                    ));
                }
                map_relationship_contract(clauses.validate_canonical())?;
                relationship_digest(evidence_digest, "evidence_digest")
            }
            RelationshipSourceControlIntentActionV1::Archive => {
                if !matches!(
                    self.authority,
                    RelationshipSourceControlAuthorityV1::SystemGovernor { .. }
                ) || !matches!(
                    self.expected_state,
                    RelationshipSourceExpectedStateV1::Exact {
                        state: RelationshipSourceStateV1::Active,
                        ..
                    }
                ) {
                    return Err(RelationshipSourceControlContractErrorV1::new(
                        RelationshipSourceControlErrorKeyV1::AuthorityDenied,
                        "archive requires SystemGovernor and exact active roots",
                    ));
                }
                Ok(())
            }
            RelationshipSourceControlIntentActionV1::Terminate => {
                if !matches!(
                    self.authority,
                    RelationshipSourceControlAuthorityV1::SystemGovernor { .. }
                ) || !matches!(
                    self.expected_state,
                    RelationshipSourceExpectedStateV1::Exact {
                        state: RelationshipSourceStateV1::Active
                            | RelationshipSourceStateV1::Archived,
                        ..
                    }
                ) {
                    return Err(RelationshipSourceControlContractErrorV1::new(
                        RelationshipSourceControlErrorKeyV1::AuthorityDenied,
                        "terminate requires SystemGovernor and exact non-terminal roots",
                    ));
                }
                Ok(())
            }
        }
    }

    pub fn intent_digest(&self) -> RelationshipSourceControlResultV1<String> {
        self.validate_contract()?;
        match self.expected_state {
            RelationshipSourceExpectedStateV1::PristineAbsent { .. } => {
                relationship_canonical_digest(
                    "relationship_source_control_intent_v1",
                    &(
                        &self.operation_id,
                        &self.memory_space_id,
                        &self.relationship_id,
                        &self.mounted_subject_id,
                        &self.counterparty_subject_ids,
                        &self.authority,
                        &self.action,
                    ),
                )
            }
            RelationshipSourceExpectedStateV1::Exact { .. } => {
                relationship_canonical_digest("relationship_source_control_intent_v1", self)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipSourceControlPlanV1 {
    pub action: RelationshipSourceControlActionV1,
    pub expected_state: RelationshipSourceExpectedStateV1,
    pub actor_subject_id: String,
    pub intent_digest: String,
    pub post_source: RelationshipSourceConstitutionV1,
    pub post_manifest: RelationshipSourceScopeManifestV1,
}

impl RelationshipSourceControlPlanV1 {
    pub fn validate_contract(&self) -> RelationshipSourceControlResultV1<()> {
        relationship_component(&self.actor_subject_id, "actor_subject_id")?;
        relationship_digest(&self.intent_digest, "intent_digest")?;
        self.expected_state.validate_contract()?;
        map_relationship_contract(validate_relationship_source_post_image(
            &self.post_source,
            &self.post_manifest,
        ))?;
        match (&self.action, &self.expected_state) {
            (
                RelationshipSourceControlActionV1::Create,
                RelationshipSourceExpectedStateV1::PristineAbsent { .. },
            ) if self.post_source.revision == 1
                && self.post_source.supersedes_revision.is_none()
                && self.post_source.state == RelationshipSourceStateV1::Active =>
            {
                Ok(())
            }
            (
                RelationshipSourceControlActionV1::UpdateContribution,
                RelationshipSourceExpectedStateV1::Exact {
                    revision,
                    state: RelationshipSourceStateV1::Active,
                    ..
                },
            ) if self.post_source.revision == revision.saturating_add(1)
                && self.post_source.supersedes_revision == Some(*revision)
                && self.post_source.state == RelationshipSourceStateV1::Active =>
            {
                Ok(())
            }
            (
                RelationshipSourceControlActionV1::Archive,
                RelationshipSourceExpectedStateV1::Exact {
                    revision,
                    state: RelationshipSourceStateV1::Active,
                    ..
                },
            ) if self.post_source.revision == revision.saturating_add(1)
                && self.post_source.supersedes_revision == Some(*revision)
                && self.post_source.state == RelationshipSourceStateV1::Archived =>
            {
                Ok(())
            }
            (
                RelationshipSourceControlActionV1::Terminate,
                RelationshipSourceExpectedStateV1::Exact {
                    revision,
                    state: RelationshipSourceStateV1::Active | RelationshipSourceStateV1::Archived,
                    ..
                },
            ) if self.post_source.revision == revision.saturating_add(1)
                && self.post_source.supersedes_revision == Some(*revision)
                && self.post_source.state == RelationshipSourceStateV1::Terminated =>
            {
                Ok(())
            }
            _ => Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::RevisionConflict,
                "relationship plan action does not match its exact post-image lineage",
            )),
        }
    }
}

pub fn plan_relationship_source_control(
    intent: &RelationshipSourceControlIntentV1,
    previous_source: Option<&RelationshipSourceConstitutionV1>,
    previous_manifest: Option<&RelationshipSourceScopeManifestV1>,
    recorded_at: u64,
) -> RelationshipSourceControlResultV1<RelationshipSourceControlPlanV1> {
    intent.validate_contract()?;
    let intent_digest = intent.intent_digest()?;
    let (mut post_source, retained_revision_refs) = match &intent.action {
        RelationshipSourceControlIntentActionV1::Create { .. } => {
            if previous_source.is_some() || previous_manifest.is_some() {
                return Err(RelationshipSourceControlContractErrorV1::new(
                    RelationshipSourceControlErrorKeyV1::RevisionConflict,
                    "create cannot consume existing relationship roots",
                ));
            }
            let contribution = build_relationship_contribution(intent, recorded_at)?;
            (
                RelationshipSourceConstitutionV1 {
                    schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
                    memory_space_id: intent.memory_space_id.clone(),
                    relationship_id: intent.relationship_id.clone(),
                    mounted_subject_id: intent.mounted_subject_id.clone(),
                    counterparty_subject_ids: intent.counterparty_subject_ids.clone(),
                    revision: 1,
                    supersedes_revision: None,
                    state: RelationshipSourceStateV1::Active,
                    clauses: contribution.clauses.clone(),
                    contributions: vec![contribution],
                    content_digest: String::new(),
                },
                Vec::new(),
            )
        }
        RelationshipSourceControlIntentActionV1::UpdateContribution { clauses, .. } => {
            let (previous, manifest) =
                validated_relationship_previous(intent, previous_source, previous_manifest)?;
            let mut post = previous.clone();
            post.revision = previous.revision.saturating_add(1);
            post.supersedes_revision = Some(previous.revision);
            let next = build_relationship_contribution(intent, recorded_at)?;
            if !matches!(
                intent.authority,
                RelationshipSourceControlAuthorityV1::HumanUser { .. }
            ) {
                if let Some(prior) = previous.contributions.iter().find(|value| {
                    value.contributor_subject_id == intent.authority.actor_subject_id()
                        && value.provenance.source == intent.authority.source_kind()
                }) {
                    if !clauses.tightens_or_equals(&prior.clauses) {
                        return Err(RelationshipSourceControlContractErrorV1::new(
                            RelationshipSourceControlErrorKeyV1::InvalidClauseMutation,
                            "Agent/System contribution may only tighten its existing floor",
                        ));
                    }
                }
            }
            post.contributions.retain(|value| {
                value.contributor_subject_id != intent.authority.actor_subject_id()
                    || value.provenance.source != intent.authority.source_kind()
            });
            post.contributions.push(next);
            post.contributions.sort_by(|left, right| {
                (left.provenance.source, left.contributor_subject_id.as_str()).cmp(&(
                    right.provenance.source,
                    right.contributor_subject_id.as_str(),
                ))
            });
            post.clauses = aggregate_relationship_contributions(&post.contributions)?;
            let previous_ref = canonical_relationship_source_revision_ref_v1(
                &previous.memory_space_id,
                &previous.relationship_id,
                previous.revision,
            )?;
            (
                post,
                union_sorted(
                    &manifest.retained_revision_refs,
                    std::slice::from_ref(&previous_ref),
                ),
            )
        }
        RelationshipSourceControlIntentActionV1::Archive
        | RelationshipSourceControlIntentActionV1::Terminate => {
            let (previous, manifest) =
                validated_relationship_previous(intent, previous_source, previous_manifest)?;
            if previous.state == RelationshipSourceStateV1::Terminated {
                return Err(RelationshipSourceControlContractErrorV1::new(
                    RelationshipSourceControlErrorKeyV1::Terminated,
                    "terminated relationship source is final",
                ));
            }
            let mut post = previous.clone();
            post.revision = previous.revision.saturating_add(1);
            post.supersedes_revision = Some(previous.revision);
            post.state = match intent.action {
                RelationshipSourceControlIntentActionV1::Archive => {
                    RelationshipSourceStateV1::Archived
                }
                RelationshipSourceControlIntentActionV1::Terminate => {
                    RelationshipSourceStateV1::Terminated
                }
                _ => unreachable!("matched lifecycle actions"),
            };
            let previous_ref = canonical_relationship_source_revision_ref_v1(
                &previous.memory_space_id,
                &previous.relationship_id,
                previous.revision,
            )?;
            (
                post,
                union_sorted(
                    &manifest.retained_revision_refs,
                    std::slice::from_ref(&previous_ref),
                ),
            )
        }
    };
    post_source.refresh_digest().map_err(|error| {
        RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::RepairRequired,
            error.reason,
        )
    })?;
    let mut post_manifest = RelationshipSourceScopeManifestV1 {
        schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
        memory_space_id: intent.memory_space_id.clone(),
        relationship_id: intent.relationship_id.clone(),
        current_revision: post_source.revision,
        current_digest: post_source.content_digest.clone(),
        retained_revision_refs,
        closure_digest: String::new(),
    };
    post_manifest.refresh_digest().map_err(|error| {
        RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::RepairRequired,
            error.reason,
        )
    })?;
    let post_input = RelationshipSourceControlPostImageInputV1 {
        operation_id: intent.operation_id.clone(),
        memory_space_id: intent.memory_space_id.clone(),
        relationship_id: intent.relationship_id.clone(),
        mounted_subject_id: intent.mounted_subject_id.clone(),
        counterparty_subject_ids: intent.counterparty_subject_ids.clone(),
        expected_state: intent.expected_state.clone(),
        authority: intent.authority.clone(),
        action: intent.action.kind(),
        previous_source: previous_source.cloned().map(Box::new),
        previous_manifest: previous_manifest.cloned().map(Box::new),
        post_source: Box::new(post_source.clone()),
        post_manifest: Box::new(post_manifest.clone()),
    };
    post_input.validate_contract()?;
    let plan = RelationshipSourceControlPlanV1 {
        action: intent.action.kind(),
        expected_state: intent.expected_state.clone(),
        actor_subject_id: intent.authority.actor_subject_id().to_string(),
        intent_digest,
        post_source,
        post_manifest,
    };
    plan.validate_contract()?;
    Ok(plan)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationshipSourceControlPostImageInputV1 {
    pub operation_id: String,
    pub memory_space_id: String,
    pub relationship_id: String,
    pub mounted_subject_id: String,
    pub counterparty_subject_ids: Vec<String>,
    pub expected_state: RelationshipSourceExpectedStateV1,
    pub authority: RelationshipSourceControlAuthorityV1,
    pub action: RelationshipSourceControlActionV1,
    pub previous_source: Option<Box<RelationshipSourceConstitutionV1>>,
    pub previous_manifest: Option<Box<RelationshipSourceScopeManifestV1>>,
    pub post_source: Box<RelationshipSourceConstitutionV1>,
    pub post_manifest: Box<RelationshipSourceScopeManifestV1>,
}

impl RelationshipSourceControlPostImageInputV1 {
    pub fn validate_contract(&self) -> RelationshipSourceControlResultV1<()> {
        relationship_component(&self.operation_id, "operation_id")?;
        relationship_component(&self.memory_space_id, "memory_space_id")?;
        relationship_component(&self.relationship_id, "relationship_id")?;
        relationship_component(&self.mounted_subject_id, "mounted_subject_id")?;
        relationship_sorted_members(&self.counterparty_subject_ids)?;
        self.expected_state.validate_contract()?;
        self.authority.validate_contract()?;
        map_relationship_contract(self.post_source.validate_contract())?;
        map_relationship_contract(validate_relationship_source_post_image(
            &self.post_source,
            &self.post_manifest,
        ))?;
        self.validate_target(&self.post_source)?;

        match self.action {
            RelationshipSourceControlActionV1::Create => self.validate_create(),
            RelationshipSourceControlActionV1::UpdateContribution => self.validate_update(),
            RelationshipSourceControlActionV1::Archive => {
                self.validate_state_only(RelationshipSourceStateV1::Archived)
            }
            RelationshipSourceControlActionV1::Terminate => {
                self.validate_state_only(RelationshipSourceStateV1::Terminated)
            }
        }
    }

    pub fn intent_digest(&self) -> RelationshipSourceControlResultV1<String> {
        self.validate_contract()?;
        canonical_digest("relationship_source_control_intent_v1", self).map_err(|error| {
            RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::RepairRequired,
                error.reason,
            )
        })
    }

    fn validate_target(
        &self,
        source: &RelationshipSourceConstitutionV1,
    ) -> RelationshipSourceControlResultV1<()> {
        if source.memory_space_id != self.memory_space_id
            || source.relationship_id != self.relationship_id
            || source.mounted_subject_id != self.mounted_subject_id
            || source.counterparty_subject_ids != self.counterparty_subject_ids
        {
            return Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::TargetMismatch,
                "relationship control target does not match the exact root owner",
            ));
        }
        let actor = self.authority.actor_subject_id();
        match &self.authority {
            RelationshipSourceControlAuthorityV1::HumanUser { .. }
                if actor == self.mounted_subject_id
                    || !self
                        .counterparty_subject_ids
                        .iter()
                        .any(|value| value == actor) =>
            {
                Err(RelationshipSourceControlContractErrorV1::new(
                    RelationshipSourceControlErrorKeyV1::MembershipMismatch,
                    "HumanUser actor must be an exact relationship counterparty",
                ))
            }
            RelationshipSourceControlAuthorityV1::MountedAgentPersona { .. }
                if actor != self.mounted_subject_id =>
            {
                Err(RelationshipSourceControlContractErrorV1::new(
                    RelationshipSourceControlErrorKeyV1::MembershipMismatch,
                    "AgentPersona actor must be the exact mounted subject",
                ))
            }
            RelationshipSourceControlAuthorityV1::SystemGovernor { .. }
                if actor == self.mounted_subject_id
                    || self
                        .counterparty_subject_ids
                        .iter()
                        .any(|value| value == actor) =>
            {
                Err(RelationshipSourceControlContractErrorV1::new(
                    RelationshipSourceControlErrorKeyV1::AuthorityDenied,
                    "SystemGovernor actor cannot be a relationship member",
                ))
            }
            _ => Ok(()),
        }
    }

    fn validate_create(&self) -> RelationshipSourceControlResultV1<()> {
        if !matches!(
            self.expected_state,
            RelationshipSourceExpectedStateV1::PristineAbsent { .. }
        ) || self.previous_source.is_some()
            || self.previous_manifest.is_some()
            || self.post_source.revision != 1
            || self.post_source.state != RelationshipSourceStateV1::Active
            || self.post_source.contributions.len() != 1
        {
            return Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::RevisionConflict,
                "create requires pristine absence and one revision-one contribution",
            ));
        }
        self.validate_actor_contribution(&self.post_source.contributions[0], None)
    }

    fn validate_update(&self) -> RelationshipSourceControlResultV1<()> {
        let (previous, previous_manifest) = self.previous_pair()?;
        self.validate_exact_expected(previous, previous_manifest)?;
        if previous.state != RelationshipSourceStateV1::Active
            || self.post_source.state != RelationshipSourceStateV1::Active
            || self.post_source.revision != previous.revision.saturating_add(1)
            || self.post_source.supersedes_revision != Some(previous.revision)
        {
            return Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::RevisionConflict,
                "relationship contribution update requires an exact active successor",
            ));
        }
        let actor = self.authority.actor_subject_id();
        let kind = self.authority.source_kind();
        for contribution in &previous.contributions {
            if (contribution.contributor_subject_id != actor
                || contribution.provenance.source != kind)
                && !self.post_source.contributions.contains(contribution)
            {
                return Err(RelationshipSourceControlContractErrorV1::new(
                    RelationshipSourceControlErrorKeyV1::AuthorityDenied,
                    "relationship update changed another contributor's clause owner",
                ));
            }
        }
        let next = self
            .post_source
            .contributions
            .iter()
            .find(|value| value.contributor_subject_id == actor && value.provenance.source == kind)
            .ok_or_else(|| {
                RelationshipSourceControlContractErrorV1::new(
                    RelationshipSourceControlErrorKeyV1::InvalidClauseMutation,
                    "relationship update is missing the actor contribution",
                )
            })?;
        let prior = previous
            .contributions
            .iter()
            .find(|value| value.contributor_subject_id == actor && value.provenance.source == kind);
        self.validate_actor_contribution(next, prior)
    }

    fn validate_state_only(
        &self,
        target_state: RelationshipSourceStateV1,
    ) -> RelationshipSourceControlResultV1<()> {
        if !matches!(
            self.authority,
            RelationshipSourceControlAuthorityV1::SystemGovernor { .. }
        ) {
            return Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::AuthorityDenied,
                "archive/terminate requires typed SystemGovernor lifecycle authority",
            ));
        }
        let (previous, previous_manifest) = self.previous_pair()?;
        self.validate_exact_expected(previous, previous_manifest)?;
        if previous.state == RelationshipSourceStateV1::Terminated
            || self.post_source.state != target_state
            || self.post_source.revision != previous.revision.saturating_add(1)
            || self.post_source.supersedes_revision != Some(previous.revision)
            || self.post_source.clauses != previous.clauses
            || self.post_source.contributions != previous.contributions
        {
            return Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::InvalidClauseMutation,
                "archive/terminate may change only state and exact revision lineage",
            ));
        }
        Ok(())
    }

    fn previous_pair(
        &self,
    ) -> RelationshipSourceControlResultV1<(
        &RelationshipSourceConstitutionV1,
        &RelationshipSourceScopeManifestV1,
    )> {
        let source = self.previous_source.as_deref().ok_or_else(|| {
            RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::RepairRequired,
                "relationship successor requires previous source",
            )
        })?;
        let manifest = self.previous_manifest.as_deref().ok_or_else(|| {
            RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::RepairRequired,
                "relationship successor requires previous manifest",
            )
        })?;
        map_relationship_contract(validate_relationship_source_post_image(source, manifest))?;
        self.validate_target(source)?;
        Ok((source, manifest))
    }

    fn validate_exact_expected(
        &self,
        previous: &RelationshipSourceConstitutionV1,
        previous_manifest: &RelationshipSourceScopeManifestV1,
    ) -> RelationshipSourceControlResultV1<()> {
        match &self.expected_state {
            RelationshipSourceExpectedStateV1::Exact {
                revision,
                state,
                source_digest,
                manifest_digest,
            } if *revision == previous.revision
                && *state == previous.state
                && source_digest == &previous.content_digest
                && manifest_digest == &previous_manifest.closure_digest =>
            {
                Ok(())
            }
            _ => Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::RevisionConflict,
                "expected relationship source/manifest CAS does not match previous roots",
            )),
        }
    }

    fn validate_actor_contribution(
        &self,
        next: &RelationshipSourceContributionV1,
        previous: Option<&RelationshipSourceContributionV1>,
    ) -> RelationshipSourceControlResultV1<()> {
        if next.contributor_subject_id != self.authority.actor_subject_id()
            || next.provenance.source != self.authority.source_kind()
        {
            return Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::AuthorityDenied,
                "authority can write only its own typed contribution",
            ));
        }
        if !matches!(
            self.authority,
            RelationshipSourceControlAuthorityV1::HumanUser { .. }
        ) {
            if let Some(previous) = previous {
                if !next.clauses.tightens_or_equals(&previous.clauses) {
                    return Err(RelationshipSourceControlContractErrorV1::new(
                        RelationshipSourceControlErrorKeyV1::InvalidClauseMutation,
                        "Agent/System contribution may only tighten its existing floor",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipSourceControlOutcomeV1 {
    Committed,
    Replayed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipSourceControlReportV1 {
    pub outcome: RelationshipSourceControlOutcomeV1,
    pub action: RelationshipSourceControlActionV1,
    pub revision: u64,
    pub state: RelationshipSourceStateV1,
    pub source_digest: String,
    pub manifest_digest: String,
    pub intent_digest: String,
    pub transaction_id: String,
    pub durable_receipt_ref: String,
    pub safe_event_ref: String,
    pub replayed: bool,
}

impl RelationshipSourceControlReportV1 {
    pub fn validate_contract(&self) -> RelationshipSourceControlResultV1<()> {
        if self.revision == 0
            || self.replayed != (self.outcome == RelationshipSourceControlOutcomeV1::Replayed)
        {
            return Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::RepairRequired,
                "relationship report outcome/revision invariant failed",
            ));
        }
        relationship_digest(&self.source_digest, "source_digest")?;
        relationship_digest(&self.manifest_digest, "manifest_digest")?;
        relationship_digest(&self.intent_digest, "intent_digest")?;
        relationship_component(&self.transaction_id, "transaction_id")?;
        relationship_component(&self.durable_receipt_ref, "durable_receipt_ref")?;
        relationship_component(&self.safe_event_ref, "safe_event_ref")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "selector", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelationshipSourceReadSelectorV1 {
    Current,
    Exact {
        revision: u64,
        source_digest: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipSourceReadRequestV1 {
    pub memory_space_id: String,
    pub relationship_id: String,
    pub mounted_subject_id: String,
    pub selector: RelationshipSourceReadSelectorV1,
}

impl RelationshipSourceReadRequestV1 {
    pub fn validate_contract(&self) -> RelationshipSourceControlResultV1<()> {
        relationship_component(&self.memory_space_id, "memory_space_id")?;
        relationship_component(&self.relationship_id, "relationship_id")?;
        relationship_component(&self.mounted_subject_id, "mounted_subject_id")?;
        if let RelationshipSourceReadSelectorV1::Exact {
            revision,
            source_digest,
        } = &self.selector
        {
            if *revision == 0 {
                return Err(RelationshipSourceControlContractErrorV1::new(
                    RelationshipSourceControlErrorKeyV1::RevisionConflict,
                    "exact relationship selector requires positive revision",
                ));
            }
            relationship_digest(source_digest, "source_digest")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulRelationshipProjectionV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub subject_id: String,
    pub soul_id: String,
    pub relationship_id: String,
    pub generation: u64,
    pub soul_revision: u64,
    pub soul_material_digest: String,
    pub relationship_source_revision: u64,
    pub relationship_source_digest: String,
    pub mental_privacy_ceiling: RelationshipDisclosureCeilingV1,
    pub soul_self_boundary_ceiling: RelationshipDisclosureCeilingV1,
    pub effective_disclosure_ceiling: RelationshipDisclosureCeilingV1,
    pub inherited_postures: Vec<String>,
    pub response_commitments: Vec<String>,
    pub content_digest: String,
}

impl SubjectSoulRelationshipProjectionV1 {
    pub fn refresh_digest(&mut self) -> SubjectSoulContractResult<()> {
        self.content_digest.clear();
        self.content_digest = canonical_digest("subject_soul_relationship_projection_v1", self)?;
        Ok(())
    }

    pub fn validate_contract(
        &self,
        source: &RelationshipSourceConstitutionV1,
        material: &SubjectSoulRevisionMaterialV1,
    ) -> SubjectSoulContractResult<()> {
        source.validate_contract()?;
        material.validate_contract()?;
        validate_owner(
            self.schema_version,
            &self.memory_space_id,
            &self.subject_id,
            &self.soul_id,
        )?;
        validate_component(&self.relationship_id, "relationship_id")?;
        validate_sorted_unique_components(&self.inherited_postures, "inherited_postures")?;
        validate_sorted_unique_components(&self.response_commitments, "response_commitments")?;
        if self.memory_space_id != source.memory_space_id
            || self.relationship_id != source.relationship_id
            || self.relationship_source_revision != source.revision
            || self.relationship_source_digest != source.content_digest
            || self.memory_space_id != material.memory_space_id
            || self.subject_id != material.subject_id
            || self.soul_id != material.soul_id
            || self.generation != material.generation
            || self.soul_revision != material.revision
            || self.soul_material_digest != material.content_digest
        {
            return Err(SubjectSoulContractError::repair(
                "relationship projection is stale or bound to the wrong roots",
            ));
        }
        let expected_ceiling = RelationshipConstraintLatticeV1 {
            mental_privacy: self.mental_privacy_ceiling,
            relationship_source: source.clauses.disclosure_ceiling,
            soul_self_boundary: self.soul_self_boundary_ceiling,
        }
        .effective_disclosure_ceiling();
        if self.effective_disclosure_ceiling != expected_ceiling {
            return Err(SubjectSoulContractError::repair(
                "relationship projection widened the restrictive disclosure lattice",
            ));
        }
        validate_digest(&self.content_digest, "content_digest")?;
        let mut canonical = self.clone();
        canonical.content_digest.clear();
        if canonical_digest("subject_soul_relationship_projection_v1", &canonical)?
            != self.content_digest
        {
            return Err(SubjectSoulContractError::repair(
                "relationship projection digest mismatch",
            ));
        }
        Ok(())
    }

    fn validate_self_contained_contract(&self) -> SubjectSoulContractResult<()> {
        validate_owner(
            self.schema_version,
            &self.memory_space_id,
            &self.subject_id,
            &self.soul_id,
        )?;
        validate_component(&self.relationship_id, "relationship_id")?;
        if self.generation == 0 || self.soul_revision == 0 || self.relationship_source_revision == 0
        {
            return Err(SubjectSoulContractError::repair(
                "relationship projection root revisions must be positive",
            ));
        }
        validate_digest(&self.soul_material_digest, "soul_material_digest")?;
        validate_digest(
            &self.relationship_source_digest,
            "relationship_source_digest",
        )?;
        validate_sorted_unique_components(&self.inherited_postures, "inherited_postures")?;
        validate_sorted_unique_components(&self.response_commitments, "response_commitments")?;
        validate_digest(&self.content_digest, "content_digest")?;
        let mut canonical = self.clone();
        canonical.content_digest.clear();
        if canonical_digest("subject_soul_relationship_projection_v1", &canonical)?
            != self.content_digest
        {
            return Err(SubjectSoulContractError::repair(
                "relationship projection digest mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSoulRelationshipProjectionPlanV1 {
    NoEffect,
    Upsert {
        projection: Box<SubjectSoulRelationshipProjectionV1>,
        post_head: Box<SubjectSoulLifecycleHeadV1>,
        post_manifest: Box<SubjectSoulScopeManifestV1>,
    },
    Delete {
        post_head: Box<SubjectSoulLifecycleHeadV1>,
        post_manifest: Box<SubjectSoulScopeManifestV1>,
    },
}

#[allow(clippy::too_many_arguments)]
pub fn plan_subject_soul_relationship_projection_v1(
    snapshot: &SubjectSoulVerifiedSnapshotV1,
    relationship_source: &RelationshipSourceConstitutionV1,
    relationship_manifest: &RelationshipSourceScopeManifestV1,
    existing_projection: Option<&SubjectSoulRelationshipProjectionV1>,
    projection_address: &SubjectSoulManifestAddressV1,
    mental_privacy_ceiling: RelationshipDisclosureCeilingV1,
    soul_self_boundary_ceiling: RelationshipDisclosureCeilingV1,
    recorded_at: u64,
) -> SubjectSoulContractResult<SubjectSoulRelationshipProjectionPlanV1> {
    snapshot.validate_contract()?;
    validate_relationship_source_post_image(relationship_source, relationship_manifest)?;
    projection_address.validate_contract()?;
    if recorded_at == 0
        || relationship_source.memory_space_id != snapshot.head.memory_space_id
        || relationship_source.mounted_subject_id != snapshot.head.subject_id
    {
        return Err(SubjectSoulContractError::repair(
            "relationship projection roots do not belong to the mounted Soul",
        ));
    }
    validate_existing_projection_binding(snapshot, existing_projection, projection_address)?;
    let should_project = snapshot.head.state == SubjectSoulLifecycleStateV1::Active
        && relationship_source.state == RelationshipSourceStateV1::Active;
    if !should_project {
        if existing_projection.is_none() {
            return Ok(SubjectSoulRelationshipProjectionPlanV1::NoEffect);
        }
        let (post_head, post_manifest) = update_subject_soul_projection_manifest(
            snapshot,
            projection_address,
            None,
            recorded_at,
        )?;
        return Ok(SubjectSoulRelationshipProjectionPlanV1::Delete {
            post_head: Box::new(post_head),
            post_manifest: Box::new(post_manifest),
        });
    }
    let material = snapshot.current_material.as_ref().ok_or_else(|| {
        SubjectSoulContractError::repair("active Soul projection requires current material")
    })?;
    let mut projection = SubjectSoulRelationshipProjectionV1 {
        schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
        memory_space_id: snapshot.head.memory_space_id.clone(),
        subject_id: snapshot.head.subject_id.clone(),
        soul_id: snapshot.head.soul_id.clone(),
        relationship_id: relationship_source.relationship_id.clone(),
        generation: snapshot.head.generation,
        soul_revision: material.revision,
        soul_material_digest: material.content_digest.clone(),
        relationship_source_revision: relationship_source.revision,
        relationship_source_digest: relationship_source.content_digest.clone(),
        mental_privacy_ceiling,
        soul_self_boundary_ceiling,
        effective_disclosure_ceiling: RelationshipConstraintLatticeV1 {
            mental_privacy: mental_privacy_ceiling,
            relationship_source: relationship_source.clauses.disclosure_ceiling,
            soul_self_boundary: soul_self_boundary_ceiling,
        }
        .effective_disclosure_ceiling(),
        inherited_postures: relationship_source
            .clauses
            .mutual_boundary_commitments
            .clone(),
        response_commitments: union_sorted(
            &relationship_source.clauses.truth_commitments,
            &relationship_source.clauses.repair_commitments,
        ),
        content_digest: String::new(),
    };
    projection.refresh_digest()?;
    projection.validate_contract(relationship_source, material)?;
    if existing_projection == Some(&projection) {
        return Ok(SubjectSoulRelationshipProjectionPlanV1::NoEffect);
    }
    let (post_head, post_manifest) = update_subject_soul_projection_manifest(
        snapshot,
        projection_address,
        Some(&projection),
        recorded_at,
    )?;
    Ok(SubjectSoulRelationshipProjectionPlanV1::Upsert {
        projection: Box::new(projection),
        post_head: Box::new(post_head),
        post_manifest: Box::new(post_manifest),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RelationshipConstraintLatticeV1 {
    pub mental_privacy: RelationshipDisclosureCeilingV1,
    pub relationship_source: RelationshipDisclosureCeilingV1,
    pub soul_self_boundary: RelationshipDisclosureCeilingV1,
}

impl RelationshipConstraintLatticeV1 {
    pub fn effective_disclosure_ceiling(self) -> RelationshipDisclosureCeilingV1 {
        self.mental_privacy
            .most_restrictive(self.relationship_source)
            .most_restrictive(self.soul_self_boundary)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulRelationshipRuntimeProjectionDispositionV1 {
    CurrentProjection,
    RecompiledMissingProjection,
    RecompiledStaleProjection,
    SourceOnlyUnseeded,
    InactiveSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulRelationshipRuntimeInputV1 {
    pub source: RelationshipSourceConstitutionV1,
    pub current_material: Option<SubjectSoulRevisionMaterialV1>,
    pub stored_projection: Option<SubjectSoulRelationshipProjectionV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectSoulRelationshipRuntimeViewV1 {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub relationship_id: String,
    pub relationship_source_revision: u64,
    pub relationship_source_digest: String,
    pub relationship_source_state: RelationshipSourceStateV1,
    pub projection_disposition: SubjectSoulRelationshipRuntimeProjectionDispositionV1,
    pub effective_disclosure_ceiling: RelationshipDisclosureCeilingV1,
    pub access_constraints: Vec<RelationshipAccessConstraintV1>,
    pub truth_commitments: Vec<String>,
    pub mutual_boundary_commitments: Vec<String>,
    pub repair_commitments: Vec<String>,
    pub inherited_postures: Vec<String>,
    pub response_commitments: Vec<String>,
}

pub fn compile_subject_soul_relationship_runtime_view_v1(
    mounted_subject_id: &str,
    input: &SubjectSoulRelationshipRuntimeInputV1,
    mental_privacy_state: Option<&super::MentalPrivacyState>,
) -> SubjectSoulContractResult<SubjectSoulRelationshipRuntimeViewV1> {
    input.source.validate_contract()?;
    validate_component(mounted_subject_id, "mounted_subject_id")?;
    if input.source.mounted_subject_id != mounted_subject_id {
        return Err(SubjectSoulContractError::repair(
            "relationship runtime source belongs to another mounted subject",
        ));
    }
    let source = &input.source;
    let response_commitments = union_sorted(
        &source.clauses.truth_commitments,
        &source.clauses.repair_commitments,
    );
    let mut view = SubjectSoulRelationshipRuntimeViewV1 {
        memory_space_id: source.memory_space_id.clone(),
        mounted_subject_id: source.mounted_subject_id.clone(),
        relationship_id: source.relationship_id.clone(),
        relationship_source_revision: source.revision,
        relationship_source_digest: source.content_digest.clone(),
        relationship_source_state: source.state,
        projection_disposition:
            SubjectSoulRelationshipRuntimeProjectionDispositionV1::InactiveSource,
        effective_disclosure_ceiling: RelationshipDisclosureCeilingV1::None,
        access_constraints: source.clauses.access_constraints.clone(),
        truth_commitments: source.clauses.truth_commitments.clone(),
        mutual_boundary_commitments: source.clauses.mutual_boundary_commitments.clone(),
        repair_commitments: source.clauses.repair_commitments.clone(),
        inherited_postures: source.clauses.mutual_boundary_commitments.clone(),
        response_commitments,
    };
    if source.state != RelationshipSourceStateV1::Active {
        view.access_constraints.clear();
        view.truth_commitments.clear();
        view.mutual_boundary_commitments.clear();
        view.repair_commitments.clear();
        view.inherited_postures.clear();
        view.response_commitments.clear();
        return Ok(view);
    }

    let mental_privacy_ceiling = relationship_mental_privacy_ceiling_v1(mental_privacy_state);
    let soul_self_boundary_ceiling =
        relationship_soul_self_boundary_ceiling_v1(mental_privacy_state);
    let effective_disclosure_ceiling = RelationshipConstraintLatticeV1 {
        mental_privacy: mental_privacy_ceiling,
        relationship_source: source.clauses.disclosure_ceiling,
        soul_self_boundary: soul_self_boundary_ceiling,
    }
    .effective_disclosure_ceiling();
    view.effective_disclosure_ceiling = effective_disclosure_ceiling;

    let Some(material) = input.current_material.as_ref() else {
        view.projection_disposition =
            SubjectSoulRelationshipRuntimeProjectionDispositionV1::SourceOnlyUnseeded;
        return Ok(view);
    };
    material.validate_contract()?;
    if material.memory_space_id != source.memory_space_id
        || material.subject_id != mounted_subject_id
    {
        return Err(SubjectSoulContractError::repair(
            "relationship runtime material belongs to another Soul owner",
        ));
    }
    let mut expected = SubjectSoulRelationshipProjectionV1 {
        schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
        memory_space_id: material.memory_space_id.clone(),
        subject_id: material.subject_id.clone(),
        soul_id: material.soul_id.clone(),
        relationship_id: source.relationship_id.clone(),
        generation: material.generation,
        soul_revision: material.revision,
        soul_material_digest: material.content_digest.clone(),
        relationship_source_revision: source.revision,
        relationship_source_digest: source.content_digest.clone(),
        mental_privacy_ceiling,
        soul_self_boundary_ceiling,
        effective_disclosure_ceiling,
        inherited_postures: source.clauses.mutual_boundary_commitments.clone(),
        response_commitments: view.response_commitments.clone(),
        content_digest: String::new(),
    };
    expected.refresh_digest()?;
    expected.validate_contract(source, material)?;
    if let Some(stored) = input.stored_projection.as_ref() {
        stored.validate_self_contained_contract()?;
        if stored.memory_space_id != material.memory_space_id
            || stored.subject_id != material.subject_id
            || stored.soul_id != material.soul_id
            || stored.relationship_id != source.relationship_id
        {
            return Err(SubjectSoulContractError::repair(
                "stored relationship projection belongs to another owner",
            ));
        }
    }
    view.projection_disposition = match input.stored_projection.as_ref() {
        None => SubjectSoulRelationshipRuntimeProjectionDispositionV1::RecompiledMissingProjection,
        Some(stored)
            if stored.relationship_source_revision != source.revision
                || stored.relationship_source_digest != source.content_digest
                || stored.soul_revision != material.revision
                || stored.soul_material_digest != material.content_digest
                || stored.generation != material.generation =>
        {
            SubjectSoulRelationshipRuntimeProjectionDispositionV1::RecompiledStaleProjection
        }
        Some(stored) => {
            stored.validate_contract(source, material)?;
            if stored == &expected {
                SubjectSoulRelationshipRuntimeProjectionDispositionV1::CurrentProjection
            } else {
                SubjectSoulRelationshipRuntimeProjectionDispositionV1::RecompiledStaleProjection
            }
        }
    };
    Ok(view)
}

pub fn relationship_mental_privacy_ceiling_v1(
    state: Option<&super::MentalPrivacyState>,
) -> RelationshipDisclosureCeilingV1 {
    let Some(state) = state else {
        return RelationshipDisclosureCeilingV1::RefusalOnly;
    };
    state.envelopes.values().fold(
        RelationshipDisclosureCeilingV1::FullGovernedDisclosure,
        |ceiling, envelope| {
            let envelope_ceiling = match envelope.visibility {
                super::MentalPrivacyVisibility::Direct => {
                    RelationshipDisclosureCeilingV1::FullGovernedDisclosure
                }
                super::MentalPrivacyVisibility::SummaryOnly => {
                    RelationshipDisclosureCeilingV1::GovernedSummary
                }
                super::MentalPrivacyVisibility::RequestOnly => {
                    RelationshipDisclosureCeilingV1::RefusalOnly
                }
                super::MentalPrivacyVisibility::Sealed => RelationshipDisclosureCeilingV1::None,
            };
            ceiling.most_restrictive(envelope_ceiling)
        },
    )
}

pub fn relationship_soul_self_boundary_ceiling_v1(
    state: Option<&super::MentalPrivacyState>,
) -> RelationshipDisclosureCeilingV1 {
    let Some(state) = state else {
        return RelationshipDisclosureCeilingV1::RefusalOnly;
    };
    let posture = match state.boundary_persona.posture {
        super::BoundaryPersonaPosture::Open => {
            RelationshipDisclosureCeilingV1::FullGovernedDisclosure
        }
        super::BoundaryPersonaPosture::Warm => RelationshipDisclosureCeilingV1::GovernedSummary,
        super::BoundaryPersonaPosture::Guarded => RelationshipDisclosureCeilingV1::RefusalOnly,
        super::BoundaryPersonaPosture::Sealed => RelationshipDisclosureCeilingV1::None,
    };
    let style = match state.boundary_persona.disclosure_style {
        super::BoundaryDisclosureStyle::Relational => {
            RelationshipDisclosureCeilingV1::FullGovernedDisclosure
        }
        super::BoundaryDisclosureStyle::SummaryFirst
        | super::BoundaryDisclosureStyle::Selective => {
            RelationshipDisclosureCeilingV1::GovernedSummary
        }
        super::BoundaryDisclosureStyle::Reserved => RelationshipDisclosureCeilingV1::RefusalOnly,
    };
    posture.most_restrictive(style)
}

fn canonicalize_optional(
    value: &mut Option<String>,
    field: &'static str,
) -> SubjectSoulContractResult<()> {
    if let Some(content) = value {
        *content = content.trim().to_string();
        if content.is_empty() {
            return Err(SubjectSoulContractError::invalid(format!(
                "{field} cannot be blank when present"
            )));
        }
        validate_clause(content, field)?;
    }
    Ok(())
}

fn canonicalize_list(
    values: &mut Vec<String>,
    field: &'static str,
) -> SubjectSoulContractResult<()> {
    if values.len() > SUBJECT_SOUL_MAX_CLAUSES_PER_FIELD {
        return Err(SubjectSoulContractError::invalid(format!(
            "{field} exceeds the clause count budget"
        )));
    }
    let mut seen = BTreeSet::new();
    let mut canonical = Vec::with_capacity(values.len());
    for value in values.iter() {
        let value = value.trim().to_string();
        validate_clause(&value, field)?;
        if !seen.insert(value.clone()) {
            return Err(SubjectSoulContractError::invalid(format!(
                "{field} contains a duplicate clause"
            )));
        }
        canonical.push(value);
    }
    *values = canonical;
    Ok(())
}

fn validate_clause(value: &str, field: &'static str) -> SubjectSoulContractResult<()> {
    if value.is_empty() {
        return Err(SubjectSoulContractError::invalid(format!(
            "{field} contains a blank clause"
        )));
    }
    if value.chars().count() > SUBJECT_SOUL_MAX_CLAUSE_CHARS {
        return Err(SubjectSoulContractError::invalid(format!(
            "{field} clause exceeds the character budget"
        )));
    }
    Ok(())
}

fn validate_owner(
    schema_version: u32,
    memory_space_id: &str,
    subject_id: &str,
    soul_id: &str,
) -> SubjectSoulContractResult<()> {
    if schema_version != SUBJECT_SOUL_SCHEMA_VERSION {
        return Err(SubjectSoulContractError::repair(
            "unsupported Soul schema version",
        ));
    }
    validate_component(memory_space_id, "memory_space_id")?;
    validate_component(subject_id, "subject_id")?;
    validate_component(soul_id, "soul_id")
}

fn validate_component(value: &str, field: &'static str) -> SubjectSoulContractResult<()> {
    if value.is_empty() || value.trim() != value || value.len() > 256 {
        return Err(SubjectSoulContractError::invalid(format!(
            "{field} must be a canonical non-empty component"
        )));
    }
    Ok(())
}

fn validate_optional_component(
    value: &Option<String>,
    field: &'static str,
) -> SubjectSoulContractResult<()> {
    if let Some(value) = value {
        validate_component(value, field)?;
    }
    Ok(())
}

fn validate_sorted_unique_components(
    values: &[String],
    field: &'static str,
) -> SubjectSoulContractResult<()> {
    let mut previous: Option<&str> = None;
    for value in values {
        validate_component(value, field)?;
        if previous.is_some_and(|previous| previous >= value.as_str()) {
            return Err(SubjectSoulContractError::invalid(format!(
                "{field} must be sorted and unique"
            )));
        }
        previous = Some(value);
    }
    Ok(())
}

fn validate_digest(value: &str, field: &'static str) -> SubjectSoulContractResult<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SubjectSoulContractError::repair(format!(
            "{field} must be a canonical sha256 digest"
        )));
    }
    Ok(())
}

fn validate_optional_digest(
    value: &Option<String>,
    field: &'static str,
) -> SubjectSoulContractResult<()> {
    if let Some(value) = value {
        validate_digest(value, field)?;
    }
    Ok(())
}

fn canonical_digest<T: Serialize>(domain: &str, value: &T) -> SubjectSoulContractResult<String> {
    let payload = serde_json::to_vec(value).map_err(|_| {
        SubjectSoulContractError::repair("canonical Soul material serialization failed")
    })?;
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update([0]);
    digest.update(payload);
    Ok(format!("{:x}", digest.finalize()))
}

fn union_sorted<T: Clone + Ord>(left: &[T], right: &[T]) -> Vec<T> {
    left.iter()
        .chain(right)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn is_ordered_superset<T: Ord>(candidate: &[T], required: &[T]) -> bool {
    required
        .iter()
        .all(|item| candidate.binary_search(item).is_ok())
}

fn map_relationship_contract<T>(
    result: SubjectSoulContractResult<T>,
) -> RelationshipSourceControlResultV1<T> {
    result.map_err(|error| {
        RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::RepairRequired,
            error.reason,
        )
    })
}

fn relationship_component(
    value: &str,
    field: &'static str,
) -> RelationshipSourceControlResultV1<()> {
    validate_component(value, field).map_err(|error| {
        RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::TargetMismatch,
            error.reason,
        )
    })
}

fn relationship_digest(value: &str, field: &'static str) -> RelationshipSourceControlResultV1<()> {
    validate_digest(value, field).map_err(|error| {
        RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::RepairRequired,
            error.reason,
        )
    })
}

fn relationship_sorted_members(values: &[String]) -> RelationshipSourceControlResultV1<()> {
    validate_sorted_unique_components(values, "counterparty_subject_ids").map_err(|error| {
        RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::MembershipMismatch,
            error.reason,
        )
    })?;
    if values.is_empty() {
        return Err(RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::MembershipMismatch,
            "relationship control requires at least one counterparty",
        ));
    }
    Ok(())
}

fn hash_relationship_ref_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn relationship_canonical_digest<T: Serialize>(
    domain: &str,
    value: &T,
) -> RelationshipSourceControlResultV1<String> {
    canonical_digest(domain, value).map_err(|error| {
        RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::RepairRequired,
            error.reason,
        )
    })
}

fn validate_relationship_control_membership(
    mounted_subject_id: &str,
    counterparty_subject_ids: &[String],
    authority: &RelationshipSourceControlAuthorityV1,
) -> RelationshipSourceControlResultV1<()> {
    let actor = authority.actor_subject_id();
    match authority {
        RelationshipSourceControlAuthorityV1::HumanUser { .. }
            if actor == mounted_subject_id
                || !counterparty_subject_ids.iter().any(|value| value == actor) =>
        {
            Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::MembershipMismatch,
                "HumanUser actor must be an exact relationship counterparty",
            ))
        }
        RelationshipSourceControlAuthorityV1::MountedAgentPersona { .. }
            if actor != mounted_subject_id =>
        {
            Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::MembershipMismatch,
                "AgentPersona actor must be the exact mounted subject",
            ))
        }
        RelationshipSourceControlAuthorityV1::SystemGovernor { .. }
            if actor == mounted_subject_id
                || counterparty_subject_ids.iter().any(|value| value == actor) =>
        {
            Err(RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::AuthorityDenied,
                "SystemGovernor actor cannot be a relationship member",
            ))
        }
        _ => Ok(()),
    }
}

fn build_relationship_contribution(
    intent: &RelationshipSourceControlIntentV1,
    recorded_at: u64,
) -> RelationshipSourceControlResultV1<RelationshipSourceContributionV1> {
    if recorded_at == 0 {
        return Err(RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::InvalidClauseMutation,
            "relationship contribution recorded_at must be positive",
        ));
    }
    let (clauses, source_asserted_at, evidence_digest) =
        intent.action.contribution_input().ok_or_else(|| {
            RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::InvalidClauseMutation,
                "relationship lifecycle action has no contribution payload",
            )
        })?;
    if source_asserted_at.is_some_and(|value| value > recorded_at) {
        return Err(RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::InvalidClauseMutation,
            "relationship contribution source_asserted_at cannot exceed recorded_at",
        ));
    }
    let mut contribution = RelationshipSourceContributionV1 {
        contributor_subject_id: intent.authority.actor_subject_id().to_string(),
        clauses: clauses.clone(),
        provenance: RelationshipSourceProvenanceV1 {
            source: intent.authority.source_kind(),
            source_subject_id: intent.authority.actor_subject_id().to_string(),
            source_asserted_at,
            recorded_at,
            evidence_digest: evidence_digest.to_string(),
        },
        contribution_digest: String::new(),
    };
    contribution.refresh_digest().map_err(|error| {
        RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::RepairRequired,
            error.reason,
        )
    })?;
    map_relationship_contract(contribution.validate_contract())?;
    Ok(contribution)
}

fn aggregate_relationship_contributions(
    contributions: &[RelationshipSourceContributionV1],
) -> RelationshipSourceControlResultV1<RelationshipSourceClausesV1> {
    contributions
        .iter()
        .map(|value| &value.clauses)
        .cloned()
        .reduce(|current, next| current.most_restrictive_merge(&next))
        .ok_or_else(|| {
            RelationshipSourceControlContractErrorV1::new(
                RelationshipSourceControlErrorKeyV1::RepairRequired,
                "relationship source cannot have an empty contribution set",
            )
        })
}

fn validated_relationship_previous<'a>(
    intent: &RelationshipSourceControlIntentV1,
    previous_source: Option<&'a RelationshipSourceConstitutionV1>,
    previous_manifest: Option<&'a RelationshipSourceScopeManifestV1>,
) -> RelationshipSourceControlResultV1<(
    &'a RelationshipSourceConstitutionV1,
    &'a RelationshipSourceScopeManifestV1,
)> {
    let source = previous_source.ok_or_else(|| {
        RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::RepairRequired,
            "relationship successor requires previous source",
        )
    })?;
    let manifest = previous_manifest.ok_or_else(|| {
        RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::RepairRequired,
            "relationship successor requires previous manifest",
        )
    })?;
    map_relationship_contract(validate_relationship_source_post_image(source, manifest))?;
    if source.memory_space_id != intent.memory_space_id
        || source.relationship_id != intent.relationship_id
        || source.mounted_subject_id != intent.mounted_subject_id
        || source.counterparty_subject_ids != intent.counterparty_subject_ids
    {
        return Err(RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::TargetMismatch,
            "relationship intent does not match the exact previous root owner",
        ));
    }
    match &intent.expected_state {
        RelationshipSourceExpectedStateV1::Exact {
            revision,
            state,
            source_digest,
            manifest_digest,
        } if *revision == source.revision
            && *state == source.state
            && source_digest == &source.content_digest
            && manifest_digest == &manifest.closure_digest =>
        {
            Ok((source, manifest))
        }
        _ => Err(RelationshipSourceControlContractErrorV1::new(
            RelationshipSourceControlErrorKeyV1::RevisionConflict,
            "expected relationship source/manifest CAS does not match previous roots",
        )),
    }
}

fn validate_destructive_binding(
    target_subject_id: &str,
    reason_code: &str,
    action: &SubjectSoulLifecycleActionV1,
    confirmation: &HumanSoulLifecycleConfirmationV1,
) -> SubjectSoulContractResult<()> {
    confirmation.validate_contract()?;
    let expected_action = match action {
        SubjectSoulLifecycleActionV1::Reset { .. } => SubjectSoulTerminalActionV1::Reset,
        SubjectSoulLifecycleActionV1::Reseed { .. } => SubjectSoulTerminalActionV1::Reseed,
        SubjectSoulLifecycleActionV1::Delete { .. } => SubjectSoulTerminalActionV1::Delete,
        SubjectSoulLifecycleActionV1::Archive | SubjectSoulLifecycleActionV1::Restore => {
            return Err(SubjectSoulContractError {
                key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
                reason: "non-destructive action cannot consume destructive confirmation"
                    .to_string(),
            });
        }
    };
    if confirmation.target_subject_id != target_subject_id
        || confirmation.reason_code != reason_code
        || confirmation.action != expected_action
    {
        return Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
            reason: "human confirmation does not bind the exact destructive intent".to_string(),
        });
    }
    Ok(())
}

fn subject_soul_manifest_entry(
    address: &SubjectSoulManifestAddressV1,
    generation: u64,
    revision: u64,
    content_digest: String,
) -> SubjectSoulScopeManifestEntryV1 {
    SubjectSoulScopeManifestEntryV1 {
        namespace: address.namespace.clone(),
        physical_key: address.physical_key.clone(),
        owner_role: SubjectSoulManifestOwnerRoleV1::SubjectGlobal,
        generation,
        revision: Some(revision),
        content_digest,
    }
}

fn subject_soul_owned_document_digest(
    document: &SubjectSoulOwnedDocumentV1,
) -> SubjectSoulContractResult<String> {
    let mut canonical = document.clone();
    canonical.content_digest.clear();
    let encoded = serde_json::to_vec(&canonical).map_err(|_| {
        SubjectSoulContractError::repair("owned Soul document serialization failed")
    })?;
    let mut hasher = Sha256::new();
    hash_relationship_ref_field(&mut hasher, b"subject_soul_owned_json_document_v1");
    hash_relationship_ref_field(&mut hasher, &encoded);
    Ok(format!("{:x}", hasher.finalize()))
}

struct BuiltSubjectSoulFoundingRevisionV1 {
    head: SubjectSoulLifecycleHeadV1,
    manifest: SubjectSoulScopeManifestV1,
    material: SubjectSoulRevisionMaterialV1,
    core: SelfAuthoredCore,
    core_document: SubjectSoulOwnedDocumentV1,
    revision_ledger: CoreRevisionLedger,
    revision_ledger_document: SubjectSoulOwnedDocumentV1,
    revision_ledger_digest: String,
}

#[allow(clippy::too_many_arguments)]
fn build_subject_soul_founding_revision(
    owner: &SubjectSoulOwnerV1,
    generation: u64,
    manifest_revision: u64,
    operation_id: &str,
    human_actor_subject_id: &str,
    charter: &SubjectSoulFoundingCharterSeedV1,
    source_asserted_at: Option<u64>,
    addresses: &SubjectSoulRevisionAddressBindingsV1,
    recorded_at: u64,
) -> SubjectSoulContractResult<BuiltSubjectSoulFoundingRevisionV1> {
    if generation == 0
        || manifest_revision == 0
        || recorded_at == 0
        || source_asserted_at.is_some_and(|value| value > recorded_at)
    {
        return Err(SubjectSoulContractError::repair(
            "founding revision generation/time is invalid",
        ));
    }
    validate_component(operation_id, "operation_id")?;
    validate_component(human_actor_subject_id, "human_actor_subject_id")?;
    charter.validate_canonical()?;
    addresses.validate_contract()?;
    let core = compile_subject_soul_founding_core(charter, recorded_at)?;
    let provenance = SubjectSoulRevisionProvenanceV1 {
        origin: SubjectSoulRevisionOriginV1::HumanFoundingCharter,
        source_authority: SubjectSoulSourceAuthorityV1::ActiveHumanUser,
        source_subject_id: human_actor_subject_id.to_string(),
        source_asserted_at,
        recorded_at,
        operation_ref: Some(operation_id.to_string()),
        proposal_ref: None,
        source_refs: Vec::new(),
    };
    provenance.validate_contract()?;
    let mut material = SubjectSoulRevisionMaterialV1 {
        schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
        memory_space_id: owner.memory_space_id.clone(),
        subject_id: owner.subject_id.clone(),
        soul_id: owner.soul_id.clone(),
        generation,
        revision: 1,
        supersedes_revision: None,
        core: core.clone(),
        provenance,
        content_digest: String::new(),
    };
    material.refresh_digest()?;
    let revision_ledger = append_core_revision_record(
        CoreRevisionLedger::default(),
        CoreRevisionRecord {
            based_on_revision: 0,
            resulting_revision: 1,
            source_layers: vec!["human_founding_charter".to_string()],
            outcome: CoreRevisionOutcome::Adopted,
            evidence_summary: vec!["typed_human_founding_charter".to_string()],
            adjudication_reason: "human_founding_charter".to_string(),
            rationale: "Initialized revision one from a typed human founding charter.".to_string(),
            reviewed_at: recorded_at,
            ..CoreRevisionRecord::default()
        },
    );
    let core_document =
        SubjectSoulOwnedDocumentV1::new(owner, generation, Some(1), &addresses.core, &core)?;
    let revision_ledger_document = SubjectSoulOwnedDocumentV1::new(
        owner,
        generation,
        Some(1),
        &addresses.revision_ledger,
        &revision_ledger,
    )?;
    let revision_ledger_digest = revision_ledger_document.content_digest.clone();
    let mut entries = vec![
        subject_soul_manifest_entry(
            &addresses.material,
            generation,
            1,
            material.content_digest.clone(),
        ),
        subject_soul_manifest_entry(
            &addresses.core,
            generation,
            1,
            core_document.content_digest.clone(),
        ),
        subject_soul_manifest_entry(
            &addresses.revision_ledger,
            generation,
            1,
            revision_ledger_digest.clone(),
        ),
    ];
    entries.sort();
    let mut manifest = SubjectSoulScopeManifestV1 {
        schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
        memory_space_id: owner.memory_space_id.clone(),
        subject_id: owner.subject_id.clone(),
        soul_id: owner.soul_id.clone(),
        generation,
        manifest_revision,
        entries,
        closure_digest: String::new(),
    };
    manifest.refresh_digest()?;
    let mut head = SubjectSoulLifecycleHeadV1 {
        schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
        memory_space_id: owner.memory_space_id.clone(),
        subject_id: owner.subject_id.clone(),
        soul_id: owner.soul_id.clone(),
        generation,
        state: SubjectSoulLifecycleStateV1::Active,
        current_revision: Some(1),
        current_material_digest: Some(material.content_digest.clone()),
        current_ledger_digest: Some(revision_ledger_digest.clone()),
        scope_manifest_digest: manifest.closure_digest.clone(),
        retained_revision_refs: Vec::new(),
        retained_tombstone_refs: Vec::new(),
        updated_at: recorded_at,
        head_digest: String::new(),
    };
    head.refresh_digest()?;
    Ok(BuiltSubjectSoulFoundingRevisionV1 {
        head,
        manifest,
        material,
        core,
        core_document,
        revision_ledger,
        revision_ledger_document,
        revision_ledger_digest,
    })
}

fn validate_self_authored_revision_basis(
    owner: &SubjectSoulOwnerV1,
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
) -> SubjectSoulContractResult<()> {
    match basis {
        SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded {
            closure_certificate_digest,
        } => validate_digest(closure_certificate_digest, "closure_certificate_digest"),
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } => {
            snapshot.validate_contract()?;
            validate_subject_soul_snapshot_owner(owner, snapshot)?;
            match snapshot.head.state {
                SubjectSoulLifecycleStateV1::Unseeded | SubjectSoulLifecycleStateV1::Active => {
                    Ok(())
                }
                SubjectSoulLifecycleStateV1::Archived => Err(SubjectSoulContractError {
                    key: SubjectSoulLifecycleErrorKey::Archived,
                    reason: "archived Soul cannot accept autonomous revision planning".to_string(),
                }),
                SubjectSoulLifecycleStateV1::Deleted => Err(SubjectSoulContractError {
                    key: SubjectSoulLifecycleErrorKey::Deleted,
                    reason: "deleted Soul cannot accept autonomous revision planning".to_string(),
                }),
            }
        }
    }
}

fn self_authored_basis_expected_state(
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
) -> SubjectSoulExpectedStateV1 {
    match basis {
        SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded {
            closure_certificate_digest,
        } => SubjectSoulExpectedStateV1::PristineAbsent {
            closure_certificate_digest: closure_certificate_digest.clone(),
        },
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } => {
            SubjectSoulExpectedStateV1::Exact {
                generation: snapshot.head.generation,
                revision: snapshot.head.current_revision,
                lifecycle_state: snapshot.head.state,
                head_digest: snapshot.head.head_digest.clone(),
                manifest_digest: snapshot.manifest.closure_digest.clone(),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_subject_soul_reviewed_rejection(
    owner: &SubjectSoulOwnerV1,
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
    computed_expected: &SelfAuthoredCoreExpectedPriorV1,
    expected_prior: &SelfAuthoredCoreExpectedPriorV1,
    observed_ledger: &CoreRevisionLedger,
    next_ledger: &CoreRevisionLedger,
    origin: SubjectSoulRevisionOriginV1,
    proposal_ref: &str,
    source_refs: &[String],
    addresses: Option<&SubjectSoulSelfAuthoredPostImageAddressesV1>,
    expected_state: SubjectSoulExpectedStateV1,
    intent_digest: String,
    recorded_at: u64,
) -> SubjectSoulContractResult<SubjectSoulSelfAuthoredCommitPlanV1> {
    let SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } = basis else {
        return Err(SubjectSoulContractError::repair(
            "unseeded Soul cannot persist a rejected revision without an existing Core",
        ));
    };
    if snapshot.head.state != SubjectSoulLifecycleStateV1::Active {
        return Err(SubjectSoulContractError::repair(
            "review-only ledger updates require an active Soul",
        ));
    }
    validate_self_authored_plan_metadata(
        computed_expected,
        expected_prior,
        observed_ledger,
        next_ledger,
        origin,
        proposal_ref,
        source_refs,
        snapshot.head.current_revision,
        false,
        recorded_at,
    )?;
    let SubjectSoulSelfAuthoredPostImageAddressesV1::ReviewedRejected { revision_ledger } =
        addresses.ok_or_else(|| {
            SubjectSoulContractError::repair(
                "reviewed rejection requires an exact ledger post-image address",
            )
        })?
    else {
        return Err(SubjectSoulContractError::repair(
            "reviewed rejection cannot allocate revision material or Core",
        ));
    };
    revision_ledger.validate_contract()?;
    let current_revision = snapshot.head.current_revision.ok_or_else(|| {
        SubjectSoulContractError::repair("active Soul is missing current revision")
    })?;
    let previous_ledger_document = snapshot
        .current_revision_ledger_document
        .as_ref()
        .ok_or_else(|| {
            SubjectSoulContractError::repair("active Soul is missing ledger envelope")
        })?;
    let next_document = SubjectSoulOwnedDocumentV1::new(
        owner,
        snapshot.head.generation,
        Some(current_revision),
        revision_ledger,
        next_ledger,
    )?;
    let old_address = SubjectSoulManifestAddressV1 {
        namespace: previous_ledger_document.namespace.clone(),
        physical_key: previous_ledger_document.physical_key.clone(),
    };
    let mut post_manifest = snapshot.manifest.clone();
    post_manifest.manifest_revision =
        next_subject_soul_manifest_revision(post_manifest.manifest_revision)?;
    remove_exact_manifest_entry(
        &mut post_manifest.entries,
        &old_address,
        &previous_ledger_document.content_digest,
    )?;
    ensure_manifest_address_absent(&post_manifest.entries, revision_ledger)?;
    post_manifest.entries.push(subject_soul_manifest_entry(
        revision_ledger,
        snapshot.head.generation,
        current_revision,
        next_document.content_digest.clone(),
    ));
    post_manifest.entries.sort();
    post_manifest.refresh_digest()?;
    let mut post_head = snapshot.head.clone();
    post_head.current_ledger_digest = Some(next_document.content_digest.clone());
    post_head.scope_manifest_digest = post_manifest.closure_digest.clone();
    post_head.updated_at = recorded_at;
    post_head.refresh_digest()?;
    validate_subject_soul_post_image(
        &post_head,
        &post_manifest,
        snapshot.current_material.as_ref(),
        snapshot.current_core.as_ref(),
        Some(&next_document.content_digest),
    )?;
    let purge_manifest_addresses = (old_address != *revision_ledger)
        .then_some(old_address)
        .into_iter()
        .collect();
    Ok(SubjectSoulSelfAuthoredCommitPlanV1::ReviewedRejected {
        expected_state,
        intent_digest,
        post_head: Box::new(post_head),
        post_manifest: Box::new(post_manifest),
        revision_ledger: Box::new(next_ledger.clone()),
        revision_ledger_document: Box::new(next_document),
        purge_manifest_addresses,
    })
}

#[allow(clippy::too_many_arguments)]
fn plan_subject_soul_adopted_revision(
    owner: &SubjectSoulOwnerV1,
    basis: &SubjectSoulSelfAuthoredRevisionBasisV1,
    computed_expected: &SelfAuthoredCoreExpectedPriorV1,
    expected_prior: &SelfAuthoredCoreExpectedPriorV1,
    observed_ledger: &CoreRevisionLedger,
    next_core: &SelfAuthoredCore,
    next_ledger: &CoreRevisionLedger,
    origin: SubjectSoulRevisionOriginV1,
    proposal_ref: &str,
    source_refs: &[String],
    addresses: Option<&SubjectSoulSelfAuthoredPostImageAddressesV1>,
    expected_state: SubjectSoulExpectedStateV1,
    intent_digest: String,
    recorded_at: u64,
) -> SubjectSoulContractResult<SubjectSoulSelfAuthoredCommitPlanV1> {
    let (generation, previous_revision, manifest_revision) = match basis {
        SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded { .. } => (1, None, 1),
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } => (
            snapshot.head.generation,
            snapshot.head.current_revision,
            next_subject_soul_manifest_revision(snapshot.manifest.manifest_revision)?,
        ),
    };
    let next_revision = match previous_revision {
        Some(revision) => revision
            .checked_add(1)
            .ok_or_else(|| SubjectSoulContractError {
                key: SubjectSoulLifecycleErrorKey::GenerationConflict,
                reason: "Soul revision overflow".to_string(),
            })?,
        None => 1,
    };
    let expected_origin = if previous_revision.is_some() {
        SubjectSoulRevisionOriginV1::SelfGovernedRevision
    } else {
        SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap
    };
    if origin != expected_origin
        || next_core.revision != next_revision
        || next_core.supersedes_revision != previous_revision
        || !next_core.is_meaningful()
        || next_core.updated_at != recorded_at
        || next_core.last_reviewed_at != recorded_at
    {
        return Err(SubjectSoulContractError::repair(
            "self-authored adoption origin/revision/time post-image is invalid",
        ));
    }
    validate_self_authored_plan_metadata(
        computed_expected,
        expected_prior,
        observed_ledger,
        next_ledger,
        origin,
        proposal_ref,
        source_refs,
        Some(next_revision),
        true,
        recorded_at,
    )?;
    let SubjectSoulSelfAuthoredPostImageAddressesV1::Adopt { revision } =
        addresses.ok_or_else(|| {
            SubjectSoulContractError::repair(
                "self-authored adoption requires exact revision artifact addresses",
            )
        })?
    else {
        return Err(SubjectSoulContractError::repair(
            "self-authored adoption cannot use a ledger-only address",
        ));
    };
    revision.validate_contract()?;

    let provenance = SubjectSoulRevisionProvenanceV1 {
        origin,
        source_authority: SubjectSoulSourceAuthorityV1::SoulSelfGovernance,
        source_subject_id: owner.subject_id.clone(),
        source_asserted_at: None,
        recorded_at,
        operation_ref: None,
        proposal_ref: Some(proposal_ref.to_string()),
        source_refs: source_refs.to_vec(),
    };
    provenance.validate_contract()?;
    let mut material = SubjectSoulRevisionMaterialV1 {
        schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
        memory_space_id: owner.memory_space_id.clone(),
        subject_id: owner.subject_id.clone(),
        soul_id: owner.soul_id.clone(),
        generation,
        revision: next_revision,
        supersedes_revision: previous_revision,
        core: next_core.clone(),
        provenance,
        content_digest: String::new(),
    };
    material.refresh_digest()?;
    let core_document = SubjectSoulOwnedDocumentV1::new(
        owner,
        generation,
        Some(next_revision),
        &revision.core,
        next_core,
    )?;
    let ledger_document = SubjectSoulOwnedDocumentV1::new(
        owner,
        generation,
        Some(next_revision),
        &revision.revision_ledger,
        next_ledger,
    )?;

    let (mut post_head, mut post_manifest, mut purge_manifest_addresses) = match basis {
        SubjectSoulSelfAuthoredRevisionBasisV1::ImplicitUnseeded { .. } => (
            SubjectSoulLifecycleHeadV1 {
                schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
                memory_space_id: owner.memory_space_id.clone(),
                subject_id: owner.subject_id.clone(),
                soul_id: owner.soul_id.clone(),
                generation,
                state: SubjectSoulLifecycleStateV1::Active,
                current_revision: None,
                current_material_digest: None,
                current_ledger_digest: None,
                scope_manifest_digest: String::new(),
                retained_revision_refs: Vec::new(),
                retained_tombstone_refs: Vec::new(),
                updated_at: recorded_at,
                head_digest: String::new(),
            },
            SubjectSoulScopeManifestV1 {
                schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
                memory_space_id: owner.memory_space_id.clone(),
                subject_id: owner.subject_id.clone(),
                soul_id: owner.soul_id.clone(),
                generation,
                manifest_revision,
                entries: Vec::new(),
                closure_digest: String::new(),
            },
            Vec::new(),
        ),
        SubjectSoulSelfAuthoredRevisionBasisV1::Verified { snapshot } => {
            let mut head = snapshot.head.clone();
            let mut manifest = snapshot.manifest.clone();
            manifest.manifest_revision = manifest_revision;
            let purge = prepare_manifest_for_self_authored_adoption(
                snapshot,
                revision,
                &mut head,
                &mut manifest,
            )?;
            (head, manifest, purge)
        }
    };
    for address in [
        &revision.material,
        &revision.core,
        &revision.revision_ledger,
    ] {
        ensure_manifest_address_absent(&post_manifest.entries, address)?;
    }
    if post_head
        .retained_revision_refs
        .iter()
        .any(|value| value == &revision.material.physical_key)
    {
        return Err(SubjectSoulContractError::repair(
            "new immutable material address collides with retained history",
        ));
    }
    post_manifest.entries.extend([
        subject_soul_manifest_entry(
            &revision.material,
            generation,
            next_revision,
            material.content_digest.clone(),
        ),
        subject_soul_manifest_entry(
            &revision.core,
            generation,
            next_revision,
            core_document.content_digest.clone(),
        ),
        subject_soul_manifest_entry(
            &revision.revision_ledger,
            generation,
            next_revision,
            ledger_document.content_digest.clone(),
        ),
    ]);
    post_manifest.entries.sort();
    post_manifest.refresh_digest()?;
    post_head.state = SubjectSoulLifecycleStateV1::Active;
    post_head.current_revision = Some(next_revision);
    post_head.current_material_digest = Some(material.content_digest.clone());
    post_head.current_ledger_digest = Some(ledger_document.content_digest.clone());
    post_head.scope_manifest_digest = post_manifest.closure_digest.clone();
    post_head.updated_at = recorded_at;
    post_head.refresh_digest()?;
    purge_manifest_addresses.sort_by(|left, right| {
        (&left.namespace, &left.physical_key).cmp(&(&right.namespace, &right.physical_key))
    });
    purge_manifest_addresses.dedup_by(|left, right| left == right);
    validate_subject_soul_post_image(
        &post_head,
        &post_manifest,
        Some(&material),
        Some(next_core),
        Some(&ledger_document.content_digest),
    )?;
    Ok(SubjectSoulSelfAuthoredCommitPlanV1::Adopt {
        expected_state,
        intent_digest,
        post_head: Box::new(post_head),
        post_manifest: Box::new(post_manifest),
        material: Box::new(material),
        core: Box::new(next_core.clone()),
        core_document: Box::new(core_document),
        revision_ledger: Box::new(next_ledger.clone()),
        revision_ledger_document: Box::new(ledger_document),
        purge_manifest_addresses,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_self_authored_plan_metadata(
    computed_expected: &SelfAuthoredCoreExpectedPriorV1,
    expected_prior: &SelfAuthoredCoreExpectedPriorV1,
    observed_ledger: &CoreRevisionLedger,
    next_ledger: &CoreRevisionLedger,
    origin: SubjectSoulRevisionOriginV1,
    proposal_ref: &str,
    source_refs: &[String],
    expected_resulting_revision: Option<u64>,
    adopted: bool,
    recorded_at: u64,
) -> SubjectSoulContractResult<()> {
    if computed_expected != expected_prior {
        return Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::GenerationConflict,
            reason: "self-authored plan expected prior does not match verified roots".to_string(),
        });
    }
    validate_component(proposal_ref, "proposal_ref")?;
    validate_sorted_unique_components(source_refs, "source_refs")?;
    if next_ledger.updated_at != recorded_at {
        return Err(SubjectSoulContractError::repair(
            "self-authored ledger time does not match commit time",
        ));
    }
    let latest = next_ledger.entries.last().ok_or_else(|| {
        SubjectSoulContractError::repair("self-authored plan is missing its ledger decision")
    })?;
    if append_core_revision_record(observed_ledger.clone(), latest.clone()) != *next_ledger {
        return Err(SubjectSoulContractError::repair(
            "self-authored ledger is not the exact canonical successor",
        ));
    }
    let resulting_revision = expected_resulting_revision.unwrap_or(0);
    let expected_based_on = if adopted {
        resulting_revision.saturating_sub(1)
    } else {
        resulting_revision
    };
    if latest.based_on_revision != expected_based_on
        || latest.resulting_revision != resulting_revision
        || latest.reviewed_at != recorded_at
        || (latest.outcome == CoreRevisionOutcome::Adopted) != adopted
        || (adopted && origin == SubjectSoulRevisionOriginV1::HumanFoundingCharter)
        || (!adopted && origin != SubjectSoulRevisionOriginV1::SelfGovernedRevision)
    {
        return Err(SubjectSoulContractError::repair(
            "self-authored ledger decision does not match the planned Soul revision",
        ));
    }
    if next_ledger.entries.iter().any(|entry| {
        entry.outcome == CoreRevisionOutcome::Adopted
            && entry.resulting_revision > resulting_revision
    }) {
        return Err(SubjectSoulContractError::repair(
            "self-authored ledger claims a future adopted revision",
        ));
    }
    Ok(())
}

fn prepare_manifest_for_self_authored_adoption(
    snapshot: &SubjectSoulVerifiedSnapshotV1,
    revision: &SubjectSoulRevisionAddressBindingsV1,
    head: &mut SubjectSoulLifecycleHeadV1,
    manifest: &mut SubjectSoulScopeManifestV1,
) -> SubjectSoulContractResult<Vec<SubjectSoulManifestAddressV1>> {
    let mut purge = Vec::new();
    if snapshot.head.state == SubjectSoulLifecycleStateV1::Active {
        let material_digest = snapshot
            .head
            .current_material_digest
            .as_deref()
            .ok_or_else(|| {
                SubjectSoulContractError::repair("active Soul is missing material digest")
            })?;
        let current_revision = snapshot.head.current_revision.ok_or_else(|| {
            SubjectSoulContractError::repair("active Soul is missing current revision")
        })?;
        let matching_materials = manifest
            .entries
            .iter()
            .filter(|entry| {
                entry.owner_role == SubjectSoulManifestOwnerRoleV1::SubjectGlobal
                    && entry.revision == Some(current_revision)
                    && entry.content_digest == material_digest
            })
            .cloned()
            .collect::<Vec<_>>();
        if matching_materials.len() != 1 {
            return Err(SubjectSoulContractError::repair(
                "active Soul manifest does not identify one exact current material",
            ));
        }
        let material_entry = &matching_materials[0];
        if material_entry.physical_key == revision.material.physical_key {
            return Err(SubjectSoulContractError::repair(
                "immutable Soul revision material address cannot be reused",
            ));
        }
        head.retained_revision_refs = union_sorted(
            &head.retained_revision_refs,
            std::slice::from_ref(&material_entry.physical_key),
        );
        let current_core = snapshot.current_core_document.as_ref().ok_or_else(|| {
            SubjectSoulContractError::repair("active Soul is missing current Core envelope")
        })?;
        let current_ledger = snapshot
            .current_revision_ledger_document
            .as_ref()
            .ok_or_else(|| {
                SubjectSoulContractError::repair("active Soul is missing current ledger envelope")
            })?;
        let core_address = SubjectSoulManifestAddressV1 {
            namespace: current_core.namespace.clone(),
            physical_key: current_core.physical_key.clone(),
        };
        let ledger_address = SubjectSoulManifestAddressV1 {
            namespace: current_ledger.namespace.clone(),
            physical_key: current_ledger.physical_key.clone(),
        };
        remove_exact_manifest_entry(
            &mut manifest.entries,
            &SubjectSoulManifestAddressV1 {
                namespace: material_entry.namespace.clone(),
                physical_key: material_entry.physical_key.clone(),
            },
            &material_entry.content_digest,
        )?;
        remove_exact_manifest_entry(
            &mut manifest.entries,
            &core_address,
            &current_core.content_digest,
        )?;
        remove_exact_manifest_entry(
            &mut manifest.entries,
            &ledger_address,
            &current_ledger.content_digest,
        )?;
        if core_address != revision.core {
            purge.push(core_address);
        }
        if ledger_address != revision.revision_ledger {
            purge.push(ledger_address);
        }
    }
    let stale_projection_addresses = manifest
        .entries
        .iter()
        .filter(|entry| entry.owner_role == SubjectSoulManifestOwnerRoleV1::RelationshipProjection)
        .map(|entry| SubjectSoulManifestAddressV1 {
            namespace: entry.namespace.clone(),
            physical_key: entry.physical_key.clone(),
        })
        .collect::<Vec<_>>();
    manifest
        .entries
        .retain(|entry| entry.owner_role != SubjectSoulManifestOwnerRoleV1::RelationshipProjection);
    purge.extend(stale_projection_addresses);
    Ok(purge)
}

fn remove_exact_manifest_entry(
    entries: &mut Vec<SubjectSoulScopeManifestEntryV1>,
    address: &SubjectSoulManifestAddressV1,
    expected_digest: &str,
) -> SubjectSoulContractResult<()> {
    let Some(index) = entries.iter().position(|entry| {
        entry.namespace == address.namespace && entry.physical_key == address.physical_key
    }) else {
        return Err(SubjectSoulContractError::repair(
            "manifest is missing an exact current artifact address",
        ));
    };
    if entries[index].content_digest != expected_digest {
        return Err(SubjectSoulContractError::repair(
            "manifest current artifact digest mismatch",
        ));
    }
    entries.remove(index);
    Ok(())
}

fn ensure_manifest_address_absent(
    entries: &[SubjectSoulScopeManifestEntryV1],
    address: &SubjectSoulManifestAddressV1,
) -> SubjectSoulContractResult<()> {
    if entries.iter().any(|entry| {
        entry.namespace == address.namespace && entry.physical_key == address.physical_key
    }) {
        return Err(SubjectSoulContractError::repair(
            "Soul post-image address collides with an existing manifest owner",
        ));
    }
    Ok(())
}

fn validate_subject_soul_snapshot_owner(
    owner: &SubjectSoulOwnerV1,
    snapshot: &SubjectSoulVerifiedSnapshotV1,
) -> SubjectSoulContractResult<()> {
    if snapshot.head.memory_space_id != owner.memory_space_id
        || snapshot.head.subject_id != owner.subject_id
        || snapshot.head.soul_id != owner.soul_id
    {
        return Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::TargetNotMounted,
            reason: "verified Soul snapshot belongs to another owner".to_string(),
        });
    }
    Ok(())
}

fn validate_subject_soul_snapshot_documents(
    snapshot: &SubjectSoulVerifiedSnapshotV1,
) -> SubjectSoulContractResult<()> {
    match snapshot.head.state {
        SubjectSoulLifecycleStateV1::Active | SubjectSoulLifecycleStateV1::Archived => {
            let core = snapshot.current_core.as_ref().ok_or_else(|| {
                SubjectSoulContractError::repair("verified snapshot is missing current Core")
            })?;
            let ledger = snapshot.current_revision_ledger.as_ref().ok_or_else(|| {
                SubjectSoulContractError::repair("verified snapshot is missing revision ledger")
            })?;
            let core_document = snapshot.current_core_document.as_ref().ok_or_else(|| {
                SubjectSoulContractError::repair("verified snapshot is missing Core envelope")
            })?;
            let ledger_document = snapshot
                .current_revision_ledger_document
                .as_ref()
                .ok_or_else(|| {
                    SubjectSoulContractError::repair(
                        "verified snapshot is missing revision-ledger envelope",
                    )
                })?;
            for document in [core_document, ledger_document] {
                document.validate_contract()?;
                if document.memory_space_id != snapshot.head.memory_space_id
                    || document.subject_id != snapshot.head.subject_id
                    || document.soul_id != snapshot.head.soul_id
                    || document.generation != snapshot.head.generation
                    || document.revision != snapshot.head.current_revision
                {
                    return Err(SubjectSoulContractError::repair(
                        "verified snapshot envelope owner/revision mismatch",
                    ));
                }
            }
            if core_document.body
                != serde_json::to_value(core).map_err(|_| {
                    SubjectSoulContractError::repair("current Core is not serializable")
                })?
                || ledger_document.body
                    != serde_json::to_value(ledger).map_err(|_| {
                        SubjectSoulContractError::repair("revision ledger is not serializable")
                    })?
                || snapshot.head.current_ledger_digest.as_deref()
                    != Some(ledger_document.content_digest.as_str())
            {
                return Err(SubjectSoulContractError::repair(
                    "verified snapshot body/envelope binding mismatch",
                ));
            }
        }
        SubjectSoulLifecycleStateV1::Unseeded | SubjectSoulLifecycleStateV1::Deleted => {
            if snapshot.current_core_document.is_some()
                || snapshot.current_revision_ledger_document.is_some()
            {
                return Err(SubjectSoulContractError::repair(
                    "unseeded/deleted snapshot cannot carry current envelopes",
                ));
            }
        }
    }
    Ok(())
}

fn validate_subject_soul_expected_snapshot(
    expected: &SubjectSoulExpectedStateV1,
    snapshot: &SubjectSoulVerifiedSnapshotV1,
) -> SubjectSoulContractResult<()> {
    match expected {
        SubjectSoulExpectedStateV1::Exact {
            generation,
            revision,
            lifecycle_state,
            head_digest,
            manifest_digest,
        } if *generation == snapshot.head.generation
            && *revision == snapshot.head.current_revision
            && *lifecycle_state == snapshot.head.state
            && head_digest == &snapshot.head.head_digest
            && manifest_digest == &snapshot.manifest.closure_digest =>
        {
            Ok(())
        }
        _ => Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::GenerationConflict,
            reason: "Soul lifecycle expected state does not match the verified snapshot"
                .to_string(),
        }),
    }
}

fn ensure_no_lifecycle_allocation(
    revision_addresses: Option<&SubjectSoulRevisionAddressBindingsV1>,
    tombstone_physical_ref: Option<&str>,
) -> SubjectSoulContractResult<()> {
    if revision_addresses.is_some() || tombstone_physical_ref.is_some() {
        return Err(SubjectSoulContractError::repair(
            "non-destructive lifecycle transition cannot allocate artifacts",
        ));
    }
    Ok(())
}

fn required_tombstone_ref(value: Option<&str>) -> SubjectSoulContractResult<&str> {
    let value = value.ok_or_else(|| {
        SubjectSoulContractError::repair("destructive lifecycle requires a tombstone address")
    })?;
    validate_component(value, "tombstone_physical_ref")?;
    Ok(value)
}

fn destructive_system_actor(
    authority: &SubjectSoulLifecycleAuthorityV1,
) -> SubjectSoulContractResult<&str> {
    match authority {
        SubjectSoulLifecycleAuthorityV1::Destructive {
            system_actor_subject_id,
            ..
        } => Ok(system_actor_subject_id),
        _ => Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
            reason: "destructive lifecycle requires destructive authority".to_string(),
        }),
    }
}

fn destructive_human_actor(
    authority: &SubjectSoulLifecycleAuthorityV1,
) -> SubjectSoulContractResult<&str> {
    match authority {
        SubjectSoulLifecycleAuthorityV1::Destructive {
            human_confirmation, ..
        } => Ok(&human_confirmation.human_subject_id),
        _ => Err(SubjectSoulContractError {
            key: SubjectSoulLifecycleErrorKey::AuthorityDenied,
            reason: "reseed requires exact human confirmation".to_string(),
        }),
    }
}

fn build_subject_soul_tombstone(
    owner: &SubjectSoulOwnerV1,
    previous: &SubjectSoulVerifiedSnapshotV1,
    terminal_action: SubjectSoulTerminalActionV1,
    actor_subject_id: &str,
    reason_code: &str,
    recorded_at: u64,
    next_generation: Option<u64>,
) -> SubjectSoulContractResult<SubjectSoulGenerationTombstoneV1> {
    let mut tombstone = SubjectSoulGenerationTombstoneV1 {
        schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
        memory_space_id: owner.memory_space_id.clone(),
        subject_id: owner.subject_id.clone(),
        soul_id: owner.soul_id.clone(),
        generation: previous.head.generation,
        terminal_action,
        terminal_revision: previous.head.current_revision,
        terminal_material_digest: previous.head.current_material_digest.clone(),
        actor_subject_id: actor_subject_id.to_string(),
        reason_code: reason_code.to_string(),
        terminated_at: recorded_at,
        prior_head_digest: previous.head.head_digest.clone(),
        next_generation,
        tombstone_digest: String::new(),
    };
    tombstone.refresh_digest()?;
    tombstone.validate_contract()?;
    Ok(tombstone)
}

fn empty_subject_soul_manifest(
    owner: &SubjectSoulOwnerV1,
    generation: u64,
    manifest_revision: u64,
) -> SubjectSoulContractResult<SubjectSoulScopeManifestV1> {
    let mut manifest = SubjectSoulScopeManifestV1 {
        schema_version: SUBJECT_SOUL_SCHEMA_VERSION,
        memory_space_id: owner.memory_space_id.clone(),
        subject_id: owner.subject_id.clone(),
        soul_id: owner.soul_id.clone(),
        generation,
        manifest_revision,
        entries: Vec::new(),
        closure_digest: String::new(),
    };
    manifest.refresh_digest()?;
    Ok(manifest)
}

fn clear_subject_soul_current(head: &mut SubjectSoulLifecycleHeadV1) {
    head.current_revision = None;
    head.current_material_digest = None;
    head.current_ledger_digest = None;
}

fn next_subject_soul_generation(current: u64) -> SubjectSoulContractResult<u64> {
    current
        .checked_add(1)
        .ok_or_else(|| SubjectSoulContractError::repair("Soul generation overflow"))
}

fn next_subject_soul_manifest_revision(current: u64) -> SubjectSoulContractResult<u64> {
    current
        .checked_add(1)
        .ok_or_else(|| SubjectSoulContractError::repair("Soul manifest revision overflow"))
}

fn retain_subject_soul_tombstone_only(
    head: &mut SubjectSoulLifecycleHeadV1,
    previous: &SubjectSoulVerifiedSnapshotV1,
    tombstone_ref: &str,
) {
    head.retained_revision_refs.clear();
    head.retained_tombstone_refs = union_sorted(
        &previous.head.retained_tombstone_refs,
        &[tombstone_ref.to_string()],
    );
}

fn subject_soul_destructive_purge_set(
    previous: &SubjectSoulVerifiedSnapshotV1,
) -> (Vec<SubjectSoulManifestAddressV1>, Vec<String>) {
    let mut addresses = previous
        .manifest
        .entries
        .iter()
        .map(|entry| SubjectSoulManifestAddressV1 {
            namespace: entry.namespace.clone(),
            physical_key: entry.physical_key.clone(),
        })
        .collect::<Vec<_>>();
    addresses.sort_by(|left, right| {
        (&left.namespace, &left.physical_key).cmp(&(&right.namespace, &right.physical_key))
    });
    addresses.dedup();
    (addresses, previous.head.retained_revision_refs.clone())
}

fn validate_existing_projection_binding(
    snapshot: &SubjectSoulVerifiedSnapshotV1,
    existing_projection: Option<&SubjectSoulRelationshipProjectionV1>,
    address: &SubjectSoulManifestAddressV1,
) -> SubjectSoulContractResult<()> {
    let manifest_entry = snapshot.manifest.entries.iter().find(|entry| {
        entry.namespace == address.namespace && entry.physical_key == address.physical_key
    });
    match (existing_projection, manifest_entry) {
        (None, None) => Ok(()),
        (Some(projection), Some(entry)) => {
            validate_digest(&projection.content_digest, "projection.content_digest")?;
            let mut canonical = projection.clone();
            canonical.content_digest.clear();
            if canonical_digest("subject_soul_relationship_projection_v1", &canonical)?
                != projection.content_digest
                || projection.memory_space_id != snapshot.head.memory_space_id
                || projection.subject_id != snapshot.head.subject_id
                || projection.soul_id != snapshot.head.soul_id
                || projection.generation != snapshot.head.generation
                || entry.owner_role != SubjectSoulManifestOwnerRoleV1::RelationshipProjection
                || entry.content_digest != projection.content_digest
            {
                return Err(SubjectSoulContractError::repair(
                    "existing relationship projection/manifest binding is invalid",
                ));
            }
            Ok(())
        }
        _ => Err(SubjectSoulContractError::repair(
            "relationship projection and manifest membership must be exact",
        )),
    }
}

fn update_subject_soul_projection_manifest(
    snapshot: &SubjectSoulVerifiedSnapshotV1,
    address: &SubjectSoulManifestAddressV1,
    projection: Option<&SubjectSoulRelationshipProjectionV1>,
    recorded_at: u64,
) -> SubjectSoulContractResult<(SubjectSoulLifecycleHeadV1, SubjectSoulScopeManifestV1)> {
    let mut manifest = snapshot.manifest.clone();
    manifest.manifest_revision = manifest
        .manifest_revision
        .checked_add(1)
        .ok_or_else(|| SubjectSoulContractError::repair("manifest revision overflow"))?;
    manifest.entries.retain(|entry| {
        entry.namespace != address.namespace || entry.physical_key != address.physical_key
    });
    if let Some(projection) = projection {
        manifest.entries.push(SubjectSoulScopeManifestEntryV1 {
            namespace: address.namespace.clone(),
            physical_key: address.physical_key.clone(),
            owner_role: SubjectSoulManifestOwnerRoleV1::RelationshipProjection,
            generation: snapshot.head.generation,
            revision: Some(projection.soul_revision),
            content_digest: projection.content_digest.clone(),
        });
    }
    manifest.entries.sort();
    manifest.refresh_digest()?;
    let mut head = snapshot.head.clone();
    head.scope_manifest_digest = manifest.closure_digest.clone();
    head.updated_at = recorded_at;
    head.refresh_digest()?;
    let ledger_digest = snapshot
        .current_revision_ledger_document
        .as_ref()
        .map(|document| document.content_digest.as_str());
    validate_subject_soul_post_image(
        &head,
        &manifest,
        snapshot.current_material.as_ref(),
        snapshot.current_core.as_ref(),
        ledger_digest,
    )?;
    Ok((head, manifest))
}
