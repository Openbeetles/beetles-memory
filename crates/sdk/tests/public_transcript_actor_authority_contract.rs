#![cfg(feature = "nonproduction-replay-harness")]

mod support;
use bm_sdk::*;

fn turn(runtime: &MemoryRuntime, id: &str) -> CanonicalTurnDelta {
    CanonicalTurnDelta {
        turn_id: id.into(),
        conversation: ConversationScope {
            channel: "sdk.direct".into(),
            chat_id: "actor-contract".into(),
            conversation_id: Some("actor-contract".into()),
        },
        subject: runtime.subject_id().into(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: MemoryTurnSource {
            ingress: IngressKind::User,
            channel: "sdk.direct".into(),
            provider: None,
            protocol: MemoryTurnProtocol::Native,
            endpoint: None,
            model_alias: None,
            model_resolved: None,
            request_id: None,
            client_conversation_hint: None,
        },
        actor: Some(ActorAttribution {
            speaker_id: "opaque-host-speaker".into(),
            speaker_kind: "host-participant".into(),
            subject_id: Some(runtime.subject_id().into()),
            actor_subject_id: Some(runtime.scoped_runtime().actor_subject_id.clone()),
            mounted_subject_id: Some(runtime.subject_id().into()),
            agent_id: None,
            triggered_by: None,
        }),
        input_messages: vec![TranscriptInputMessage::user(format!("input-{id}"))],
        assistant_message: Some(TranscriptInputMessage::assistant(format!("reply-{id}"))),
        tool_observations: vec![],
        external_content_used: false,
        candidate_ids: vec![],
    }
}

#[test]
fn public_turn_intake_rejects_forged_actor_and_mount_before_any_write() {
    let profile = support::host_test_profile();
    let store = support::empty_store_platform(profile);
    let runtime =
        support::test_runtime_with_scope(store.clone(), profile, "sdk.direct", "actor-contract");
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: turn(&runtime, "positive"),
            host_refs: vec![],
        })
        .expect("matching runtime attribution is accepted");
    let before = store.export_replay_snapshot().unwrap();
    assert!(!before.json_docs.is_empty());
    for use_finalize in [false, true] {
        for field in ["subject", "actor", "mounted"] {
            let mut delta = turn(&runtime, &format!("forged-{use_finalize}-{field}"));
            let actor = delta.actor.as_mut().unwrap();
            match field {
                "subject" => actor.subject_id = Some("forged-subject".into()),
                "actor" => actor.actor_subject_id = Some("forged-actor".into()),
                _ => actor.mounted_subject_id = Some("forged-mount".into()),
            }
            let rejected = if use_finalize {
                runtime
                    .finalize_turn(MemoryTurnFinalizeRequest {
                        turn: delta,
                        learning: PostTurnLearningInputV2::empty(),
                        pressure: PressureLevel::Normal,
                        mode_input: RuntimeLifecycleModeInput::default(),
                    })
                    .is_err()
            } else {
                runtime
                    .commit_transcript(MemoryTranscriptCommitRequest {
                        turn: delta,
                        host_refs: vec![],
                    })
                    .is_err()
            };
            assert!(
                rejected,
                "forged {field} passed public intake (finalize={use_finalize})"
            );
            assert_eq!(store.export_replay_snapshot().unwrap(), before);
        }
    }
}

#[test]
fn an_unpinned_scope_accepts_an_explicit_conversation_without_forging_a_receipt() {
    let profile = support::host_test_profile();
    let store = support::empty_store_platform(profile);
    let runtime =
        support::test_runtime_with_scope(store.clone(), profile, "sdk.direct", "actor-contract");
    assert!(runtime.config().scope.conversation_id.is_none());
    let mut delta = turn(&runtime, "explicit-conversation");
    delta.conversation.conversation_id = Some("explicit-window".into());
    let result = runtime.finalize_turn(MemoryTurnFinalizeRequest {
        turn: delta,
        learning: PostTurnLearningInputV2::empty(),
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    });
    assert!(
        result.is_ok(),
        "an unpinned runtime must accept the canonical turn's explicit conversation"
    );
    assert!(store
        .export_replay_snapshot()
        .unwrap()
        .json_docs
        .iter()
        .any(|document| {
            document.namespace == "conversation_transcript"
                && document.value["key"]["conversation_id"] == "explicit-window"
        }));
}
