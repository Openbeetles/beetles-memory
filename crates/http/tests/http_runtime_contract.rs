#![cfg(feature = "server-std")]

mod support;

#[cfg(feature = "nonproduction-replay-harness")]
use bm_adapter::AdapterRuntimeServices;
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
#[cfg(feature = "nonproduction-replay-harness")]
use bm_http::handle_http_in_process_request_with_services;
use bm_http::{handle_http_in_process_request, HttpRuntimeRequest};
use bm_sdk::{
    AgentToolDescriptor, AgentToolRegistrySnapshot, MemoryCapabilityPolicy, MemoryPrivacyPolicy,
    StoreBackendConfig,
};
#[cfg(feature = "nonproduction-replay-harness")]
use bm_sdk::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, Message, ResponseBody, StopReason,
    ToolChoicePolicy, ToolSpec,
};
use serde_json::{json, Value};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "http-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            conversation_id: None,
            channel: "http".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_disabled().with_cli(true),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

fn assert_exact_governed_result(body: &str) {
    let value: Value = serde_json::from_str(body).expect("governed response JSON");
    let result = value["result"].clone();
    let dto: bm_adapter::AdapterGovernedSafeReportV1 =
        serde_json::from_value(result.clone()).expect("strict adapter governed safe DTO");
    assert_eq!(
        serde_json::to_value(dto).expect("serialize adapter governed safe DTO"),
        result
    );
}

fn agent_tool_registry() -> AgentToolRegistrySnapshot {
    let mut tool = AgentToolDescriptor::compact("pdf.extract", "Extract PDF text", "schema-pdf-v1");
    tool.permission_tags = vec!["filesystem.read".to_string()];
    tool.risk_tags = vec!["external_content".to_string()];
    AgentToolRegistrySnapshot::compact("host-tools", "host", vec![tool], 1_800_000_000)
}

#[test]
fn http_runtime_dispatches_capabilities_and_recall_through_entry_runtime() {
    let runtime = runtime();

    let caps = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::get("/memory/profile/capabilities"),
    )
    .expect("capabilities");
    assert_eq!(caps.status_code, 200);
    assert!(caps.body.contains("\"profile\""));
    assert!(caps.body.contains("\"mutation_operation_inventory\""));
    assert!(caps.body.contains("\"mutation_receipt_policy\""));
    assert!(caps.body.contains("\"durable_store_receipt\""));
    assert!(caps.body.contains("\"explicitly_non_durable\""));
    assert!(caps.budget_report_id.starts_with("rtb-v2-"));

    let recall = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/recall",
            r#"{"temporal_operation":{"kind":"current"},"query":"release","limit":2}"#,
        ),
    )
    .expect("recall");
    assert_eq!(recall.status_code, 200);
    assert!(recall.body.contains("\"status\""));
    assert_exact_governed_result(&recall.body);

    let long_term = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/long-term/list", r#"{"query":{},"limit":2}"#),
    )
    .expect("long-term list");
    assert_eq!(long_term.status_code, 200);
    assert!(long_term.body.contains("\"total_visible\""));
}

#[test]
fn http_fallback_uses_stable_public_report_kind_without_debug_wire() {
    let runtime = runtime();
    let response = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/inspect",
            r#"{"query":"release","system_max_len":4096}"#,
        ),
    )
    .expect("inspect response");
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("response JSON");

    assert_eq!(body["status"], "accepted");
    assert_eq!(body["report_kind"], "inspect");
    assert!(!response.body.contains("MemoryInspectionReport"));
}

#[test]
fn http_runtime_registers_compact_agent_tool_registry_without_router_behavior() {
    let runtime = runtime();
    let registry = agent_tool_registry();
    let body = serde_json::to_string(&registry).expect("registry body");

    let upsert = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::put_json("/agent-tool-registries/host-tools", body),
    )
    .expect("upsert registry");
    assert_eq!(upsert.status_code, 200);
    let upsert_body: Value = serde_json::from_str(&upsert.body).expect("upsert json");
    assert_eq!(upsert_body["status"], "accepted");
    assert_eq!(upsert_body["report"]["registries"], 1);
    assert_eq!(upsert_body["report"]["tools"], 1);

    let listed =
        handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/agent-tool-registries"))
            .expect("list registries");
    let listed_body: Value = serde_json::from_str(&listed.body).expect("list json");
    assert_eq!(
        listed_body["registries"]
            .as_array()
            .expect("registries")
            .len(),
        1
    );
    assert_eq!(
        listed_body["registries"][0]["registry_id"],
        registry.registry_id.as_str()
    );

    let fetched = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::get("/agent-tool-registries/host-tools"),
    )
    .expect("fetch registry");
    let fetched_body: Value = serde_json::from_str(&fetched.body).expect("fetch json");
    assert_eq!(
        fetched_body["registry"]["fingerprint"],
        registry.fingerprint.as_str()
    );

    let deleted = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::delete("/agent-tool-registries/host-tools"),
    )
    .expect("delete registry");
    assert_eq!(deleted.status_code, 200);
    let after_delete = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::get("/agent-tool-registries/host-tools"),
    )
    .expect("fetch deleted registry");
    assert_eq!(after_delete.status_code, 404);
}

#[test]
fn http_runtime_registry_alone_never_fabricates_agent_tool_experience() {
    let runtime = runtime();
    let registry = agent_tool_registry();
    handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::put_json(
            "/agent-tool-registries/host-tools",
            serde_json::to_string(&registry).expect("registry body"),
        ),
    )
    .expect("upsert registry");

    let project_without_experience = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/project",
            json!({
                "binding": {"kind": "preview"},
                "temporal_operation": {"kind": "current"},
                "user_query": "extract text from this PDF",
                "system_max_len": 4096,
                "recent_messages_limit": 8,
                "tool_registry_refs": [registry.registry_ref()]
            })
            .to_string(),
        ),
    )
    .expect("project without experience");
    let no_hint: Value =
        serde_json::from_str(&project_without_experience.body).expect("project json");
    assert!(no_hint["result"]["report"]["agent_tool_hints"]
        .as_array()
        .expect("hints")
        .is_empty());

    let projected = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/project",
            json!({
                "binding": {"kind": "preview"},
                "temporal_operation": {"kind": "current"},
                "user_query": "extract text from this PDF",
                "system_max_len": 4096,
                "recent_messages_limit": 8,
                "tool_registry_refs": [registry.registry_ref()]
            })
            .to_string(),
        ),
    )
    .expect("project with experience");
    let projected_body: Value = serde_json::from_str(&projected.body).expect("project json");
    assert!(projected_body.get("system_memory_block").is_none());
    assert_eq!(projected_body["result"]["operation"], "project");
    assert_eq!(
        projected_body["result"]["report"]["chars"],
        projected_body["result"]["report"]["projection_block"]
            .as_str()
            .expect("projection block")
            .chars()
            .count()
    );
    assert!(projected_body["result"]["report"]["agent_tool_hints"]
        .as_array()
        .expect("hints")
        .is_empty());
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn http_runtime_decodes_declared_memory_routes_through_entry_runtime() {
    let runtime = runtime();
    let routes = [
        (
            "/memory/project",
            r#"{"binding":{"kind":"preview"},"temporal_operation":{"kind":"current"},"user_query":"release","system_max_len":1024,"recent_messages_limit":2}"#,
        ),
        (
            "/memory/inspect",
            r#"{"query":"release","system_max_len":1024}"#,
        ),
        ("/memory/recover", r#"{}"#),
        ("/memory/replay", r#"{"chat_id":"chat-1","limit":2}"#),
    ];

    for (path, body) in routes {
        let response =
            handle_http_in_process_request(&runtime, HttpRuntimeRequest::post_json(path, body))
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
fn http_runtime_body_limit_comes_from_runtime_budget_report() {
    let runtime = runtime();
    let over_budget = "x".repeat(runtime.runtime_budget().adapter_budget.http_body_max_bytes + 1);
    let error = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/recall", &over_budget),
    )
    .expect_err("runtime budget must reject oversized body before decode");

    assert_eq!(error.stage(), "http_body");
}

#[test]
fn http_runtime_accepts_body_at_exact_pinned_budget() {
    let runtime = runtime();
    let max_bytes = runtime.runtime_budget().adapter_budget.http_body_max_bytes;
    let mut request = HttpRuntimeRequest::get("/memory/profile/capabilities");
    request.body = " ".repeat(max_bytes);

    let response = handle_http_in_process_request(&runtime, request).expect("exact boundary body");

    assert_eq!(response.status_code, 200);
    assert!(response.budget_report_id.starts_with("rtb-v2-"));
}

#[test]
fn http_runtime_agent_tool_registry_body_limit_uses_runtime_budget_report() {
    let runtime = runtime();
    let over_budget = "x".repeat(runtime.runtime_budget().adapter_budget.http_body_max_bytes + 1);
    let error = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::put_json("/agent-tool-registries/host-tools", &over_budget),
    )
    .expect_err("registry upsert must reject oversized body before decode");

    assert_eq!(error.stage(), "http_body");
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn http_runtime_runs_maintenance_when_llm_services_are_injected() {
    let runtime = runtime();
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let response = handle_http_in_process_request_with_services(
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
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("response JSON");
    assert_eq!(body["status"], "accepted");
    assert_eq!(body["report_kind"], "maintain");
    assert!(!response.body.contains("MemoryMaintenanceReport"));
}

#[cfg(feature = "nonproduction-replay-harness")]
struct StaticHttpClient;

#[cfg(feature = "nonproduction-replay-harness")]
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

#[cfg(feature = "nonproduction-replay-harness")]
struct StaticLlmClient;

#[cfg(feature = "nonproduction-replay-harness")]
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
