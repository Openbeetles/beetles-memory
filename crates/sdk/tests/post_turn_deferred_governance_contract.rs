#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::platform::Platform as _;
use bm_sdk::{
    CanonicalTurnDelta, ConversationScope, MemoryConsolidationState,
    MemoryGovernanceActiveJobsRequest, MemoryGovernanceAttemptAuthorityRequest,
    MemoryGovernanceJobClaimRequest, MemoryGovernanceJobRenewRequest,
    MemoryGovernanceJobRetryRequest, MemoryGovernanceJobRunRequest,
    MemoryGovernanceJobStatusRequest, MemoryGovernanceReconcileRequest, MemoryPrivacyPolicy,
    MemoryStoreHandle, MemoryTranscriptCommitRequest, MemoryTranscriptLifecycleRequest,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    Message, PostTurnGovernanceAttemptAuthorityV2, PostTurnGovernanceErrorClassV2,
    PostTurnGovernanceJobStatusV2, PostTurnGovernanceJobV2, PressureLevel, ResponseBody,
    RuntimeLifecycleModeInput, StopReason, StoreBackendConfig, ToolChoicePolicy, ToolSpec,
    TranscriptInputMessage, TranscriptLifecycleTransition,
};
use bm_sdk::{LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, MemoryRuntime, Result};

use support::{
    empty_store_platform, test_runtime_with_identity_scope_and_subject,
    test_runtime_with_scope_and_subject, test_runtime_with_scope_subject_and_privacy,
    StaticHttpClient,
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

fn finalize_request(
    subject: &str,
    conversation_id: &str,
    turn_id: &str,
    user: &str,
) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn_id.to_string(),
            conversation: ConversationScope {
                channel: "llm.gateway".to_string(),
                chat_id: "chat-a".to_string(),
                conversation_id: Some(conversation_id.to_string()),
            },
            subject: subject.to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            actor: None,
            input_messages: vec![TranscriptInputMessage::user(user)],
            assistant_message: Some(TranscriptInputMessage::assistant("已收到。")),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: vec!["candidate-a".to_string()],
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

fn attempt_authority() -> PostTurnGovernanceAttemptAuthorityV2 {
    PostTurnGovernanceAttemptAuthorityV2 {
        binding_id: "governance-model:test".to_string(),
        config_revision: 1,
        model_id: "qwen3:8b".to_string(),
        privacy_revision: 1,
        privacy_digest: "sha256:3333333333333333333333333333333333333333333333333333333333333333"
            .to_string(),
        transcript_lifecycle_revision: 1,
        disclosure_authority_digest:
            "sha256:4444444444444444444444444444444444444444444444444444444444444444".to_string(),
    }
}

fn attempt_authority_for_job(
    job: &PostTurnGovernanceJobV2,
) -> PostTurnGovernanceAttemptAuthorityV2 {
    PostTurnGovernanceAttemptAuthorityV2 {
        privacy_revision: job.pinned_privacy_revision,
        privacy_digest: job.pinned_privacy_digest.clone(),
        ..attempt_authority()
    }
}

#[derive(Default)]
struct CountingHttpClient {
    calls: AtomicUsize,
}

impl LlmHttpClient for CountingHttpClient {
    fn do_post(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok((200, ResponseBody::Heap(Vec::new())))
    }
}

struct LifecycleRevokingLlm {
    runtime: Arc<MemoryRuntime>,
    revoked: AtomicBool,
}

struct ConflictInjectingLlm {
    store: MemoryStoreHandle,
    calls: AtomicUsize,
}

struct NetworkAttemptingLlm;

struct PlaneAwareStaticLlm {
    long_term_content: &'static str,
}

impl LlmClient for PlaneAwareStaticLlm {
    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: if system.contains("private garden") {
                "null".to_string()
            } else {
                self.long_term_content.to_string()
            },
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for NetworkAttemptingLlm {
    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let _ = http.do_post("http://model.invalid", &[], b"private transcript")?;
        Ok(LlmResponse {
            content: "null".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for ConflictInjectingLlm {
    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let content = if call == 0 {
            r#"{"writes":[{"path":"journal/conflict.md","content":"planned private value"}]}"#
                .to_string()
        } else {
            self.store.replay_harness().seed_private_garden_doc(
                "subject-default",
                "journal/conflict.md",
                "concurrent winner",
                1_800_000_001,
            )?;
            r#"[
                {
                    "plane": "factual",
                    "op": "upsert",
                    "kind": "profile",
                    "source_authority": "user_asserted",
                    "topic": "atomic_conflict_probe",
                    "content": "This semantic write must not partially commit.",
                    "keywords": ["atomic conflict"]
                }
            ]"#
            .to_string()
        };
        Ok(LlmResponse {
            content,
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

impl LlmClient for LifecycleRevokingLlm {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        if !self.revoked.swap(true, Ordering::SeqCst) {
            self.runtime
                .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
                    memory_space_id: self.runtime.memory_space_id().to_string(),
                    channel_id: "llm.gateway".to_string(),
                    conversation_id: "window-a".to_string(),
                    turn_id: Some("turn-egress-revoked".to_string()),
                    transition: TranscriptLifecycleTransition::Mask,
                    reason: "owner revoked before egress".to_string(),
                })?;
        }
        http.do_post("http://model.invalid/v1/chat/completions", &[], b"{}")?;
        Ok(LlmResponse {
            content: "[]".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

fn persistent_test_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "bm-sdk-post-turn-governance-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

fn assert_independent_open_claim(config: StoreBackendConfig) {
    let profile = support::host_test_profile();
    let first_store = support::open_memory_store(config.clone()).expect("first open");
    let second_store = support::open_memory_store(config).expect("second open");
    let first = Arc::new(test_runtime_with_scope_and_subject(
        first_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let second = Arc::new(test_runtime_with_scope_and_subject(
        second_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let finalized = first
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "turn-persistent", "持久并发"),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let barrier = Arc::new(Barrier::new(3));
    let workers = [
        (first, "persistent-worker-a"),
        (second, "persistent-worker-b"),
    ]
    .into_iter()
    .map(|(runtime, worker)| {
        let barrier = Arc::clone(&barrier);
        let job_id = job_id.clone();
        std::thread::spawn(move || {
            barrier.wait();
            runtime.claim_governance_job(MemoryGovernanceJobClaimRequest {
                job_id,
                lease_owner: worker.to_string(),
                lease_until: 1_800_000_060,
                authority: attempt_authority(),
            })
        })
    })
    .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
}

fn assert_persistent_atomic_completion(config: StoreBackendConfig) {
    let profile = support::host_test_profile();
    let first_store = support::open_memory_store(config.clone()).expect("first open");
    let first = test_runtime_with_scope_and_subject(
        first_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = first
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-persistent-complete",
                "持久化原子完成",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = first
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = first
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:persistent-complete".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let mut http = StaticHttpClient;
    let llm = PlaneAwareStaticLlm {
        long_term_content: r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "profile",
                "source_authority": "user_asserted",
                "topic": "persistent_atomic_completion",
                "content": "Persistent atomic governance completed.",
                "keywords": ["persistent", "atomic"]
            }
        ]"#,
    };
    let request = MemoryGovernanceJobRunRequest {
        job_id: job_id.clone(),
        lease_owner: "worker:persistent-complete".to_string(),
        lease_epoch: leased.lease_epoch,
    };
    let completed = first
        .run_claimed_governance(&mut http, Some(&llm), request.clone())
        .expect("complete");
    let receipt = completed.job.receipt.clone().expect("receipt");

    let reopened_store = support::open_memory_store(config).expect("reopen");
    let reopened_snapshot_store = reopened_store.clone();
    let reopened = test_runtime_with_scope_and_subject(
        reopened_store,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let reopened_status = reopened
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("reopened status")
        .job;
    assert_eq!(
        reopened_status.status,
        PostTurnGovernanceJobStatusV2::Succeeded
    );
    assert_eq!(reopened_status.receipt.as_ref(), Some(&receipt));
    let duplicate = reopened
        .run_claimed_governance(&mut http, Some(&llm), request)
        .expect("reopened idempotent completion");
    assert_eq!(duplicate.job.receipt.as_ref(), Some(&receipt));
    let encoded = serde_json::to_string(
        &reopened_snapshot_store
            .replay_harness()
            .export_store_snapshot()
            .expect("reopened snapshot")
            .json_docs,
    )
    .expect("snapshot json");
    assert!(encoded.contains("Persistent atomic governance completed."));
}

#[test]
fn maintenance_unavailable_commits_transcript_and_durable_v2_intent_without_raw_copy() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );

    let report = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-deferred-1",
                "叫我青川，这句话不得复制进任务",
            ),
        )
        .expect("finalize");
    assert!(report.session_commit.committed);
    assert!(report
        .transcript_commit
        .as_ref()
        .is_some_and(|commit| commit.committed));
    assert_eq!(
        report.memory_consolidation.state,
        MemoryConsolidationState::Queued
    );
    let job_id = report
        .memory_consolidation
        .job_id
        .as_deref()
        .expect("stable job id");
    let status = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.to_string(),
        })
        .expect("exact job status");
    assert_eq!(status.job.status, PostTurnGovernanceJobStatusV2::Pending);
    assert_eq!(status.job.identity.conversation_id, "window-a");

    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    let governance_docs = snapshot
        .json_docs
        .iter()
        .filter(|doc| {
            matches!(
                doc.namespace.as_str(),
                "post_turn_governance_jobs" | "post_turn_governance_scope_indexes"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(governance_docs.len(), 2);
    let encoded = serde_json::to_string(&governance_docs).expect("governance docs");
    assert!(!encoded.contains("叫我青川"));
    assert!(!encoded.contains("不得复制进任务"));
    assert!(!encoded.contains("inputMessages"));
}

#[test]
fn bounded_reconciliation_repairs_transcript_intent_gaps_and_advances_exact_cursor() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    for (turn_id, content) in [
        ("turn-reconcile-1", "第一条已提交但缺 intent"),
        ("turn-reconcile-2", "第二条已提交但缺 intent"),
    ] {
        runtime
            .commit_transcript(MemoryTranscriptCommitRequest {
                turn: finalize_request("subject-default", "window-a", turn_id, content).turn,
                host_refs: Vec::new(),
            })
            .expect("transcript-only commit");
    }

    let first = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 1 })
        .expect("first bounded reconciliation");
    assert_eq!(first.inspected, 1);
    assert_eq!(first.created, 1);
    assert_eq!(first.cursor_sequence, 1);
    assert!(first.has_more);

    let second = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 1 })
        .expect("second bounded reconciliation");
    assert_eq!(second.inspected, 1);
    assert_eq!(second.created, 1);
    assert_eq!(second.cursor_sequence, 2);
    assert!(!second.has_more);

    let exhausted = runtime
        .reconcile_governance_intents(MemoryGovernanceReconcileRequest { limit: 1 })
        .expect("exhausted reconciliation");
    assert_eq!(exhausted.inspected, 0);
    assert_eq!(exhausted.created, 0);
    assert_eq!(exhausted.cursor_sequence, 2);
    assert!(!exhausted.has_more);
    let queue = runtime
        .deferred_governance_report()
        .expect("exact queue report");
    assert_eq!(queue.pending, 2);
}

#[test]
fn active_lease_renewal_is_fenced_by_owner_epoch_and_strict_deadline() {
    let profile = support::host_test_profile();
    let runtime = test_runtime_with_scope_and_subject(
        empty_store_platform(profile),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-renew",
                "续租必须受 fencing 约束",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:renew".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let renewed = runtime
        .renew_governance_job_lease(MemoryGovernanceJobRenewRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:renew".to_string(),
            lease_epoch: leased.lease_epoch,
            lease_until: 1_800_000_120,
        })
        .expect("renew")
        .job;
    assert_eq!(renewed.lease_epoch, leased.lease_epoch);
    assert_eq!(renewed.lease_until, Some(1_800_000_120));
    assert!(renewed.state_revision > leased.state_revision);

    let stale_owner = runtime
        .renew_governance_job_lease(MemoryGovernanceJobRenewRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:other".to_string(),
            lease_epoch: leased.lease_epoch,
            lease_until: 1_800_000_180,
        })
        .expect_err("another worker must not renew");
    assert_eq!(stale_owner.stage(), "post_turn_governance_renew");
    let non_extending = runtime
        .renew_governance_job_lease(MemoryGovernanceJobRenewRequest {
            job_id,
            lease_owner: "worker:renew".to_string(),
            lease_epoch: leased.lease_epoch,
            lease_until: 1_800_000_120,
        })
        .expect_err("renew must strictly extend");
    assert_eq!(non_extending.stage(), "post_turn_governance_renew");
}

#[test]
fn legacy_raw_turn_queue_requires_explicit_reset_and_is_never_auto_consumed() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    platform
        .replay_harness()
        .state_fs()
        .write(
            "memory/governance_jobs/pending.json",
            br#"[{"turn":{"raw":"must-not-migrate"}}]"#,
        )
        .expect("seed legacy queue");
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let error = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-legacy-reset",
                "不得迁移旧队列",
            ),
        )
        .err()
        .expect("legacy queue must block V2 worker readiness");
    assert!(error
        .to_string()
        .contains("legacy_governance_queue_reset_required"));
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert!(snapshot
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "post_turn_governance_jobs"));
    assert_eq!(
        platform
            .replay_harness()
            .state_fs()
            .read("memory/governance_jobs/pending.json")
            .expect("read legacy queue")
            .as_deref(),
        Some(br#"[{"turn":{"raw":"must-not-migrate"}}]"#.as_slice())
    );
}

#[test]
fn duplicate_finalize_repairs_or_reuses_one_exact_intent() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let request = finalize_request("subject-default", "window-a", "turn-deferred-1", "同一内容");
    let first = runtime
        .finalize_turn_with_inline_governance(None, None, request.clone())
        .expect("first finalize");
    let second = runtime
        .finalize_turn_with_inline_governance(None, None, request)
        .expect("duplicate finalize");
    assert_eq!(
        first.memory_consolidation.job_id,
        second.memory_consolidation.job_id
    );
    assert_eq!(
        second.memory_consolidation.reason,
        "governance_intent_already_present"
    );
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert_eq!(
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == "post_turn_governance_jobs")
            .count(),
        1
    );
}

#[test]
fn transcript_mask_cancels_pending_governance_before_claim() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-private",
                "这段内容已经撤回",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");

    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "window-a".to_string(),
            turn_id: Some("turn-private".to_string()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "owner withdrew model use".to_string(),
        })
        .expect("mask transcript");

    let status = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("job status");
    assert_eq!(status.job.status, PostTurnGovernanceJobStatusV2::Cancelled);
    assert_eq!(
        status.job.blocking_reason.as_deref(),
        Some("transcript_masked")
    );
    assert!(runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id,
            lease_owner: "worker-after-mask".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority(),
        })
        .is_err());
}

#[test]
fn same_turn_id_in_different_conversations_has_distinct_jobs() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let first = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "same-turn", "第一段"),
        )
        .expect("first conversation");
    let second = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-b", "same-turn", "第二段"),
        )
        .expect("second conversation");
    assert_ne!(
        first.memory_consolidation.job_id,
        second.memory_consolidation.job_id
    );
    let active = runtime
        .active_governance_jobs(MemoryGovernanceActiveJobsRequest { limit: 8 })
        .expect("runtime-scope active jobs");
    assert_eq!(active.jobs.len(), 2);
    assert!(!active.has_more);
    assert_eq!(
        active.jobs[0].scope_index_key,
        active.jobs[1].scope_index_key
    );
    assert_eq!(
        active
            .jobs
            .iter()
            .map(|job| job.identity.conversation_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["window-a", "window-b"])
    );
}

#[test]
fn concurrent_claim_has_one_winner_and_pins_first_attempt_authority() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let first = Arc::new(test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let second = Arc::new(test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let finalized = first
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "turn-claim", "并发领取"),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let barrier = Arc::new(Barrier::new(3));
    let workers = [(first, "worker-a"), (second, "worker-b")]
        .into_iter()
        .map(|(runtime, worker)| {
            let barrier = Arc::clone(&barrier);
            let job_id = job_id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                runtime.claim_governance_job(MemoryGovernanceJobClaimRequest {
                    job_id,
                    lease_owner: worker.to_string(),
                    lease_until: 1_800_000_060,
                    authority: attempt_authority(),
                })
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let winner = results.into_iter().find_map(Result::ok).expect("winner");
    assert_eq!(winner.job.status, PostTurnGovernanceJobStatusV2::Leased);
    assert_eq!(winner.job.attempt_count, 1);
    assert_eq!(winner.job.lease_epoch, 1);
    assert_eq!(winner.job.attempt_authority, Some(attempt_authority()));
}

#[test]
fn independent_file_opens_still_allow_only_one_claim_winner() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("file-claim");
    {
        let config = StoreBackendConfig::file(&root, profile).expect("file config");
        assert_independent_open_claim(config);
    }
    std::fs::remove_dir_all(&root).expect("remove file claim store");
}

#[test]
fn file_reopen_preserves_atomic_memory_and_terminal_receipt() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("file-complete");
    {
        let config = StoreBackendConfig::file(&root, profile).expect("file config");
        assert_persistent_atomic_completion(config);
    }
    std::fs::remove_dir_all(&root).expect("remove file completion store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn independent_sqlite_opens_still_allow_only_one_claim_winner() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("sqlite-claim");
    std::fs::create_dir_all(&root).expect("create sqlite claim root");
    {
        let config = StoreBackendConfig::sqlite(root.join("memory.sqlite3"), profile)
            .expect("sqlite config");
        assert_independent_open_claim(config);
    }
    std::fs::remove_dir_all(&root).expect("remove sqlite claim store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_reopen_preserves_atomic_memory_and_terminal_receipt() {
    let profile = support::host_test_profile();
    let root = persistent_test_root("sqlite-complete");
    std::fs::create_dir_all(&root).expect("create sqlite completion root");
    {
        let config = StoreBackendConfig::sqlite(root.join("memory.sqlite3"), profile)
            .expect("sqlite config");
        assert_persistent_atomic_completion(config);
    }
    std::fs::remove_dir_all(&root).expect("remove sqlite completion store");
}

#[test]
fn retry_requires_current_lease_and_enforces_runtime_backoff() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-default", "window-a", "turn-retry", "稍后重试"),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker-a".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority(),
        })
        .expect("claim")
        .job;

    assert!(runtime
        .retry_governance_job(MemoryGovernanceJobRetryRequest {
            job_id: job_id.clone(),
            lease_owner: "stale-worker".to_string(),
            lease_epoch: leased.lease_epoch,
            error_class: PostTurnGovernanceErrorClassV2::ServiceUnavailable,
        })
        .is_err());
    let retry = runtime
        .retry_governance_job(MemoryGovernanceJobRetryRequest {
            job_id: job_id.clone(),
            lease_owner: "worker-a".to_string(),
            lease_epoch: leased.lease_epoch,
            error_class: PostTurnGovernanceErrorClassV2::ServiceUnavailable,
        })
        .expect("schedule retry")
        .job;
    assert_eq!(retry.status, PostTurnGovernanceJobStatusV2::RetryWaiting);
    assert_eq!(retry.next_attempt_at, Some(1_800_000_005));
    assert_eq!(
        retry.last_error_class,
        Some(PostTurnGovernanceErrorClassV2::ServiceUnavailable)
    );
    assert!(runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id,
            lease_owner: "worker-b".to_string(),
            lease_until: 1_800_000_120,
            authority: attempt_authority(),
        })
        .is_err());
}

#[test]
fn exact_job_query_rejects_another_memory_space_and_subject() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_identity_scope_and_subject(
        platform.clone(),
        profile,
        "agent-a",
        "owner-a",
        "subject-a",
        "llm.gateway",
        "chat-a",
    );
    let runtime_b = test_runtime_with_identity_scope_and_subject(
        platform,
        profile,
        "agent-b",
        "owner-b",
        "subject-b",
        "llm.gateway",
        "chat-a",
    );
    let finalized = runtime_a
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request("subject-a", "window-a", "turn-a", "主体隔离"),
        )
        .expect("finalize a");
    let error = runtime_b
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: finalized.memory_consolidation.job_id.expect("job id"),
        })
        .expect_err("foreign runtime must fail closed");
    assert!(error.to_string().contains("mounted runtime"));
}

#[test]
fn due_query_does_not_starve_pending_work_behind_future_retries() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    for index in 0..32 {
        let finalized = runtime
            .finalize_turn_with_inline_governance(
                None,
                None,
                finalize_request(
                    "subject-default",
                    &format!("future-conversation-{index}"),
                    &format!("future-turn-{index}"),
                    "稍后重试的任务",
                ),
            )
            .expect("future retry finalize");
        let job_id = finalized.memory_consolidation.job_id.expect("job id");
        let pending = runtime
            .governance_job_status(MemoryGovernanceJobStatusRequest {
                job_id: job_id.clone(),
            })
            .expect("pending")
            .job;
        let leased = runtime
            .claim_governance_job(MemoryGovernanceJobClaimRequest {
                job_id: job_id.clone(),
                lease_owner: format!("worker:future-{index}"),
                lease_until: 1_800_000_060,
                authority: attempt_authority_for_job(&pending),
            })
            .expect("claim future retry")
            .job;
        runtime
            .retry_governance_job(MemoryGovernanceJobRetryRequest {
                job_id,
                lease_owner: format!("worker:future-{index}"),
                lease_epoch: leased.lease_epoch,
                error_class: PostTurnGovernanceErrorClassV2::ServiceUnavailable,
            })
            .expect("schedule future retry");
    }
    let pending = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "due-conversation",
                "due-turn",
                "现在应当执行的任务",
            ),
        )
        .expect("due finalize");
    let pending_job_id = pending.memory_consolidation.job_id.expect("pending job id");

    let active = runtime
        .active_governance_jobs(MemoryGovernanceActiveJobsRequest { limit: 32 })
        .expect("bounded active jobs");
    assert!(active.has_more);
    assert!(
        active.jobs.iter().any(|job| job.job_id == pending_job_id),
        "actionable pending work must be selected ahead of future retries"
    );
}

#[test]
fn claimed_governance_commits_memory_and_terminal_receipt_once() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-atomic-learning",
                "以后叫我青川",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending status")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:atomic-learning".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let mut http = StaticHttpClient;
    let llm = PlaneAwareStaticLlm {
        long_term_content: r#"[
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
    };
    let request = MemoryGovernanceJobRunRequest {
        job_id: job_id.clone(),
        lease_owner: "worker:atomic-learning".to_string(),
        lease_epoch: leased.lease_epoch,
    };
    let completed = runtime
        .run_claimed_governance(&mut http, Some(&llm), request.clone())
        .expect("atomic governance run");
    assert_eq!(
        completed.job.status,
        PostTurnGovernanceJobStatusV2::Succeeded
    );
    assert!(completed.job.receipt.is_some());
    assert_eq!(completed.semantic_governance.accepted_count, 1);
    let queue = runtime
        .deferred_governance_report()
        .expect("terminal queue report");
    assert_eq!(queue.pending, 0);
    assert_eq!(queue.terminal, 1);
    assert_eq!(queue.recent_jobs[0].job_id, job_id);
    let first_snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("first snapshot");
    assert!(serde_json::to_string(&first_snapshot.json_docs)
        .expect("snapshot json")
        .contains("Qingchuan"));

    let duplicate = runtime
        .run_claimed_governance(&mut http, Some(&llm), request)
        .expect("idempotent governance run");
    assert_eq!(duplicate.job.receipt, completed.job.receipt);
    let second_snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("second snapshot");
    assert_eq!(second_snapshot.json_docs, first_snapshot.json_docs);
}

#[test]
fn transcript_revocation_after_prompt_assembly_blocks_first_network_byte_and_memory_commit() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = Arc::new(test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    ));
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-egress-revoked",
                "这段内容不得发给模型",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:egress".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let llm = LifecycleRevokingLlm {
        runtime: Arc::clone(&runtime),
        revoked: AtomicBool::new(false),
    };
    let mut http = CountingHttpClient::default();
    let error = runtime
        .run_claimed_governance(
            &mut http,
            Some(&llm),
            MemoryGovernanceJobRunRequest {
                job_id: job_id.clone(),
                lease_owner: "worker:egress".to_string(),
                lease_epoch: leased.lease_epoch,
            },
        )
        .expect_err("revoked disclosure must fail closed");
    assert!(error
        .to_string()
        .contains("network disclosure lease is stale"));
    assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    let status = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("cancelled status")
        .job;
    assert_eq!(status.status, PostTurnGovernanceJobStatusV2::Cancelled);
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert!(snapshot
        .json_docs
        .iter()
        .all(|doc| doc.namespace != "private_garden" && doc.namespace != "long_term"));
}

#[test]
fn stricter_runtime_privacy_after_claim_blocks_first_network_byte() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let original = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = original
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-privacy-tightened",
                "这段内容只允许在原隐私策略下处理",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let authority = original
        .prepare_governance_attempt_authority(MemoryGovernanceAttemptAuthorityRequest {
            job_id: job_id.clone(),
            binding_id: "governance-model:test".to_string(),
            config_revision: 1,
            model_id: "qwen3:8b".to_string(),
        })
        .expect("original privacy authority")
        .authority;
    let leased = original
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:privacy-tightened".to_string(),
            lease_until: 1_800_000_060,
            authority,
        })
        .expect("claim")
        .job;

    let mut stricter = MemoryPrivacyPolicy::standard_private_boundary();
    stricter.governance_model_disclosure_allowed = false;
    let tightened = test_runtime_with_scope_subject_and_privacy(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
        stricter,
    );
    let mut http = CountingHttpClient::default();
    let error = tightened
        .run_claimed_governance(
            &mut http,
            Some(&NetworkAttemptingLlm),
            MemoryGovernanceJobRunRequest {
                job_id: job_id.clone(),
                lease_owner: "worker:privacy-tightened".to_string(),
                lease_epoch: leased.lease_epoch,
            },
        )
        .expect_err("tightened privacy must revoke disclosure");
    assert!(error.to_string().contains("privacy authority"));
    assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    let status = tightened
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("job status")
        .job;
    assert_eq!(status.status, PostTurnGovernanceJobStatusV2::Leased);
    assert!(status.receipt.is_none());
}

#[test]
fn memory_precondition_conflict_keeps_job_leased_and_commits_no_semantic_post_image() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalized = runtime
        .finalize_turn_with_inline_governance(
            None,
            None,
            finalize_request(
                "subject-default",
                "window-a",
                "turn-atomic-conflict",
                "验证原子冲突",
            ),
        )
        .expect("finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let pending = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("pending")
        .job;
    let leased = runtime
        .claim_governance_job(MemoryGovernanceJobClaimRequest {
            job_id: job_id.clone(),
            lease_owner: "worker:conflict".to_string(),
            lease_until: 1_800_000_060,
            authority: attempt_authority_for_job(&pending),
        })
        .expect("claim")
        .job;
    let llm = ConflictInjectingLlm {
        store: platform.clone(),
        calls: AtomicUsize::new(0),
    };
    let mut http = StaticHttpClient;
    runtime
        .run_claimed_governance(
            &mut http,
            Some(&llm),
            MemoryGovernanceJobRunRequest {
                job_id: job_id.clone(),
                lease_owner: "worker:conflict".to_string(),
                lease_epoch: leased.lease_epoch,
            },
        )
        .expect_err("stale private post image must reject the whole completion transaction");
    let current = runtime
        .governance_job_status(MemoryGovernanceJobStatusRequest { job_id })
        .expect("leased status")
        .job;
    assert_eq!(current.status, PostTurnGovernanceJobStatusV2::Leased);
    assert!(current.receipt.is_none());
    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    let encoded = serde_json::to_string(&snapshot.json_docs).expect("snapshot json");
    assert!(encoded.contains("concurrent winner"));
    assert!(!encoded.contains("This semantic write must not partially commit."));
}
