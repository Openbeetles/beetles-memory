use serde::{Deserialize, Serialize};

use crate::skills::{runtime_skill_name_for_topic, RuntimeSkillWrite};

use super::{
    CanonicalEntityRef, GovernedWriteDecision, LongTermMemoryConfidence, LongTermMemoryDraft,
    LongTermMemoryFreshness, LongTermMemoryKind, LongTermMemorySourceScope,
    LongTermMemorySourceType, LongTermMemoryStaleHint, MemoryEvidenceAuthority,
    MemoryPlaneGovernanceReport, MemoryPrivacyClass, MemoryWriteAuthority, MemoryWriteDomain,
    PostTurnSemanticGovernanceReport, SoulCandidateDisposition, SoulCandidateHandoffReport,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum MemoryCandidateTarget {
    LongTermMemory {
        kind: LongTermMemoryKind,
        topic: String,
    },
    ProceduralMemory {
        name: String,
        topic: String,
    },
    PrivateGarden {
        path: String,
    },
    Soul {
        surface: String,
    },
    OperatorDiagnostic {
        name: String,
    },
}

impl MemoryCandidateTarget {
    pub fn plane(&self) -> &'static str {
        match self {
            Self::LongTermMemory { .. } => "long_term_memory",
            Self::ProceduralMemory { .. } => "runtime_skill",
            Self::PrivateGarden { .. } => "private_garden",
            Self::Soul { .. } => "soul",
            Self::OperatorDiagnostic { .. } => "operator_diagnostic",
        }
    }

    pub fn domain(&self) -> MemoryWriteDomain {
        match self {
            Self::LongTermMemory { kind, .. } => match kind {
                LongTermMemoryKind::Profile
                | LongTermMemoryKind::Preference
                | LongTermMemoryKind::Relationship => MemoryWriteDomain::Subject,
                LongTermMemoryKind::Project
                | LongTermMemoryKind::Task
                | LongTermMemoryKind::Constraint
                | LongTermMemoryKind::Fact => MemoryWriteDomain::Program,
            },
            Self::ProceduralMemory { .. } => MemoryWriteDomain::Procedural,
            Self::PrivateGarden { .. } | Self::Soul { .. } => MemoryWriteDomain::Subject,
            Self::OperatorDiagnostic { .. } => MemoryWriteDomain::Program,
        }
    }

    fn topic(&self) -> &str {
        match self {
            Self::LongTermMemory { topic, .. } => topic,
            Self::ProceduralMemory { topic, .. } => topic,
            Self::PrivateGarden { path } => path,
            Self::Soul { surface } => surface,
            Self::OperatorDiagnostic { name } => name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryCandidateContent {
    Text {
        topic: String,
        body: String,
        #[serde(default)]
        keywords: Vec<String>,
    },
    RuntimeSkill {
        name: String,
        topic: String,
        title: String,
        summary: String,
        content: String,
        #[serde(default)]
        citations: Vec<String>,
    },
}

impl MemoryCandidateContent {
    fn is_empty(&self) -> bool {
        match self {
            Self::Text { body, .. } => body.trim().is_empty(),
            Self::RuntimeSkill { content, .. } => content.trim().is_empty(),
        }
    }

    pub fn body(&self) -> &str {
        match self {
            Self::Text { body, .. } => body,
            Self::RuntimeSkill { content, .. } => content,
        }
    }

    pub fn keywords(&self) -> Vec<String> {
        match self {
            Self::Text { keywords, .. } => keywords.clone(),
            Self::RuntimeSkill { topic, title, .. } => vec![topic.clone(), title.clone()],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySemanticJudgmentSource {
    LlmGovernance,
    RuntimeGate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateSemanticDecision {
    Accept,
    Reject,
    Defer,
    HandoffToSoulGovernance,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCandidateSemanticJudgment {
    pub source: MemorySemanticJudgmentSource,
    pub decision: MemoryCandidateSemanticDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governed_target: Option<MemoryCandidateTarget>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryWriteCandidate {
    pub candidate_id: String,
    pub authority: MemoryEvidenceAuthority,
    pub target: MemoryCandidateTarget,
    pub privacy: MemoryPrivacyClass,
    pub content: MemoryCandidateContent,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub canonical_entities: Vec<CanonicalEntityRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_judgment: Option<MemoryCandidateSemanticJudgment>,
}

impl MemoryWriteCandidate {
    pub fn to_long_term_draft(
        &self,
        source_chat_id: &str,
        now_secs: u64,
    ) -> Option<LongTermMemoryDraft> {
        self.to_long_term_draft_for_target(&self.target, source_chat_id, now_secs)
    }

    pub fn to_long_term_draft_for_target(
        &self,
        target: &MemoryCandidateTarget,
        source_chat_id: &str,
        now_secs: u64,
    ) -> Option<LongTermMemoryDraft> {
        let MemoryCandidateTarget::LongTermMemory { kind, topic } = target else {
            return None;
        };
        if self.content.is_empty() {
            return None;
        }
        Some(LongTermMemoryDraft {
            kind: kind.clone(),
            topic: topic.clone(),
            content: self.content.body().trim().to_string(),
            keywords: self.content.keywords(),
            privacy: self.privacy,
            source_chat_id: Some(source_chat_id.to_string()),
            source_type: Some(match self.authority {
                MemoryEvidenceAuthority::RuntimeObservation => {
                    LongTermMemorySourceType::SystemRuntime
                }
                MemoryEvidenceAuthority::WorldObservation => {
                    LongTermMemorySourceType::ExternalObservation
                }
                _ => LongTermMemorySourceType::Conversation,
            }),
            source_scope: Some(match self.target.domain() {
                MemoryWriteDomain::Subject => LongTermMemorySourceScope::User,
                MemoryWriteDomain::Program => {
                    if self.authority == MemoryEvidenceAuthority::WorldObservation {
                        LongTermMemorySourceScope::World
                    } else {
                        LongTermMemorySourceScope::User
                    }
                }
                MemoryWriteDomain::Procedural => LongTermMemorySourceScope::Chat,
            }),
            confidence: Some(match self.authority {
                MemoryEvidenceAuthority::UserAsserted
                | MemoryEvidenceAuthority::RuntimeObservation
                | MemoryEvidenceAuthority::ProgramMemoryCanonical => LongTermMemoryConfidence::High,
                _ => LongTermMemoryConfidence::Medium,
            }),
            freshness: Some(LongTermMemoryFreshness::Stable),
            stale_hint: Some(LongTermMemoryStaleHint::None),
            supporting_citations: self.evidence_refs.clone(),
            canonical_entities: self.canonical_entities.clone(),
            evidence_count: Some(self.evidence_refs.len().max(1) as u32),
            observed_at: Some(now_secs),
            last_confirmed_at: Some(now_secs),
            source_revision: None,
        })
    }

    pub fn to_runtime_skill_write(
        &self,
        source_chat_id: &str,
        now_secs: u64,
    ) -> Option<RuntimeSkillWrite> {
        self.to_runtime_skill_write_for_target(&self.target, source_chat_id, now_secs)
    }

    pub fn to_runtime_skill_write_for_target(
        &self,
        target: &MemoryCandidateTarget,
        source_chat_id: &str,
        now_secs: u64,
    ) -> Option<RuntimeSkillWrite> {
        let MemoryCandidateTarget::ProceduralMemory { name, topic } = target else {
            return None;
        };
        if self.content.is_empty() {
            return None;
        }
        let (content_name, content_topic, title, summary, content, citations) = match &self.content
        {
            MemoryCandidateContent::RuntimeSkill {
                name: content_name,
                topic: content_topic,
                title,
                summary,
                content,
                citations,
            } => (
                content_name.as_str(),
                content_topic.as_str(),
                title.clone(),
                summary.clone(),
                content.clone(),
                citations.clone(),
            ),
            MemoryCandidateContent::Text {
                topic: content_topic,
                body,
                ..
            } => (
                "",
                content_topic.as_str(),
                topic.replace('_', " "),
                body.lines().next().unwrap_or(body).trim().to_string(),
                body.clone(),
                self.evidence_refs.clone(),
            ),
        };
        let topic_value = if topic.trim().is_empty() {
            content_topic.trim()
        } else {
            topic.trim()
        };
        let name_value = if name.trim().is_empty() {
            content_name.trim()
        } else {
            name.trim()
        };
        Some(RuntimeSkillWrite {
            name: if name_value.is_empty() {
                runtime_skill_name_for_topic(topic_value)
            } else {
                name_value.to_string()
            },
            topic: topic_value.to_string(),
            title,
            summary,
            content,
            citations,
            source_chat_id: Some(source_chat_id.to_string()),
            observed_at: now_secs,
        })
    }

    pub fn governed_target(&self) -> Option<&MemoryCandidateTarget> {
        self.semantic_judgment
            .as_ref()
            .and_then(|judgment| judgment.governed_target.as_ref())
    }
}

pub fn govern_write_candidates(
    candidates: &[MemoryWriteCandidate],
) -> PostTurnSemanticGovernanceReport {
    let mut accepted_count = 0;
    let mut rejected_count = 0;
    let mut deferred_count = 0;
    let mut plane_reports = Vec::new();
    let mut soul_candidate_handoffs = Vec::new();

    for candidate in candidates {
        let mut evidence_refs = candidate.evidence_refs.clone();
        if !candidate.candidate_id.trim().is_empty()
            && !evidence_refs
                .iter()
                .any(|item| item == &candidate.candidate_id)
        {
            evidence_refs.push(candidate.candidate_id.clone());
        }
        let governed_target = candidate.governed_target().unwrap_or(&candidate.target);
        if matches!(
            candidate.target,
            MemoryCandidateTarget::PrivateGarden { .. }
        ) || matches!(candidate.privacy, MemoryPrivacyClass::PrivateGarden)
            || candidate.authority == MemoryEvidenceAuthority::PrivateGardenInternal
        {
            deferred_count += 1;
            plane_reports.push(MemoryPlaneGovernanceReport {
                domain: MemoryWriteDomain::Subject,
                plane: "private_garden".to_string(),
                authority: MemoryWriteAuthority::LlmPrivateGardenFreeform,
                decision: GovernedWriteDecision::Deferred,
                reason: "private_garden_uses_private_garden_governance_not_candidate_write"
                    .to_string(),
                evidence_refs,
                privacy_decision: MemoryPrivacyClass::PrivateGarden.label().to_string(),
                profile_decision: "private_garden_boundary".to_string(),
            });
            continue;
        }

        if matches!(governed_target, MemoryCandidateTarget::Soul { .. })
            || matches!(candidate.target, MemoryCandidateTarget::Soul { .. })
        {
            let judgment_reason = candidate
                .semantic_judgment
                .as_ref()
                .map(|judgment| judgment.reason.clone())
                .filter(|reason| !reason.trim().is_empty())
                .unwrap_or_else(|| {
                    "soul_candidate_handed_off_without_memory_plane_mutation".to_string()
                });
            soul_candidate_handoffs.push(SoulCandidateHandoffReport {
                surface: governed_target.topic().to_string(),
                disposition: SoulCandidateDisposition::HandedOff,
                existing_gate: "soul_governance".to_string(),
                reason: judgment_reason,
                evidence_refs,
            });
            continue;
        }

        let (decision, reason, authority) = if candidate.content.is_empty() {
            (
                GovernedWriteDecision::Rejected,
                "empty_candidate_content".to_string(),
                MemoryWriteAuthority::RuntimeDeterministic,
            )
        } else if candidate.authority == MemoryEvidenceAuthority::AssistantSelfClaim {
            (
                GovernedWriteDecision::Rejected,
                "assistant_self_claim_is_not_identity_memory".to_string(),
                MemoryWriteAuthority::RuntimeDeterministic,
            )
        } else if let Some(judgment) = &candidate.semantic_judgment {
            let authority = match judgment.source {
                MemorySemanticJudgmentSource::LlmGovernance => {
                    MemoryWriteAuthority::LlmGovernedSemantic
                }
                MemorySemanticJudgmentSource::RuntimeGate => {
                    MemoryWriteAuthority::RuntimeDeterministic
                }
            };
            match judgment.decision {
                MemoryCandidateSemanticDecision::Accept => (
                    GovernedWriteDecision::Accepted,
                    if judgment.reason.trim().is_empty() {
                        "llm_semantic_judgment_accepted".to_string()
                    } else {
                        judgment.reason.clone()
                    },
                    authority,
                ),
                MemoryCandidateSemanticDecision::Reject => (
                    GovernedWriteDecision::Rejected,
                    if judgment.reason.trim().is_empty() {
                        "llm_semantic_judgment_rejected".to_string()
                    } else {
                        judgment.reason.clone()
                    },
                    authority,
                ),
                MemoryCandidateSemanticDecision::Defer => (
                    GovernedWriteDecision::Deferred,
                    if judgment.reason.trim().is_empty() {
                        "llm_semantic_judgment_deferred".to_string()
                    } else {
                        judgment.reason.clone()
                    },
                    authority,
                ),
                MemoryCandidateSemanticDecision::HandoffToSoulGovernance => (
                    GovernedWriteDecision::Deferred,
                    if judgment.reason.trim().is_empty() {
                        "soul_handoff_requires_soul_target".to_string()
                    } else {
                        judgment.reason.clone()
                    },
                    authority,
                ),
            }
        } else {
            (
                GovernedWriteDecision::Deferred,
                "llm_semantic_judgment_required_before_plane_mutation".to_string(),
                MemoryWriteAuthority::RuntimeDeterministic,
            )
        };

        match decision {
            GovernedWriteDecision::Accepted => accepted_count += 1,
            GovernedWriteDecision::Rejected => rejected_count += 1,
            GovernedWriteDecision::Deferred => deferred_count += 1,
            GovernedWriteDecision::Merged
            | GovernedWriteDecision::Superseded
            | GovernedWriteDecision::NotApplicable => {}
        }

        plane_reports.push(MemoryPlaneGovernanceReport {
            domain: governed_target.domain(),
            plane: governed_target.plane().to_string(),
            authority,
            decision,
            reason,
            evidence_refs,
            privacy_decision: candidate.privacy.label().to_string(),
            profile_decision: "sdk_candidate_contract".to_string(),
        });
    }

    PostTurnSemanticGovernanceReport {
        attempted: !candidates.is_empty(),
        executed: true,
        skipped_reason: None,
        proposal_count: candidates.len(),
        accepted_count,
        rejected_count,
        deferred_count,
        plane_reports,
        soul_candidate_handoffs,
    }
}
