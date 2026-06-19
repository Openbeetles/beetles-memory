use crate::error::{Error, Result};
use crate::feature_gate::ProfileId;
use crate::platform::SkillStorage;
use crate::skills::runtime::RuntimeSkillStorageMutation;
use std::collections::{BTreeSet, HashSet};

pub const AGENT_TOOL_NO_EXPERIENCE_REASON: &str = "no_governed_tool_experience";
pub const AGENT_TOOL_REGISTRY_FORBIDDEN_BY_PROFILE: &str =
    "agent_tool_registry_forbidden_by_profile";
pub const AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH: &str =
    "agent_tool_registry_fingerprint_mismatch";
const AGENT_TOOL_EXPERIENCE_PREFIX: &str = "agent_tool_experience__";

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
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
    pub governed_experiences: usize,
    pub stale_experiences: usize,
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
    pub trigger_summary: String,
    pub usage_guidance: String,
    pub constraints: Vec<String>,
    pub evidence_count: u32,
    pub success_count: u32,
    pub failure_count: u32,
    pub last_outcome: AgentToolOutcome,
    pub confidence: AgentToolExperienceConfidence,
    pub status: AgentToolExperienceStatus,
    pub evidence_refs: Vec<String>,
    pub private_content_used: bool,
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
            trigger_summary: String::new(),
            usage_guidance: usage_guidance.into(),
            constraints: Vec::new(),
            evidence_count: 2,
            success_count: 2,
            failure_count: 0,
            last_outcome: AgentToolOutcome::Succeeded,
            confidence: AgentToolExperienceConfidence::High,
            status: AgentToolExperienceStatus::Active,
            evidence_refs: Vec::new(),
            private_content_used: false,
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
    pub confidence: AgentToolExperienceConfidence,
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
    pub governed_experience_candidates: usize,
}

impl AgentToolExperienceStatusReport {
    pub fn no_experience(registry_refs_checked: usize, candidates: usize) -> Self {
        Self {
            available: false,
            reason: AGENT_TOOL_NO_EXPERIENCE_REASON.to_string(),
            host_fallback_required: true,
            cold_start_selection_used: false,
            registry_refs_checked,
            governed_experience_candidates: candidates,
        }
    }

    pub fn available(registry_refs_checked: usize, candidates: usize) -> Self {
        Self {
            available: true,
            reason: "governed_tool_experience_available".to_string(),
            host_fallback_required: false,
            cold_start_selection_used: false,
            registry_refs_checked,
            governed_experience_candidates: candidates,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolSelectionReport {
    pub tool_hints: Vec<AgentToolHint>,
    pub tool_experience_status: AgentToolExperienceStatusReport,
    pub audit: AgentToolProjectionAudit,
}

impl AgentToolSelectionReport {
    pub fn empty(registry_refs_checked: usize, candidates: usize) -> Self {
        Self {
            tool_hints: Vec::new(),
            tool_experience_status: AgentToolExperienceStatusReport::no_experience(
                registry_refs_checked,
                candidates,
            ),
            audit: AgentToolProjectionAudit::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolObservationDigest {
    pub observation_id: String,
    pub registry_id: String,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub call_id: Option<String>,
    pub task_signature: String,
    pub summary: String,
    pub outcome: AgentToolOutcome,
    pub error_code: Option<String>,
    pub external_content: bool,
    pub private_content_used: bool,
    pub permission_tags: Vec<String>,
    pub risk_tags: Vec<String>,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolUsageFeedback {
    pub registry_ref: AgentToolRegistryRef,
    pub observations: Vec<AgentToolObservationDigest>,
    pub user_visible_result_summary: Option<String>,
    pub reuse_outcome: crate::skills::RuntimeSkillReuseOutcome,
    pub operator_note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolExperienceGovernanceDecision {
    AcceptedAsEvidence,
    DeferredUntilRepeated,
    MergedIntoExistingExperience,
    PromotedToRuntimeSkill,
    RejectedByPrivacy,
    RejectedBySchemaDrift,
    RejectedByLowConfidence,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentToolExperienceGovernanceReport {
    pub accepted: bool,
    pub changed: usize,
    pub decision: AgentToolExperienceGovernanceDecision,
    pub reason: String,
    pub experience: Option<AgentToolExperienceRecord>,
}

pub fn agent_tool_registries_forbidden_by_profile(profile: ProfileId) -> bool {
    matches!(
        profile,
        ProfileId::EspStandaloneMemory | ProfileId::EspEmbeddedSdk
    )
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
    if snapshot.registry_id.trim().is_empty() {
        return Err(Error::config(
            "agent_tool_registry",
            "agent_tool_registry_id_empty",
        ));
    }
    let mut seen = HashSet::new();
    for tool in &snapshot.tools {
        if tool.tool_id.trim().is_empty() {
            return Err(Error::config("agent_tool_registry", "agent_tool_id_empty"));
        }
        if !seen.insert(tool.tool_id.as_str()) {
            return Err(Error::config(
                "agent_tool_registry",
                "agent_tool_id_duplicate",
            ));
        }
        if tool.schema_fingerprint.trim().is_empty() {
            return Err(Error::config(
                "agent_tool_registry",
                "agent_tool_schema_fingerprint_empty",
            ));
        }
    }
    if !snapshot.fingerprint.trim().is_empty()
        && snapshot.fingerprint != fingerprint_agent_tool_registry(snapshot)
    {
        return Err(Error::config(
            "agent_tool_registry",
            AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH,
        ));
    }
    Ok(())
}

pub fn list_agent_tool_experience_records(
    storage: &dyn SkillStorage,
) -> Vec<AgentToolExperienceRecord> {
    let mut records = Vec::new();
    for name in super::list_skill_names(storage) {
        if !name.starts_with(AGENT_TOOL_EXPERIENCE_PREFIX) {
            continue;
        }
        let Some(content) = super::get_skill_content(storage, &name) else {
            continue;
        };
        if let Ok(record) = serde_json::from_str::<AgentToolExperienceRecord>(&content) {
            records.push(record);
        }
    }
    records.sort_by(|left, right| {
        left.registry_id
            .cmp(&right.registry_id)
            .then_with(|| left.tool_id.cmp(&right.tool_id))
            .then_with(|| left.experience_id.cmp(&right.experience_id))
    });
    records
}

pub fn write_agent_tool_experience_record(
    storage: &dyn SkillStorage,
    record: &AgentToolExperienceRecord,
) -> Result<bool> {
    let name = agent_tool_experience_storage_name(record);
    let rendered = serde_json::to_string_pretty(record)
        .map_err(|error| Error::config("agent_tool_experience", error.to_string()))?;
    let changed = super::get_skill_content(storage, &name)
        .map(|existing| existing.trim() != rendered.trim())
        .unwrap_or(true);
    if changed {
        super::write_skill(storage, &name, &rendered)?;
    }
    Ok(changed)
}

pub fn plan_agent_tool_experience_record(
    storage: &dyn SkillStorage,
    record: &AgentToolExperienceRecord,
) -> Result<Option<RuntimeSkillStorageMutation>> {
    let name = agent_tool_experience_storage_name(record);
    let rendered = serde_json::to_string_pretty(record)
        .map_err(|error| Error::config("agent_tool_experience", error.to_string()))?;
    let changed = super::get_skill_content(storage, &name)
        .map(|existing| existing.trim() != rendered.trim())
        .unwrap_or(true);
    if changed {
        Ok(Some(RuntimeSkillStorageMutation::Upsert {
            name,
            content: rendered.into_bytes(),
        }))
    } else {
        Ok(None)
    }
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
        governed_experiences: experiences.len(),
        stale_experiences,
        forbidden_by_profile: agent_tool_registries_forbidden_by_profile(profile),
        warnings: Vec::new(),
    }
}

pub fn select_agent_tool_hints(
    registries: &[AgentToolRegistrySnapshot],
    experiences: &[AgentToolExperienceRecord],
    registry_refs: &[AgentToolRegistryRef],
    max_hints: usize,
) -> AgentToolSelectionReport {
    if max_hints == 0 || registries.is_empty() {
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
        .filter(|experience| {
            matches!(
                experience.status,
                AgentToolExperienceStatus::Active | AgentToolExperienceStatus::Candidate
            )
        })
        .filter(|experience| !experience.private_content_used)
        .filter(|experience| {
            refs.iter()
                .any(|registry_ref| registry_ref.registry_id == experience.registry_id)
        })
        .filter_map(|experience| {
            let registry = registries
                .iter()
                .find(|registry| registry.registry_id == experience.registry_id)?;
            if refs.iter().any(|registry_ref| {
                registry_ref.registry_id == registry.registry_id
                    && !registry_ref.fingerprint.trim().is_empty()
                    && registry_ref.fingerprint != registry.fingerprint
            }) {
                rejected.push(AgentToolProjectionRejection {
                    registry_id: experience.registry_id.clone(),
                    tool_id: experience.tool_id.clone(),
                    experience_id: Some(experience.experience_id.clone()),
                    reason: AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH.to_string(),
                });
                return None;
            }
            let tool = registry
                .tools
                .iter()
                .find(|tool| tool.tool_id == experience.tool_id)?;
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
            let reason = if experience.trigger_summary.trim().is_empty() {
                experience.usage_guidance.clone()
            } else {
                format!(
                    "{}; {}",
                    experience.trigger_summary.trim(),
                    experience.usage_guidance.trim()
                )
            };
            Some((
                experience,
                AgentToolHint {
                    registry_id: experience.registry_id.clone(),
                    tool_id: experience.tool_id.clone(),
                    schema_fingerprint: experience.schema_fingerprint.clone(),
                    experience_id: experience.experience_id.clone(),
                    reason,
                    confidence: experience.confidence,
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
            .confidence
            .cmp(&left_exp.confidence)
            .then_with(|| right_exp.success_count.cmp(&left_exp.success_count))
            .then_with(|| right_exp.updated_at.cmp(&left_exp.updated_at))
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

pub fn govern_agent_tool_usage_feedback(
    registries: &[AgentToolRegistrySnapshot],
    feedback: &AgentToolUsageFeedback,
    now_secs: u64,
) -> AgentToolExperienceGovernanceReport {
    if feedback.reuse_outcome == crate::skills::RuntimeSkillReuseOutcome::Mismatch {
        return governance_report(
            AgentToolExperienceGovernanceDecision::RejectedByLowConfidence,
            "agent_tool_feedback_reuse_outcome_mismatch",
            None,
        );
    }
    let Some(registry) = registries
        .iter()
        .find(|registry| registry.registry_id == feedback.registry_ref.registry_id)
    else {
        return governance_report(
            AgentToolExperienceGovernanceDecision::RejectedBySchemaDrift,
            "agent_tool_registry_not_found",
            None,
        );
    };
    if !feedback.registry_ref.fingerprint.trim().is_empty()
        && feedback.registry_ref.fingerprint != registry.fingerprint
    {
        return governance_report(
            AgentToolExperienceGovernanceDecision::RejectedBySchemaDrift,
            AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH,
            None,
        );
    }
    let valid_observations = feedback
        .observations
        .iter()
        .filter(|observation| !observation.summary.trim().is_empty())
        .collect::<Vec<_>>();
    if valid_observations
        .iter()
        .any(|observation| observation.private_content_used)
    {
        return governance_report(
            AgentToolExperienceGovernanceDecision::RejectedByPrivacy,
            "agent_tool_feedback_private_content_requires_private_governance",
            None,
        );
    }
    let succeeded = valid_observations
        .iter()
        .filter(|observation| observation.outcome == AgentToolOutcome::Succeeded)
        .count();
    let operator_confirmed = feedback
        .operator_note
        .as_deref()
        .map(str::trim)
        .filter(|note| !note.is_empty())
        .is_some();
    if succeeded == 0 {
        return governance_report(
            AgentToolExperienceGovernanceDecision::RejectedByLowConfidence,
            "agent_tool_feedback_has_no_successful_observation",
            None,
        );
    }
    if succeeded < 2 && !operator_confirmed {
        return governance_report(
            AgentToolExperienceGovernanceDecision::DeferredUntilRepeated,
            "agent_tool_feedback_requires_repeated_success_or_operator_confirmation",
            None,
        );
    }
    let Some(first) = valid_observations.first() else {
        return governance_report(
            AgentToolExperienceGovernanceDecision::RejectedByLowConfidence,
            "agent_tool_feedback_empty",
            None,
        );
    };
    if valid_observations.iter().any(|observation| {
        observation.registry_id != registry.registry_id
            || observation.tool_id != first.tool_id
            || observation.schema_fingerprint != first.schema_fingerprint
    }) {
        return governance_report(
            AgentToolExperienceGovernanceDecision::RejectedBySchemaDrift,
            "agent_tool_feedback_mixed_registry_tool_or_schema",
            None,
        );
    }
    let Some(tool) = registry
        .tools
        .iter()
        .find(|tool| tool.tool_id == first.tool_id)
    else {
        return governance_report(
            AgentToolExperienceGovernanceDecision::RejectedBySchemaDrift,
            "agent_tool_feedback_tool_not_in_registry",
            None,
        );
    };
    if tool.schema_fingerprint != first.schema_fingerprint {
        return governance_report(
            AgentToolExperienceGovernanceDecision::RejectedBySchemaDrift,
            "agent_tool_feedback_schema_fingerprint_mismatch",
            None,
        );
    }
    let evidence_refs = valid_observations
        .iter()
        .map(|observation| observation.observation_id.clone())
        .collect::<Vec<_>>();
    let experience = AgentToolExperienceRecord {
        experience_id: stable_agent_tool_experience_id(
            &registry.registry_id,
            &first.tool_id,
            &first.schema_fingerprint,
            &first.task_signature,
        ),
        registry_id: registry.registry_id.clone(),
        tool_id: first.tool_id.clone(),
        schema_fingerprint: first.schema_fingerprint.clone(),
        task_signature: first.task_signature.clone(),
        trigger_summary: feedback
            .user_visible_result_summary
            .clone()
            .unwrap_or_else(|| first.summary.clone()),
        usage_guidance: feedback
            .operator_note
            .clone()
            .unwrap_or_else(|| first.summary.clone()),
        constraints: Vec::new(),
        evidence_count: valid_observations.len() as u32,
        success_count: succeeded as u32,
        failure_count: valid_observations.len().saturating_sub(succeeded) as u32,
        last_outcome: first.outcome,
        confidence: if operator_confirmed || succeeded >= 3 {
            AgentToolExperienceConfidence::High
        } else {
            AgentToolExperienceConfidence::Medium
        },
        status: AgentToolExperienceStatus::Active,
        evidence_refs,
        private_content_used: false,
        created_at: now_secs,
        updated_at: now_secs,
    };
    governance_report(
        AgentToolExperienceGovernanceDecision::AcceptedAsEvidence,
        "agent_tool_feedback_accepted_as_governed_experience",
        Some(experience),
    )
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

fn governance_report(
    decision: AgentToolExperienceGovernanceDecision,
    reason: impl Into<String>,
    experience: Option<AgentToolExperienceRecord>,
) -> AgentToolExperienceGovernanceReport {
    AgentToolExperienceGovernanceReport {
        accepted: matches!(
            decision,
            AgentToolExperienceGovernanceDecision::AcceptedAsEvidence
                | AgentToolExperienceGovernanceDecision::MergedIntoExistingExperience
                | AgentToolExperienceGovernanceDecision::PromotedToRuntimeSkill
        ),
        changed: usize::from(experience.is_some()),
        decision,
        reason: reason.into(),
        experience,
    }
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

fn stable_agent_tool_experience_id(
    registry_id: &str,
    tool_id: &str,
    schema_fingerprint: &str,
    task_signature: &str,
) -> String {
    let seed = format!("{registry_id}:{tool_id}:{schema_fingerprint}:{task_signature}");
    format!("agent_tool_exp_{:016x}", fnv1a64(seed.as_bytes()))
}

fn agent_tool_experience_storage_name(record: &AgentToolExperienceRecord) -> String {
    format!(
        "{AGENT_TOOL_EXPERIENCE_PREFIX}{:016x}",
        fnv1a64(record.experience_id.as_bytes())
    )
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

    #[test]
    fn single_success_without_operator_confirmation_is_deferred() {
        let registry = registry();
        let feedback = AgentToolUsageFeedback {
            registry_ref: registry.registry_ref(),
            observations: vec![AgentToolObservationDigest {
                observation_id: "obs-1".to_string(),
                registry_id: "host-tools".to_string(),
                tool_id: "pdf.extract".to_string(),
                schema_fingerprint: "schema-pdf-v1".to_string(),
                call_id: None,
                task_signature: "pdf review".to_string(),
                summary: "extracted text".to_string(),
                outcome: AgentToolOutcome::Succeeded,
                error_code: None,
                external_content: true,
                private_content_used: false,
                permission_tags: Vec::new(),
                risk_tags: Vec::new(),
                started_at: None,
                completed_at: None,
            }],
            user_visible_result_summary: None,
            reuse_outcome: crate::skills::RuntimeSkillReuseOutcome::Succeeded,
            operator_note: None,
        };
        let report = govern_agent_tool_usage_feedback(&[registry], &feedback, 300);
        assert_eq!(
            report.decision,
            AgentToolExperienceGovernanceDecision::DeferredUntilRepeated
        );
        assert!(report.experience.is_none());
    }

    #[test]
    fn mixed_tool_or_schema_observations_are_rejected() {
        let mut registry = registry();
        registry.tools.push(AgentToolDescriptor::compact(
            "image.resize",
            "Resize image",
            "schema-image-v1",
        ));
        registry.fingerprint = fingerprint_agent_tool_registry(&registry);
        let feedback = AgentToolUsageFeedback {
            registry_ref: registry.registry_ref(),
            observations: vec![
                AgentToolObservationDigest {
                    observation_id: "obs-1".to_string(),
                    registry_id: "host-tools".to_string(),
                    tool_id: "pdf.extract".to_string(),
                    schema_fingerprint: "schema-pdf-v1".to_string(),
                    call_id: None,
                    task_signature: "document task".to_string(),
                    summary: "extracted text".to_string(),
                    outcome: AgentToolOutcome::Succeeded,
                    error_code: None,
                    external_content: true,
                    private_content_used: false,
                    permission_tags: Vec::new(),
                    risk_tags: Vec::new(),
                    started_at: None,
                    completed_at: None,
                },
                AgentToolObservationDigest {
                    observation_id: "obs-2".to_string(),
                    registry_id: "host-tools".to_string(),
                    tool_id: "image.resize".to_string(),
                    schema_fingerprint: "schema-image-v1".to_string(),
                    call_id: None,
                    task_signature: "document task".to_string(),
                    summary: "resized preview".to_string(),
                    outcome: AgentToolOutcome::Succeeded,
                    error_code: None,
                    external_content: true,
                    private_content_used: false,
                    permission_tags: Vec::new(),
                    risk_tags: Vec::new(),
                    started_at: None,
                    completed_at: None,
                },
            ],
            user_visible_result_summary: None,
            reuse_outcome: crate::skills::RuntimeSkillReuseOutcome::Succeeded,
            operator_note: None,
        };
        let report = govern_agent_tool_usage_feedback(&[registry], &feedback, 300);
        assert_eq!(
            report.decision,
            AgentToolExperienceGovernanceDecision::RejectedBySchemaDrift
        );
        assert_eq!(
            report.reason,
            "agent_tool_feedback_mixed_registry_tool_or_schema"
        );
        assert!(report.experience.is_none());
    }

    #[test]
    fn mismatch_reuse_outcome_rejects_feedback() {
        let registry = registry();
        let mut feedback = AgentToolUsageFeedback {
            registry_ref: registry.registry_ref(),
            observations: vec![AgentToolObservationDigest {
                observation_id: "obs-1".to_string(),
                registry_id: "host-tools".to_string(),
                tool_id: "pdf.extract".to_string(),
                schema_fingerprint: "schema-pdf-v1".to_string(),
                call_id: None,
                task_signature: "document task".to_string(),
                summary: "extracted text".to_string(),
                outcome: AgentToolOutcome::Succeeded,
                error_code: None,
                external_content: true,
                private_content_used: false,
                permission_tags: Vec::new(),
                risk_tags: Vec::new(),
                started_at: None,
                completed_at: None,
            }],
            user_visible_result_summary: None,
            reuse_outcome: crate::skills::RuntimeSkillReuseOutcome::Mismatch,
            operator_note: Some("operator confirmed".to_string()),
        };
        let report =
            govern_agent_tool_usage_feedback(std::slice::from_ref(&registry), &feedback, 300);
        assert_eq!(
            report.decision,
            AgentToolExperienceGovernanceDecision::RejectedByLowConfidence
        );
        assert_eq!(report.reason, "agent_tool_feedback_reuse_outcome_mismatch");
        assert!(report.experience.is_none());

        feedback.reuse_outcome = crate::skills::RuntimeSkillReuseOutcome::Succeeded;
        let accepted = govern_agent_tool_usage_feedback(&[registry], &feedback, 300);
        assert!(accepted.accepted);
    }
}
