#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    default_memory_space_id, primary_human_subject_id, relationship_scope, ConversationKey,
};
use bm_core::platform::Platform as _;
use bm_sdk::{
    AuthorizedGovernanceEnvelope, GovernanceEgressAuthority, GovernanceExecutionOperation,
    GovernanceExecutionPort, GovernanceExecutionPortFailure, ImmutableGovernanceExecutionBinding,
    LlmClient, LlmHttpClient, LlmResponse, MemoryGovernanceBindingInstallRequest,
    MemoryLearningCycleRequest, MemoryLearningEngine, Message,
    PostTurnGovernanceProviderProtocolV1, StopReason, ToolChoicePolicy, ToolSpec,
};
use bm_sdk::{
    BeetleVisualInferenceClient, BeetleVisualInferenceRequest, BeetleVisualInferenceResponse,
    CanonicalTurnDelta, ConversationScope, ExternalContentCompletenessV1, HostSentVisualPart,
    HostVisualDeliveryReceipt, LongTermMemoryQuery, MemoryArchiveScope, MemoryCapabilityPolicy,
    MemoryIdentity, MemoryLongTermControlView, MemoryLongTermListRequest, MemoryPrivacyPolicy,
    MemoryProjectionRequest, MemoryRecallTemporalOperation, MemoryRuntime, MemoryScope,
    MemorySpaceExportRequest, MemorySpaceImportRequest, MemorySpacePrivateMaterialPolicy,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    NoopMemoryAuditSink, PerceptionCapabilitySnapshot, PerceptionCapabilityState,
    PerceptionHostHandle, PerceptionInputEnvelope, PerceptionModality, PerceptionRetentionCeiling,
    PerceptionScope, PerceptionSource, PerceptionSourceKey, PerceptionSourceKind,
    PerceptionSourceRef, PerceptionTime, PerceptionUnavailableReason, PressureLevel,
    ProceduralProjectionBindingV1, ProviderVisualWireRole, RuntimeLifecycleModeInput,
    SubjectScopedRuntime, TranscriptAssistantMessage, TranscriptExternalContentManifestV1,
    TranscriptInputMessage, UserVisualAttachmentBinding, VisualCaptureContext, VisualCaptureForm,
    VisualDeviceUse, VisualFrame, VisualFrameStream, VisualImage, VisualInferenceAuthorization,
    VisualInput, VisualMediaUnit, VisualMemoryObservationDecision, VisualSourceAuthorization,
    VisualToolRegistrySnapshot, VisualTurnPosition, VisualVideo,
};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use support::empty_store_platform;

struct VisualInferenceFixture {
    person_count: u8,
    synthetic_user_role: bool,
    needs_revisit: bool,
    description: &'static str,
}

impl BeetleVisualInferenceClient for VisualInferenceFixture {
    fn infer(
        &mut self,
        request: &BeetleVisualInferenceRequest,
    ) -> bm_sdk::Result<BeetleVisualInferenceResponse> {
        let body = b"exact-sidecar-request".to_vec();
        let search_terms = if self.description.contains("红裙子") {
            vec!["红裙子"]
        } else if self.description.contains("蓝帽子") {
            vec!["蓝帽子"]
        } else if self.description.contains("猫") {
            vec!["猫"]
        } else {
            vec!["画面"]
        };
        Ok(BeetleVisualInferenceResponse {
            recipient_id: request.recipient_id.clone(),
            request_commitment: request.request_commitment.clone(),
            sent_request_body: body.clone(),
            ordered_sent_parts: request
                .media_parts
                .iter()
                .enumerate()
                .map(|(index, part)| HostSentVisualPart {
                    input_id: part.input_id.clone(),
                    source_key: part.source_key.clone(),
                    ordinal: part.ordinal,
                    step_id: part.step_id.clone(),
                    message_index: index,
                    content_index: 0,
                    wire_role: ProviderVisualWireRole::User,
                    synthetic_user_role: self.synthetic_user_role,
                    parent_sha256: part.sha256.clone(),
                    sent_bytes: part.bytes().to_vec(),
                    transform_steps: Vec::new(),
                })
                .collect(),
            response_request_sha256: format!("sha256:{:x}", Sha256::digest(&body)),
            response_body: serde_json::json!({
                "description": self.description,
                "search_terms": search_terms,
                "uncertainty": "low",
                "visible_person_count": self.person_count,
                "third_party_or_visitor_present": false,
                "conflicting_identity_evidence": false,
                "sensitivity": "ordinary",
                "minor_present": false,
                "sensitive_document_present": false,
                "memory_worthy": true,
                "needs_revisit": self.needs_revisit,
            })
            .to_string()
            .into_bytes(),
        })
    }
}

fn runtime(store: bm_sdk::MemoryStoreHandle) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").unwrap())
        .subject_id("subject-default")
        .scope(MemoryScope::new("llm.gateway", "chat-a").unwrap())
        .store(store)
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .unwrap()
}

fn runtime_in_conversation(
    store: bm_sdk::MemoryStoreHandle,
    conversation_id: &str,
) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").unwrap())
        .subject_id("subject-default")
        .scope(
            MemoryScope::new("llm.gateway", "chat-a")
                .unwrap()
                .with_conversation_id(conversation_id)
                .unwrap(),
        )
        .store(store)
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .unwrap()
}

fn png() -> Vec<u8> {
    "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63f8cfc0f01f00050001ff89993d1d0000000049454e44ae426082"
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

struct VisualPersonaModel {
    interpretations: AtomicUsize,
}

impl LlmClient for VisualPersonaModel {
    fn chat(
        &self,
        _: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        _: Option<&[ToolSpec]>,
        _: ToolChoicePolicy,
    ) -> bm_sdk::Result<LlmResponse> {
        let content = if system.contains("subject visual persona interpretation") {
            self.interpretations.fetch_add(1, Ordering::SeqCst);
            let input: serde_json::Value = serde_json::from_str(&messages[0].content).unwrap();
            let visuals = input["visual_observations"].as_array().unwrap();
            let text = input["text_observations"].as_array().unwrap();
            assert_eq!(visuals.len(), 2);
            assert_eq!(text.len(), 1);
            serde_json::json!({
                "decision": "growth",
                "scope": "subject",
                "priority_order": [],
                "value_orientation": "我只据可核验的共同经历形成记忆，并尊重视觉边界",
                "relationship_posture": "",
                "supporting_observations": [{
                    "message_id": text[0]["message_id"],
                    "quote": text[0]["text"],
                }],
                "counter_observations": [],
                "supporting_visual_sources": visuals.iter().map(|item| item["episode_source"].clone()).collect::<Vec<_>>(),
                "counter_visual_sources": [],
            })
            .to_string()
        } else if system.contains("visual persona interpretation") {
            self.interpretations.fetch_add(1, Ordering::SeqCst);
            let input: serde_json::Value = serde_json::from_str(&messages[0].content).unwrap();
            let observations = input["visual_observations"].as_array().unwrap();
            assert_eq!(observations.len(), 2);
            serde_json::json!({
                "decision": "growth",
                "scope": "active_relationship",
                "priority_order": [],
                "value_orientation": "",
                "relationship_posture": "我会记得共同经历，同时尊重她关掉视觉的边界",
                "supporting_observations": [],
                "counter_observations": [],
                "supporting_visual_sources": observations.iter().map(|item| item["episode_source"].clone()).collect::<Vec<_>>(),
                "counter_visual_sources": [],
            })
            .to_string()
        } else if system.contains("private garden") {
            "null".into()
        } else if system.contains("inward autonomy runtime") {
            "{}".into()
        } else {
            "[]".into()
        };
        Ok(LlmResponse {
            content,
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

struct VisualPersonaPort<'a>(&'a VisualPersonaModel);

impl GovernanceExecutionPort for VisualPersonaPort<'_> {
    fn execute(
        &mut self,
        _: &AuthorizedGovernanceEnvelope,
        _: &ImmutableGovernanceExecutionBinding,
        egress: &GovernanceEgressAuthority,
        operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        egress
            .revalidate_before_egress()
            .map_err(GovernanceExecutionPortFailure::Other)?;
        operation
            .run(&mut support::StaticHttpClient, self.0)
            .map_err(|error| {
                eprintln!("visual persona governance failure: {error:?}");
                GovernanceExecutionPortFailure::Other(error)
            })
    }
}

fn source_key(device_id: Option<&str>, source_id: &str) -> PerceptionSourceKey {
    PerceptionSourceKey {
        host_id: "host-a".into(),
        device_id: device_id.map(str::to_string),
        source_id: source_id.into(),
        source_generation: 1,
    }
}

trait TestVisualRegistry {
    fn update_perception_capability(
        &self,
        handle: &PerceptionHostHandle,
        capability: PerceptionCapabilitySnapshot,
    ) -> bm_sdk::Result<()>;
}

impl TestVisualRegistry for MemoryRuntime {
    fn update_perception_capability(
        &self,
        handle: &PerceptionHostHandle,
        capability: PerceptionCapabilitySnapshot,
    ) -> bm_sdk::Result<()> {
        self.register_visual_tool_snapshot(
            handle,
            VisualToolRegistrySnapshot::new(
                capability,
                PerceptionRetentionCeiling::DurableMedia,
                true,
            ),
        )
    }
}

fn authorize_input(
    runtime: &MemoryRuntime,
    handle: &PerceptionHostHandle,
    input: &PerceptionInputEnvelope<VisualInput>,
    retention: PerceptionRetentionCeiling,
) {
    let attachment = match &input.source.source_ref {
        PerceptionSourceRef::UserAttachment {
            message_id,
            attachment_id,
        } => Some(UserVisualAttachmentBinding {
            turn_id: input.scope.turn_id.clone().unwrap(),
            message_id: message_id.clone(),
            attachment_id: attachment_id.clone(),
            input_message_index: 0,
            user_message: TranscriptInputMessage::user("看一眼"),
            media_sha256: input
                .payload
                .media_parts()
                .iter()
                .map(|part| part.sha256.to_string())
                .collect(),
        }),
        _ => None,
    };
    runtime
        .authorize_visual_source(
            handle,
            VisualSourceAuthorization {
                source_key: input.source.key.clone(),
                source_kind: input.source.kind,
                revoke_epoch: input.revoke_epoch,
                authorization_generation: 1,
                retention_ceiling: retention,
                user_consent_attested: true,
                user_attachment: attachment,
                audio_free_video_sha256: match &input.payload {
                    VisualInput::Video(video) => Some(video.media.sha256.clone()),
                    _ => None,
                },
            },
        )
        .unwrap();
}

fn authorized_input(
    runtime: &MemoryRuntime,
    handle: &PerceptionHostHandle,
    input: PerceptionInputEnvelope<VisualInput>,
    retention: PerceptionRetentionCeiling,
) -> PerceptionInputEnvelope<VisualInput> {
    authorize_input(runtime, handle, &input, retention);
    input
}

fn visual_input(
    input_id: &str,
    turn_id: &str,
    key: PerceptionSourceKey,
    kind: PerceptionSourceKind,
    source_ref: PerceptionSourceRef,
    step_id: Option<&str>,
) -> PerceptionInputEnvelope<VisualInput> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    PerceptionInputEnvelope {
        input_id: input_id.into(),
        modality: PerceptionModality::Visual,
        source: PerceptionSource {
            key,
            kind,
            source_ref,
        },
        scope: PerceptionScope {
            memory_space_id: default_memory_space_id("owner-default"),
            subject_id: "subject-default".into(),
            relationship_id: Some(
                relationship_scope(
                    "subject-default",
                    "llm.gateway",
                    "chat-a",
                    Some("chat-a".into()),
                )
                .relationship_id,
            ),
            session_id: "chat-a".into(),
            turn_id: Some(turn_id.into()),
            step_id: step_id.map(str::to_string),
        },
        time: PerceptionTime {
            observed_at_unix_ms: Some(now_ms),
            presented_at_unix_ms: now_ms,
        },
        revoke_epoch: 0,
        payload: VisualInput::Image(VisualImage {
            media: VisualMediaUnit::new("image/png", png()).unwrap(),
            width: 1,
            height: 1,
            derivation: None,
        }),
    }
}

fn turn(turn_id: &str, assistant: &[&str]) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn_id.into(),
            conversation: ConversationScope {
                channel: "llm.gateway".into(),
                chat_id: "chat-a".into(),
                conversation_id: Some("chat-a".into()),
            },
            subject: "subject-default".into(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: bm_sdk::IngressKind::User,
                channel: "llm.gateway".into(),
                provider: Some("local".into()),
                protocol: MemoryTurnProtocol::OllamaChat,
                endpoint: Some("/api/chat".into()),
                model_alias: Some("vision".into()),
                model_resolved: Some("vision".into()),
                request_id: Some(format!("request-{turn_id}")),
                client_conversation_hint: Some("chat-a".into()),
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user("看一眼")],
            assistant_messages: assistant
                .iter()
                .map(|text| {
                    TranscriptAssistantMessage::complete(TranscriptInputMessage::assistant(*text))
                })
                .collect(),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        learning: bm_sdk::PostTurnLearningInputV3::empty(),
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn receipt(
    packet: &bm_sdk::ProviderVisualDeliveryPacket,
    synthetic_user_role: bool,
) -> HostVisualDeliveryReceipt {
    let body = b"host-final-request-with-image".to_vec();
    HostVisualDeliveryReceipt {
        packet_id: packet.packet_id.clone(),
        turn_id: packet.turn_id.clone(),
        final_request_sha256: format!("sha256:{:x}", Sha256::digest(&body)),
        final_request_body: body,
        ordered_parts: packet
            .parts
            .iter()
            .enumerate()
            .map(|(index, part)| HostSentVisualPart {
                input_id: part.input_id.clone(),
                source_key: part.source_key.clone(),
                ordinal: part.ordinal,
                step_id: part.step_id.clone(),
                message_index: index,
                content_index: 0,
                wire_role: ProviderVisualWireRole::User,
                synthetic_user_role,
                parent_sha256: part.sha256.clone(),
                sent_bytes: part.bytes().to_vec(),
                transform_steps: Vec::new(),
            })
            .collect(),
    }
}

#[test]
fn midturn_camera_receipt_attributes_only_the_later_assistant_reply() {
    let profile = support::host_test_profile();
    let store = empty_store_platform(profile);
    let runtime = runtime(store.clone());
    let host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    let key = source_key(Some("desktop-a"), "camera-stream");
    let input = visual_input(
        "image-1",
        "turn-mid",
        key.clone(),
        PerceptionSourceKind::HostVisualObservation,
        PerceptionSourceRef::Sensor {
            sensor_id: "camera".into(),
        },
        Some("camera-step"),
    );
    let capture_at = input.time.observed_at_unix_ms.unwrap();
    let context = VisualCaptureContext::new(
        key,
        VisualDeviceUse::PersonalSingleUser,
        true,
        Some("turn-mid".into()),
        Some(capture_at),
        capture_at,
    )
    .unwrap();
    authorize_input(
        &runtime,
        &host,
        &input,
        PerceptionRetentionCeiling::DurableMedia,
    );
    runtime
        .submit_visual_input(
            &host,
            input,
            PerceptionRetentionCeiling::DurableMedia,
            VisualTurnPosition::AfterAssistantMessages { completed_count: 1 },
            Some(context),
        )
        .unwrap();
    let packet = runtime
        .append_visual_delivery(&host, "turn-mid", 0)
        .unwrap()
        .unwrap();
    assert!(packet.parts[0].bytes().starts_with(b"\x89PNG"));
    let other_host = runtime
        .register_perception_host("host-b", Some("phone".into()))
        .unwrap();
    assert!(runtime
        .attest_visual_delivery(&other_host, receipt(&packet, true))
        .is_err());
    runtime
        .attest_visual_delivery(&host, receipt(&packet, true))
        .unwrap();
    runtime
        .authorize_visual_inference(
            &host,
            "image-1",
            VisualInferenceAuthorization {
                recipient_id: "vision-provider".into(),
                source_key: packet.parts[0].source_key.clone(),
                media_sha256: packet
                    .parts
                    .iter()
                    .map(|part| part.sha256.clone())
                    .collect(),
                authorization_generation: 1,
                user_consent_attested: true,
            },
        )
        .unwrap();
    assert_eq!(
        runtime
            .infer_visual_for_memory(
                "image-1",
                &mut VisualInferenceFixture {
                    person_count: 1,
                    synthetic_user_role: true,
                    needs_revisit: true,
                    description: "我看到你穿着红裙子。",
                }
            )
            .unwrap(),
        VisualMemoryObservationDecision::MediaCandidate
    );
    runtime
        .record_visual_provider_response(
            &host,
            &packet.packet_id,
            &receipt(&packet, true).final_request_sha256,
            b"model-response".to_vec(),
        )
        .unwrap();
    assert!(runtime
        .record_visual_provider_response(
            &host,
            &packet.packet_id,
            &format!("sha256:{}", "0".repeat(64)),
            b"unrelated-response".to_vec(),
        )
        .is_err());
    assert!(runtime
        .record_visual_provider_response(
            &host,
            &packet.packet_id,
            &receipt(&packet, true).final_request_sha256,
            b"changed-response".to_vec()
        )
        .is_err());
    let uses = runtime
        .prepare_turn_visual_external_uses(&host, "turn-mid", None)
        .unwrap();
    assert_eq!(uses.len(), 1);
    let manifest = TranscriptExternalContentManifestV1 {
        completeness: ExternalContentCompletenessV1::HostAttestedComplete,
        request_sha256: vec![receipt(&packet, true).final_request_sha256],
        uses,
    };
    runtime
        .submit_turn_external_content_manifest(&host, "turn-mid", manifest.clone())
        .unwrap();
    let mut request = turn("turn-mid", &["我去看一眼。", "我看见了一张画面。"]);
    request.turn.external_content_used = true;
    runtime.finalize_turn(request.clone()).unwrap();
    let key = ConversationKey::from_delta(runtime.memory_space_id(), &request.turn).unwrap();
    let record = store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&key, runtime.subject_id(), &request.turn.turn_id)
        .unwrap()
        .unwrap();
    assert_eq!(record.perception_source_uses.len(), 1);
    assert_eq!(record.external_content_manifest, Some(manifest));
    assert_eq!(
        record.perception_source_uses[0].first_affected_assistant_sequence,
        3
    );
    let snapshot = store.export_replay_snapshot().unwrap();
    let episode = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "perception_episodes")
        .expect("same-transaction visual episode");
    assert_eq!(episode.value["observation"]["text"], "我看到你穿着红裙子。");
    assert!(episode.value["media"].is_object());
    assert_eq!(
        snapshot
            .blobs
            .iter()
            .filter(|blob| blob.namespace == "perception_media_evidence")
            .count(),
        1
    );
    let visible_long_term = || {
        runtime
            .list_long_term_memory(MemoryLongTermListRequest {
                query: LongTermMemoryQuery {
                    limit: 10,
                    ..LongTermMemoryQuery::default()
                },
                cursor: None,
                limit: 10,
                view: MemoryLongTermControlView::HostUi,
            })
            .unwrap()
    };
    assert_eq!(visible_long_term().total_visible, 1);
    let material: bm_core::memory::LongTermMemoryVersionMaterial = serde_json::from_value(
        store
            .export_replay_snapshot()
            .unwrap()
            .json_docs
            .into_iter()
            .find(|doc| doc.namespace == "long_term_version_materials")
            .expect("visual long-term material")
            .value,
    )
    .unwrap();
    assert!(material.governed_content.content.contains("红裙子"));
    let archive_scope =
        MemoryArchiveScope::subject(runtime.memory_space_id(), runtime.subject_id()).unwrap();
    let archive_request = MemorySpaceExportRequest {
        scope: archive_scope.clone(),
        private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
    };
    let semantic_archive = runtime
        .export_memory_space(archive_request.clone())
        .unwrap();
    assert_eq!(semantic_archive.archive.root().blob_count, 0);
    assert!(semantic_archive
        .archive
        .contains_json_namespace("perception_episodes"));
    let excluded_archive = runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: archive_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        })
        .unwrap();
    assert_eq!(excluded_archive.archive.root().blob_count, 0);
    let excluded_target = self::runtime(empty_store_platform(support::host_test_profile()));
    excluded_target
        .import_memory_space(MemorySpaceImportRequest {
            scope: archive_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
            archive: excluded_archive.archive,
        })
        .unwrap();
    assert!(excluded_target
        .list_visual_memory(&primary_human_subject_id("owner-default"))
        .unwrap()
        .is_empty());
    assert_eq!(
        excluded_target
            .list_long_term_memory(MemoryLongTermListRequest {
                query: LongTermMemoryQuery {
                    limit: 10,
                    ..LongTermMemoryQuery::default()
                },
                cursor: None,
                limit: 10,
                view: MemoryLongTermControlView::HostUi,
            })
            .unwrap()
            .total_visible,
        0
    );
    let media_archive = runtime
        .export_memory_space_with_visual_media(archive_request)
        .unwrap();
    assert_eq!(media_archive.archive.root().blob_count, 1);
    let semantic_target = self::runtime(empty_store_platform(support::host_test_profile()));
    semantic_target
        .import_memory_space(MemorySpaceImportRequest {
            scope: archive_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: semantic_archive.archive.clone(),
        })
        .unwrap();
    let requester = primary_human_subject_id("owner-default");
    let imported_semantic = semantic_target.list_visual_memory(&requester).unwrap();
    assert_eq!(imported_semantic.len(), 1);
    assert!(!imported_semantic[0].media_available);
    let media_target = self::runtime(empty_store_platform(support::host_test_profile()));
    media_target
        .import_memory_space(MemorySpaceImportRequest {
            scope: archive_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: media_archive.archive.clone(),
        })
        .unwrap();
    let imported_media = media_target.list_visual_memory(&requester).unwrap();
    assert_eq!(imported_media.len(), 1);
    assert!(imported_media[0].media_available);
    assert_eq!(
        media_target
            .read_visual_memory_media(&requester, &imported_media[0].source)
            .unwrap()
            .unwrap()
            .1,
        png()
    );
    let unprojected = turn("turn-unprojected-after-vision", &["刚才我说你穿着红裙子。"]);
    runtime.finalize_turn(unprojected.clone()).unwrap();
    let unprojected_key =
        ConversationKey::from_delta(runtime.memory_space_id(), &unprojected.turn).unwrap();
    assert!(store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(
            &unprojected_key,
            runtime.subject_id(),
            &unprojected.turn.turn_id
        )
        .unwrap()
        .is_none());
    let clean_turn_id = "turn-clean-after-vision";
    runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Turn {
                turn_id: clean_turn_id.into(),
            },
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "现在讨论新的事情。".into(),
            system_max_len: 12_000,
            recent_messages_limit: 0,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();
    runtime
        .submit_turn_external_content_manifest(
            &host,
            clean_turn_id,
            TranscriptExternalContentManifestV1 {
                completeness: ExternalContentCompletenessV1::HostAttestedComplete,
                request_sha256: vec![format!("sha256:{:x}", Sha256::digest(b"clean-request"))],
                uses: Vec::new(),
            },
        )
        .unwrap();
    let clean_turn = turn(clean_turn_id, &["我们开始聊新的事情。"]);
    runtime.finalize_turn(clean_turn.clone()).unwrap();
    let clean_key =
        ConversationKey::from_delta(runtime.memory_space_id(), &clean_turn.turn).unwrap();
    let clean_record = store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&clean_key, runtime.subject_id(), clean_turn_id)
        .unwrap()
        .expect("complete visual-free inventory restores ordinary turn persistence");
    assert!(clean_record.perception_source_uses.is_empty());
    let replay_turn_id = "turn-replayed-vision";
    let replay_projection = runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Turn {
                turn_id: replay_turn_id.into(),
            },
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "你刚才看见什么？".into(),
            system_max_len: 12_000,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();
    assert!(replay_projection
        .provider_payload()
        .visual_delivery()
        .is_none());
    assert!(replay_projection
        .provider_payload()
        .recent_messages()
        .iter()
        .any(|message| message.content.contains("画面")));
    let replayed = turn(replay_turn_id, &["刚才我看见你穿着红裙子。"]);
    runtime.finalize_turn(replayed.clone()).unwrap();
    let replay_key =
        ConversationKey::from_delta(runtime.memory_space_id(), &replayed.turn).unwrap();
    let replay_record = store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&replay_key, runtime.subject_id(), replay_turn_id)
        .unwrap()
        .unwrap();
    assert_eq!(replay_record.perception_source_uses.len(), 1);
    assert_eq!(
        replay_record.perception_source_uses[0].source,
        record.perception_source_uses[0].source
    );
    let cross_runtime = runtime_in_conversation(store.clone(), "chat-b");
    let cross_turn_id = "turn-cross-conversation-vision";
    let cross_projection = cross_runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Turn {
                turn_id: cross_turn_id.into(),
            },
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "你以前看见的红裙子呢？".into(),
            system_max_len: 12_000,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();
    assert!(
        cross_projection
            .provider_payload()
            .system_memory_block()
            .contains("我曾在画面里看到当前用户"),
        "exact visual fact was not rendered: {}",
        cross_projection.provider_payload().system_memory_block()
    );
    let mut cross_turn = turn(cross_turn_id, &["我还记得那条红裙子。"]);
    cross_turn.turn.conversation.conversation_id = Some("chat-b".into());
    cross_runtime.finalize_turn(cross_turn.clone()).unwrap();
    let cross_key =
        ConversationKey::from_delta(cross_runtime.memory_space_id(), &cross_turn.turn).unwrap();
    let cross_record = store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&cross_key, cross_runtime.subject_id(), cross_turn_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        cross_record.perception_source_uses.len(),
        1,
        "cross-conversation projection delivered a visual fact without source use: {}",
        cross_projection.provider_payload().system_memory_block()
    );
    assert_eq!(
        cross_record.perception_source_uses[0].source,
        record.perception_source_uses[0].source
    );
    let other_channel = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").unwrap())
        .subject_id("subject-default")
        .scope(
            MemoryScope::new("llm.other", "chat-a")
                .unwrap()
                .with_conversation_id("chat-c")
                .unwrap(),
        )
        .store(store.clone())
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .unwrap();
    let other_turn_id = "turn-cross-channel-vision";
    let other_projection = other_channel
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Turn {
                turn_id: other_turn_id.into(),
            },
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "你以前看见的红裙子呢？".into(),
            system_max_len: 12_000,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();
    assert!(other_projection
        .provider_payload()
        .system_memory_block()
        .contains("我曾在画面里看到当前用户"));
    let mut other_turn = turn(other_turn_id, &["我还记得那条红裙子。"]);
    other_turn.turn.conversation.channel = "llm.other".into();
    other_turn.turn.conversation.conversation_id = Some("chat-c".into());
    other_turn.turn.source.channel = "llm.other".into();
    other_channel.finalize_turn(other_turn.clone()).unwrap();
    let other_key =
        ConversationKey::from_delta(other_channel.memory_space_id(), &other_turn.turn).unwrap();
    let other_record = store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&other_key, other_channel.subject_id(), other_turn_id)
        .unwrap()
        .unwrap();
    assert_eq!(other_record.perception_source_uses.len(), 1);
    assert_eq!(
        other_record.perception_source_uses[0].source,
        record.perception_source_uses[0].source
    );
    let memories = runtime.list_visual_memory(&requester).unwrap();
    assert_eq!(memories.len(), 1);
    assert_eq!(
        memories[0].observation.as_deref(),
        Some("我看到你穿着红裙子。")
    );
    let media = runtime
        .read_visual_memory_media(&requester, &memories[0].source)
        .unwrap()
        .expect("governed visual media");
    assert_eq!(media.0, "image/png");
    assert_eq!(media.1, png());
    let deleted = runtime
        .forget_visual_memory(&requester, memories[0].source.clone())
        .unwrap();
    assert!(deleted.deleted);
    assert_eq!(runtime.list_visual_memory(&requester).unwrap().len(), 0);
    let replay_after_delete = store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&replay_key, runtime.subject_id(), replay_turn_id)
        .unwrap();
    let replay_after_delete = replay_after_delete.expect("redacted Transcript tombstone");
    assert_eq!(
        replay_after_delete.redaction_state,
        bm_sdk::TranscriptRedactionState::RawDeleted
    );
    assert!(replay_after_delete.perception_source_uses.is_empty());
    assert!(replay_after_delete
        .input_messages
        .iter()
        .all(|message| message.content.is_empty()));
    assert!(replay_after_delete
        .assistant_messages
        .iter()
        .all(|message| message.message.content.is_empty()));
    let cross_after_delete = store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&cross_key, cross_runtime.subject_id(), cross_turn_id)
        .unwrap()
        .expect("cross-conversation redacted Transcript tombstone");
    assert_eq!(
        cross_after_delete.redaction_state,
        bm_sdk::TranscriptRedactionState::RawDeleted
    );
    assert!(cross_after_delete
        .assistant_messages
        .iter()
        .all(|message| message.message.content.is_empty()));
    let other_after_delete = store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&other_key, other_channel.subject_id(), other_turn_id)
        .unwrap()
        .expect("cross-channel redacted Transcript tombstone");
    assert_eq!(
        other_after_delete.redaction_state,
        bm_sdk::TranscriptRedactionState::RawDeleted
    );
    assert!(other_after_delete
        .assistant_messages
        .iter()
        .all(|message| message.message.content.is_empty()));
    assert!(runtime
        .read_visual_memory_media(&requester, &memories[0].source)
        .is_err());
    assert_eq!(visible_long_term().total_visible, 0);
    assert!(runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope: archive_scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: media_archive.archive,
        })
        .is_err());
}

#[test]
fn deleting_one_of_two_visual_sources_redacts_the_turn_but_preserves_the_other_memory() {
    let store = empty_store_platform(support::host_test_profile());
    let runtime = runtime(store.clone());
    let host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    let turn_id = "turn-two-visual-sources";
    let mut since_revision = 0;
    for (ordinal, input_id, source_id, description) in [
        (0, "visual-a", "camera-a", "我看到你穿着红裙子。"),
        (1, "visual-b", "camera-b", "我看到你戴着蓝帽子。"),
    ] {
        let position = if ordinal == 0 {
            VisualTurnPosition::BeforeFirstAssistant
        } else {
            VisualTurnPosition::AfterAssistantMessages { completed_count: 1 }
        };
        let source = source_key(Some("desktop-a"), source_id);
        let input = visual_input(
            input_id,
            turn_id,
            source.clone(),
            PerceptionSourceKind::HostVisualObservation,
            PerceptionSourceRef::Sensor {
                sensor_id: "camera".into(),
            },
            (ordinal == 1).then_some("step-b"),
        );
        let captured_at = input.time.observed_at_unix_ms.unwrap();
        let capture_context = VisualCaptureContext::new(
            source,
            VisualDeviceUse::PersonalSingleUser,
            true,
            Some(turn_id.into()),
            Some(captured_at),
            captured_at,
        )
        .unwrap();
        authorize_input(
            &runtime,
            &host,
            &input,
            PerceptionRetentionCeiling::DurableMedia,
        );
        runtime
            .submit_visual_input(
                &host,
                input,
                PerceptionRetentionCeiling::DurableMedia,
                position,
                Some(capture_context),
            )
            .unwrap();
        let packet = runtime
            .append_visual_delivery(&host, turn_id, since_revision)
            .unwrap()
            .unwrap();
        assert_eq!(packet.parts.len(), 1);
        since_revision = packet.revision;
        runtime
            .attest_visual_delivery(&host, receipt(&packet, true))
            .unwrap();
        runtime
            .record_visual_provider_response(
                &host,
                &packet.packet_id,
                &receipt(&packet, true).final_request_sha256,
                format!("model-response-{input_id}").into_bytes(),
            )
            .unwrap();
        runtime
            .authorize_visual_inference(
                &host,
                input_id,
                VisualInferenceAuthorization {
                    recipient_id: "vision-provider".into(),
                    source_key: packet.parts[0].source_key.clone(),
                    media_sha256: packet
                        .parts
                        .iter()
                        .map(|part| part.sha256.clone())
                        .collect(),
                    authorization_generation: 1,
                    user_consent_attested: true,
                },
            )
            .unwrap();
        assert_eq!(
            runtime
                .infer_visual_for_memory(
                    input_id,
                    &mut VisualInferenceFixture {
                        person_count: 1,
                        synthetic_user_role: true,
                        needs_revisit: true,
                        description,
                    }
                )
                .unwrap(),
            VisualMemoryObservationDecision::MediaCandidate
        );
    }
    runtime
        .finalize_turn(turn(turn_id, &["我看到了红裙子。", "我还看到了蓝帽子。"]))
        .unwrap();
    let requester = primary_human_subject_id("owner-default");
    let before = runtime.list_visual_memory(&requester).unwrap();
    assert_eq!(before.len(), 2);
    let source_a = before
        .iter()
        .find(|entry| entry.observation.as_deref() == Some("我看到你穿着红裙子。"))
        .unwrap()
        .source
        .clone();
    let source_b = before
        .iter()
        .find(|entry| entry.observation.as_deref() == Some("我看到你戴着蓝帽子。"))
        .unwrap()
        .source
        .clone();
    let long_term_entries = || {
        runtime
            .list_long_term_memory(MemoryLongTermListRequest {
                query: LongTermMemoryQuery {
                    limit: 10,
                    ..LongTermMemoryQuery::default()
                },
                cursor: None,
                limit: 10,
                view: MemoryLongTermControlView::HostUi,
            })
            .unwrap()
            .records
    };
    let before_long_term = long_term_entries();
    assert!(
        before_long_term
            .iter()
            .any(|entry| entry.record.perception_sources.contains(&source_b)),
        "second visual long-term owner was not formed: {before_long_term:?}"
    );
    assert!(
        runtime
            .forget_visual_memory(&requester, source_a)
            .unwrap()
            .deleted
    );
    let remaining = runtime.list_visual_memory(&requester).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].source, source_b);
    assert_eq!(
        runtime
            .read_visual_memory_media(&requester, &source_b)
            .unwrap()
            .unwrap()
            .1,
        png()
    );
    let turn_request = turn(turn_id, &["我看到了红裙子。", "我还看到了蓝帽子。"]);
    let key = ConversationKey::from_delta(runtime.memory_space_id(), &turn_request.turn).unwrap();
    let redacted = store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&key, runtime.subject_id(), turn_id)
        .unwrap()
        .expect("redacted two-source Transcript tombstone");
    assert_eq!(
        redacted.redaction_state,
        bm_sdk::TranscriptRedactionState::RawDeleted
    );
    assert!(redacted
        .assistant_messages
        .iter()
        .all(|entry| entry.message.content.is_empty()));
    let after_long_term = long_term_entries();
    assert!(
        after_long_term
            .iter()
            .any(|entry| entry.record.perception_sources.contains(&source_b)),
        "second visual long-term owner was withdrawn with first source: {after_long_term:?}"
    );
    assert!(
        runtime
            .forbid_visual_source_memory(&requester, source_b.clone())
            .unwrap()
            .deleted
    );
    drop(runtime);
    let reopened = self::runtime(store);
    assert!(reopened.list_visual_memory(&requester).unwrap().is_empty());
    assert!(reopened
        .read_visual_memory_media(&requester, &source_b)
        .is_err());
    assert_eq!(
        reopened
            .list_long_term_memory(MemoryLongTermListRequest {
                query: LongTermMemoryQuery {
                    limit: 10,
                    ..LongTermMemoryQuery::default()
                },
                cursor: None,
                limit: 10,
                view: MemoryLongTermControlView::HostUi,
            })
            .unwrap()
            .total_visible,
        0
    );
    let reopened_host = reopened
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    reopened
        .update_perception_capability(
            &reopened_host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 2,
                authorization_generation: 2,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    assert!(reopened
        .authorize_visual_source(
            &reopened_host,
            VisualSourceAuthorization {
                source_key: PerceptionSourceKey {
                    source_generation: 2,
                    ..source_key(Some("desktop-a"), "camera-b")
                },
                source_kind: PerceptionSourceKind::HostVisualObservation,
                revoke_epoch: 1,
                authorization_generation: 2,
                retention_ceiling: PerceptionRetentionCeiling::DurableMedia,
                user_consent_attested: true,
                user_attachment: None,
                audio_free_video_sha256: None,
            }
        )
        .is_err());
}

#[test]
fn disabled_camera_does_not_block_user_shared_photo_or_upgrade_its_retention() {
    let profile = support::host_test_profile();
    let store = empty_store_platform(profile);
    let runtime = runtime(store.clone());
    let camera_host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &camera_host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::PresentUnavailable,
                unavailable_reason: Some(PerceptionUnavailableReason::UserDisabled),
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    let shared_host = runtime.register_perception_host("host-a", None).unwrap();
    runtime
        .submit_visual_input(
            &shared_host,
            authorized_input(
                &runtime,
                &shared_host,
                visual_input(
                    "shared-1",
                    "turn-shared",
                    source_key(None, "attachment-1"),
                    PerceptionSourceKind::HumanUserSharedMedia,
                    PerceptionSourceRef::UserAttachment {
                        message_id: "message-1".into(),
                        attachment_id: "attachment-1".into(),
                    },
                    None,
                ),
                PerceptionRetentionCeiling::CurrentTurn,
            ),
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
    let packet = runtime
        .append_visual_delivery(&shared_host, "turn-shared", 0)
        .unwrap()
        .unwrap();
    assert!(runtime
        .attest_visual_delivery(&shared_host, receipt(&packet, true))
        .is_err());
    runtime
        .attest_visual_delivery(&shared_host, receipt(&packet, false))
        .unwrap();
    let request = turn("turn-shared", &["我看到你分享的照片了。"]);
    runtime.finalize_turn(request.clone()).unwrap();
    let key = ConversationKey::from_delta(runtime.memory_space_id(), &request.turn).unwrap();
    assert!(store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&key, runtime.subject_id(), &request.turn.turn_id)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        0
    );
}

#[test]
fn shared_photo_of_a_cat_can_form_sourced_semantic_memory_with_camera_disabled() {
    let profile = support::host_test_profile();
    let store = empty_store_platform(profile);
    let runtime = runtime(store.clone());
    let canonical_turn = turn("turn-cat", &["我看到一只猫。"]);
    let canonical_key =
        ConversationKey::from_delta(runtime.memory_space_id(), &canonical_turn.turn).unwrap();
    let user_message_id = canonical_turn
        .turn
        .planned_input_message_id(&canonical_key, 0)
        .unwrap();
    let camera_host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &camera_host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::PresentUnavailable,
                unavailable_reason: Some(PerceptionUnavailableReason::UserDisabled),
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    let shared_host = runtime.register_perception_host("host-a", None).unwrap();
    runtime
        .submit_visual_input(
            &shared_host,
            authorized_input(
                &runtime,
                &shared_host,
                visual_input(
                    "cat-photo",
                    "turn-cat",
                    source_key(None, "cat-attachment"),
                    PerceptionSourceKind::HumanUserSharedMedia,
                    PerceptionSourceRef::UserAttachment {
                        message_id: user_message_id,
                        attachment_id: "cat-attachment".into(),
                    },
                    None,
                ),
                PerceptionRetentionCeiling::DurableSemantic,
            ),
            PerceptionRetentionCeiling::DurableSemantic,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
    let packet = runtime
        .append_visual_delivery(&shared_host, "turn-cat", 0)
        .unwrap()
        .unwrap();
    runtime
        .attest_visual_delivery(&shared_host, receipt(&packet, false))
        .unwrap();
    runtime
        .record_visual_provider_response(
            &shared_host,
            &packet.packet_id,
            &receipt(&packet, false).final_request_sha256,
            b"cat-photo-model-response".to_vec(),
        )
        .unwrap();
    runtime
        .authorize_visual_inference(
            &shared_host,
            "cat-photo",
            VisualInferenceAuthorization {
                recipient_id: "vision-provider".into(),
                source_key: packet.parts[0].source_key.clone(),
                media_sha256: packet
                    .parts
                    .iter()
                    .map(|part| part.sha256.clone())
                    .collect(),
                authorization_generation: 1,
                user_consent_attested: true,
            },
        )
        .unwrap();
    assert_eq!(
        runtime
            .infer_visual_for_memory(
                "cat-photo",
                &mut VisualInferenceFixture {
                    person_count: 0,
                    synthetic_user_role: false,
                    needs_revisit: true,
                    description: "我看到一只猫。",
                }
            )
            .unwrap(),
        VisualMemoryObservationDecision::SemanticCandidate
    );
    runtime.finalize_turn(canonical_turn).unwrap();
    let episode = store
        .export_replay_snapshot()
        .unwrap()
        .json_docs
        .into_iter()
        .find(|doc| doc.namespace == "perception_episodes")
        .unwrap();
    assert_eq!(episode.value["observation"]["text"], "我看到一只猫。");
    assert!(episode.value["media"].is_null());
}

#[test]
fn independent_beetle_sight_survives_without_a_host_reply_receipt_but_reply_is_masked() {
    let profile = support::host_test_profile();
    let store = empty_store_platform(profile);
    let runtime = runtime(store.clone());
    let host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    let source = source_key(Some("desktop-a"), "independent-camera");
    let input = visual_input(
        "independent-1",
        "turn-independent",
        source.clone(),
        PerceptionSourceKind::HostVisualObservation,
        PerceptionSourceRef::Sensor {
            sensor_id: "camera".into(),
        },
        None,
    );
    let capture_at = input.time.observed_at_unix_ms.unwrap();
    let context = VisualCaptureContext::new(
        source.clone(),
        VisualDeviceUse::PersonalSingleUser,
        true,
        Some("turn-independent".into()),
        Some(capture_at),
        capture_at,
    )
    .unwrap();
    authorize_input(
        &runtime,
        &host,
        &input,
        PerceptionRetentionCeiling::DurableMedia,
    );
    runtime
        .submit_visual_input(
            &host,
            input,
            PerceptionRetentionCeiling::DurableMedia,
            VisualTurnPosition::BeforeFirstAssistant,
            Some(context),
        )
        .unwrap();
    runtime
        .authorize_visual_inference(
            &host,
            "independent-1",
            VisualInferenceAuthorization {
                recipient_id: "vision-provider".into(),
                source_key: source,
                media_sha256: vec![format!("sha256:{:x}", Sha256::digest(png()))],
                authorization_generation: 1,
                user_consent_attested: true,
            },
        )
        .unwrap();
    assert_eq!(
        runtime
            .infer_visual_for_memory(
                "independent-1",
                &mut VisualInferenceFixture {
                    person_count: 1,
                    synthetic_user_role: true,
                    needs_revisit: true,
                    description: "我看到你穿着红裙子。",
                }
            )
            .unwrap(),
        VisualMemoryObservationDecision::MediaCandidate
    );
    let suppressed_reply = turn("turn-independent", &["我看见了你。"]);
    runtime.finalize_turn(suppressed_reply.clone()).unwrap();
    let late_packet = runtime
        .append_visual_delivery(&host, "turn-independent", 0)
        .unwrap()
        .unwrap();
    runtime
        .attest_visual_delivery(&host, receipt(&late_packet, true))
        .unwrap();
    runtime.finalize_turn(suppressed_reply).unwrap();
    let requester = primary_human_subject_id("owner-default");
    let entries = runtime.list_visual_memory(&requester).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].media_available);
    assert!(store
        .export_replay_snapshot()
        .unwrap()
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "conversation_transcript"));
    assert!(
        runtime
            .forget_visual_memory(&requester, entries[0].source.clone())
            .unwrap()
            .deleted
    );
    assert!(runtime.list_visual_memory(&requester).unwrap().is_empty());
}

#[test]
fn ephemeral_visual_reply_survives_next_projection_only_in_ram_and_revocation_clears_it() {
    let profile = support::host_test_profile();
    let store = empty_store_platform(profile);
    let runtime = runtime(store.clone());
    let host = runtime.register_perception_host("host-a", None).unwrap();
    runtime
        .submit_visual_input(
            &host,
            authorized_input(
                &runtime,
                &host,
                visual_input(
                    "shared-ephemeral",
                    "turn-ephemeral",
                    source_key(None, "attachment-ephemeral"),
                    PerceptionSourceKind::HumanUserSharedMedia,
                    PerceptionSourceRef::UserAttachment {
                        message_id: "message-ephemeral".into(),
                        attachment_id: "attachment-ephemeral".into(),
                    },
                    None,
                ),
                PerceptionRetentionCeiling::EphemeralSession,
            ),
            PerceptionRetentionCeiling::EphemeralSession,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
    let packet = runtime
        .append_visual_delivery(&host, "turn-ephemeral", 0)
        .unwrap()
        .unwrap();
    runtime
        .attest_visual_delivery(&host, receipt(&packet, false))
        .unwrap();
    let finalized = turn("turn-ephemeral", &["你刚才分享了一张照片。"]);
    runtime.finalize_turn(finalized.clone()).unwrap();
    let key = ConversationKey::from_delta(runtime.memory_space_id(), &finalized.turn).unwrap();
    assert!(store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&key, runtime.subject_id(), "turn-ephemeral")
        .unwrap()
        .is_none());
    let project = || {
        runtime
            .project(MemoryProjectionRequest {
                binding: ProceduralProjectionBindingV1::Preview,
                temporal_operation: MemoryRecallTemporalOperation::Current,
                user_query: "你刚才说什么？".into(),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
            })
            .unwrap()
    };
    assert!(project()
        .provider_payload()
        .recent_messages()
        .iter()
        .any(|message| message.content == "你刚才分享了一张照片。"));
    runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Turn {
                turn_id: "turn-ephemeral-followup".into(),
            },
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "你后来还说了什么？".into(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();
    runtime
        .finalize_turn(turn(
            "turn-ephemeral-followup",
            &["我后来又提到了那张照片。"],
        ))
        .unwrap();
    assert!(project()
        .provider_payload()
        .recent_messages()
        .iter()
        .any(|message| message.content == "我后来又提到了那张照片。"));
    runtime
        .revoke_visual_source(&host, "attachment-ephemeral")
        .unwrap();
    assert!(project()
        .provider_payload()
        .recent_messages()
        .iter()
        .all(|message| message.content != "你刚才分享了一张照片。"
            && message.content != "我后来又提到了那张照片。"));
}

#[test]
fn preturn_automatic_capture_binds_to_the_next_turn_without_persistent_reply() {
    let profile = support::host_test_profile();
    let store = empty_store_platform(profile);
    let runtime = runtime(store.clone());
    let host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    let mut input = visual_input(
        "auto-1",
        "turn-auto",
        source_key(Some("desktop-a"), "auto-camera"),
        PerceptionSourceKind::HostVisualObservation,
        PerceptionSourceRef::Sensor {
            sensor_id: "camera".into(),
        },
        None,
    );
    input.scope.turn_id = None;
    runtime
        .submit_visual_input(
            &host,
            input,
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
    let packet = runtime
        .append_visual_delivery(&host, "turn-auto", 0)
        .unwrap()
        .unwrap();
    runtime
        .attest_visual_delivery(&host, receipt(&packet, true))
        .unwrap();
    let request = turn("turn-auto", &["我看了一眼。"]);
    runtime.finalize_turn(request.clone()).unwrap();
    let key = ConversationKey::from_delta(runtime.memory_space_id(), &request.turn).unwrap();
    assert!(store
        .replay_harness()
        .conversation_transcript_store()
        .get_turn(&key, runtime.subject_id(), "turn-auto")
        .unwrap()
        .is_none());
}

#[test]
fn revocation_invalidates_an_in_flight_packet_and_blocks_durable_finalization() {
    let profile = support::host_test_profile();
    let store = empty_store_platform(profile);
    let runtime = runtime(store.clone());
    let host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    runtime
        .submit_visual_input(
            &host,
            authorized_input(
                &runtime,
                &host,
                visual_input(
                    "revoked-camera",
                    "turn-revoked",
                    source_key(Some("desktop-a"), "revoked-source"),
                    PerceptionSourceKind::HostVisualObservation,
                    PerceptionSourceRef::Sensor {
                        sensor_id: "camera".into(),
                    },
                    None,
                ),
                PerceptionRetentionCeiling::DurableSemantic,
            ),
            PerceptionRetentionCeiling::DurableSemantic,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
    let packet = runtime
        .append_visual_delivery(&host, "turn-revoked", 0)
        .unwrap()
        .unwrap();
    runtime
        .revoke_visual_source(&host, "revoked-source")
        .unwrap();
    assert!(runtime
        .attest_visual_delivery(&host, receipt(&packet, true))
        .is_err());
    runtime
        .finalize_turn(turn("turn-revoked", &["我看见了。"]))
        .unwrap();
    assert!(store
        .export_replay_snapshot()
        .unwrap()
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "perception_episodes"));
    assert_eq!(
        store
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        0
    );
    assert!(store
        .export_replay_snapshot()
        .unwrap()
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "conversation_transcript"));
}

#[test]
fn capability_projection_keeps_remote_camera_installed_but_not_callable_here() {
    let profile = support::host_test_profile();
    let runtime = runtime(empty_store_platform(profile));
    let desktop = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    let phone = runtime
        .register_perception_host("host-a", Some("phone-a".into()))
        .unwrap();
    for (handle, device_id) in [(&desktop, "desktop-a"), (&phone, "phone-a")] {
        runtime
            .update_perception_capability(
                handle,
                PerceptionCapabilitySnapshot {
                    host_id: "host-a".into(),
                    device_id: device_id.into(),
                    sensor_id: "camera".into(),
                    tool_id: format!("camera-tool-{device_id}"),
                    tool_registry_revision: 1,
                    authorization_generation: 1,
                    state: PerceptionCapabilityState::Available,
                    unavailable_reason: None,
                    visual_forms: vec![VisualCaptureForm::Image],
                },
            )
            .unwrap();
    }
    let request = || MemoryProjectionRequest {
        binding: ProceduralProjectionBindingV1::Preview,
        temporal_operation: MemoryRecallTemporalOperation::Current,
        user_query: "现在能看吗？".into(),
        system_max_len: 12_000,
        recent_messages_limit: 8,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
        structured_query_facets: Vec::new(),
        tool_registry_refs: Vec::new(),
    };
    let desktop_projection = runtime
        .project_for_perception_host(request(), &desktop)
        .unwrap();
    let text = desktop_projection.provider_payload().system_memory_block();
    assert!(text.contains("I can request a visual look through device host-a/desktop-a"));
    assert!(
        text.contains("visual ability on device host-a/phone-a, but it is currently unavailable")
    );
    assert!(!text.contains("I can request a visual look through device host-a/phone-a"));
    let anonymous = runtime.project(request()).unwrap();
    assert!(!anonymous
        .provider_payload()
        .system_memory_block()
        .contains("I can request a visual look"));
}

#[test]
fn global_visual_revoke_requires_fresh_source_generation_before_reenable() {
    let profile = support::host_test_profile();
    let runtime = runtime(empty_store_platform(profile));
    let host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    let capability = |revision, generation, state, reason| PerceptionCapabilitySnapshot {
        host_id: "host-a".into(),
        device_id: "desktop-a".into(),
        sensor_id: "camera".into(),
        tool_id: "camera-tool".into(),
        tool_registry_revision: revision,
        authorization_generation: generation,
        state,
        unavailable_reason: reason,
        visual_forms: vec![VisualCaptureForm::Image],
    };
    runtime
        .update_perception_capability(
            &host,
            capability(1, 1, PerceptionCapabilityState::Available, None),
        )
        .unwrap();
    let old_source = source_key(Some("desktop-a"), "global-source");
    runtime
        .submit_visual_input(
            &host,
            visual_input(
                "global-old",
                "turn-global-old",
                old_source.clone(),
                PerceptionSourceKind::HostVisualObservation,
                PerceptionSourceRef::Sensor {
                    sensor_id: "camera".into(),
                },
                None,
            ),
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
    let old_packet = runtime
        .append_visual_delivery(&host, "turn-global-old", 0)
        .unwrap()
        .unwrap();
    let revoked = runtime
        .revoke_all_current_visual_sources(vec![capability(
            2,
            2,
            PerceptionCapabilityState::PresentUnavailable,
            Some(PerceptionUnavailableReason::UserDisabled),
        )])
        .unwrap();
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].revoke_epoch, 1);
    assert!(runtime
        .attest_visual_delivery(&host, receipt(&old_packet, true))
        .is_err());
    runtime
        .update_perception_capability(
            &host,
            capability(3, 3, PerceptionCapabilityState::Available, None),
        )
        .unwrap();
    assert!(runtime
        .submit_visual_input(
            &host,
            visual_input(
                "global-stale",
                "turn-global-stale",
                old_source.clone(),
                PerceptionSourceKind::HostVisualObservation,
                PerceptionSourceRef::Sensor {
                    sensor_id: "camera".into()
                },
                None
            ),
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .is_err());
    let mut fresh = visual_input(
        "global-new",
        "turn-global-new",
        PerceptionSourceKey {
            source_generation: 2,
            ..old_source
        },
        PerceptionSourceKind::HostVisualObservation,
        PerceptionSourceRef::Sensor {
            sensor_id: "camera".into(),
        },
        None,
    );
    fresh.revoke_epoch = revoked[0].revoke_epoch;
    runtime
        .submit_visual_input(
            &host,
            fresh,
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
}

#[test]
fn durable_visual_requires_exact_host_consent_and_user_attachment_binding() {
    let store = empty_store_platform(support::host_test_profile());
    let runtime = runtime(store);
    let camera = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &camera,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    let camera_input = visual_input(
        "forged-durable",
        "turn-forged-durable",
        source_key(Some("desktop-a"), "camera-forged"),
        PerceptionSourceKind::HostVisualObservation,
        PerceptionSourceRef::Sensor {
            sensor_id: "camera".into(),
        },
        None,
    );
    assert!(runtime
        .submit_visual_input(
            &camera,
            camera_input,
            PerceptionRetentionCeiling::DurableMedia,
            VisualTurnPosition::BeforeFirstAssistant,
            None
        )
        .is_err());

    let shared = runtime.register_perception_host("host-a", None).unwrap();
    let user_input = visual_input(
        "forged-user",
        "turn-forged-user",
        source_key(None, "attachment-forged"),
        PerceptionSourceKind::HumanUserSharedMedia,
        PerceptionSourceRef::UserAttachment {
            message_id: "message-forged".into(),
            attachment_id: "attachment-forged".into(),
        },
        None,
    );
    assert!(runtime
        .submit_visual_input(
            &shared,
            user_input.clone(),
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None
        )
        .is_err());
    authorize_input(
        &runtime,
        &shared,
        &user_input,
        PerceptionRetentionCeiling::CurrentTurn,
    );
    runtime
        .submit_visual_input(
            &shared,
            user_input,
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
    let packet = runtime
        .append_visual_delivery(&shared, "turn-forged-user", 0)
        .unwrap()
        .unwrap();
    let mut wrong_message = receipt(&packet, false);
    wrong_message.ordered_parts[0].message_index = 1;
    assert!(runtime
        .attest_visual_delivery(&shared, wrong_message)
        .is_err());
    runtime
        .attest_visual_delivery(&shared, receipt(&packet, false))
        .unwrap();
    let mut forged_turn = turn("turn-forged-user", &["我看了。"]);
    forged_turn.turn.input_messages[0] = TranscriptInputMessage::user("不是登记的用户消息");
    assert!(runtime.finalize_turn(forged_turn).is_err());
}

#[test]
fn media_packet_is_host_bound_and_video_requires_exact_audio_free_attestation() {
    let store = empty_store_platform(support::host_test_profile());
    let runtime = runtime(store);
    let host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    let other = runtime
        .register_perception_host("host-b", Some("phone-b".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::Video, VisualCaptureForm::FrameStream],
            },
        )
        .unwrap();
    let mp4 = b"\x00\x00\x00\x14ftypisom\x00\x00\x02\x00isom".to_vec();
    let mut video = visual_input(
        "video-1",
        "turn-video",
        source_key(Some("desktop-a"), "video-camera"),
        PerceptionSourceKind::HostVisualObservation,
        PerceptionSourceRef::Sensor {
            sensor_id: "camera".into(),
        },
        None,
    );
    video.payload = VisualInput::Video(VisualVideo {
        media: VisualMediaUnit::new("video/mp4", mp4).unwrap(),
        duration_ms: 1000,
        timebase_num: 1,
        timebase_den: 1000,
        audio_track_present: false,
        derivation: None,
    });
    assert!(runtime
        .submit_visual_input(
            &host,
            video.clone(),
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None
        )
        .is_err());
    authorize_input(
        &runtime,
        &host,
        &video,
        PerceptionRetentionCeiling::CurrentTurn,
    );
    let mut flagged_audio = video.clone();
    if let VisualInput::Video(payload) = &mut flagged_audio.payload {
        payload.audio_track_present = true;
    }
    assert!(runtime
        .submit_visual_input(
            &host,
            flagged_audio,
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None
        )
        .is_err());
    runtime
        .submit_visual_input(
            &host,
            video,
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
    assert!(runtime
        .append_visual_delivery(&other, "turn-video", 0)
        .is_err());
    let packet = runtime
        .append_visual_delivery(&host, "turn-video", 0)
        .unwrap()
        .unwrap();
    assert_eq!(packet.parts.len(), 1);
    assert_eq!(packet.parts[0].mime, "video/mp4");
    assert_eq!(
        packet.parts[0].bytes(),
        b"\x00\x00\x00\x14ftypisom\x00\x00\x02\x00isom"
    );
}

#[test]
fn frame_stream_retains_ordered_real_frame_bytes() {
    let store = empty_store_platform(support::host_test_profile());
    let runtime = runtime(store);
    let host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::FrameStream],
            },
        )
        .unwrap();
    let mut stream = visual_input(
        "frames-1",
        "turn-frames",
        source_key(Some("desktop-a"), "frame-camera"),
        PerceptionSourceKind::HostVisualObservation,
        PerceptionSourceRef::Sensor {
            sensor_id: "camera".into(),
        },
        None,
    );
    let now = stream.time.observed_at_unix_ms.unwrap();
    let image = VisualImage {
        media: VisualMediaUnit::new("image/png", png()).unwrap(),
        width: 1,
        height: 1,
        derivation: None,
    };
    stream.payload = VisualInput::FrameStream(VisualFrameStream {
        stream_id: "stream-a".into(),
        stopped: false,
        frames: vec![
            VisualFrame {
                sequence: 1,
                observed_at_unix_ms: now,
                image: image.clone(),
            },
            VisualFrame {
                sequence: 2,
                observed_at_unix_ms: now + 10,
                image,
            },
        ],
    });
    runtime
        .submit_visual_input(
            &host,
            stream,
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
    let packet = runtime
        .append_visual_delivery(&host, "turn-frames", 0)
        .unwrap()
        .unwrap();
    assert_eq!(packet.parts.len(), 2);
    assert_eq!(
        packet
            .parts
            .iter()
            .map(|part| part.frame_sequence)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2)]
    );
    assert!(packet.parts.iter().all(|part| part.bytes() == png()));
}

#[test]
fn durable_source_fence_survives_runtime_reopen() {
    let store = empty_store_platform(support::host_test_profile());
    let first = runtime(store.clone());
    let host = first
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    let capability = PerceptionCapabilitySnapshot {
        host_id: "host-a".into(),
        device_id: "desktop-a".into(),
        sensor_id: "camera".into(),
        tool_id: "camera-tool".into(),
        tool_registry_revision: 1,
        authorization_generation: 1,
        state: PerceptionCapabilityState::Available,
        unavailable_reason: None,
        visual_forms: vec![VisualCaptureForm::Image],
    };
    first
        .update_perception_capability(&host, capability.clone())
        .unwrap();
    let source = source_key(Some("desktop-a"), "reopen-source");
    first
        .submit_visual_input(
            &host,
            visual_input(
                "reopen-old",
                "turn-reopen-old",
                source.clone(),
                PerceptionSourceKind::HostVisualObservation,
                PerceptionSourceRef::Sensor {
                    sensor_id: "camera".into(),
                },
                None,
            ),
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
    assert_eq!(
        first.revoke_visual_source(&host, "reopen-source").unwrap(),
        1
    );
    drop(first);
    let reopened = runtime(store);
    let host = reopened
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    reopened
        .update_perception_capability(&host, capability)
        .unwrap();
    assert!(reopened
        .submit_visual_input(
            &host,
            visual_input(
                "reopen-stale",
                "turn-reopen-stale",
                source.clone(),
                PerceptionSourceKind::HostVisualObservation,
                PerceptionSourceRef::Sensor {
                    sensor_id: "camera".into()
                },
                None
            ),
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .is_err());
    let mut fresh = visual_input(
        "reopen-fresh",
        "turn-reopen-fresh",
        PerceptionSourceKey {
            source_generation: 2,
            ..source
        },
        PerceptionSourceKind::HostVisualObservation,
        PerceptionSourceRef::Sensor {
            sensor_id: "camera".into(),
        },
        None,
    );
    fresh.revoke_epoch = 1;
    reopened
        .submit_visual_input(
            &host,
            fresh,
            PerceptionRetentionCeiling::CurrentTurn,
            VisualTurnPosition::BeforeFirstAssistant,
            None,
        )
        .unwrap();
}

#[test]
fn two_source_visual_persona_evidence_is_withdrawn_with_its_episode() {
    let store = empty_store_platform(support::host_test_profile());
    let runtime = Arc::new(runtime(store.clone()));
    runtime
        .install_governance_binding(MemoryGovernanceBindingInstallRequest {
            source_owner_id: "visual-persona-test".into(),
            source_config_id: "vision-governance".into(),
            source_revision: 1,
            protocol: PostTurnGovernanceProviderProtocolV1::OllamaNative,
            endpoint: "http://127.0.0.1:11434/api".into(),
            model_id: "synthetic".into(),
            credential_reference: None,
            request_timeout_ms: 30_000,
            max_input_tokens: 8192,
            max_output_tokens: 2048,
            provider_permission_generation: 1,
        })
        .unwrap();
    let host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    let model = VisualPersonaModel {
        interpretations: AtomicUsize::new(0),
    };
    let engine = MemoryLearningEngine::attach(runtime.clone()).unwrap();
    for (turn_id, input_id, source_id, description) in [
        (
            "turn-persona-visual-a",
            "input-persona-visual-a",
            "persona-camera-a",
            "我看到你穿着红裙子。",
        ),
        (
            "turn-persona-visual-b",
            "input-persona-visual-b",
            "persona-camera-b",
            "我看到你戴着蓝帽子。",
        ),
    ] {
        let projected = runtime
            .project(MemoryProjectionRequest {
                binding: ProceduralProjectionBindingV1::Turn {
                    turn_id: turn_id.into(),
                },
                temporal_operation: MemoryRecallTemporalOperation::Current,
                user_query: "你之前看到的红裙子和现在的蓝帽子呢？".into(),
                system_max_len: 12_000,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
            })
            .unwrap();
        let selection = projected.report().selection_receipt().unwrap().clone();
        let key = source_key(Some("desktop-a"), source_id);
        let input = visual_input(
            input_id,
            turn_id,
            key.clone(),
            PerceptionSourceKind::HostVisualObservation,
            PerceptionSourceRef::Sensor {
                sensor_id: "camera".into(),
            },
            None,
        );
        let observed_at = input.time.observed_at_unix_ms.unwrap();
        authorize_input(
            &runtime,
            &host,
            &input,
            PerceptionRetentionCeiling::DurableMedia,
        );
        runtime
            .submit_visual_input(
                &host,
                input,
                PerceptionRetentionCeiling::DurableMedia,
                VisualTurnPosition::BeforeFirstAssistant,
                Some(
                    VisualCaptureContext::new(
                        key,
                        VisualDeviceUse::PersonalSingleUser,
                        true,
                        Some(turn_id.into()),
                        Some(observed_at),
                        observed_at,
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        let packet = runtime
            .append_visual_delivery(&host, turn_id, 0)
            .unwrap()
            .unwrap();
        let delivered = receipt(&packet, true);
        runtime
            .attest_visual_delivery(&host, delivered.clone())
            .unwrap();
        runtime
            .record_visual_provider_response(
                &host,
                &packet.packet_id,
                &delivered.final_request_sha256,
                b"model-response".to_vec(),
            )
            .unwrap();
        runtime
            .authorize_visual_inference(
                &host,
                input_id,
                VisualInferenceAuthorization {
                    recipient_id: "vision-provider".into(),
                    source_key: packet.parts[0].source_key.clone(),
                    media_sha256: packet
                        .parts
                        .iter()
                        .map(|part| part.sha256.clone())
                        .collect(),
                    authorization_generation: 1,
                    user_consent_attested: true,
                },
            )
            .unwrap();
        assert_eq!(
            runtime
                .infer_visual_for_memory(
                    input_id,
                    &mut VisualInferenceFixture {
                        person_count: 1,
                        synthetic_user_role: true,
                        needs_revisit: false,
                        description,
                    },
                )
                .unwrap(),
            VisualMemoryObservationDecision::SemanticCandidate
        );
        let uses = runtime
            .prepare_turn_visual_external_uses(
                &host,
                turn_id,
                Some(&delivered.final_request_sha256),
            )
            .unwrap();
        assert_eq!(
            uses.len(),
            if source_id == "persona-camera-a" {
                1
            } else {
                2
            }
        );
        runtime
            .submit_turn_external_content_manifest(
                &host,
                turn_id,
                TranscriptExternalContentManifestV1 {
                    completeness: ExternalContentCompletenessV1::HostAttestedComplete,
                    request_sha256: vec![delivered.final_request_sha256],
                    uses,
                },
            )
            .unwrap();
        let mut finalized = turn(turn_id, &["我记得刚才看见的画面。"]);
        finalized.turn.external_content_used = true;
        finalized.learning.selection_receipt = Some(selection);
        runtime.finalize_turn(finalized).unwrap();
        engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: format!("visual-persona-{turn_id}"),
                    lease_duration_secs: 60,
                },
                &mut VisualPersonaPort(&model),
            )
            .unwrap();
    }
    assert_eq!(model.interpretations.load(Ordering::SeqCst), 1);
    assert!(store
        .export_replay_snapshot()
        .unwrap()
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "soul_turn_persona_evidence"));
    let requester = primary_human_subject_id("owner-default");
    let source_a = runtime
        .list_visual_memory(&requester)
        .unwrap()
        .into_iter()
        .find(|entry| entry.source.source.source_id == "persona-camera-a")
        .unwrap()
        .source;
    runtime.forget_visual_memory(&requester, source_a).unwrap();
    assert!(store
        .export_replay_snapshot()
        .unwrap()
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "soul_turn_persona_evidence"));
}

fn unscoped_visual_persona_case(
    second_turn_text: Option<&str>,
    include_second: bool,
) -> (usize, usize) {
    let store = empty_store_platform(support::host_test_profile());
    let runtime = Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("agent-main", "owner-default").unwrap())
            .subject_id("subject-default")
            .scope(MemoryScope::new("llm.gateway", "chat-a").unwrap())
            .store(store.clone())
            .scoped_runtime(SubjectScopedRuntime {
                memory_space_id: default_memory_space_id("owner-default"),
                mounted_subject_id: "subject-default".into(),
                actor_subject_id: "subject-default".into(),
                agent_id: "agent-main".into(),
                relationship_scope: None,
                projection_policy: "subject_aware_default".into(),
                write_policy: "subject_candidate_then_space_governance".into(),
            })
            .capability_policy(MemoryCapabilityPolicy::strict_profile())
            .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
            .audit_sink(Arc::new(NoopMemoryAuditSink))
            .build()
            .unwrap(),
    );
    runtime
        .install_governance_binding(MemoryGovernanceBindingInstallRequest {
            source_owner_id: "visual-subject-test".into(),
            source_config_id: "vision-governance".into(),
            source_revision: 1,
            protocol: PostTurnGovernanceProviderProtocolV1::OllamaNative,
            endpoint: "http://127.0.0.1:11434/api".into(),
            model_id: "synthetic".into(),
            credential_reference: None,
            request_timeout_ms: 30_000,
            max_input_tokens: 8192,
            max_output_tokens: 2048,
            provider_permission_generation: 1,
        })
        .unwrap();
    let host = runtime
        .register_perception_host("host-a", Some("desktop-a".into()))
        .unwrap();
    runtime
        .update_perception_capability(
            &host,
            PerceptionCapabilitySnapshot {
                host_id: "host-a".into(),
                device_id: "desktop-a".into(),
                sensor_id: "camera".into(),
                tool_id: "camera-tool".into(),
                tool_registry_revision: 1,
                authorization_generation: 1,
                state: PerceptionCapabilityState::Available,
                unavailable_reason: None,
                visual_forms: vec![VisualCaptureForm::Image],
            },
        )
        .unwrap();
    let model = VisualPersonaModel {
        interpretations: AtomicUsize::new(0),
    };
    let engine = MemoryLearningEngine::attach(runtime.clone()).unwrap();
    let cases = [
        (
            "turn-subject-visual-a",
            "input-subject-visual-a",
            "subject-camera-a",
            "我看到你穿着红裙子。",
        ),
        (
            "turn-subject-visual-b",
            "input-subject-visual-b",
            "subject-camera-b",
            "我看到你戴着蓝帽子。",
        ),
    ];
    for (index, &(turn_id, input_id, source_id, description)) in cases.iter().enumerate() {
        if index == 1 && !include_second {
            break;
        }
        let projected = runtime
            .project(MemoryProjectionRequest {
                binding: ProceduralProjectionBindingV1::Turn {
                    turn_id: turn_id.into(),
                },
                temporal_operation: MemoryRecallTemporalOperation::Current,
                user_query: "你还记得共同经历吗？".into(),
                system_max_len: 12_000,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
            })
            .unwrap();
        let selection = projected.report().selection_receipt().unwrap().clone();
        let key = source_key(Some("desktop-a"), source_id);
        let mut input = visual_input(
            input_id,
            turn_id,
            key.clone(),
            PerceptionSourceKind::HostVisualObservation,
            PerceptionSourceRef::Sensor {
                sensor_id: "camera".into(),
            },
            None,
        );
        input.scope.relationship_id = None;
        let observed_at = input.time.observed_at_unix_ms.unwrap();
        authorize_input(
            &runtime,
            &host,
            &input,
            PerceptionRetentionCeiling::DurableSemantic,
        );
        runtime
            .submit_visual_input(
                &host,
                input,
                PerceptionRetentionCeiling::DurableSemantic,
                VisualTurnPosition::BeforeFirstAssistant,
                Some(
                    VisualCaptureContext::new(
                        key,
                        VisualDeviceUse::PersonalSingleUser,
                        true,
                        Some(turn_id.into()),
                        Some(observed_at),
                        observed_at,
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        let packet = runtime
            .append_visual_delivery(&host, turn_id, 0)
            .unwrap()
            .unwrap();
        let delivered = receipt(&packet, true);
        runtime
            .attest_visual_delivery(&host, delivered.clone())
            .unwrap();
        runtime
            .record_visual_provider_response(
                &host,
                &packet.packet_id,
                &delivered.final_request_sha256,
                b"model-response".to_vec(),
            )
            .unwrap();
        runtime
            .authorize_visual_inference(
                &host,
                input_id,
                VisualInferenceAuthorization {
                    recipient_id: "vision-provider".into(),
                    source_key: packet.parts[0].source_key.clone(),
                    media_sha256: packet
                        .parts
                        .iter()
                        .map(|part| part.sha256.clone())
                        .collect(),
                    authorization_generation: 1,
                    user_consent_attested: true,
                },
            )
            .unwrap();
        assert_eq!(
            runtime
                .infer_visual_for_memory(
                    input_id,
                    &mut VisualInferenceFixture {
                        person_count: 1,
                        synthetic_user_role: true,
                        needs_revisit: false,
                        description,
                    },
                )
                .unwrap(),
            VisualMemoryObservationDecision::SemanticCandidate
        );
        let uses = runtime
            .prepare_turn_visual_external_uses(
                &host,
                turn_id,
                Some(&delivered.final_request_sha256),
            )
            .unwrap();
        runtime
            .submit_turn_external_content_manifest(
                &host,
                turn_id,
                TranscriptExternalContentManifestV1 {
                    completeness: ExternalContentCompletenessV1::HostAttestedComplete,
                    request_sha256: vec![delivered.final_request_sha256],
                    uses,
                },
            )
            .unwrap();
        let mut finalized = turn(turn_id, &["我记得刚才看见的画面。"]);
        finalized.turn.external_content_used = true;
        finalized.turn.input_messages = match (index, second_turn_text) {
            (1, Some(text)) => vec![TranscriptInputMessage::user(text)],
            (1, None) => Vec::new(),
            _ => vec![TranscriptInputMessage::user("请看一眼")],
        };
        finalized.learning.selection_receipt = Some(selection);
        runtime.finalize_turn(finalized).unwrap();
        engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: format!("subject-visual-{turn_id}"),
                    lease_duration_secs: 60,
                },
                &mut VisualPersonaPort(&model),
            )
            .unwrap();
    }
    let snapshot = store.export_replay_snapshot().unwrap();
    let evidence = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == "soul_turn_persona_evidence")
        .collect::<Vec<_>>();
    if let Some(document) = evidence.first() {
        use bm_core::memory::{
            GovernedTurnPersonaEvidenceV1, SubjectSoulOwnedDocumentV1,
            TurnPersonaInterpretationScopeV1, TurnPersonaInterpretationV1,
        };
        let owned: SubjectSoulOwnedDocumentV1 =
            serde_json::from_value(document.value.clone()).unwrap();
        let body: GovernedTurnPersonaEvidenceV1 = serde_json::from_value(owned.body).unwrap();
        assert!(matches!(
            body.interpretation,
            TurnPersonaInterpretationV1::Growth {
                scope: TurnPersonaInterpretationScopeV1::Subject,
                ..
            }
        ));
        assert_eq!(body.visual_observations.len(), 2);
        assert!(body
            .visual_observations
            .iter()
            .all(|item| item.episode_source.relationship_id.is_none()));
    }
    (model.interpretations.load(Ordering::SeqCst), evidence.len())
}

#[test]
fn unscoped_visual_subject_growth_requires_independent_canonical_text_and_two_sights() {
    assert_eq!(
        unscoped_visual_persona_case(
            Some("我希望你只据可核验的共同经历形成记忆，并尊重我关掉视觉的决定。"),
            true,
        ),
        (1, 1)
    );
    assert_eq!(unscoped_visual_persona_case(None, true), (0, 0));
    assert_eq!(
        unscoped_visual_persona_case(Some("我重视真实和自主边界。"), false),
        (0, 0)
    );
}
