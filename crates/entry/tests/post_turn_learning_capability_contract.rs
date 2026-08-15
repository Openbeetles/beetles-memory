#![cfg(not(feature = "governance-model-client-std"))]

mod support;

use std::time::{Duration, Instant};

use bm_entry::{
    EntryAuthConfig, EntryGovernanceCoordinatorState, EntryIdempotencyConfig, EntryIdentity,
    EntryRuntime, EntryRuntimeConfig, EntryScope, EntryTransportConfig,
};
use bm_sdk::{
    CanonicalTurnDelta, ConversationScope, MemoryCapabilityPolicy, MemoryPrivacyPolicy,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    PostTurnGovernanceJobStatusV2, PressureLevel, RuntimeLifecycleModeInput, StoreBackendConfig,
    TranscriptInputMessage,
};

fn runtime_without_model_client() -> EntryRuntime {
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "capability-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "capability-contract".to_string(),
            chat_id: "chat-a".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::host_production_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_disabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability: MemoryCapabilityPolicy::strict_profile(),
    })
    .expect("entry runtime")
}

fn finalize_request() -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: "turn-capability".to_string(),
            conversation: ConversationScope {
                channel: "capability-contract".to_string(),
                chat_id: "chat-a".to_string(),
                conversation_id: Some("conversation-capability".to_string()),
            },
            subject: "agent:capability-agent".to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: bm_sdk::IngressKind::User,
                channel: "capability-contract".to_string(),
                provider: None,
                protocol: MemoryTurnProtocol::Native,
                endpoint: Some("capability-contract".to_string()),
                model_alias: None,
                model_resolved: None,
                request_id: Some("request-capability".to_string()),
                client_conversation_hint: Some("conversation-capability".to_string()),
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user("请记住这条能力阻断测试。")],
            assistant_message: Some(TranscriptInputMessage::assistant("已收到。")),
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

#[test]
fn missing_compiled_model_client_durably_blocks_the_job_without_hot_looping() {
    let runtime = runtime_without_model_client();
    let finalized = runtime
        .runtime()
        .finalize_turn(finalize_request())
        .expect("queued finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    // This test calls the SDK facade directly, so it intentionally exercises the
    // coordinator's bounded five-second reconciliation poll rather than Entry's
    // request-path wake signal.
    let deadline = Instant::now() + Duration::from_secs(7);
    loop {
        let job = runtime
            .runtime()
            .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest {
                job_id: job_id.clone(),
            })
            .expect("job status")
            .job;
        if job.status == PostTurnGovernanceJobStatusV2::BlockedCapability {
            assert_eq!(
                job.blocking_reason.as_deref(),
                Some("governance_model_client_not_compiled")
            );
            break;
        }
        assert!(Instant::now() < deadline, "job remained {:?}", job.status);
        std::thread::sleep(Duration::from_millis(10));
    }
    let blocked = runtime.governance_coordinator_report();
    assert_eq!(
        blocked.state,
        EntryGovernanceCoordinatorState::BlockedCapability
    );
    assert_eq!(blocked.blocked_jobs, 1);
    std::thread::sleep(Duration::from_millis(200));
    let settled = runtime.governance_coordinator_report();
    assert_eq!(settled.blocked_jobs, blocked.blocked_jobs);
    assert!(settled.cycles <= blocked.cycles.saturating_add(1));
}
