#![cfg(feature = "server-std")]

use bm_adapter::AdapterRuntimeServices;
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{handle_http_request, handle_http_request_with_services, HttpRuntimeRequest};
use bm_sdk::{
    AgentToolDescriptor, AgentToolRegistrySnapshot, LlmClient, LlmHttpClient, LlmModelCompat,
    LlmResponse, MemoryCapabilityPolicy, MemoryPrivacyPolicy, Message, ProfileId, ResponseBody,
    StopReason, StoreBackendKind, ToolChoicePolicy, ToolSpec,
};
use serde_json::{json, Value};

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

fn agent_tool_registry() -> AgentToolRegistrySnapshot {
    let mut tool = AgentToolDescriptor::compact("pdf.extract", "Extract PDF text", "schema-pdf-v1");
    tool.permission_tags = vec!["filesystem.read".to_string()];
    tool.risk_tags = vec!["external_content".to_string()];
    AgentToolRegistrySnapshot::compact("host-tools", "host", vec![tool], 1_800_000_000)
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

    let long_term = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/long-term/list", r#"{"query":{},"limit":2}"#),
    )
    .expect("long-term list");
    assert_eq!(long_term.status_code, 200);
    assert!(long_term.body.contains("\"total_visible\""));
}

#[test]
fn http_runtime_registers_compact_agent_tool_registry_without_router_behavior() {
    let runtime = runtime();
    let registry = agent_tool_registry();
    let body = serde_json::to_string(&registry).expect("registry body");

    let upsert = handle_http_request(
        &runtime,
        HttpRuntimeRequest::put_json("/agent-tool-registries/host-tools", body),
    )
    .expect("upsert registry");
    assert_eq!(upsert.status_code, 200);
    let upsert_body: Value = serde_json::from_str(&upsert.body).expect("upsert json");
    assert_eq!(upsert_body["status"], "accepted");
    assert_eq!(upsert_body["report"]["registries"], 1);
    assert_eq!(upsert_body["report"]["tools"], 1);

    let listed = handle_http_request(&runtime, HttpRuntimeRequest::get("/agent-tool-registries"))
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

    let fetched = handle_http_request(
        &runtime,
        HttpRuntimeRequest::get("/agent-tool-registries/host-tools"),
    )
    .expect("fetch registry");
    let fetched_body: Value = serde_json::from_str(&fetched.body).expect("fetch json");
    assert_eq!(
        fetched_body["registry"]["fingerprint"],
        registry.fingerprint.as_str()
    );

    let deleted = handle_http_request(
        &runtime,
        HttpRuntimeRequest::delete("/agent-tool-registries/host-tools"),
    )
    .expect("delete registry");
    assert_eq!(deleted.status_code, 200);
    let after_delete = handle_http_request(
        &runtime,
        HttpRuntimeRequest::get("/agent-tool-registries/host-tools"),
    )
    .expect("fetch deleted registry");
    assert_eq!(after_delete.status_code, 404);
}

#[test]
fn http_runtime_projects_agent_tool_hints_only_after_feedback_experience() {
    let runtime = runtime();
    let registry = agent_tool_registry();
    handle_http_request(
        &runtime,
        HttpRuntimeRequest::put_json(
            "/agent-tool-registries/host-tools",
            serde_json::to_string(&registry).expect("registry body"),
        ),
    )
    .expect("upsert registry");

    let project_without_experience = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/project",
            json!({
                "query": "extract text from this PDF",
                "max_len": 4096,
                "recent_messages_limit": 8,
                "tool_registry_refs": [registry.registry_ref()]
            })
            .to_string(),
        ),
    )
    .expect("project without experience");
    let no_hint: Value =
        serde_json::from_str(&project_without_experience.body).expect("project json");
    assert!(no_hint["agent_tool_hints"]
        .as_array()
        .expect("hints")
        .is_empty());

    let feedback = json!({
        "tool_usage_feedback": {
            "registry_ref": registry.registry_ref(),
            "observations": [
                {
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
                    "started_at": 1800000000u64,
                    "completed_at": 1800000001u64
                },
                {
                    "observation_id": "obs-2",
                    "registry_id": "host-tools",
                    "tool_id": "pdf.extract",
                    "schema_fingerprint": "schema-pdf-v1",
                    "call_id": "call-2",
                    "task_signature": "extract_pdf_text",
                    "summary": "PDF extraction produced usable text again.",
                    "outcome": "succeeded",
                    "error_code": null,
                    "external_content": true,
                    "private_content_used": false,
                    "permission_tags": ["filesystem.read"],
                    "risk_tags": ["external_content"],
                    "started_at": 1800000002u64,
                    "completed_at": 1800000003u64
                }
            ],
            "user_visible_result_summary": "PDF extraction helped prepare notes.",
            "reuse_outcome": "succeeded",
            "operator_note": null
        }
    });
    let write = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/write", feedback.to_string()),
    )
    .expect("write feedback");
    let write_body: Value = serde_json::from_str(&write.body).expect("write json");
    assert_eq!(write_body["agent_tool_experience"]["accepted"], true);
    assert_eq!(write_body["changed"], 1);

    let projected = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/project",
            json!({
                "query": "extract text from this PDF",
                "max_len": 4096,
                "recent_messages_limit": 8,
                "tool_registry_refs": [registry.registry_ref()]
            })
            .to_string(),
        ),
    )
    .expect("project with experience");
    let projected_body: Value = serde_json::from_str(&projected.body).expect("project json");
    assert_eq!(
        projected_body["agent_tool_hints"][0]["tool_id"],
        "pdf.extract"
    );
    assert_eq!(
        projected_body["agent_tool_hints"][0]["host_execution_required"],
        true
    );
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
fn http_runtime_body_limit_comes_from_runtime_budget_report() {
    let runtime = runtime();
    let over_budget = "x".repeat(runtime.runtime_budget().adapter_budget.http_body_max_bytes + 1);
    let error = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/recall", &over_budget),
    )
    .expect_err("runtime budget must reject oversized body before decode");

    assert_eq!(error.stage(), "http_body");
}

#[test]
fn http_runtime_agent_tool_registry_body_limit_uses_runtime_budget_report() {
    let runtime = runtime();
    let over_budget = "x".repeat(runtime.runtime_budget().adapter_budget.http_body_max_bytes + 1);
    let error = handle_http_request(
        &runtime,
        HttpRuntimeRequest::put_json("/agent-tool-registries/host-tools", &over_budget),
    )
    .expect_err("registry upsert must reject oversized body before decode");

    assert_eq!(error.stage(), "http_body");
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
