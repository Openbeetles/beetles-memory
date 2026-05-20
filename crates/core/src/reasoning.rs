use crate::error::Result;
use crate::platform::SkillStorage;
use crate::skills::{
    runtime_skill_name_for_topic, write_governed_runtime_skills, RuntimeSkillWrite,
    RuntimeSkillWriteOutcome, RuntimeSkillWriteSource,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceCrystalDisposition {
    Promote,
    Observe,
    Reject,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillCrystalCandidate {
    pub topic: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub reusable_macro: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub success_score: u8,
    #[serde(default)]
    pub reuse_score: u8,
    #[serde(default)]
    pub promotion_readiness: u8,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExperienceCrystalAdjudication {
    pub disposition: ExperienceCrystalDisposition,
    pub reason_code: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_target_name: Option<String>,
}

pub fn adjudicate_skill_crystal_candidate(
    candidate: &SkillCrystalCandidate,
    distinct_runs: usize,
    existing_runtime_skill_names: &[String],
) -> ExperienceCrystalAdjudication {
    if candidate.topic.trim().is_empty() || candidate.reusable_macro.is_empty() {
        return ExperienceCrystalAdjudication {
            disposition: ExperienceCrystalDisposition::Reject,
            reason_code: "weak_procedure".to_string(),
            detail: "experience crystal adjudication rejected: candidate lacks a reusable procedural shape".to_string(),
            merge_target_name: None,
        };
    }
    let target_name = runtime_skill_name_for_topic(&candidate.topic);
    let merge_target_name = existing_runtime_skill_names
        .iter()
        .find(|name| name.as_str() == target_name)
        .cloned();
    if distinct_runs >= 2 || candidate.promotion_readiness >= 80 {
        ExperienceCrystalAdjudication {
            disposition: ExperienceCrystalDisposition::Promote,
            reason_code: "ready".to_string(),
            detail: "candidate is ready for procedural memory governance".to_string(),
            merge_target_name,
        }
    } else {
        ExperienceCrystalAdjudication {
            disposition: ExperienceCrystalDisposition::Observe,
            reason_code: "needs_more_evidence".to_string(),
            detail: "candidate retained as evidence until repeated success is observed".to_string(),
            merge_target_name,
        }
    }
}

pub fn promote_skill_crystal_candidates(
    storage: &dyn SkillStorage,
    candidates: &[SkillCrystalCandidate],
    source_chat_id: Option<&str>,
    observed_at: u64,
) -> Result<RuntimeSkillWriteOutcome> {
    let writes = candidates
        .iter()
        .map(|candidate| RuntimeSkillWrite {
            name: runtime_skill_name_for_topic(&candidate.topic),
            topic: candidate.topic.clone(),
            title: candidate.title.clone(),
            summary: candidate.summary.clone(),
            content: candidate.reusable_macro.join("\n"),
            citations: candidate.evidence_refs.clone(),
            source_chat_id: source_chat_id.map(ToString::to_string),
            observed_at,
        })
        .collect::<Vec<_>>();
    write_governed_runtime_skills(storage, &writes, RuntimeSkillWriteSource::TaskLearning)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdleMemoryForgeOperatorSummary {
    pub last_run_at: u64,
    pub total_candidates: usize,
    pub attack_findings: usize,
    pub distillation_candidates: usize,
    pub adjudication_state: IdleMemoryForgeAdjudicationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_source_channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_finding: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdleMemoryForgeAdjudicationState {
    #[default]
    Idle,
}

pub fn load_idle_memory_forge_operator_summary(
    _state_fs: &dyn crate::platform::StateFs,
) -> Result<Option<IdleMemoryForgeOperatorSummary>> {
    Ok(Some(IdleMemoryForgeOperatorSummary::default()))
}
