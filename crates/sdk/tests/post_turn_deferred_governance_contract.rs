mod support;

use bm_core::platform::Platform as _;
use bm_sdk::{
    default_memory_space_id, CanonicalTurnDelta, ConversationScope, DeferredGovernanceJob,
    DeferredGovernanceJobStatus, MemoryDeferredGovernanceRunRequest, MemoryInspectionRequest,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    PressureLevel, ProfileId, RuntimeLifecycleModeInput, TranscriptInputMessage,
};

use support::{
    empty_store_platform, test_runtime_with_identity_scope_and_subject,
    test_runtime_with_scope_and_subject, StaticHttpClient, StaticLlmClient,
};

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

fn finalize_request(user: &str, assistant: &str) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: "turn-deferred-1".to_string(),
            conversation: ConversationScope {
                channel: "llm.gateway".to_string(),
                chat_id: "chat-a".to_string(),
                conversation_id: Some("window-a".to_string()),
            },
            subject: "subject-default".to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            input_messages: vec![TranscriptInputMessage::user(user)],
            assistant_message: Some(TranscriptInputMessage::assistant(assistant)),
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

fn finalize_request_for_scope(subject_id: &str, chat_id: &str) -> MemoryTurnFinalizeRequest {
    let mut request = finalize_request("同 turn 跨 subject", "已收到。");
    request.turn.subject = subject_id.to_string();
    request.turn.conversation.chat_id = chat_id.to_string();
    request
}

#[test]
fn maintenance_unavailable_commits_turn_and_enqueues_deferred_governance() {
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
        .finalize_turn_and_maintain(None, None, finalize_request("叫我青川", "你好，青川。"))
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

#[test]
fn duplicate_canonical_turn_does_not_enqueue_duplicate_deferred_governance() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );

    let first = runtime
        .finalize_turn_and_maintain(None, None, finalize_request("叫我青川", "你好，青川。"))
        .expect("first finalize");
    let second = runtime
        .finalize_turn_and_maintain(None, None, finalize_request("叫我青川", "你好，青川。"))
        .expect("second finalize");

    assert!(first.session_commit.committed);
    assert!(!second.session_commit.committed);
    assert_eq!(first.semantic_governance.deferred_count, 1);
    assert_eq!(second.semantic_governance.deferred_count, 0);

    let raw = platform
        .state_fs()
        .read("memory/governance_jobs/pending.json")
        .expect("read jobs")
        .expect("pending jobs");
    let jobs: Vec<DeferredGovernanceJob> = serde_json::from_slice(&raw).expect("jobs json");
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        jobs[0].idempotency_key,
        "space:owner-default:subject-default:llm.gateway:chat-a:turn-deferred-1"
    );
    assert_eq!(jobs[0].subject_id, "subject-default");
    assert_eq!(
        jobs[0].memory_space_id,
        default_memory_space_id("owner-default")
    );
    assert_eq!(jobs[0].turn_id, "turn-deferred-1");
    assert_eq!(jobs[0].candidate_ids, Vec::<String>::new());
    assert_eq!(jobs[0].retry_policy, "standard_backoff");
    assert_eq!(
        jobs[0].turn.as_ref().map(|turn| turn.turn_id.as_str()),
        Some("turn-deferred-1")
    );
}

#[test]
fn deferred_governance_worker_runs_stored_turn_without_recommitting_session() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );

    runtime
        .finalize_turn_and_maintain(None, None, finalize_request("叫我青川", "你好，青川。"))
        .expect("defer finalize");
    assert_eq!(
        platform
            .session_store()
            .message_count("chat-a")
            .expect("message count"),
        2
    );

    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response("Summary: deferred governance processed");
    let report = runtime
        .run_due_governance(
            &mut http,
            Some(&llm),
            MemoryDeferredGovernanceRunRequest { limit: 4 },
        )
        .expect("run due governance");

    assert_eq!(report.attempted, 1);
    assert_eq!(report.succeeded, 1);
    assert_eq!(report.failed, 0);
    assert_eq!(report.remaining_pending, 0);
    assert_eq!(
        platform
            .session_store()
            .message_count("chat-a")
            .expect("message count after worker"),
        2
    );

    let raw = platform
        .state_fs()
        .read("memory/governance_jobs/pending.json")
        .expect("read jobs")
        .expect("pending jobs");
    let jobs: Vec<DeferredGovernanceJob> = serde_json::from_slice(&raw).expect("jobs json");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].status, DeferredGovernanceJobStatus::Terminal);
    assert_eq!(jobs[0].attempts, 1);
    assert!(jobs[0].last_error.is_none());
}

#[test]
fn deferred_governance_queue_report_exposes_operator_visible_scope_and_status() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );

    runtime
        .finalize_turn_and_maintain(None, None, finalize_request("叫我青川", "你好，青川。"))
        .expect("defer finalize");

    let queue = runtime
        .deferred_governance_report()
        .expect("deferred queue report");
    assert_eq!(queue.total, 1);
    assert_eq!(queue.pending, 1);
    assert_eq!(queue.retrying, 0);
    assert_eq!(queue.failed, 0);
    assert_eq!(queue.terminal, 0);
    assert_eq!(queue.oldest_pending_at, Some(1_800_000_000));
    assert_eq!(queue.newest_pending_at, Some(1_800_000_000));
    assert_eq!(queue.recent_jobs.len(), 1);
    let job = &queue.recent_jobs[0];
    assert_eq!(job.status, DeferredGovernanceJobStatus::Pending);
    assert_eq!(
        job.memory_space_id,
        default_memory_space_id("owner-default")
    );
    assert_eq!(job.subject_id, "subject-default");
    assert_eq!(job.chat_id, "chat-a");
    assert_eq!(job.conversation_id.as_deref(), Some("window-a"));
    assert_eq!(job.turn_id, "turn-deferred-1");
    assert_eq!(job.reason, "maintenance_http_unavailable");
    assert_eq!(job.retry_policy, "standard_backoff");
    assert_eq!(job.attempts, 0);
    assert!(job.last_error.is_none());

    let inspect = runtime
        .inspect(MemoryInspectionRequest {
            query: "青川".to_string(),
            system_max_len: 2048,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("inspect");
    assert_eq!(inspect.deferred_governance.pending, 1);
    assert_eq!(
        inspect.deferred_governance.recent_jobs[0].turn_id,
        "turn-deferred-1"
    );
}

#[test]
fn deferred_governance_worker_and_report_are_isolated_by_memory_space_subject_and_channel() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_identity_scope_and_subject(
        platform.clone(),
        profile,
        "agent-a",
        "owner-a",
        "subject-a",
        "llm.gateway",
        "shared-chat",
    );
    let runtime_b = test_runtime_with_identity_scope_and_subject(
        platform.clone(),
        profile,
        "agent-b",
        "owner-b",
        "subject-b",
        "llm.gateway",
        "shared-chat",
    );

    runtime_a
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request_for_scope("subject-a", "shared-chat"),
        )
        .expect("runtime a defer");
    let raw = platform
        .state_fs()
        .read("memory/governance_jobs/pending.json")
        .expect("read jobs")
        .expect("pending jobs");
    let mut jobs: Vec<DeferredGovernanceJob> = serde_json::from_slice(&raw).expect("jobs json");
    assert_eq!(jobs.len(), 1);
    let mut other_scope_job = jobs[0].clone();
    other_scope_job.job_id = "governance-manual-other-scope".to_string();
    other_scope_job.idempotency_key =
        "space:owner-b:subject-b:llm.gateway:shared-chat:turn-deferred-1".to_string();
    other_scope_job.memory_space_id = default_memory_space_id("owner-b");
    other_scope_job.subject_id = "subject-b".to_string();
    if let Some(turn) = other_scope_job.turn.as_mut() {
        turn.subject = "subject-b".to_string();
    }
    jobs.push(other_scope_job);
    platform
        .state_fs()
        .write(
            "memory/governance_jobs/pending.json",
            &serde_json::to_vec_pretty(&jobs).expect("jobs json"),
        )
        .expect("write jobs");

    let raw = platform
        .state_fs()
        .read("memory/governance_jobs/pending.json")
        .expect("read jobs")
        .expect("pending jobs");
    let jobs: Vec<DeferredGovernanceJob> = serde_json::from_slice(&raw).expect("jobs json");
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().any(|job| {
        job.memory_space_id == default_memory_space_id("owner-a")
            && job.subject_id == "subject-a"
            && job.idempotency_key
                == "space:owner-a:subject-a:llm.gateway:shared-chat:turn-deferred-1"
    }));
    assert!(jobs.iter().any(|job| {
        job.memory_space_id == default_memory_space_id("owner-b")
            && job.subject_id == "subject-b"
            && job.idempotency_key
                == "space:owner-b:subject-b:llm.gateway:shared-chat:turn-deferred-1"
    }));

    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response("Summary: only one scoped job processed");
    let report = runtime_a
        .run_due_governance(
            &mut http,
            Some(&llm),
            MemoryDeferredGovernanceRunRequest { limit: 8 },
        )
        .expect("runtime a due governance");
    assert_eq!(report.attempted, 1);
    assert_eq!(report.succeeded, 1);
    assert_eq!(report.remaining_pending, 0);
    assert_eq!(report.queue.terminal, 1);
    assert_eq!(report.queue.pending, 0);
    assert!(report
        .queue
        .recent_jobs
        .iter()
        .all(
            |job| job.memory_space_id == default_memory_space_id("owner-a")
                && job.subject_id == "subject-a"
        ));

    let raw = platform
        .state_fs()
        .read("memory/governance_jobs/pending.json")
        .expect("read jobs")
        .expect("pending jobs");
    let jobs: Vec<DeferredGovernanceJob> = serde_json::from_slice(&raw).expect("jobs json");
    let job_a = jobs
        .iter()
        .find(|job| job.memory_space_id == default_memory_space_id("owner-a"))
        .expect("owner a job");
    let job_b = jobs
        .iter()
        .find(|job| job.memory_space_id == default_memory_space_id("owner-b"))
        .expect("owner b job");
    assert_eq!(job_a.status, DeferredGovernanceJobStatus::Terminal);
    assert_eq!(job_b.status, DeferredGovernanceJobStatus::Pending);

    let queue_b = runtime_b
        .deferred_governance_report()
        .expect("runtime b queue report");
    assert_eq!(queue_b.pending, 1);
    assert_eq!(queue_b.terminal, 0);
    assert_eq!(
        queue_b.recent_jobs[0].memory_space_id,
        default_memory_space_id("owner-b")
    );
    assert_eq!(queue_b.recent_jobs[0].subject_id, "subject-b");
}
