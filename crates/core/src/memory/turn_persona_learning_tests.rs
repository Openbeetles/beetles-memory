use super::*;
use crate::memory::{
    CanonicalTurnDelta, ConversationKey, ConversationScope, IngressKind, MemoryTurnProtocol,
    MemoryTurnSource, TranscriptAssistantMessage, TranscriptInputMessage,
};

fn fixture() -> (
    SoulTurnSourceAdmissionV1,
    PostTurnGovernanceIdentityV2,
    TranscriptTurnRecord,
    TurnPersonaInterpretationV1,
) {
    fixture_for("turn-1")
}

fn fixture_for(
    turn_id: &str,
) -> (
    SoulTurnSourceAdmissionV1,
    PostTurnGovernanceIdentityV2,
    TranscriptTurnRecord,
    TurnPersonaInterpretationV1,
) {
    let admission = SoulTurnSourceAdmissionV1 {
        purpose: SoulTurnSourcePurposeV1::Growth,
        owner: SubjectSoulOwnerV1 {
            memory_space_id: "space".into(),
            subject_id: "agent-a".into(),
            soul_id: "soul-a".into(),
        },
        basis: SubjectSoulExpectedStateV1::PristineAbsent {
            closure_certificate_digest: "a".repeat(64),
        },
    };
    let delta = CanonicalTurnDelta {
        turn_id: turn_id.into(),
        conversation: ConversationScope {
            channel: "test".into(),
            chat_id: "chat".into(),
            conversation_id: Some("conversation".into()),
        },
        subject: "agent-a".into(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: MemoryTurnSource {
            ingress: IngressKind::User,
            channel: "test".into(),
            provider: None,
            protocol: MemoryTurnProtocol::Native,
            endpoint: None,
            model_alias: None,
            model_resolved: None,
            request_id: None,
            client_conversation_hint: None,
        },
        actor: None,
        input_messages: vec![TranscriptInputMessage::user(
            "When you admitted uncertainty instead of guessing, I could trust the answer.",
        )],
        assistant_messages: vec![TranscriptAssistantMessage::complete(
            TranscriptInputMessage::assistant(
                "I will keep distinguishing what I know from what still needs checking.",
            ),
        )],
        tool_observations: vec![],
        external_content_used: false,
        candidate_ids: vec![],
    };
    let key = ConversationKey::from_delta("space", &delta).unwrap();
    let transcript = TranscriptTurnRecord::from_delta(&key, 1, &delta, vec![], 100).unwrap();
    let identity = PostTurnGovernanceIdentityV2::new(
        "space",
        "agent-a",
        "test",
        "chat",
        "conversation",
        turn_id,
    )
    .unwrap();
    let interpretation = TurnPersonaInterpretationV1::Growth {
        scope: TurnPersonaInterpretationScopeV1::Subject,
        priority_order: vec![],
        value_orientation: "grounded candor instead of invented certainty".into(),
        relationship_posture: String::new(),
        supporting_observations: vec![PersonaObservationRefV1 {
            message_id: transcript.input_messages[0].message_id.clone(),
            quote: "admitted uncertainty instead of guessing".into(),
        }],
        counter_observations: vec![],
        supporting_visual_sources: vec![],
        counter_visual_sources: vec![],
    };
    (admission, identity, transcript, interpretation)
}

fn visual_persona_observation(
    episode_id: &str,
    input_id: &str,
    originating_turn_id: &str,
) -> VisualPersonaObservationV1 {
    let source = PerceptionEpisodeSourceRef {
        memory_space_id: "space".into(),
        subject_id: "agent-a".into(),
        relationship_id: Some("relation".into()),
        source: crate::memory::PerceptionSourceKey {
            host_id: "host".into(),
            device_id: Some("device".into()),
            source_id: "eye".into(),
            source_generation: 1,
        },
        revoke_epoch: 0,
        episode_id: episode_id.into(),
        content_digest: format!("sha256:{}", "a".repeat(64)),
    };
    VisualPersonaObservationV1 {
        episode_source: source.clone(),
        observation: VisualObservation {
            input_id: input_id.into(),
            source_key: source.source,
            scope: crate::memory::PerceptionScope {
                memory_space_id: source.memory_space_id,
                subject_id: source.subject_id,
                relationship_id: source.relationship_id,
                session_id: "session".into(),
                turn_id: Some(originating_turn_id.into()),
                step_id: None,
            },
            revoke_epoch: 0,
            observed_at_unix_ms: Some(1_800_000_000_000),
            text: "我看到当前用户在画面里微笑".into(),
            search_terms: vec!["微笑".into()],
            uncertainty: VisualObservationUncertainty::Low,
            classification: crate::memory::VisualClassification {
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
        },
    }
}

#[test]
fn visual_persona_growth_requires_two_source_bound_independent_turns() {
    let (admission, identity, mut transcript, _) = fixture();
    let first = visual_persona_observation("episode-a", "input-a", "prior-turn-a");
    let second = visual_persona_observation("episode-b", "input-b", "prior-turn-b");
    transcript.external_content_used = true;
    let request_sha256 = format!("sha256:{}", "d".repeat(64));
    let replayed = [first.episode_source.clone(), second.episode_source.clone()]
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            let owner = crate::memory::PerceptionReplayOwnerV1::LongTermMemory {
                owner_id: format!("fact-{index}"),
                owner_revision: 1,
            };
            (
                crate::memory::TranscriptPerceptionSourceUseV1 {
                    source: source.clone(),
                    first_affected_assistant_sequence: 2,
                    delivery: crate::memory::PerceptionReplyEvidenceV1::ReplayedOwner {
                        owner: owner.clone(),
                    },
                },
                crate::memory::TranscriptExternalContentUseV1::ReplayedVisual {
                    source,
                    owner,
                    request_sha256: request_sha256.clone(),
                },
            )
        })
        .collect::<Vec<_>>();
    let mut source_uses = replayed
        .iter()
        .map(|(source, _)| source.clone())
        .collect::<Vec<_>>();
    source_uses.sort();
    let mut external_uses = replayed
        .into_iter()
        .map(|(_, usage)| usage)
        .collect::<Vec<_>>();
    external_uses.sort();
    transcript = transcript
        .with_perception_source_uses(source_uses)
        .unwrap()
        .with_external_content_manifest(Some(crate::memory::TranscriptExternalContentManifestV1 {
            completeness: crate::memory::ExternalContentCompletenessV1::HostAttestedComplete,
            request_sha256: vec![request_sha256],
            uses: external_uses,
        }))
        .unwrap();
    let interpretation = TurnPersonaInterpretationV1::Growth {
        scope: TurnPersonaInterpretationScopeV1::ActiveRelationship,
        priority_order: vec![],
        value_orientation: String::new(),
        relationship_posture: "remembering repeated shared presence".into(),
        supporting_observations: vec![],
        counter_observations: vec![],
        supporting_visual_sources: vec![
            first.episode_source.clone(),
            second.episode_source.clone(),
        ],
        counter_visual_sources: vec![],
    };
    let evidence = GovernedTurnPersonaEvidenceV1::bind_with_visual(
        &admission,
        identity,
        &transcript,
        Some("relation".into()),
        interpretation.clone(),
        vec![first.clone(), second.clone()],
    )
    .unwrap()
    .unwrap();
    assert_eq!(evidence.visual_observations.len(), 2);
    evidence.validate_source(&admission, &transcript).unwrap();

    let no_manifest = transcript
        .clone()
        .with_external_content_manifest(None)
        .unwrap();
    assert!(interpretation
        .validate_against_with_visual(
            &no_manifest,
            Some("relation"),
            &[first.clone(), second.clone()]
        )
        .is_err());
    let mut with_other = transcript.clone();
    let manifest = with_other.external_content_manifest.as_mut().unwrap();
    manifest
        .uses
        .push(crate::memory::TranscriptExternalContentUseV1::Other {
            category: crate::memory::ExternalContentCategoryV1::RetrievedContent,
            content_digest: format!("sha256:{}", "e".repeat(64)),
            request_sha256: manifest.request_sha256[0].clone(),
            tool_observation_id: None,
        });
    manifest.uses.sort();
    assert!(interpretation
        .validate_against_with_visual(
            &with_other,
            Some("relation"),
            &[first.clone(), second.clone()]
        )
        .is_err());
    let mut uncovered_tool = transcript.clone();
    uncovered_tool
        .tool_observations
        .push(crate::memory::ToolObservationDigest {
            observation_id: "other-tool-output".into(),
            call_id: "other-tool-call".into(),
            tool_name: "search".into(),
            summary: "external result".into(),
            external_content: true,
        });
    assert!(uncovered_tool
        .external_content_manifest
        .as_ref()
        .unwrap()
        .validate_against(&uncovered_tool)
        .is_err());

    let mut only_one = interpretation.clone();
    if let TurnPersonaInterpretationV1::Growth {
        supporting_visual_sources,
        ..
    } = &mut only_one
    {
        supporting_visual_sources.pop();
    }
    assert!(only_one
        .validate_against_with_visual(&transcript, Some("relation"), &[first.clone()])
        .is_err());

    let mut same_turn = second.clone();
    same_turn.observation.scope.turn_id = first.observation.scope.turn_id.clone();
    assert!(interpretation
        .validate_against_with_visual(&transcript, Some("relation"), &[first.clone(), same_turn])
        .is_err());

    let mut private = second;
    private.observation.classification.sensitivity = VisualSensitivity::Private;
    assert!(interpretation
        .validate_against_with_visual(&transcript, Some("relation"), &[first, private])
        .is_err());
}

#[test]
fn archive_restore_preserves_persona_source_but_content_change_does_not() {
    let (admission, identity, transcript, interpretation) = fixture();
    let evidence = GovernedTurnPersonaEvidenceV1::bind(
        &admission,
        identity,
        &transcript,
        None,
        interpretation,
    )
    .unwrap()
    .unwrap();
    let mut archived = transcript.clone();
    archived.lifecycle_state = crate::memory::TranscriptLifecycleState::Archived;
    archived.updated_at += 10;
    evidence.validate_source(&admission, &archived).unwrap();
    archived.lifecycle_state = crate::memory::TranscriptLifecycleState::Active;
    archived.updated_at += 10;
    evidence.validate_source(&admission, &archived).unwrap();

    // Even when the quoted observation still exists, a different source body
    // cannot inherit the previous exact evidence identity.
    archived.input_messages[0]
        .content
        .push_str(" This is corrected content.");
    assert!(evidence.validate_source(&admission, &archived).is_err());
}

#[test]
fn persona_interpreter_has_no_authority_or_identity_fields() {
    let (_, _, transcript, proposal) = fixture();
    assert!(proposal.validate_against(&transcript, None).is_ok());
    let payload = serde_json::to_value(&proposal).unwrap();
    for forbidden in [
        "subject_id",
        "generation",
        "revision",
        "confirmed",
        "meaningful_turns",
        "source_digest",
    ] {
        let mut forged = payload.clone();
        forged[forbidden] = serde_json::json!(4);
        assert!(TurnPersonaInterpretationV1::parse(&forged.to_string()).is_err());
    }
    assert!(TurnPersonaInterpretationV1::parse(&format!("Analysis: {}", payload)).is_err());
}

#[test]
fn persona_growth_cannot_confuse_values_or_aliases_with_governance_priority_domains() {
    let (_, _, transcript, mut proposal) = fixture();
    let TurnPersonaInterpretationV1::Growth { priority_order, .. } = &mut proposal else {
        panic!("growth fixture");
    };
    *priority_order = vec!["self_authored_core".into(), "boundary".into()];
    assert!(proposal.validate_against(&transcript, None).is_ok());
    for value in [
        "truthfulness",
        "truth before convenience",
        "core",
        "Boundary",
        "help",
    ] {
        let mut invalid = proposal.clone();
        let TurnPersonaInterpretationV1::Growth { priority_order, .. } = &mut invalid else {
            unreachable!();
        };
        *priority_order = vec![value.into()];
        assert!(
            invalid.validate_against(&transcript, None).is_err(),
            "{value} is not a canonical governance domain"
        );
    }
}

#[test]
fn persona_value_observation_is_distinct_from_governance_order() {
    let (admission, identity, transcript, proposal) = fixture();
    let mut payload = serde_json::to_value(proposal).unwrap();
    payload["priority_order"] = serde_json::json!([]);
    payload["value_orientation"] =
        serde_json::json!("grounded candor instead of invented certainty");
    let proposal = TurnPersonaInterpretationV1::parse(&payload.to_string())
        .expect("canonical value observation must not require a fabricated governance order");
    let evidence =
        GovernedTurnPersonaEvidenceV1::bind(&admission, identity, &transcript, None, proposal)
            .unwrap()
            .unwrap();
    let view = evidence
        .continuity_view(
            &admission,
            &transcript,
            crate::memory::PersonalityRelationshipScope::SubjectOnly,
        )
        .unwrap()
        .unwrap();
    let (second_admission, second_identity, second_transcript, second_proposal) =
        fixture_for("turn-2");
    let second_evidence = GovernedTurnPersonaEvidenceV1::bind(
        &second_admission,
        second_identity,
        &second_transcript,
        None,
        second_proposal,
    )
    .unwrap()
    .unwrap();
    let second_view = second_evidence
        .continuity_view(
            &second_admission,
            &second_transcript,
            crate::memory::PersonalityRelationshipScope::SubjectOnly,
        )
        .unwrap()
        .unwrap();
    assert_ne!(evidence.evidence_id, second_evidence.evidence_id);
    let single = crate::memory::derive_recent_persona_evidence_from_continuity_evidence(
        std::slice::from_ref(&view),
        2,
    )
    .unwrap();
    assert!(
        !single.has_promotable_growth_signals(),
        "one observation must not create stable values"
    );
    let recent = crate::memory::derive_recent_persona_evidence_from_continuity_evidence(
        &[view.clone(), second_view.clone()],
        2,
    )
    .unwrap();
    let rendered = crate::memory::render_recent_persona_evidence_block(&recent, 2048).unwrap();
    assert!(recent.repeated_priority_order.is_empty());
    assert!(recent.has_promotable_growth_signals());
    assert!(rendered.contains("grounded candor instead of invented certainty"));
    let persisted: GovernedTurnPersonaEvidenceV1 =
        serde_json::from_value(serde_json::to_value(&evidence).unwrap()).unwrap();
    persisted.validate_source(&admission, &transcript).unwrap();
    assert_eq!(persisted, evidence);
    let mut counter = second_view;
    counter.growth_counterevidence = true;
    let held =
        crate::memory::derive_recent_persona_evidence_from_continuity_evidence(&[view, counter], 2)
            .unwrap();
    assert!(!held.has_promotable_growth_signals());
    assert!(held.repeated_value_orientation.is_empty());
}

#[test]
fn subject_values_are_canonical_and_cannot_cross_relationship_scope() {
    let (_, _, transcript, proposal) = fixture();
    let original = serde_json::to_value(proposal).unwrap();
    for value in [
        "",
        " ",
        " unsupported leading space",
        "value with trailing space ",
    ] {
        let mut payload = original.clone();
        payload["value_orientation"] = serde_json::json!(value);
        let parsed = TurnPersonaInterpretationV1::parse(&payload.to_string()).unwrap();
        assert!(parsed.validate_against(&transcript, None).is_err());
    }
    let mut relation = original.clone();
    relation["scope"] = serde_json::json!("active_relationship");
    relation["relationship_posture"] = serde_json::json!("repair first");
    let parsed = TurnPersonaInterpretationV1::parse(&relation.to_string()).unwrap();
    assert!(parsed
        .validate_against(&transcript, Some("relationship-a"))
        .is_err());
    relation["value_orientation"] = serde_json::json!("");
    let parsed = TurnPersonaInterpretationV1::parse(&relation.to_string()).unwrap();
    parsed
        .validate_against(&transcript, Some("relationship-a"))
        .unwrap();
    let mut missing = original;
    missing.as_object_mut().unwrap().remove("value_orientation");
    assert!(TurnPersonaInterpretationV1::parse(&missing.to_string()).is_err());
}

#[test]
fn projection_self_echo_cancelled_or_external_turn_cannot_supply_growth() {
    let (admission, identity, transcript, proposal) = fixture();
    for authority in [
        MemoryEvidenceAuthority::SubjectProjection,
        MemoryEvidenceAuthority::AssistantSelfClaim,
        MemoryEvidenceAuthority::AssistantUtterance,
        MemoryEvidenceAuthority::ExternalContent,
    ] {
        let mut invalid = transcript.clone();
        invalid.input_messages[0].authority = authority;
        assert!(GovernedTurnPersonaEvidenceV1::bind(
            &admission,
            identity.clone(),
            &invalid,
            None,
            proposal.clone()
        )
        .is_err());
    }
    for delivery in [
        MemoryTurnDeliveryStatus::Cancelled,
        MemoryTurnDeliveryStatus::IncompleteStream,
        MemoryTurnDeliveryStatus::UserOnly,
    ] {
        let mut invalid = transcript.clone();
        invalid.delivery_status = delivery;
        assert!(GovernedTurnPersonaEvidenceV1::bind(
            &admission,
            identity.clone(),
            &invalid,
            None,
            proposal.clone()
        )
        .is_err());
    }
    let mut external = transcript.clone();
    external.external_content_used = true;
    assert!(proposal.validate_against(&external, None).is_err());
    assert!(GovernedTurnPersonaEvidenceV1::bind(
        &admission,
        identity,
        &transcript,
        None,
        TurnPersonaInterpretationV1::OperationalOnly
    )
    .unwrap()
    .is_none());
}

#[test]
fn retry_cannot_make_another_sample_and_source_change_invalidates_evidence() {
    let (admission, identity, mut transcript, proposal) = fixture();
    let evidence = GovernedTurnPersonaEvidenceV1::bind(
        &admission,
        identity.clone(),
        &transcript,
        None,
        proposal.clone(),
    )
    .unwrap()
    .unwrap();
    let retry = GovernedTurnPersonaEvidenceV1::bind(
        &admission,
        identity.clone(),
        &transcript,
        None,
        proposal,
    )
    .unwrap()
    .unwrap();
    assert_eq!(evidence, retry);
    assert!(evidence
        .continuity_view(
            &admission,
            &transcript,
            super::super::PersonalityRelationshipScope::SubjectOnly
        )
        .unwrap()
        .is_some());
    transcript.updated_at += 1;
    evidence.validate_source(&admission, &transcript).unwrap();
    let mut corrected = transcript.clone();
    corrected.assistant_messages[0]
        .message
        .content
        .push_str(" Corrected canonical content.");
    assert!(evidence.validate_source(&admission, &corrected).is_err());
    let mut changed = retry.interpretation.clone();
    if let TurnPersonaInterpretationV1::Growth {
        value_orientation, ..
    } = &mut changed
    {
        *value_orientation = "clarity about uncertainty".into();
    }
    let replacement =
        GovernedTurnPersonaEvidenceV1::bind(&admission, identity, &transcript, None, changed)
            .unwrap()
            .unwrap();
    assert_eq!(evidence.evidence_id, replacement.evidence_id);
    assert_ne!(evidence.content_digest, replacement.content_digest);
}

#[test]
fn generation_admission_survives_growth_but_not_reset_archive_or_subject_change() {
    let (admission, _, _, _) = fixture();
    let mut current = admission.clone();
    current.basis = SubjectSoulExpectedStateV1::Exact {
        generation: 1,
        revision: Some(2),
        lifecycle_state: SubjectSoulLifecycleStateV1::Active,
        head_digest: "b".repeat(64),
        manifest_digest: "c".repeat(64),
    };
    assert!(admission.permits_current(&current));
    if let SubjectSoulExpectedStateV1::Exact { generation, .. } = &mut current.basis {
        *generation = 2;
    }
    assert!(!admission.permits_current(&current));
    if let SubjectSoulExpectedStateV1::Exact {
        generation,
        lifecycle_state,
        ..
    } = &mut current.basis
    {
        *generation = 1;
        *lifecycle_state = SubjectSoulLifecycleStateV1::Archived;
    }
    assert!(!admission.permits_current(&current));
    current = admission.clone();
    current.owner.subject_id = "agent-b".into();
    assert!(!admission.permits_current(&current));
}

#[test]
fn relationship_interpretation_requires_explicit_relationship_and_counterevidence_is_retained() {
    let (admission, identity, transcript, mut proposal) = fixture();
    if let TurnPersonaInterpretationV1::Growth {
        scope,
        value_orientation,
        relationship_posture,
        ..
    } = &mut proposal
    {
        *scope = TurnPersonaInterpretationScopeV1::ActiveRelationship;
        value_orientation.clear();
        *relationship_posture = "respectful".into();
    }
    assert!(proposal.validate_against(&transcript, None).is_err());
    let positive = GovernedTurnPersonaEvidenceV1::bind(
        &admission,
        identity.clone(),
        &transcript,
        Some("relation-a".into()),
        proposal.clone(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(positive.relationship_id.as_deref(), Some("relation-a"));
    if let TurnPersonaInterpretationV1::Growth {
        counter_observations,
        ..
    } = &mut proposal
    {
        counter_observations.push(PersonaObservationRefV1 {
            message_id: transcript.input_messages[0].message_id.clone(),
            quote: "instead of guessing".into(),
        });
    }
    let caution = GovernedTurnPersonaEvidenceV1::bind(
        &admission,
        identity,
        &transcript,
        Some("relation-a".into()),
        proposal,
    )
    .unwrap()
    .unwrap();
    assert!(
        caution
            .continuity_view(
                &admission,
                &transcript,
                super::super::PersonalityRelationshipScope::Active("relation-a")
            )
            .unwrap()
            .expect("counterevidence must occupy the observation window")
            .growth_counterevidence
    );
}

#[test]
fn source_review_admission_cannot_impersonate_ordinary_growth_or_another_epoch() {
    let (mut admission, _, _, _) = fixture();
    admission.basis = SubjectSoulExpectedStateV1::Exact {
        generation: 1,
        revision: Some(1),
        lifecycle_state: SubjectSoulLifecycleStateV1::Active,
        head_digest: "a".repeat(64),
        manifest_digest: "b".repeat(64),
    };
    let ordinary = admission.clone();
    admission.purpose = SoulTurnSourcePurposeV1::SourceReview {
        basis: crate::memory::SubjectSoulSourceReviewBasisV1 {
            epoch: 1,
            pending_digest: "c".repeat(64),
        },
    };
    assert!(!admission.permits_current(&ordinary));
    assert!(!ordinary.permits_current(&admission));
    assert!(admission.permits_current(&admission));
    let mut newer = admission.clone();
    if let SoulTurnSourcePurposeV1::SourceReview { basis } = &mut newer.purpose {
        basis.epoch += 1;
    }
    assert!(!admission.permits_current(&newer));
}
