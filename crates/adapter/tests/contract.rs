use std::sync::Arc;

use bm_adapter::{
    decode_json_adapter_command, dispatch_adapter_command, dispatch_adapter_command_with_services,
    AdapterAuthContext, AdapterCommand, AdapterEnvelope, AdapterErrorKey,
    AdapterJsonCommandOptions, AdapterOperation, AdapterResponse, AdapterRuntimeServices,
    AdapterSdkReport, AdapterSource, TransportKind, TransportMode,
};
use bm_sdk::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, MemoryCapabilityPolicy, MemoryClock,
    MemoryIdentity, MemoryPrivacyPolicy, MemoryRecallRequest, MemoryRuntime, MemoryScope, Message,
    NoopMemoryAuditSink, ProfileId, ResponseBody, StopReason, StoreBackendConfig, StorePlatform,
    ToolChoicePolicy, ToolSpec,
};

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
        (AdapterOperation::Close, r#"{"reason":"operator close"}"#),
    ];

    for (operation, body) in cases {
        let command =
            decode_json_adapter_command(operation, body, &options).expect("decode command");
        assert_eq!(command.operation(), operation);
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
