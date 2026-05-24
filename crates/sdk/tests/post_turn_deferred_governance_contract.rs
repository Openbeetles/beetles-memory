mod support;

use bm_core::platform::Platform as _;
use bm_sdk::{
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    PressureLevel, ProfileId, RuntimeLifecycleModeInput, TranscriptInputMessage,
};

use support::{empty_store_platform, test_runtime_with_scope};

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_sdk::IngressKind::User,
        channel: "llm.gateway".to_string(),
        provider: Some("ollama".to_string()),
        protocol: MemoryTurnProtocol::OllamaChat,
        endpoint: Some("/api/chat".to_string()),
        model_alias: Some("qwen".to_string()),
        model_resolved: Some("qwen3".to_string()),
        request_id: Some("req-1".to_string()),
        client_conversation_hint: Some("window-a".to_string()),
    }
}

#[test]
fn maintenance_unavailable_commits_turn_and_enqueues_deferred_governance() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform.clone(), profile, "llm.gateway", "chat-a");

    let report = runtime
        .finalize_turn_and_maintain(
            None,
            None,
            MemoryTurnFinalizeRequest {
                delivery_status: MemoryTurnDeliveryStatus::Delivered,
                source: turn_source(),
                user_content: "叫我青川".to_string(),
                input_messages: vec![TranscriptInputMessage::user("叫我青川")],
                assistant_content: Some("你好，青川。".to_string()),
                tool_calls: 0,
                external_content_used: false,
                runtime_skill_selected_ids: Vec::new(),
                task_learning_selected_ids: Vec::new(),
                reuse_outcome_note: String::new(),
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .expect("finalize");

    assert!(report.session_commit.committed);
    assert!(report.semantic_governance.attempted);
    assert_eq!(report.semantic_governance.deferred_count, 1);
    assert_eq!(
        report.semantic_governance.skipped_reason.as_deref(),
        Some("maintenance_http_unavailable")
    );

    let raw = platform
        .state_fs()
        .read("memory/governance_jobs/pending.json")
        .expect("read jobs")
        .expect("pending jobs");
    let jobs = String::from_utf8(raw).expect("utf8");
    assert!(jobs.contains("chat-a"));
    assert!(jobs.contains("maintenance_http_unavailable"));
}
