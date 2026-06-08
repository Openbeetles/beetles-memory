use std::sync::Arc;

use bm_adapter::{
    decode_json_adapter_command, dispatch_adapter_command, dispatch_adapter_command_with_services,
    AdapterAuthContext, AdapterCommand, AdapterEnvelope, AdapterErrorKey,
    AdapterJsonCommandOptions, AdapterOperation, AdapterResponse, AdapterRuntimeServices,
    AdapterSdkReport, AdapterSource, TransportKind, TransportMode,
};
use bm_sdk::{
    CanonicalTurnDelta, ConversationKey, ConversationScope, HostRefVisibility, LlmClient,
    LlmHttpClient, LlmModelCompat, LlmResponse, MemoryCapabilityPolicy, MemoryClock,
    MemoryIdentity, MemoryLongTermControlView, MemoryLongTermListRequest, MemoryPrivacyPolicy,
    MemoryRecallRequest, MemoryRuntime, MemoryScope, MemoryTranscriptAttrWriteRequest,
    MemoryTranscriptCommitRequest, MemoryTranscriptReplayRequest, MemoryTurnDeliveryStatus,
    MemoryTurnProtocol, MemoryTurnSource, MemoryWriteRequest, Message, NoopMemoryAuditSink,
    ProfileId, ResponseBody, StopReason, StoreBackendConfig, StorePlatform, ToolChoicePolicy,
    ToolSpec, TranscriptAttrEnvelope, TranscriptAttrGovernance, TranscriptAttrLink,
    TranscriptAttrRedactionPolicy, TranscriptAttrScope, TranscriptAttrSource,
    TranscriptAttrSourceKind, TranscriptAttrTarget, TranscriptAttrValueKind,
    TranscriptInputMessage, TranscriptReplayView,
};
use serde_json::json;

struct FixedClock;

impl MemoryClock for FixedClock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}

fn runtime() -> MemoryRuntime {
    let store = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .profile(ProfileId::ServerLinuxDevFull)
        .store_platform(store)
        .clock(Arc::new(FixedClock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime")
}

fn envelope<T>(operation: AdapterOperation, payload: T) -> AdapterEnvelope<T> {
    AdapterEnvelope {
        request_id: "req-1".to_string(),
        transport: TransportKind::Http,
        mode: TransportMode::Server,
        operation,
        source: AdapterSource {
            source_id: "source-1".to_string(),
            source_kind: "http_client".to_string(),
            agent_id: "agent-main".to_string(),
            owner_id: "owner-default".to_string(),
            channel: "local".to_string(),
            chat_id: "chat-1".to_string(),
        },
        auth: AdapterAuthContext {
            authenticated: true,
            auth_kind: "token".to_string(),
            principal: "operator".to_string(),
        },
        idempotency_key: "idem-1".to_string(),
        audit_id: "audit-1".to_string(),
        payload,
    }
}

#[test]
fn recall_command_dispatches_through_memory_runtime() {
    let runtime = runtime();
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            AdapterOperation::Recall,
            AdapterCommand::Recall(MemoryRecallRequest {
                query: "release".to_string(),
                limit: 2,
                tool_registry_refs: Vec::new(),
            }),
        ),
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Accepted {
            request_id,
            audit_id,
            report: AdapterSdkReport::Recall(report),
        } => {
            assert_eq!(request_id, "req-1");
            assert_eq!(audit_id, "audit-1");
            assert_eq!(report.query, "release");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn operation_mismatch_is_rejected_before_runtime_call() {
    let runtime = runtime();
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            AdapterOperation::Write,
            AdapterCommand::Recall(MemoryRecallRequest {
                query: "release".to_string(),
                limit: 2,
                tool_registry_refs: Vec::new(),
            }),
        ),
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Rejected { error_key, .. } => {
            assert_eq!(error_key, AdapterErrorKey::OperationMismatch);
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn long_term_list_command_dispatches_through_memory_runtime() {
    let runtime = runtime();
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            AdapterOperation::LongTermList,
            AdapterCommand::LongTermList(MemoryLongTermListRequest {
                query: bm_sdk::LongTermMemoryQuery::default(),
                cursor: None,
                limit: 10,
                view: MemoryLongTermControlView::HostUi,
            }),
        ),
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Accepted {
            request_id,
            audit_id,
            report: AdapterSdkReport::LongTermList(report),
        } => {
            assert_eq!(request_id, "req-1");
            assert_eq!(audit_id, "audit-1");
            assert_eq!(report.total_visible, 0);
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

fn transcript_attr(
    key: ConversationKey,
    turn_id: impl Into<String>,
    message_id: impl Into<String>,
) -> TranscriptAttrEnvelope {
    TranscriptAttrEnvelope {
        attr_id: "adapter-usage-1".to_string(),
        target: TranscriptAttrTarget {
            key,
            scope: TranscriptAttrScope::Message,
            turn_id: turn_id.into(),
            message_id: Some(message_id.into()),
        },
        key: "host.adapter.model_usage".to_string(),
        value_kind: TranscriptAttrValueKind::JsonObject,
        schema_ref: Some("adapter.model-usage.v1".to_string()),
        value: json!({"input_tokens": 9, "output_tokens": 3, "usage_source": "provider_reported"}),
        visibility: HostRefVisibility::HostUi,
        source: TranscriptAttrSource {
            writer: "adapter-test".to_string(),
            source_kind: TranscriptAttrSourceKind::ProviderReported,
            written_at: 1_800_000_000,
            audit_reason: "adapter transcript attr contract".to_string(),
        },
        governance: TranscriptAttrGovernance {
            max_value_bytes: 4096,
            redaction_policy: TranscriptAttrRedactionPolicy::MetadataSurvivesMask,
            export_allowed: false,
        },
        links: vec![TranscriptAttrLink {
            relation: "model_invocation".to_string(),
            ref_kind: "model_invocation_id".to_string(),
            ref_id: "adapter-model-1".to_string(),
        }],
        created_at: 1_800_000_000,
        updated_at: 1_800_000_000,
    }
}

fn transcript_turn() -> CanonicalTurnDelta {
    CanonicalTurnDelta {
        turn_id: "turn-adapter-1".to_string(),
        conversation: ConversationScope {
            channel: "local".to_string(),
            chat_id: "chat-1".to_string(),
            conversation_id: Some("conversation-a".to_string()),
        },
        subject: bm_sdk::default_agent_subject_id("agent-main"),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: MemoryTurnSource {
            ingress: bm_sdk::IngressKind::User,
            channel: "local".to_string(),
            provider: Some("adapter".to_string()),
            protocol: MemoryTurnProtocol::Native,
            endpoint: None,
            model_alias: None,
            model_resolved: None,
            request_id: Some("adapter-req-1".to_string()),
            client_conversation_hint: Some("conversation-a".to_string()),
        },
        actor: None,
        input_messages: vec![TranscriptInputMessage::user("adapter user")],
        assistant_message: Some(TranscriptInputMessage::assistant("adapter assistant")),
        tool_observations: Vec::new(),
        external_content_used: false,
        candidate_ids: Vec::new(),
    }
}

#[test]
fn transcript_attr_write_command_dispatches_through_memory_runtime() {
    let runtime = runtime();
    let key = ConversationKey::new(runtime.memory_space_id(), "local", "conversation-a").unwrap();
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: transcript_turn(),
            host_refs: Vec::new(),
        })
        .expect("commit transcript");
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::RawOwnerOnly,
        })
        .expect("replay transcript");
    let turn = &replay.slice.turns[0];
    let message_id = turn
        .assistant_message
        .as_ref()
        .expect("assistant message")
        .message_id
        .clone();

    let attr = transcript_attr(key.clone(), turn.turn_id.clone(), message_id);
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            AdapterOperation::TranscriptAttrWrite,
            AdapterCommand::TranscriptAttrWrite(MemoryTranscriptAttrWriteRequest {
                memory_space_id: key.memory_space_id,
                channel_id: key.channel_id,
                conversation_id: key.conversation_id,
                attrs: vec![attr],
                idempotency_key: Some("adapter-attr-write-1".to_string()),
                dry_run: false,
            }),
        ),
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Accepted {
            report: AdapterSdkReport::TranscriptAttrWrite(report),
            ..
        } => {
            assert_eq!(report.accepted_attrs.len(), 1);
            assert!(report.rejected_attrs.is_empty());
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn json_decoder_covers_adapter_memory_operations() {
    let options =
        AdapterJsonCommandOptions::new("test-adapter").with_default_source_chat_id("chat-1");
    let cases = [
        (
            AdapterOperation::Write,
            r#"{"name":"runtime_skill__adapter_write","topic":"adapter","title":"Adapter write","summary":"Adapter write summary","content":"1. Decode write payload.\n2. Dispatch common adapter command."}"#,
        ),
        (AdapterOperation::Recall, r#"{"query":"release","limit":2}"#),
        (
            AdapterOperation::Project,
            r#"{"query":"release","max_len":1024,"recent_messages_limit":2}"#,
        ),
        (
            AdapterOperation::Maintain,
            r#"{"user_content":"remember release guard","reply_content":"I will verify artifacts.","tool_calls":0}"#,
        ),
        (
            AdapterOperation::Inspect,
            r#"{"query":"release","max_len":1024}"#,
        ),
        (AdapterOperation::Recover, r#"{}"#),
        (
            AdapterOperation::Replay,
            r#"{"chat_id":"chat-1","limit":2}"#,
        ),
        (AdapterOperation::Export, r#"{"chat_id":"chat-1"}"#),
        (
            AdapterOperation::Import,
            r#"{"target_chat_id":"chat-1","snapshot":{"version":5,"exported_at":1800000000,"mode":"full_restore","chat_id":"chat-1"}}"#,
        ),
        (
            AdapterOperation::LongTermList,
            r#"{"query":{"kind":"project"},"limit":10}"#,
        ),
        (
            AdapterOperation::LongTermPolicy,
            r#"{"operation":{"suppress":{"selector":{"kind":"preference","topic_pattern":"temporary-*"},"duration":"until_manual_resume"}},"reason":"operator suppression"}"#,
        ),
        (
            AdapterOperation::TranscriptAttrWrite,
            r#"{
                "memory_space_id":"memory-space-owner-default",
                "channel_id":"local",
                "conversation_id":"conversation-a",
                "attrs":[],
                "idempotency_key":"attr-write-1",
                "dry_run":true
            }"#,
        ),
        (AdapterOperation::Close, r#"{"reason":"operator close"}"#),
    ];

    for (operation, body) in cases {
        let command =
            decode_json_adapter_command(operation, body, &options).expect("decode command");
        assert_eq!(command.operation(), operation);
    }
}

#[test]
fn json_decoder_accepts_agent_tool_usage_feedback_as_write_payload() {
    let command = decode_json_adapter_command(
        AdapterOperation::Write,
        r#"{
            "tool_usage_feedback": {
                "registry_ref": {
                    "registry_id": "host-tools",
                    "fingerprint": "registry-fp",
                    "scope": "global"
                },
                "observations": [{
                    "observation_id": "obs-1",
                    "registry_id": "host-tools",
                    "tool_id": "pdf.extract",
                    "schema_fingerprint": "schema-pdf-v1",
                    "call_id": "call-1",
                    "task_signature": "extract_pdf_text",
                    "summary": "PDF extraction produced usable text.",
                    "outcome": "succeeded",
                    "error_code": null,
                    "external_content": true,
                    "private_content_used": false,
                    "permission_tags": ["filesystem.read"],
                    "risk_tags": ["external_content"],
                    "started_at": 1800000000,
                    "completed_at": 1800000001
                }],
                "user_visible_result_summary": "PDF extraction worked.",
                "reuse_outcome": "succeeded",
                "operator_note": null
            }
        }"#,
        &AdapterJsonCommandOptions::new("test-adapter"),
    )
    .expect("decode feedback");

    match command {
        AdapterCommand::Write(MemoryWriteRequest::AgentToolUsageFeedback { feedback }) => {
            assert_eq!(feedback.registry_ref.registry_id, "host-tools");
            assert_eq!(feedback.observations[0].tool_id, "pdf.extract");
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn maintain_dispatch_uses_injected_runtime_services() {
    let runtime = runtime();
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let command = decode_json_adapter_command(
        AdapterOperation::Maintain,
        r#"{"user_content":"remember the release process","reply_content":"I will verify artifacts first."}"#,
        &AdapterJsonCommandOptions::new("test-adapter"),
    )
    .expect("maintain command");

    let response = dispatch_adapter_command_with_services(
        &runtime,
        envelope(AdapterOperation::Maintain, command),
        AdapterRuntimeServices {
            http: Some(&mut http),
            llm: Some(&llm),
        },
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Accepted {
            report: AdapterSdkReport::Maintain(report),
            ..
        } => {
            assert!(report.report.is_some());
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

struct StaticHttpClient;

impl LlmHttpClient for StaticHttpClient {
    fn do_post(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> bm_sdk::Result<(u16, ResponseBody)> {
        Ok((200, ResponseBody::Heap(Vec::new())))
    }
}

struct StaticLlmClient;

impl LlmClient for StaticLlmClient {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> bm_sdk::Result<LlmResponse> {
        Ok(LlmResponse {
            content: "Summary: release safety".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

#[test]
fn adapter_crate_manifest_has_no_direct_core_or_store_dependency() {
    let manifest = std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")))
        .expect("manifest");
    let dependencies = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or_default()
        .split('[')
        .next()
        .unwrap_or_default();

    assert!(!dependencies.contains("bm-core"));
    assert!(!dependencies.contains("bm-store"));
    assert!(dependencies.contains("bm-sdk"));
}
