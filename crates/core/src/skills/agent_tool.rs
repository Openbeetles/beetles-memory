use crate::error::{Error, Result};
use crate::feature_gate::ProfileId;
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub const AGENT_TOOL_NO_EXPERIENCE_REASON: &str = "no_governed_tool_experience";
pub const AGENT_TOOL_REGISTRY_FORBIDDEN_BY_PROFILE: &str =
    "agent_tool_registry_forbidden_by_profile";
pub const AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH: &str =
    "agent_tool_registry_fingerprint_mismatch";

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentToolRegistryScope {
    #[default]
    Global,
    Owner,
    Project {
        project_id: String,
    },
    Workspace {
        workspace_id: String,
    },
    Conversation {
        conversation_id: String,
    },
}

impl AgentToolRegistryScope {
    pub fn validate_contract(&self) -> bool {
        match self {
            Self::Global | Self::Owner => true,
            Self::Project { project_id } => canonical_registry_identifier(project_id),
            Self::Workspace { workspace_id } => canonical_registry_identifier(workspace_id),
            Self::Conversation { conversation_id } => {
                canonical_registry_identifier(conversation_id)
            }
        }
    }
}

fn canonical_registry_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolRegistryOwner {
    #[default]
    HostRuntime,
    AgentTools,
    RequestScopedGateway,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolRegistryRef {
    pub registry_id: String,
    pub fingerprint: String,
    pub scope: AgentToolRegistryScope,
}

impl AgentToolRegistryRef {
    pub fn new(registry_id: impl Into<String>, fingerprint: impl Into<String>) -> Self {
        Self {
            registry_id: registry_id.into(),
            fingerprint: fingerprint.into(),
            scope: AgentToolRegistryScope::Global,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AgentToolDescriptor {
    pub tool_id: String,
    pub display_name: String,
    pub version: Option<String>,
    pub schema_fingerprint: String,
    pub descriptor_fingerprint: String,
    pub permission_tags: Vec<String>,
    pub risk_tags: Vec<String>,
    pub tool_groups: Vec<String>,
    pub disabled: bool,
}

impl AgentToolDescriptor {
    pub fn compact(
        tool_id: impl Into<String>,
        display_name: impl Into<String>,
        schema_fingerprint: impl Into<String>,
    ) -> Self {
        let mut descriptor = Self {
            tool_id: tool_id.into(),
            display_name: display_name.into(),
            schema_fingerprint: schema_fingerprint.into(),
            ..Self::default()
        };
        descriptor.descriptor_fingerprint = fingerprint_agent_tool_descriptor(&descriptor);
        descriptor
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolRegistrySnapshot {
    pub registry_id: String,
    pub namespace: String,
    pub owner: AgentToolRegistryOwner,
    pub scope: AgentToolRegistryScope,
    pub fingerprint: String,
    pub tools: Vec<AgentToolDescriptor>,
    pub registered_at: u64,
}

impl AgentToolRegistrySnapshot {
    pub fn compact(
        registry_id: impl Into<String>,
        namespace: impl Into<String>,
        tools: Vec<AgentToolDescriptor>,
        registered_at: u64,
    ) -> Self {
        let mut snapshot = Self {
            registry_id: registry_id.into(),
            namespace: namespace.into(),
            owner: AgentToolRegistryOwner::HostRuntime,
            scope: AgentToolRegistryScope::Global,
            fingerprint: String::new(),
            tools,
            registered_at,
        };
        for tool in &mut snapshot.tools {
            tool.descriptor_fingerprint = fingerprint_agent_tool_descriptor(tool);
        }
        snapshot.fingerprint = fingerprint_agent_tool_registry(&snapshot);
        snapshot
    }

    pub fn registry_ref(&self) -> AgentToolRegistryRef {
        AgentToolRegistryRef {
            registry_id: self.registry_id.clone(),
            fingerprint: self.fingerprint.clone(),
            scope: self.scope.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolRegistryReport {
    pub registries: usize,
    pub tools: usize,
    pub disabled_tools: usize,
    pub read_availability: crate::memory::ProceduralLearningReadAvailabilityV1,
    pub governed_experiences: Option<usize>,
    pub stale_experiences: Option<usize>,
    pub forbidden_by_profile: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolOutcome {
    Succeeded,
    Failed,
    Partial,
    Cancelled,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolExperienceConfidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolExperienceStatus {
    Candidate,
    Active,
    Stale,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolExperienceRecord {
    pub experience_id: String,
    pub registry_id: String,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub task_signature: String,
    pub usage_guidance: String,
    pub constraints: Vec<String>,
    pub status: AgentToolExperienceStatus,
    pub privacy_class: crate::memory::MemoryPrivacyClass,
    pub created_at: u64,
    pub updated_at: u64,
}

impl AgentToolExperienceRecord {
    pub fn active(
        experience_id: impl Into<String>,
        registry_id: impl Into<String>,
        tool_id: impl Into<String>,
        schema_fingerprint: impl Into<String>,
        usage_guidance: impl Into<String>,
        updated_at: u64,
    ) -> Self {
        Self {
            experience_id: experience_id.into(),
            registry_id: registry_id.into(),
            tool_id: tool_id.into(),
            schema_fingerprint: schema_fingerprint.into(),
            task_signature: String::new(),
            usage_guidance: usage_guidance.into(),
            constraints: Vec::new(),
            status: AgentToolExperienceStatus::Active,
            privacy_class: crate::memory::MemoryPrivacyClass::SharedWithSubject,
            created_at: updated_at,
            updated_at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolHint {
    pub registry_id: String,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub experience_id: String,
    pub reason: String,
    pub permission_tags: Vec<String>,
    pub risk_tags: Vec<String>,
    pub constraints: Vec<String>,
    pub host_execution_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolProjectionRejection {
    pub registry_id: String,
    pub tool_id: String,
    pub experience_id: Option<String>,
    pub reason: String,
}

impl AgentToolProjectionRejection {
    /// Denials must not disclose an inaccessible owner's identity or content.
    fn safe(reason: &'static str) -> Self {
        Self {
            registry_id: String::new(),
            tool_id: String::new(),
            experience_id: None,
            reason: reason.to_string(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolProjectionAudit {
    pub selected: Vec<AgentToolHint>,
    pub rejected: Vec<AgentToolProjectionRejection>,
    pub budget_limited: bool,
    pub cold_start_selection_used: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolExperienceStatusReport {
    pub available: bool,
    pub reason: String,
    pub host_fallback_required: bool,
    pub cold_start_selection_used: bool,
    pub registry_refs_checked: usize,
    pub read_availability: crate::memory::ProceduralLearningReadAvailabilityV1,
    pub governed_experience_candidates: Option<usize>,
}

impl AgentToolExperienceStatusReport {
    pub fn no_experience(registry_refs_checked: usize, candidates: usize) -> Self {
        Self {
            available: false,
            reason: AGENT_TOOL_NO_EXPERIENCE_REASON.to_string(),
            host_fallback_required: true,
            cold_start_selection_used: false,
            registry_refs_checked,
            read_availability: crate::memory::ProceduralLearningReadAvailabilityV1::Ready,
            governed_experience_candidates: Some(candidates),
        }
    }

    pub fn available(registry_refs_checked: usize, candidates: usize) -> Self {
        Self {
            available: true,
            reason: "governed_tool_experience_available".to_string(),
            host_fallback_required: false,
            cold_start_selection_used: false,
            registry_refs_checked,
            read_availability: crate::memory::ProceduralLearningReadAvailabilityV1::Ready,
            governed_experience_candidates: Some(candidates),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolSelectionReport {
    pub tool_hints: Vec<AgentToolHint>,
    pub selection_bindings: Vec<crate::memory::AgentToolExperienceSelectionV1>,
    pub tool_experience_status: AgentToolExperienceStatusReport,
    pub audit: AgentToolProjectionAudit,
}

impl AgentToolSelectionReport {
    pub fn unavailable(
        registry_refs_checked: usize,
        availability: crate::memory::ProceduralLearningReadAvailabilityV1,
    ) -> Self {
        Self {
            tool_hints: Vec::new(),
            selection_bindings: Vec::new(),
            tool_experience_status: AgentToolExperienceStatusReport {
                available: false,
                reason: availability.reason().into(),
                host_fallback_required: true,
                cold_start_selection_used: false,
                registry_refs_checked,
                read_availability: availability,
                governed_experience_candidates: None,
            },
            audit: AgentToolProjectionAudit::default(),
        }
    }

    pub fn empty(registry_refs_checked: usize, candidates: usize) -> Self {
        Self {
            tool_hints: Vec::new(),
            selection_bindings: Vec::new(),
            tool_experience_status: AgentToolExperienceStatusReport::no_experience(
                registry_refs_checked,
                candidates,
            ),
            audit: AgentToolProjectionAudit::default(),
        }
    }
}

pub fn agent_tool_registries_forbidden_by_profile(profile: ProfileId) -> bool {
    crate::feature_gate::profile_capability_catalog()
        .iter()
        .find(|entry| entry.profile == profile)
        .is_none_or(|entry| !entry.procedural_learning.agent_tool_registry)
}

pub fn validate_agent_tool_registry_snapshot(
    profile: ProfileId,
    snapshot: &AgentToolRegistrySnapshot,
) -> Result<()> {
    if agent_tool_registries_forbidden_by_profile(profile) {
        return Err(Error::config(
            "agent_tool_registry",
            AGENT_TOOL_REGISTRY_FORBIDDEN_BY_PROFILE,
        ));
    }
    validate_agent_tool_registry_identity(snapshot)?;
    if snapshot.fingerprint != fingerprint_agent_tool_registry(snapshot) {
        return Err(Error::config(
            "agent_tool_registry",
            AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH,
        ));
    }
    Ok(())
}

fn validate_agent_tool_registry_identity(snapshot: &AgentToolRegistrySnapshot) -> Result<()> {
    if !canonical_registry_identifier(&snapshot.registry_id) {
        return Err(Error::config(
            "agent_tool_registry",
            "agent_tool_registry_id_noncanonical",
        ));
    }
    if !snapshot.scope.validate_contract() {
        return Err(Error::config(
            "agent_tool_registry",
            "agent_tool_registry_scope_noncanonical",
        ));
    }
    let mut seen = HashSet::new();
    for tool in &snapshot.tools {
        if !canonical_registry_identifier(&tool.tool_id) {
            return Err(Error::config(
                "agent_tool_registry",
                "agent_tool_id_noncanonical",
            ));
        }
        if !seen.insert(tool.tool_id.as_str()) {
            return Err(Error::config(
                "agent_tool_registry",
                "agent_tool_id_duplicate",
            ));
        }
        if !canonical_registry_identifier(&tool.schema_fingerprint) {
            return Err(Error::config(
                "agent_tool_registry",
                "agent_tool_schema_fingerprint_noncanonical",
            ));
        }
    }
    Ok(())
}

pub fn build_agent_tool_registry_report(
    profile: ProfileId,
    registries: &[AgentToolRegistrySnapshot],
    experiences: &[AgentToolExperienceRecord],
) -> AgentToolRegistryReport {
    let registry_ids = registries
        .iter()
        .map(|registry| registry.registry_id.as_str())
        .collect::<BTreeSet<_>>();
    let tools = registries
        .iter()
        .map(|registry| registry.tools.len())
        .sum::<usize>();
    let disabled_tools = registries
        .iter()
        .flat_map(|registry| registry.tools.iter())
        .filter(|tool| tool.disabled)
        .count();
    let stale_experiences = experiences
        .iter()
        .filter(|experience| {
            !registry_ids.contains(experience.registry_id.as_str())
                || !tool_exists_with_schema(registries, experience)
        })
        .count();
    AgentToolRegistryReport {
        registries: registries.len(),
        tools,
        disabled_tools,
        read_availability: if agent_tool_registries_forbidden_by_profile(profile) {
            crate::memory::ProceduralLearningReadAvailabilityV1::ProfileUnavailable
        } else {
            crate::memory::ProceduralLearningReadAvailabilityV1::Ready
        },
        governed_experiences: (!agent_tool_registries_forbidden_by_profile(profile))
            .then_some(experiences.len()),
        stale_experiences: (!agent_tool_registries_forbidden_by_profile(profile))
            .then_some(stale_experiences),
        forbidden_by_profile: agent_tool_registries_forbidden_by_profile(profile),
        warnings: Vec::new(),
    }
}

fn select_agent_tool_hints(
    registries: &[AgentToolRegistrySnapshot],
    experiences: &[AgentToolExperienceRecord],
    registry_refs: &[AgentToolRegistryRef],
    max_hints: usize,
) -> AgentToolSelectionReport {
    if max_hints == 0 {
        return AgentToolSelectionReport::empty(registry_refs.len(), 0);
    }
    let refs = if registry_refs.is_empty() {
        registries
            .iter()
            .map(AgentToolRegistrySnapshot::registry_ref)
            .collect::<Vec<_>>()
    } else {
        registry_refs.to_vec()
    };
    let mut rejected = Vec::new();
    let mut candidates = experiences
        .iter()
        .filter_map(|experience| {
            if experience.status != AgentToolExperienceStatus::Active {
                rejected.push(AgentToolProjectionRejection::safe(
                    "agent_tool_experience_status_ineligible",
                ));
                return None;
            }
            if !experience.privacy_class.projection_content_allowed() {
                rejected.push(AgentToolProjectionRejection::safe(
                    "agent_tool_experience_privacy_blocked",
                ));
                return None;
            }
            let Some(registry) = registries
                .iter()
                .find(|registry| registry.registry_id == experience.registry_id)
            else {
                rejected.push(AgentToolProjectionRejection::safe(
                    "agent_tool_registry_not_found",
                ));
                return None;
            };
            if registries
                .iter()
                .filter(|candidate| candidate.registry_id == registry.registry_id)
                .count()
                != 1
            {
                rejected.push(AgentToolProjectionRejection::safe(
                    "agent_tool_registry_identity_ambiguous",
                ));
                return None;
            }
            if validate_agent_tool_registry_identity(registry).is_err() {
                rejected.push(AgentToolProjectionRejection::safe(
                    "agent_tool_registry_identity_invalid",
                ));
                return None;
            }
            if registry.fingerprint != fingerprint_agent_tool_registry(registry) {
                rejected.push(AgentToolProjectionRejection {
                    registry_id: experience.registry_id.clone(),
                    tool_id: experience.tool_id.clone(),
                    experience_id: Some(experience.experience_id.clone()),
                    reason: AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH.to_string(),
                });
                return None;
            }
            if !refs
                .iter()
                .any(|reference| reference == &registry.registry_ref())
            {
                rejected.push(AgentToolProjectionRejection::safe(
                    "agent_tool_registry_ref_mismatch",
                ));
                return None;
            }
            let Some(tool) = registry
                .tools
                .iter()
                .find(|tool| tool.tool_id == experience.tool_id)
            else {
                rejected.push(AgentToolProjectionRejection::safe(
                    "agent_tool_not_in_registry",
                ));
                return None;
            };
            if tool.disabled {
                rejected.push(AgentToolProjectionRejection {
                    registry_id: experience.registry_id.clone(),
                    tool_id: experience.tool_id.clone(),
                    experience_id: Some(experience.experience_id.clone()),
                    reason: "agent_tool_disabled_by_registry".to_string(),
                });
                return None;
            }
            if tool.schema_fingerprint != experience.schema_fingerprint {
                rejected.push(AgentToolProjectionRejection {
                    registry_id: experience.registry_id.clone(),
                    tool_id: experience.tool_id.clone(),
                    experience_id: Some(experience.experience_id.clone()),
                    reason: "agent_tool_experience_stale_schema".to_string(),
                });
                return None;
            }
            let reason = experience.usage_guidance.clone();
            Some((
                experience,
                AgentToolHint {
                    registry_id: experience.registry_id.clone(),
                    tool_id: experience.tool_id.clone(),
                    schema_fingerprint: experience.schema_fingerprint.clone(),
                    experience_id: experience.experience_id.clone(),
                    reason,
                    permission_tags: tool.permission_tags.clone(),
                    risk_tags: tool.risk_tags.clone(),
                    constraints: experience.constraints.clone(),
                    host_execution_required: true,
                },
            ))
        })
        .collect::<Vec<_>>();
    let governed_candidates = candidates.len();
    candidates.sort_by(|(left_exp, _), (right_exp, _)| {
        right_exp
            .updated_at
            .cmp(&left_exp.updated_at)
            .then_with(|| left_exp.tool_id.cmp(&right_exp.tool_id))
    });
    let budget_limited = candidates.len() > max_hints;
    let tool_hints = candidates
        .into_iter()
        .take(max_hints)
        .map(|(_, hint)| hint)
        .collect::<Vec<_>>();
    if tool_hints.is_empty() {
        return AgentToolSelectionReport {
            audit: AgentToolProjectionAudit {
                selected: Vec::new(),
                rejected,
                budget_limited: false,
                cold_start_selection_used: false,
            },
            ..AgentToolSelectionReport::empty(refs.len(), governed_candidates)
        };
    }
    AgentToolSelectionReport {
        selection_bindings: Vec::new(),
        tool_experience_status: AgentToolExperienceStatusReport::available(
            refs.len(),
            governed_candidates,
        ),
        audit: AgentToolProjectionAudit {
            selected: tool_hints.clone(),
            rejected,
            budget_limited,
            cold_start_selection_used: false,
        },
        tool_hints,
    }
}

/// Selection consumes only the exact immutable subject closure. Registry capability and
/// applicability are current authority even when the memory revision is historical.
pub struct AgentToolExperienceSelectionInput<'a> {
    pub query: &'a str,
    pub memory_space_id: &'a str,
    pub owning_scope: &'a super::AgentToolExperienceOwningScopeV1,
    pub owners: &'a [super::AgentToolExperienceReadProjectionV1],
    pub registries: &'a [AgentToolRegistrySnapshot],
    pub registry_refs: &'a [AgentToolRegistryRef],
    pub applicability: &'a crate::memory::ProceduralApplicabilityContextV1,
    pub as_of_time: Option<u64>,
    pub max_hints: usize,
}

pub fn select_subject_agent_tool_hints(
    input: AgentToolExperienceSelectionInput<'_>,
) -> Result<AgentToolSelectionReport> {
    if !input.applicability.validate_contract() || !input.owning_scope.validate_contract() {
        return Err(Error::config(
            "agent_tool_experience_selection",
            "invalid applicability or subject scope",
        ));
    }
    let mut selected = Vec::new();
    let mut bindings = BTreeMap::new();
    let mut rejected = Vec::new();
    for owner in input.owners {
        if owner.memory_space_id() != input.memory_space_id
            || owner.owning_scope() != input.owning_scope
            || owner.as_of_time() != input.as_of_time
        {
            return Err(Error::config(
                "agent_tool_experience_selection",
                "owner does not match exact scope",
            ));
        }
        if let Some(reason) = owner.rejection() {
            rejected.push(AgentToolProjectionRejection::safe(reason));
            continue;
        }
        let material = owner.material().ok_or_else(|| {
            Error::config(
                "agent_tool_experience_selection",
                "allowed projection has no exact material",
            )
        })?;
        if material.memory_space_id != input.memory_space_id
            || &material.owning_scope != input.owning_scope
        {
            return Err(Error::config(
                "agent_tool_experience_selection",
                "material does not match exact scope",
            ));
        }
        if !material.privacy_class.projection_content_allowed() {
            rejected.push(AgentToolProjectionRejection::safe(
                "agent_tool_experience_privacy_blocked",
            ));
            continue;
        }
        if !input
            .applicability
            .permits_registry_scope(&material.registry_scope)
        {
            rejected.push(AgentToolProjectionRejection::safe(
                "agent_tool_experience_applicability_blocked",
            ));
            continue;
        }
        let Some(registry) = input.registries.iter().find(|registry| {
            registry.registry_id == material.registry_id
                && registry.scope == material.registry_scope
        }) else {
            rejected.push(AgentToolProjectionRejection::safe(
                "agent_tool_registry_not_found",
            ));
            continue;
        };
        if validate_agent_tool_registry_identity(registry).is_err() {
            rejected.push(AgentToolProjectionRejection::safe(
                "agent_tool_registry_identity_invalid",
            ));
            continue;
        }
        if registry.fingerprint != fingerprint_agent_tool_registry(registry) {
            rejected.push(AgentToolProjectionRejection::safe(
                AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH,
            ));
            continue;
        }
        if !input.registry_refs.is_empty()
            && !input
                .registry_refs
                .iter()
                .any(|reference| reference == &registry.registry_ref())
        {
            rejected.push(AgentToolProjectionRejection::safe(
                "agent_tool_registry_ref_mismatch",
            ));
            continue;
        }
        let super::AgentToolExperienceBodyV1::Method {
            task_signature,
            procedure,
            constraints,
            ..
        } = &material.body
        else {
            // Statistics do not constitute a governed method or a cold-start hint.
            continue;
        };
        bindings.insert(
            material.owner_ref.owner_id.clone(),
            crate::memory::AgentToolExperienceSelectionV1 {
                registry_ref: registry.registry_ref(),
                tool_id: material.tool_id.clone(),
                schema_fingerprint: material.schema_fingerprint.clone(),
                experience_owner_id: material.owner_ref.owner_id.clone(),
                experience_revision: material.owner_revision,
                experience_content_digest: material.content_digest.clone(),
            },
        );
        selected.push(AgentToolExperienceRecord {
            experience_id: material.owner_ref.owner_id.clone(),
            registry_id: material.registry_id.clone(),
            tool_id: material.tool_id.clone(),
            schema_fingerprint: material.schema_fingerprint.clone(),
            task_signature: task_signature.clone(),
            usage_guidance: procedure.clone(),
            constraints: constraints.clone(),
            status: material.status,
            privacy_class: material.privacy_class,
            created_at: material.created_at,
            updated_at: material.updated_at,
        });
    }
    // Task signatures are opaque identity: never tokenize them. Natural-language
    // relevance uses only governed experience text and the shared recall scorer.
    let texts = selected
        .iter()
        .map(|experience| experience.usage_guidance.clone())
        .collect::<Vec<_>>();
    let documents = selected
        .iter()
        .zip(&texts)
        .map(|(experience, text)| crate::memory::RecallDeliveryText {
            candidate_id: &experience.experience_id,
            text,
        })
        .collect::<Vec<_>>();
    let lexical = crate::memory::score_recall_delivery_texts(input.query, &documents)
        .into_iter()
        .map(|score| (score.candidate_id, score.score))
        .collect::<BTreeMap<_, _>>();
    let anchored = documents
        .iter()
        .filter(|document| {
            crate::memory::has_recall_delivery_lexical_anchor(input.query, document.text)
        })
        .map(|document| document.candidate_id.to_string())
        .collect::<BTreeSet<_>>();
    let mut relevance = BTreeMap::new();
    selected.retain(|experience| {
        let exact_signature = input.query.trim() == experience.task_signature;
        let score = lexical.get(&experience.experience_id).copied().unwrap_or(0);
        if !exact_signature && !anchored.contains(&experience.experience_id) {
            rejected.push(AgentToolProjectionRejection::safe(
                "agent_tool_experience_query_unrelated",
            ));
            return false;
        }
        relevance.insert(experience.experience_id.clone(), (exact_signature, score));
        true
    });
    let mut report =
        select_agent_tool_hints(input.registries, &selected, input.registry_refs, usize::MAX);
    report.tool_hints.sort_by(|left, right| {
        relevance
            .get(&right.experience_id)
            .cmp(&relevance.get(&left.experience_id))
    });
    report.audit.budget_limited = report.tool_hints.len() > input.max_hints;
    report.tool_hints.truncate(input.max_hints);
    report.selection_bindings = report
        .tool_hints
        .iter()
        .filter_map(|hint| bindings.remove(&hint.experience_id))
        .collect();
    report.audit.selected = report.tool_hints.clone();
    report.audit.rejected.extend(rejected);
    if report.tool_hints.is_empty() {
        report.tool_experience_status.available = false;
        report.tool_experience_status.reason = AGENT_TOOL_NO_EXPERIENCE_REASON.into();
        report.tool_experience_status.host_fallback_required = true;
    }
    Ok(report)
}

pub fn fingerprint_agent_tool_descriptor(descriptor: &AgentToolDescriptor) -> String {
    let mut buffer = String::new();
    push_hash_part(&mut buffer, &descriptor.tool_id);
    push_hash_part(&mut buffer, &descriptor.display_name);
    push_hash_part(&mut buffer, descriptor.version.as_deref().unwrap_or(""));
    push_hash_part(&mut buffer, &descriptor.schema_fingerprint);
    push_hash_parts(&mut buffer, &descriptor.permission_tags);
    push_hash_parts(&mut buffer, &descriptor.risk_tags);
    push_hash_parts(&mut buffer, &descriptor.tool_groups);
    push_hash_part(
        &mut buffer,
        if descriptor.disabled {
            "disabled"
        } else {
            "enabled"
        },
    );
    format!("{:016x}", fnv1a64(buffer.as_bytes()))
}

pub fn fingerprint_agent_tool_registry(snapshot: &AgentToolRegistrySnapshot) -> String {
    let mut buffer = String::new();
    push_hash_part(&mut buffer, &snapshot.registry_id);
    push_hash_part(&mut buffer, &snapshot.namespace);
    push_hash_part(&mut buffer, canonical_registry_owner(&snapshot.owner));
    push_hash_part(&mut buffer, &canonical_registry_scope(&snapshot.scope));

    let mut tools = snapshot
        .tools
        .iter()
        .map(|tool| {
            (
                tool.tool_id.as_str(),
                tool.schema_fingerprint.as_str(),
                fingerprint_agent_tool_descriptor(tool),
            )
        })
        .collect::<Vec<_>>();
    tools.sort_by(|left, right| {
        left.0
            .cmp(right.0)
            .then(left.1.cmp(right.1))
            .then(left.2.cmp(&right.2))
    });
    for (tool_id, schema_fingerprint, descriptor_fingerprint) in tools {
        push_hash_part(&mut buffer, tool_id);
        push_hash_part(&mut buffer, schema_fingerprint);
        push_hash_part(&mut buffer, &descriptor_fingerprint);
    }
    format!("{:016x}", fnv1a64(buffer.as_bytes()))
}

fn tool_exists_with_schema(
    registries: &[AgentToolRegistrySnapshot],
    experience: &AgentToolExperienceRecord,
) -> bool {
    registries
        .iter()
        .find(|registry| registry.registry_id == experience.registry_id)
        .and_then(|registry| {
            registry
                .tools
                .iter()
                .find(|tool| tool.tool_id == experience.tool_id)
        })
        .map(|tool| tool.schema_fingerprint == experience.schema_fingerprint)
        .unwrap_or(false)
}

fn push_hash_part(buffer: &mut String, value: &str) {
    buffer.push_str(&value.len().to_string());
    buffer.push(':');
    buffer.push_str(value);
    buffer.push('|');
}

fn push_hash_parts(buffer: &mut String, values: &[String]) {
    let mut sorted = values.iter().map(String::as_str).collect::<Vec<_>>();
    sorted.sort_unstable();
    for value in sorted {
        push_hash_part(buffer, value);
    }
}

fn canonical_registry_owner(owner: &AgentToolRegistryOwner) -> &'static str {
    match owner {
        AgentToolRegistryOwner::HostRuntime => "host-runtime",
        AgentToolRegistryOwner::AgentTools => "agent-tools",
        AgentToolRegistryOwner::RequestScopedGateway => "request-scoped-gateway",
    }
}

fn canonical_registry_scope(scope: &AgentToolRegistryScope) -> String {
    match scope {
        AgentToolRegistryScope::Global => "global".to_string(),
        AgentToolRegistryScope::Owner => "owner".to_string(),
        AgentToolRegistryScope::Project { project_id } => format!("project:{project_id}"),
        AgentToolRegistryScope::Workspace { workspace_id } => {
            format!("workspace:{workspace_id}")
        }
        AgentToolRegistryScope::Conversation { conversation_id } => {
            format!("conversation:{conversation_id}")
        }
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registry() -> AgentToolRegistrySnapshot {
        let mut tool =
            AgentToolDescriptor::compact("pdf.extract", "Extract PDF text", "schema-pdf-v1");
        tool.permission_tags = vec!["filesystem.read".to_string()];
        tool.risk_tags = vec!["read_only".to_string()];
        AgentToolRegistrySnapshot::compact("host-tools", "host", vec![tool], 100)
    }

    #[test]
    fn no_experience_returns_empty_hints_without_cold_start_selection() {
        let registry = registry();
        let report = select_agent_tool_hints(&[registry], &[], &[], 5);
        assert!(report.tool_hints.is_empty());
        assert!(!report.audit.cold_start_selection_used);
        assert_eq!(
            report.tool_experience_status.reason,
            AGENT_TOOL_NO_EXPERIENCE_REASON
        );
        assert!(report.tool_experience_status.host_fallback_required);
    }

    #[test]
    fn governed_experience_returns_structured_hint() {
        let registry = registry();
        let experience = AgentToolExperienceRecord::active(
            "exp-1",
            "host-tools",
            "pdf.extract",
            "schema-pdf-v1",
            "Use after host decides PDF tools are visible.",
            200,
        );
        let report = select_agent_tool_hints(&[registry], &[experience], &[], 5);
        assert_eq!(report.tool_hints.len(), 1);
        assert_eq!(report.tool_hints[0].tool_id, "pdf.extract");
        assert!(report.tool_hints[0].host_execution_required);
        assert!(!report.audit.cold_start_selection_used);
    }

    #[test]
    fn schema_drift_rejects_stale_experience() {
        let registry = registry();
        let experience = AgentToolExperienceRecord::active(
            "exp-1",
            "host-tools",
            "pdf.extract",
            "schema-pdf-v0",
            "old",
            200,
        );
        let report = select_agent_tool_hints(&[registry], &[experience], &[], 5);
        assert!(report.tool_hints.is_empty());
        assert!(report
            .audit
            .rejected
            .iter()
            .any(|item| item.reason == "agent_tool_experience_stale_schema"));
    }

    #[test]
    fn registry_fingerprint_is_stable_for_descriptor_and_tag_order() {
        let mut left = registry();
        left.tools[0].permission_tags =
            vec!["filesystem.read".to_string(), "network.read".to_string()];
        left.tools[0].risk_tags = vec!["external_content".to_string(), "read_only".to_string()];
        left.tools.push(AgentToolDescriptor::compact(
            "image.resize",
            "Resize image",
            "schema-image-v1",
        ));
        left.tools[0].descriptor_fingerprint = fingerprint_agent_tool_descriptor(&left.tools[0]);
        left.fingerprint = fingerprint_agent_tool_registry(&left);

        let mut right = left.clone();
        right.tools.reverse();
        right.tools[1].permission_tags.reverse();
        right.tools[1].risk_tags.reverse();
        right.tools[1].descriptor_fingerprint = "stale-descriptor-fingerprint".to_string();
        right.fingerprint = fingerprint_agent_tool_registry(&right);

        assert_eq!(left.fingerprint, right.fingerprint);
    }
}
