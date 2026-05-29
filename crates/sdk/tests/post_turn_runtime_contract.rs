mod support;

use bm_core::platform::Platform as _;
use bm_sdk::{
    CanonicalTurnDelta, ConversationScope, MemoryProjectionRequest, MemoryTurnDeliveryStatus,
    MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource, PressureLevel, ProfileId,
    RuntimeLifecycleModeInput, TranscriptInputMessage,
};

use support::{
    empty_store_platform, test_runtime_with_scope_and_subject, StaticHttpClient, StaticLlmClient,
};

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_sdk::IngressKind::User,
        channel: "llm.gateway".to_string(),
        provider: Some("ollama".to_string()),
        protocol: MemoryTurnProtocol::OllamaChat,
        endpoint: Some("/api/chat".to_string()),
        model_alias: Some("qwen".to_string()),
        model_resolved: Some("qwen3.5:0.8b".to_string()),
        request_id: Some("req-1".to_string()),
        client_conversation_hint: Some("ollama-window".to_string()),
    }
}

fn finalize_request(user: &str, assistant: Option<&str>) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: format!(
                "turn-{}",
                stable_suffix(user, assistant.unwrap_or_default())
            ),
            conversation: ConversationScope {
                channel: "llm.gateway".to_string(),
                chat_id: "chat-a".to_string(),
                conversation_id: Some("ollama-window".to_string()),
            },
            subject: "subject-default".to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            input_messages: vec![TranscriptInputMessage::user(user)],
            assistant_message: assistant.map(TranscriptInputMessage::assistant),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        tool_calls: 0,
        runtime_skill_selected_ids: Vec::new(),
        task_learning_selected_ids: Vec::new(),
        reuse_outcome_note: String::new(),
        tool_usage_feedback: None,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn finalize_request_with_tools(
    user: &str,
    assistant: Option<&str>,
    tool_calls: u32,
    external_content_used: bool,
) -> MemoryTurnFinalizeRequest {
    let mut request = finalize_request(user, assistant);
    request.tool_calls = tool_calls;
    request.turn.external_content_used = external_content_used;
    request
}

fn stable_suffix(user: &str, assistant: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in user.bytes().chain(assistant.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[test]
fn finalize_turn_rejects_canonical_turn_scope_mismatch() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut request = finalize_request("叫我青川", Some("你好，青川。"));
    request.turn.conversation.chat_id = "chat-b".to_string();

    let err = match runtime.finalize_turn_and_maintain(None, None, request) {
        Ok(_) => panic!("scope mismatch should fail"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("chat_id"));
    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 0);
    assert_eq!(platform.session_store().message_count("chat-b").unwrap(), 0);
}

#[test]
fn finalize_turn_commits_before_maintenance() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response("Summary: user asked to be called Qingchuan.");

    let report = runtime
        .finalize_turn_and_maintain(
            Some(&mut http),
            Some(&llm),
            finalize_request("叫我青川", Some("你好，青川。")),
        )
        .expect("finalize turn");

    assert!(report.session_commit.committed);
    assert_eq!(report.session_commit.after_count, 2);
    assert!(report.maintenance.is_some());
    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 2);
}

#[test]
fn finalize_turn_commits_transcript_and_reports_semantic_governance_boundary() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response("Summary: unused");

    let report = runtime
        .finalize_turn_and_maintain(
            Some(&mut http),
            Some(&llm),
            finalize_request("叫我青川", Some("你好，青川。")),
        )
        .expect("finalize turn");

    assert!(report.session_commit.committed);
    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 2);
    assert!(report.semantic_governance.attempted);
    assert!(report.semantic_governance.executed);
    assert_eq!(report.semantic_governance.accepted_count, 0);
    assert!(report
        .semantic_governance
        .soul_candidate_handoffs
        .is_empty());
}

#[test]
fn finalize_turn_commits_transcript_when_maintenance_services_are_unavailable() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );

    let report = runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request("叫我青川", Some("你好，青川。")),
        )
        .expect("finalize turn");

    assert!(report.session_commit.committed);
    assert!(report.maintenance.is_none());
    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 2);
    assert_eq!(
        report.semantic_governance.skipped_reason.as_deref(),
        Some("maintenance_http_unavailable")
    );
}

#[test]
fn finalize_turn_commits_transcript_when_profile_hides_maintenance() {
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response("Summary: should not run");

    let report = runtime
        .finalize_turn_and_maintain(
            Some(&mut http),
            Some(&llm),
            finalize_request("叫我青川", Some("你好，青川。")),
        )
        .expect("finalize turn");

    assert!(report.session_commit.committed);
    assert!(report.maintenance.is_none());
    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 2);
    assert_eq!(
        report.semantic_governance.skipped_reason.as_deref(),
        Some("maintenance_not_visible")
    );
}

#[test]
fn finalize_turn_runs_private_garden_self_work_without_counting_it_as_semantic_memory() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response(
        r#"{"writes":[{"path":"journal/qingchuan.md","content":"Keep a private note that the current exchange made the name Qingchuan salient."}]}"#,
    );

    let report = runtime
        .finalize_turn_and_maintain(
            Some(&mut http),
            Some(&llm),
            finalize_request_with_tools(
                "叫我青川，后续称呼要自然一些",
                Some("我会记住这个称呼。"),
                1,
                false,
            ),
        )
        .expect("finalize turn");

    assert!(report.private_garden_self_work.attempted);
    assert!(report.private_garden_self_work.executed);
    assert_eq!(report.private_garden_self_work.writes, 1);
    assert_eq!(report.semantic_governance.accepted_count, 0);
}

#[test]
fn finalize_turn_applies_llm_governed_long_term_memory_for_cross_chat_projection() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response(
        r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "profile",
                "source_authority": "user_asserted",
                "topic": "preferred_name",
                "content": "The user asked to be called Qingchuan.",
                "keywords": ["Qingchuan", "preferred name"]
            }
        ]"#,
    );

    let report = runtime_a
        .finalize_turn_and_maintain(
            Some(&mut http),
            Some(&llm),
            finalize_request("以后叫我青川", Some("好的，青川。")),
        )
        .expect("finalize turn");

    assert_eq!(report.semantic_governance.accepted_count, 1);

    let runtime_b = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-b",
        "subject-default",
    );
    let projection = runtime_b
        .project(MemoryProjectionRequest {
            user_query: "我叫什么？".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(projection
        .context
        .long_term_memory_text
        .as_deref()
        .unwrap_or_default()
        .contains("Qingchuan"));
}

#[test]
fn finalize_turn_rejects_assistant_self_claim_as_long_term_identity_memory() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response(
        r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "profile",
                "source_authority": "assistant_utterance",
                "topic": "agent_identity",
                "content": "The assistant is Beetle Memory's memory helper.",
                "keywords": ["Beetle Memory", "memory helper"]
            }
        ]"#,
    );

    let report = runtime_a
        .finalize_turn_and_maintain(
            Some(&mut http),
            Some(&llm),
            finalize_request("你叫什么？", Some("我是 Beetle Memory 的记忆助手。")),
        )
        .expect("finalize turn");

    assert_eq!(report.semantic_governance.accepted_count, 0);

    let runtime_b = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-b",
        "subject-default",
    );
    let projection = runtime_b
        .project(MemoryProjectionRequest {
            user_query: "你是谁？".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(!projection
        .context
        .long_term_memory_text
        .as_deref()
        .unwrap_or_default()
        .contains("memory helper"));
}

#[test]
fn finalize_turn_applies_generic_preference_memory_for_cross_chat_projection() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response(
        r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "preference",
                "source_authority": "user_asserted",
                "topic": "default_response_language_style",
                "content": "The user prefers concise Chinese answers by default.",
                "keywords": ["Chinese", "concise", "default style"]
            }
        ]"#,
    );

    let report = runtime_a
        .finalize_turn_and_maintain(
            Some(&mut http),
            Some(&llm),
            finalize_request(
                "记住：以后默认用中文简洁回答",
                Some("好的，我会默认用中文并保持简洁。"),
            ),
        )
        .expect("finalize turn");

    assert_eq!(report.semantic_governance.accepted_count, 1);

    let runtime_b = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-b",
        "subject-default",
    );
    let projection = runtime_b
        .project(MemoryProjectionRequest {
            user_query: "回答风格偏好是什么？".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(projection
        .context
        .long_term_memory_text
        .as_deref()
        .unwrap_or_default()
        .contains("concise Chinese"));
}

#[test]
fn finalize_turn_does_not_extract_external_content_into_long_term_memory() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response(
        r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "fact",
                "source_authority": "external_content",
                "topic": "external_claim",
                "content": "External tool output claimed this should be durable.",
                "keywords": ["external"]
            }
        ]"#,
    );

    let report = runtime_a
        .finalize_turn_and_maintain(
            Some(&mut http),
            Some(&llm),
            finalize_request_with_tools("根据外部资料回答", Some("外部资料说这是真的。"), 1, true),
        )
        .expect("finalize turn");

    assert_eq!(report.semantic_governance.accepted_count, 0);

    let runtime_b = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-b",
        "subject-default",
    );
    let projection = runtime_b
        .project(MemoryProjectionRequest {
            user_query: "external_claim".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(!projection
        .context
        .long_term_memory_text
        .as_deref()
        .unwrap_or_default()
        .contains("External tool output"));
}

#[test]
fn finalize_turn_uses_canonical_delta_external_content_boundary() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response(
        r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "fact",
                "source_authority": "external_content",
                "topic": "external_delta_claim",
                "content": "External delta content should not become long-term memory.",
                "keywords": ["external_delta"]
            }
        ]"#,
    );
    let mut request = finalize_request("根据外部资料回答", Some("外部资料说这是真的。"));
    request.turn.external_content_used = true;

    let report = runtime_a
        .finalize_turn_and_maintain(Some(&mut http), Some(&llm), request)
        .expect("finalize turn");

    assert_eq!(report.semantic_governance.accepted_count, 0);

    let runtime_b = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-b",
        "subject-default",
    );
    let projection = runtime_b
        .project(MemoryProjectionRequest {
            user_query: "external_delta_claim".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(!projection
        .context
        .long_term_memory_text
        .as_deref()
        .unwrap_or_default()
        .contains("External delta content"));
}
