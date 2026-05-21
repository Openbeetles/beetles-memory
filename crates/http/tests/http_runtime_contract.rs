#![cfg(feature = "server-std")]

use bm_adapter::AdapterRuntimeServices;
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{handle_http_request, handle_http_request_with_services, HttpRuntimeRequest};
use bm_sdk::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, MemoryCapabilityPolicy,
    MemoryPrivacyPolicy, Message, ProfileId, ResponseBody, StopReason, StoreBackendKind,
    ToolChoicePolicy, ToolSpec,
};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "http-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "http".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_disabled().with_cli(true),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[test]
fn http_runtime_dispatches_capabilities_and_recall_through_entry_runtime() {
    let runtime = runtime();

    let caps = handle_http_request(
        &runtime,
        HttpRuntimeRequest::get("/memory/profile/capabilities"),
    )
    .expect("capabilities");
    assert_eq!(caps.status_code, 200);
    assert!(caps.body.contains("\"profile\""));
    assert!(caps.body.contains("\"entry\""));

    let recall = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/recall", r#"{"query":"release","limit":2}"#),
    )
    .expect("recall");
    assert_eq!(recall.status_code, 200);
    assert!(recall.body.contains("\"status\""));
}

#[test]
fn http_runtime_decodes_declared_memory_routes_through_entry_runtime() {
    let runtime = runtime();
    let routes = [
        (
            "/memory/project",
            r#"{"query":"release","max_len":1024,"recent_messages_limit":2}"#,
        ),
        ("/memory/inspect", r#"{"query":"release","max_len":1024}"#),
        ("/memory/recover", r#"{}"#),
        ("/memory/replay", r#"{"chat_id":"chat-1","limit":2}"#),
        ("/memory/export", r#"{"chat_id":"chat-1"}"#),
        (
            "/memory/import",
            r#"{"target_chat_id":"chat-1","snapshot":{"version":5,"exported_at":1800000000,"mode":"full_restore","chat_id":"chat-1"}}"#,
        ),
    ];

    for (path, body) in routes {
        let response = handle_http_request(&runtime, HttpRuntimeRequest::post_json(path, body))
            .unwrap_or_else(|err| panic!("{path} failed: {err}"));
        assert_eq!(response.status_code, 200, "{path}: {}", response.body);
        assert!(
            response.body.contains("\"status\""),
            "{path}: {}",
            response.body
        );
    }
}

#[test]
fn http_runtime_runs_maintenance_when_llm_services_are_injected() {
    let runtime = runtime();
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let response = handle_http_request_with_services(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/maintain",
            r#"{"user_content":"remember release process","reply_content":"I will verify artifacts first."}"#,
        ),
        AdapterRuntimeServices {
            http: Some(&mut http),
            llm: Some(&llm),
        },
    )
    .expect("maintain");

    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("Maintain"));
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
