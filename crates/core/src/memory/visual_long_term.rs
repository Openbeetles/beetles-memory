//! A visual observation may become a factual draft only with its exact episode source.
//! The caller must first make Beetle's governed `memory_worthy` selection; this
//! planner never promotes a reply excerpt or an ordinary frame by itself.

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

use super::{
    AttributionConfidence, LongTermMemoryConfidence, LongTermMemoryDraft, LongTermMemoryFreshness,
    LongTermMemoryKind, LongTermMemoryProvenance, LongTermMemorySourceScope,
    LongTermMemorySourceType, LongTermMemoryStaleHint, MemoryEvidenceAuthority, MemoryPrivacyClass,
    MemorySemanticJudgmentSource, MemorySubjectVisibilityPolicy, PerceptionEpisodeSourceRef,
    PerceptionSourceKind, SubjectAttribution, VisualEvidenceOrigin, VisualObservation,
    VisualObservationUncertainty, VisualSensitivity, MAX_LONG_TERM_MEMORY_CONTENT_LEN,
};

/// The draft and source are indivisible until the same owner revision is committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualLongTermDraft {
    pub draft: LongTermMemoryDraft,
    pub source: PerceptionEpisodeSourceRef,
    pub observation_input_id: String,
}

/// Convert a selected Beetle visual observation into a time-bound factual candidate.
/// `None` means policy rejected persistence; it does not erase the governed episode.
pub fn plan_visual_long_term_draft(
    observation: &VisualObservation,
    source: &PerceptionEpisodeSourceRef,
    source_kind: PerceptionSourceKind,
) -> Result<Option<VisualLongTermDraft>> {
    observation.validate_contract()?;
    source.validate_contract()?;
    if observation.source_key != source.source
        || observation.scope.memory_space_id != source.memory_space_id
        || observation.scope.subject_id != source.subject_id
        || observation.scope.relationship_id != source.relationship_id
        || observation.revoke_epoch != source.revoke_epoch
    {
        return Err(Error::invalid_input(
            "visual_long_term_source",
            "observation and episode source do not bind the same scope and generation",
        ));
    }
    if source_kind == PerceptionSourceKind::HostVisualObservation
        && source.source.device_id.is_none()
    {
        return Err(Error::invalid_input(
            "visual_long_term_source",
            "a captured visual source requires a device",
        ));
    }
    if !matches!(
        source_kind,
        PerceptionSourceKind::HostVisualObservation | PerceptionSourceKind::HumanUserSharedMedia
    ) {
        return Ok(None);
    }
    let VisualEvidenceOrigin::BeetleInference { .. } = &observation.evidence_origin else {
        return Ok(None);
    };
    let class = &observation.classification;
    if observation.uncertainty != VisualObservationUncertainty::Low
        || class.sensitivity != VisualSensitivity::Ordinary
        || class.third_party_present
        || class.minor_present
        || class.sensitive_document_present
        || class.conflicting_evidence
    {
        return Ok(None);
    }
    let current_user = matches!(
        class.subject_attribution,
        SubjectAttribution::VerifiedCurrentUser | SubjectAttribution::ContextualCurrentUser
    ) && class.attribution_confidence == AttributionConfidence::High
        && class.visible_person_count == Some(1);
    let nonhuman_shared_scene = source_kind == PerceptionSourceKind::HumanUserSharedMedia
        && class.ordinary_nonhuman_semantic_allowed();
    if !current_user && !nonhuman_shared_scene {
        return Ok(None);
    }
    let Some(observed_at) = observation
        .observed_at_unix_ms
        .map(|millis| millis / 1000)
        .filter(|seconds| *seconds > 0)
    else {
        return Ok(None);
    };
    let prefix = if current_user {
        "我曾在画面里看到当前用户："
    } else {
        "我曾在用户分享的画面里看到场景："
    };
    let content = format!("{prefix}{}", observation.text.trim());
    if content.len() > MAX_LONG_TERM_MEMORY_CONTENT_LEN {
        return Ok(None);
    }
    let digest = Sha256::digest(format!(
        "{}\0{}\0{}",
        source.content_digest, source.episode_id, observation.input_id
    ));
    let topic = format!("visual_{:x}", digest);
    let topic = topic[..31].to_string();
    let draft = LongTermMemoryDraft {
        kind: LongTermMemoryKind::Fact,
        topic,
        content,
        keywords: observation.search_terms.clone(),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: None,
        source_type: Some(
            if source_kind == PerceptionSourceKind::HostVisualObservation {
                LongTermMemorySourceType::ExternalObservation
            } else {
                LongTermMemorySourceType::Conversation
            },
        ),
        // A shared scene is an episode observed by this subject, not a universal world claim.
        source_scope: Some(LongTermMemorySourceScope::User),
        subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![source
            .subject_id
            .clone()]),
        provenance: LongTermMemoryProvenance {
            source_authority: MemoryEvidenceAuthority::WorldObservation,
            semantic_judgment_source: Some(MemorySemanticJudgmentSource::LlmGovernance),
        },
        confidence: Some(LongTermMemoryConfidence::Medium),
        freshness: Some(LongTermMemoryFreshness::Dynamic),
        stale_hint: Some(LongTermMemoryStaleHint::ReviewBeforeUse),
        supporting_citations: Vec::new(),
        canonical_entities: Vec::new(),
        evidence_count: Some(1),
        observed_at: Some(observed_at),
        source_revision: None,
    };
    let draft = draft.normalized().ok_or_else(|| {
        Error::invalid_input(
            "visual_long_term_draft",
            "selected visual fact is not a valid draft",
        )
    })?;
    Ok(Some(VisualLongTermDraft {
        draft,
        source: source.clone(),
        observation_input_id: observation.input_id.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        PerceptionScope, PerceptionSourceKey, VisualClassification, VisualSensitivity,
    };

    fn fixture() -> (VisualObservation, PerceptionEpisodeSourceRef) {
        let source = PerceptionEpisodeSourceRef {
            memory_space_id: "space".into(),
            subject_id: "soul".into(),
            relationship_id: Some("relationship".into()),
            source: PerceptionSourceKey {
                host_id: "host".into(),
                device_id: Some("camera".into()),
                source_id: "eye".into(),
                source_generation: 1,
            },
            revoke_epoch: 0,
            episode_id: "episode".into(),
            content_digest: format!("sha256:{}", "a".repeat(64)),
        };
        let observation = VisualObservation {
            input_id: "input".into(),
            source_key: source.source.clone(),
            scope: PerceptionScope {
                memory_space_id: source.memory_space_id.clone(),
                subject_id: source.subject_id.clone(),
                relationship_id: source.relationship_id.clone(),
                session_id: "session".into(),
                turn_id: Some("turn".into()),
                step_id: None,
            },
            revoke_epoch: source.revoke_epoch,
            observed_at_unix_ms: Some(1_800_000_000_000),
            text: "穿着红色裙子".into(),
            search_terms: vec!["红裙子".into(), "穿着".into()],
            uncertainty: VisualObservationUncertainty::Low,
            classification: VisualClassification {
                subject_attribution: SubjectAttribution::ContextualCurrentUser,
                attribution_confidence: AttributionConfidence::High,
                visible_person_count: Some(1),
                third_party_present: false,
                sensitivity: VisualSensitivity::Ordinary,
                minor_present: false,
                sensitive_document_present: false,
                attribution_evidence_refs: vec!["context".into()],
                conflicting_evidence: false,
            },
            evidence_origin: VisualEvidenceOrigin::BeetleInference {
                provider_id: "vision".into(),
                request_receipt_digest: format!("sha256:{}", "b".repeat(64)),
                inference_digest: format!("sha256:{}", "c".repeat(64)),
            },
        };
        (observation, source)
    }

    #[test]
    fn selected_current_user_observation_keeps_typed_source_and_narrow_visibility() {
        let (observation, source) = fixture();
        let result = plan_visual_long_term_draft(
            &observation,
            &source,
            PerceptionSourceKind::HostVisualObservation,
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.source, source);
        assert_eq!(result.observation_input_id, observation.input_id);
        assert_eq!(result.draft.kind, LongTermMemoryKind::Fact);
        assert_eq!(
            result.draft.subject_visibility,
            MemorySubjectVisibilityPolicy::OnlySubjects(vec!["soul".into()])
        );
        assert_eq!(result.draft.observed_at, Some(1_800_000_000));
        assert_eq!(result.draft.keywords, vec!["红裙子", "穿着"]);
        assert!(result
            .draft
            .content
            .starts_with("我曾在画面里看到当前用户："));
    }

    #[test]
    fn third_party_private_unknown_and_reply_sources_do_not_become_user_facts() {
        let (mut observation, source) = fixture();
        observation.classification.subject_attribution = SubjectAttribution::ThirdParty;
        observation.classification.attribution_evidence_refs.clear();
        assert!(plan_visual_long_term_draft(
            &observation,
            &source,
            PerceptionSourceKind::HostVisualObservation
        )
        .unwrap()
        .is_none());
        observation.classification.subject_attribution = SubjectAttribution::Unknown;
        observation.classification.attribution_confidence = AttributionConfidence::Low;
        assert!(plan_visual_long_term_draft(
            &observation,
            &source,
            PerceptionSourceKind::HostVisualObservation
        )
        .unwrap()
        .is_none());
        observation.classification.subject_attribution = SubjectAttribution::ContextualCurrentUser;
        observation.classification.attribution_confidence = AttributionConfidence::High;
        observation
            .classification
            .attribution_evidence_refs
            .push("context".into());
        observation.classification.sensitivity = VisualSensitivity::Private;
        assert!(plan_visual_long_term_draft(
            &observation,
            &source,
            PerceptionSourceKind::HostVisualObservation
        )
        .unwrap()
        .is_none());
        observation.classification.sensitivity = VisualSensitivity::Ordinary;
        observation.evidence_origin = VisualEvidenceOrigin::HostAttestedReply {
            delivery_receipt_digest: format!("sha256:{}", "d".repeat(64)),
            reply_digest: format!("sha256:{}", "e".repeat(64)),
        };
        observation.search_terms.clear();
        assert!(plan_visual_long_term_draft(
            &observation,
            &source,
            PerceptionSourceKind::HostVisualObservation
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn ordinary_nonhuman_shared_photo_is_scene_only_and_source_is_exact() {
        let (mut observation, mut source) = fixture();
        source.source.device_id = None;
        observation.source_key = source.source.clone();
        observation.classification.subject_attribution = SubjectAttribution::Unknown;
        observation.classification.attribution_confidence = AttributionConfidence::Low;
        observation.classification.visible_person_count = Some(0);
        observation.classification.attribution_evidence_refs.clear();
        let planned = plan_visual_long_term_draft(
            &observation,
            &source,
            PerceptionSourceKind::HumanUserSharedMedia,
        )
        .unwrap()
        .unwrap();
        assert!(planned
            .draft
            .content
            .starts_with("我曾在用户分享的画面里看到场景："));
        assert!(plan_visual_long_term_draft(
            &observation,
            &source,
            PerceptionSourceKind::HostVisualObservation
        )
        .is_err());
        source.revoke_epoch = 1;
        assert!(plan_visual_long_term_draft(
            &observation,
            &source,
            PerceptionSourceKind::HumanUserSharedMedia
        )
        .is_err());
    }
}
