use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::{
    default_agent_subject_id, default_memory_space_id, primary_human_subject_id,
    system_governor_subject_id, MemorySpaceId, RelationshipId, RelationshipScope, SubjectId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    SystemGovernor,
    HumanUser,
    AgentPersona,
}

impl SubjectKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SystemGovernor => "system_governor",
            Self::HumanUser => "human_user",
            Self::AgentPersona => "agent_persona",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectVisibility {
    Hidden,
    Visible,
    AuditOnly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectLifecycleState {
    #[default]
    Active,
    Suspended,
    Migrating,
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSoulSurface {
    SelfAuthoredCore,
    SelfContinuity,
    PrivateGarden,
    InnerLife,
    RelationshipExperience,
    ProceduralTraces,
    SoulFeedback,
    GrowthRevisionLedger,
}

impl SubjectSoulSurface {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SelfAuthoredCore => "self_authored_core",
            Self::SelfContinuity => "self_continuity",
            Self::PrivateGarden => "private_garden",
            Self::InnerLife => "inner_life",
            Self::RelationshipExperience => "relationship_experience",
            Self::ProceduralTraces => "procedural_traces",
            Self::SoulFeedback => "soul_feedback",
            Self::GrowthRevisionLedger => "growth_revision_ledger",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectSoulBinding {
    pub soul_id: String,
    pub owner_subject_id: SubjectId,
    pub surfaces: Vec<SubjectSoulSurface>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub revision_refs: Vec<String>,
}

impl SubjectSoulBinding {
    pub fn agent_persona(subject_id: impl Into<SubjectId>) -> Self {
        let subject_id = subject_id.into();
        Self {
            soul_id: format!("soul:{}", encode_subject_id_for_suffix(&subject_id)),
            owner_subject_id: subject_id,
            surfaces: vec![
                SubjectSoulSurface::SelfAuthoredCore,
                SubjectSoulSurface::SelfContinuity,
                SubjectSoulSurface::PrivateGarden,
                SubjectSoulSurface::InnerLife,
                SubjectSoulSurface::RelationshipExperience,
                SubjectSoulSurface::ProceduralTraces,
                SubjectSoulSurface::SoulFeedback,
                SubjectSoulSurface::GrowthRevisionLedger,
            ],
            revision_refs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectDescriptor {
    pub subject_id: SubjectId,
    pub kind: SubjectKind,
    pub display_name: String,
    pub visibility: SubjectVisibility,
    #[serde(default)]
    pub lifecycle_state: SubjectLifecycleState,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soul_binding: Option<SubjectSoulBinding>,
}

impl SubjectDescriptor {
    pub fn new(
        subject_id: impl Into<SubjectId>,
        kind: SubjectKind,
        display_name: impl Into<String>,
        visibility: SubjectVisibility,
    ) -> Self {
        Self {
            subject_id: subject_id.into().trim().to_string(),
            kind,
            display_name: display_name.into().trim().to_string(),
            visibility,
            lifecycle_state: SubjectLifecycleState::Active,
            metadata: BTreeMap::new(),
            tags: Vec::new(),
            soul_binding: None,
        }
    }

    pub fn agent_persona(
        subject_id: impl Into<SubjectId>,
        display_name: impl Into<String>,
    ) -> Self {
        let subject_id = subject_id.into();
        let mut descriptor = Self::new(
            subject_id.clone(),
            SubjectKind::AgentPersona,
            display_name,
            SubjectVisibility::Visible,
        );
        descriptor.soul_binding = Some(SubjectSoulBinding::agent_persona(subject_id));
        descriptor
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectContractValidation {
    pub accepted: bool,
    pub reason: String,
}

impl SubjectContractValidation {
    pub fn accepted() -> Self {
        Self {
            accepted: true,
            reason: "accepted".to_string(),
        }
    }

    pub fn rejected(reason: impl Into<String>) -> Self {
        Self {
            accepted: false,
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectRegistry {
    pub memory_space_id: MemorySpaceId,
    pub subjects: Vec<SubjectDescriptor>,
}

impl SubjectRegistry {
    pub fn empty(memory_space_id: impl Into<MemorySpaceId>) -> Self {
        Self {
            memory_space_id: memory_space_id.into().trim().to_string(),
            subjects: Vec::new(),
        }
    }

    pub fn single_agent_default(owner_id: &str, agent_id: &str) -> Result<Self, String> {
        Self::single_agent_default_with_user(owner_id, agent_id, None)
    }

    pub fn single_agent_default_with_user(
        owner_id: &str,
        agent_id: &str,
        user_id: Option<&str>,
    ) -> Result<Self, String> {
        let agent_subject_id = default_agent_subject_id(agent_id);
        Self::single_agent_default_with_subject(owner_id, agent_id, user_id, &agent_subject_id)
    }

    pub fn single_agent_default_with_subject(
        owner_id: &str,
        agent_id: &str,
        user_id: Option<&str>,
        agent_subject_id: &str,
    ) -> Result<Self, String> {
        let owner_id = checked_non_empty(owner_id, "owner_id")?;
        let _agent_id = checked_non_empty(agent_id, "agent_id")?;
        let human_user_id = checked_non_empty(user_id.unwrap_or(owner_id), "user_id")?;
        let agent_subject_id = checked_non_empty(agent_subject_id, "agent_subject_id")?;
        let mut registry = Self::empty(default_memory_space_id(owner_id));

        registry.upsert_subject(SubjectDescriptor::new(
            system_governor_subject_id(owner_id),
            SubjectKind::SystemGovernor,
            "System Governor",
            SubjectVisibility::Hidden,
        ))?;
        registry.upsert_subject(SubjectDescriptor::new(
            primary_human_subject_id(human_user_id),
            SubjectKind::HumanUser,
            "Primary Human User",
            SubjectVisibility::Visible,
        ))?;
        registry.upsert_subject(SubjectDescriptor::agent_persona(
            agent_subject_id,
            "Default Agent",
        ))?;

        let validation = registry.validate_contract();
        if validation.accepted {
            Ok(registry)
        } else {
            Err(validation.reason)
        }
    }

    pub fn upsert_subject(&mut self, subject: SubjectDescriptor) -> Result<(), String> {
        if subject.subject_id.trim().is_empty() {
            return Err("subject_id_empty".to_string());
        }
        if subject.subject_id != subject.subject_id.trim() {
            return Err("subject_id_non_canonical".to_string());
        }
        if let Some(existing) = self
            .subjects
            .iter_mut()
            .find(|existing| existing.subject_id == subject.subject_id)
        {
            *existing = subject;
        } else {
            self.subjects.push(subject);
        }
        Ok(())
    }

    pub fn subject(&self, subject_id: &str) -> Option<&SubjectDescriptor> {
        self.subjects
            .iter()
            .find(|subject| subject.subject_id == subject_id.trim())
    }

    pub fn registered_subject_ids(&self) -> Result<Vec<SubjectId>, String> {
        let validation = self.validate_contract();
        if !validation.accepted {
            return Err(validation.reason);
        }
        if self
            .subjects
            .iter()
            .any(|subject| subject.subject_id != subject.subject_id.trim())
        {
            return Err("subject_id_non_canonical".to_string());
        }
        let subject_ids = self
            .subjects
            .iter()
            .map(|subject| subject.subject_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if subject_ids.is_empty() {
            return Err("subject_registry_empty".to_string());
        }
        Ok(subject_ids)
    }

    pub fn system_governor(&self) -> Option<&SubjectDescriptor> {
        self.subjects
            .iter()
            .find(|subject| subject.kind == SubjectKind::SystemGovernor)
    }

    pub fn primary_human_user(&self) -> Option<&SubjectDescriptor> {
        self.subjects
            .iter()
            .find(|subject| subject.kind == SubjectKind::HumanUser)
    }

    pub fn default_agent(&self) -> Option<&SubjectDescriptor> {
        self.subjects
            .iter()
            .find(|subject| subject.kind == SubjectKind::AgentPersona)
    }

    pub fn validate_contract(&self) -> SubjectContractValidation {
        if self.memory_space_id.trim().is_empty() {
            return SubjectContractValidation::rejected("memory_space_id_empty");
        }
        if self.memory_space_id != self.memory_space_id.trim() {
            return SubjectContractValidation::rejected("memory_space_id_non_canonical");
        }
        if self.subjects.is_empty() {
            return SubjectContractValidation::rejected("subject_registry_empty");
        }

        let mut seen = BTreeSet::new();
        let mut soul_owner_ids = BTreeSet::new();
        for subject in &self.subjects {
            if subject.subject_id.trim().is_empty() {
                return SubjectContractValidation::rejected("subject_id_empty");
            }
            if subject.subject_id != subject.subject_id.trim() {
                return SubjectContractValidation::rejected("subject_id_non_canonical");
            }
            if !seen.insert(subject.subject_id.clone()) {
                return SubjectContractValidation::rejected("subject_id_duplicate");
            }
            if subject.display_name.trim().is_empty() {
                return SubjectContractValidation::rejected("subject_display_name_empty");
            }
            if subject
                .metadata
                .keys()
                .any(|key| is_forbidden_soul_metadata_key(key))
            {
                return SubjectContractValidation::rejected("subject_soul_metadata_forbidden");
            }
            match subject.kind {
                SubjectKind::SystemGovernor => {
                    if subject.soul_binding.is_some() {
                        return SubjectContractValidation::rejected(
                            "system_governor_soul_binding_forbidden",
                        );
                    }
                }
                SubjectKind::AgentPersona => {
                    let Some(soul) = subject.soul_binding.as_ref() else {
                        return SubjectContractValidation::rejected(
                            "agent_persona_soul_binding_required",
                        );
                    };
                    if soul.owner_subject_id != subject.subject_id {
                        return SubjectContractValidation::rejected("soul_owner_subject_mismatch");
                    }
                    if soul.soul_id.trim().is_empty() {
                        return SubjectContractValidation::rejected("soul_id_empty");
                    }
                    if !soul_owner_ids.insert(soul.soul_id.clone()) {
                        return SubjectContractValidation::rejected("agent_persona_soul_shared");
                    }
                    if soul.surfaces.is_empty() {
                        return SubjectContractValidation::rejected("soul_surfaces_empty");
                    }
                }
                SubjectKind::HumanUser => {}
            }
        }
        SubjectContractValidation::accepted()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectRelationshipKind {
    Governs,
    Represents,
    CollaboratesWith,
    Observes,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectRelationshipEdge {
    pub edge_id: String,
    pub from_subject_id: SubjectId,
    pub to_subject_id: SubjectId,
    pub kind: SubjectRelationshipKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_id: Option<RelationshipId>,
    pub visibility: SubjectVisibility,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policy_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
}

impl SubjectRelationshipEdge {
    pub fn new(
        from_subject_id: impl Into<SubjectId>,
        to_subject_id: impl Into<SubjectId>,
        kind: SubjectRelationshipKind,
    ) -> Self {
        let from_subject_id = from_subject_id.into().trim().to_string();
        let to_subject_id = to_subject_id.into().trim().to_string();
        let edge_id = format!(
            "subject-edge:{:?}:{}:{}",
            kind,
            encode_subject_id_for_suffix(&from_subject_id),
            encode_subject_id_for_suffix(&to_subject_id)
        );
        Self {
            edge_id,
            from_subject_id,
            to_subject_id,
            kind,
            relationship_id: None,
            visibility: SubjectVisibility::AuditOnly,
            policy_tags: Vec::new(),
            evidence_refs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectRelationshipGraph {
    pub memory_space_id: MemorySpaceId,
    pub edges: Vec<SubjectRelationshipEdge>,
}

impl SubjectRelationshipGraph {
    pub fn empty(memory_space_id: impl Into<MemorySpaceId>) -> Self {
        Self {
            memory_space_id: memory_space_id.into().trim().to_string(),
            edges: Vec::new(),
        }
    }

    pub fn single_agent_default(registry: &SubjectRegistry) -> Result<Self, String> {
        let agent = registry
            .default_agent()
            .map(|subject| subject.subject_id.clone());
        let Some(agent_subject_id) = agent else {
            return Err("agent_persona_missing".to_string());
        };

        Self::single_agent_default_for_subject(registry, &agent_subject_id)
    }

    pub fn single_agent_default_for_subject(
        registry: &SubjectRegistry,
        agent_subject_id: &str,
    ) -> Result<Self, String> {
        let system = registry
            .system_governor()
            .ok_or_else(|| "system_governor_missing".to_string())?;
        let human = registry
            .primary_human_user()
            .ok_or_else(|| "human_user_missing".to_string())?;
        let agent = registry
            .subject(agent_subject_id)
            .ok_or_else(|| "agent_persona_missing".to_string())?;
        if agent.kind != SubjectKind::AgentPersona {
            return Err("agent_persona_required".to_string());
        }

        let mut graph = Self::empty(registry.memory_space_id.clone());
        graph.edges.push(SubjectRelationshipEdge::new(
            system.subject_id.clone(),
            agent.subject_id.clone(),
            SubjectRelationshipKind::Governs,
        ));
        graph.edges.push(SubjectRelationshipEdge::new(
            human.subject_id.clone(),
            agent.subject_id.clone(),
            SubjectRelationshipKind::CollaboratesWith,
        ));
        graph.edges.push(SubjectRelationshipEdge::new(
            agent.subject_id.clone(),
            human.subject_id.clone(),
            SubjectRelationshipKind::Represents,
        ));
        Ok(graph)
    }

    pub fn validate_against_registry(
        &self,
        registry: &SubjectRegistry,
    ) -> SubjectContractValidation {
        if self.memory_space_id.trim().is_empty() {
            return SubjectContractValidation::rejected("relationship_graph_space_empty");
        }
        if self.memory_space_id != registry.memory_space_id {
            return SubjectContractValidation::rejected("relationship_graph_space_mismatch");
        }
        for edge in &self.edges {
            if edge.edge_id.trim().is_empty() {
                return SubjectContractValidation::rejected("relationship_edge_id_empty");
            }
            if registry.subject(&edge.from_subject_id).is_none() {
                return SubjectContractValidation::rejected("relationship_from_subject_missing");
            }
            if registry.subject(&edge.to_subject_id).is_none() {
                return SubjectContractValidation::rejected("relationship_to_subject_missing");
            }
        }
        SubjectContractValidation::accepted()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectScopedRuntime {
    pub memory_space_id: MemorySpaceId,
    pub mounted_subject_id: SubjectId,
    pub actor_subject_id: SubjectId,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_scope: Option<RelationshipScope>,
    pub projection_policy: String,
    pub write_policy: String,
}

impl SubjectScopedRuntime {
    pub fn single_agent_default(
        owner_id: &str,
        agent_id: &str,
        relationship_scope: Option<RelationshipScope>,
    ) -> Result<Self, String> {
        let owner_id = checked_non_empty(owner_id, "owner_id")?;
        let agent_id = checked_non_empty(agent_id, "agent_id")?;
        let agent_subject_id = default_agent_subject_id(agent_id);
        Ok(Self {
            memory_space_id: default_memory_space_id(owner_id),
            mounted_subject_id: agent_subject_id.clone(),
            actor_subject_id: agent_subject_id,
            agent_id: agent_id.to_string(),
            relationship_scope,
            projection_policy: "subject_aware_default".to_string(),
            write_policy: "subject_candidate_then_space_governance".to_string(),
        })
    }

    pub fn validate_against_registry(
        &self,
        registry: &SubjectRegistry,
    ) -> SubjectContractValidation {
        if self.memory_space_id != registry.memory_space_id {
            return SubjectContractValidation::rejected("runtime_space_mismatch");
        }
        if registry.subject(&self.mounted_subject_id).is_none() {
            return SubjectContractValidation::rejected("runtime_mounted_subject_missing");
        }
        if registry.subject(&self.actor_subject_id).is_none() {
            return SubjectContractValidation::rejected("runtime_actor_subject_missing");
        }
        if self.agent_id.trim().is_empty() {
            return SubjectContractValidation::rejected("runtime_agent_id_empty");
        }
        SubjectContractValidation::accepted()
    }
}

fn checked_non_empty<'a>(value: &'a str, field: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{field}_empty"))
    } else {
        Ok(value)
    }
}

fn is_forbidden_soul_metadata_key(key: &str) -> bool {
    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "soul"
            | "soul_id"
            | "self_authored_core"
            | "self_continuity"
            | "inner_life"
            | "private_garden"
            | "soul_feedback"
            | "growth_revision_ledger"
    )
}

fn encode_subject_id_for_suffix(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.trim().bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else {
            out.push('_');
            const HEX: &[u8; 16] = b"0123456789abcdef";
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
}
