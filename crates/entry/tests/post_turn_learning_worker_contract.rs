#![cfg(feature = "governance-model-client-std")]

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bm_entry::{
    EntryAuthConfig, EntryGovernanceCoordinatorState, EntryGovernanceModelAuthMode,
    EntryGovernanceModelConfigUpdate, EntryGovernanceModelProtocol, EntryIdempotencyConfig,
    EntryIdentity, EntryRuntime, EntryRuntimeConfig, EntryScope, EntryTransportConfig,
};
use bm_sdk::{
    CanonicalTurnDelta, ConversationScope, MemoryCapabilityPolicy, MemoryPrivacyPolicy,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    PostTurnGovernanceJobStatusV2, PressureLevel, RuntimeLifecycleModeInput, StoreBackendConfig,
    TranscriptInputMessage,
};

fn temp_store(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("bm-entry-pl1c-{label}-{nanos}"))
}

fn runtime(path: &std::path::Path) -> EntryRuntime {
    let profile = support::host_production_profile();
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "pl1c-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "external-ai".to_string(),
            chat_id: "chat-a".to_string(),
        },
        store: StoreBackendConfig::file(path, profile)
            .expect("file store")
            .with_fsync(false),
        transports: EntryTransportConfig::all_disabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability: MemoryCapabilityPolicy::strict_profile(),
    })
    .expect("entry runtime")
}

fn finalize_request(conversation_id: &str, turn_id: &str) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn_id.to_string(),
            conversation: ConversationScope {
                channel: "external-ai".to_string(),
                chat_id: "chat-a".to_string(),
                conversation_id: Some(conversation_id.to_string()),
            },
            subject: "agent:pl1c-agent".to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: bm_sdk::IngressKind::User,
                channel: "external-ai".to_string(),
                provider: None,
                protocol: MemoryTurnProtocol::Native,
                endpoint: Some("/memory/v1/command".to_string()),
                model_alias: None,
                model_resolved: None,
                request_id: Some(format!("request-{turn_id}")),
                client_conversation_hint: Some(conversation_id.to_string()),
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user("请记住我喜欢冷萃咖啡。")],
            assistant_message: Some(TranscriptInputMessage::assistant("记下了。")),
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

fn wait_for_job(runtime: &EntryRuntime, job_id: &str, expected: PostTurnGovernanceJobStatusV2) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let status = runtime
            .runtime()
            .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest {
                job_id: job_id.to_string(),
            })
            .expect("job status")
            .job
            .status;
        if status == expected {
            return;
        }
        assert!(Instant::now() < deadline, "timed out at status {status:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct FakeOpenAiServer {
    address: String,
    calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl FakeOpenAiServer {
    fn start() -> Self {
        Self::start_with_options(0, Duration::ZERO, false, false)
    }

    fn start_with_behavior(failures: usize, response_delay: Duration) -> Self {
        Self::start_with_options(failures, response_delay, false, false)
    }

    fn start_with_malformed_long_term_output() -> Self {
        Self::start_with_options(0, Duration::ZERO, true, false)
    }

    fn start_with_auth_rejection() -> Self {
        Self::start_with_options(0, Duration::ZERO, false, true)
    }

    fn start_with_options(
        failures: usize,
        response_delay: Duration,
        malformed_long_term_output: bool,
        auth_rejected: bool,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake model");
        listener
            .set_nonblocking(true)
            .expect("nonblocking fake model");
        let address = listener.local_addr().expect("fake address").to_string();
        let calls = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_calls = Arc::clone(&calls);
        let thread_stop = Arc::clone(&stop);
        let failures_remaining = Arc::new(AtomicUsize::new(failures));
        let thread_failures = Arc::clone(&failures_remaining);
        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let body = read_http_body(&mut stream);
                        thread_calls.fetch_add(1, Ordering::SeqCst);
                        if response_delay != Duration::ZERO {
                            std::thread::sleep(response_delay);
                        }
                        if auth_rejected {
                            let response = "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
                            stream
                                .write_all(response.as_bytes())
                                .expect("fake auth rejection");
                            continue;
                        }
                        let should_fail = thread_failures
                            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                                remaining.checked_sub(1)
                            })
                            .is_ok();
                        if should_fail {
                            let response = "HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
                            stream.write_all(response.as_bytes()).expect("fake failure");
                            continue;
                        }
                        let content = if body.contains("private garden") {
                            "null"
                        } else if malformed_long_term_output {
                            "not-json"
                        } else {
                            "[]"
                        };
                        let response_body = serde_json::json!({
                            "choices": [{
                                "message": {"content": content},
                                "finish_reason": "stop"
                            }]
                        })
                        .to_string();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                            response_body.len(),
                            response_body
                        );
                        stream
                            .write_all(response.as_bytes())
                            .expect("fake response");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("fake model accept failed: {error}"),
                }
            }
        });
        Self {
            address,
            calls,
            stop,
            handle: Some(handle),
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}/v1", self.address)
    }
}

fn configure_model(runtime: &EntryRuntime, fake: &FakeOpenAiServer) {
    runtime
        .console_update_governance_model(EntryGovernanceModelConfigUpdate {
            enabled: true,
            protocol: EntryGovernanceModelProtocol::OpenAiCompatible,
            endpoint: fake.endpoint(),
            model: "fixture-memory-model".to_string(),
            auth_mode: EntryGovernanceModelAuthMode::LocalUnauthenticated,
            request_timeout_ms: 5_000,
            max_input_tokens: 8_192,
            max_output_tokens: 512,
        })
        .expect("configure governance model");
}

impl Drop for FakeOpenAiServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle.join().expect("fake model thread");
        }
    }
}

fn read_http_body(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout");
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let count = match stream.read(&mut buffer) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "fake request read timed out");
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(error) => panic!("fake request read failed: {error}"),
        };
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        if bytes.len() >= header_end + 4 + content_length {
            return String::from_utf8_lossy(&bytes[header_end + 4..]).to_string();
        }
    }
    String::new()
}

#[test]
fn authentication_rejection_blocks_configuration_without_retrying() {
    let root = temp_store("auth-rejected");
    let runtime = runtime(&root);
    let fake = FakeOpenAiServer::start_with_auth_rejection();
    configure_model(&runtime, &fake);
    let finalized = runtime
        .runtime()
        .finalize_turn(finalize_request("conversation-auth", "turn-auth"))
        .expect("queued finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");

    wait_for_job(
        &runtime,
        &job_id,
        PostTurnGovernanceJobStatusV2::BlockedConfiguration,
    );
    let job = runtime
        .runtime()
        .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("auth rejected job status")
        .job;
    assert_eq!(
        job.blocking_reason.as_deref(),
        Some("governance_model_authentication_rejected")
    );
    assert!(job.receipt.is_none());
    assert!(job.next_attempt_at.is_none());
    let report = runtime.governance_coordinator_report();
    assert_eq!(
        report.state,
        EntryGovernanceCoordinatorState::BlockedConfiguration
    );
    assert!(!report.service_ready);
    assert_eq!(report.retried_jobs, 0);
    assert_eq!(report.last_job_id.as_deref(), Some(job_id.as_str()));
    let calls = fake.calls.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(fake.calls.load(Ordering::SeqCst), calls);
}

#[test]
fn coordinator_blocks_without_config_then_recovers_and_completes_the_same_job() {
    let root = temp_store("recover-config");
    let runtime = runtime(&root);
    let finalized = runtime
        .runtime()
        .finalize_turn(finalize_request("conversation-a", "turn-a"))
        .expect("queued finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");

    wait_for_job(
        &runtime,
        &job_id,
        PostTurnGovernanceJobStatusV2::BlockedConfiguration,
    );
    let report_deadline = Instant::now() + Duration::from_secs(2);
    let blocked_report = loop {
        let report = runtime.governance_coordinator_report();
        if report.blocked_jobs > 0 {
            break report;
        }
        assert!(
            Instant::now() < report_deadline,
            "coordinator report did not observe the durable blocked transition"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    std::thread::sleep(Duration::from_millis(200));
    let settled_report = runtime.governance_coordinator_report();
    assert!(
        settled_report.cycles <= blocked_report.cycles.saturating_add(1),
        "an unchanged blocked job must not create a hot loop"
    );
    assert_eq!(settled_report.blocked_jobs, blocked_report.blocked_jobs);
    let fake = FakeOpenAiServer::start();
    configure_model(&runtime, &fake);

    wait_for_job(&runtime, &job_id, PostTurnGovernanceJobStatusV2::Succeeded);
    let report = runtime.governance_coordinator_report();
    assert_eq!(report.completed_jobs, 1);
    assert_eq!(report.last_job_id.as_deref(), Some(job_id.as_str()));
    assert!(fake.calls.load(Ordering::SeqCst) >= 1);
    runtime.shutdown_governance_coordinator();
    assert!(matches!(
        runtime.governance_coordinator_report().state,
        EntryGovernanceCoordinatorState::Stopped | EntryGovernanceCoordinatorState::Stopping
    ));
}

#[test]
fn transient_model_failure_uses_durable_backoff_before_retrying() {
    let root = temp_store("retry-backoff");
    let runtime = runtime(&root);
    let fake = FakeOpenAiServer::start_with_behavior(1, Duration::ZERO);
    configure_model(&runtime, &fake);
    let finalized = runtime
        .runtime()
        .finalize_turn(finalize_request("conversation-retry", "turn-retry"))
        .expect("queued finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");

    wait_for_job(
        &runtime,
        &job_id,
        PostTurnGovernanceJobStatusV2::RetryWaiting,
    );
    let calls_after_failure = fake.calls.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        fake.calls.load(Ordering::SeqCst),
        calls_after_failure,
        "retry must respect the durable next_attempt_at deadline"
    );
    wait_for_job(&runtime, &job_id, PostTurnGovernanceJobStatusV2::Succeeded);
    assert!(fake.calls.load(Ordering::SeqCst) > calls_after_failure);
    assert_eq!(runtime.governance_coordinator_report().retried_jobs, 1);
}

#[test]
fn malformed_long_term_output_is_retryable_instead_of_false_success() {
    let root = temp_store("malformed-output");
    let runtime = runtime(&root);
    let fake = FakeOpenAiServer::start_with_malformed_long_term_output();
    configure_model(&runtime, &fake);
    let finalized = runtime
        .runtime()
        .finalize_turn(finalize_request("conversation-malformed", "turn-malformed"))
        .expect("queued finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");

    wait_for_job(
        &runtime,
        &job_id,
        PostTurnGovernanceJobStatusV2::RetryWaiting,
    );
    let job = runtime
        .runtime()
        .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest { job_id })
        .expect("malformed output job status")
        .job;
    assert_eq!(
        job.last_error_class,
        Some(bm_sdk::PostTurnGovernanceErrorClassV2::MalformedModelOutput)
    );
    assert!(job.receipt.is_none());
    assert!(fake.calls.load(Ordering::SeqCst) >= 2);
}

#[test]
fn two_entry_coordinators_share_one_backend_claim_and_one_terminal_receipt() {
    let root = temp_store("single-winner");
    let first = runtime(&root);
    let fake = FakeOpenAiServer::start();
    configure_model(&first, &fake);
    let second = runtime(&root);
    let finalized = first
        .runtime()
        .finalize_turn(finalize_request("conversation-race", "turn-race"))
        .expect("queued finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");

    wait_for_job(&first, &job_id, PostTurnGovernanceJobStatusV2::Succeeded);
    assert_eq!(
        first
            .governance_coordinator_report()
            .completed_jobs
            .saturating_add(second.governance_coordinator_report().completed_jobs),
        1,
        "backend CAS must leave only one coordinator completion winner"
    );
    first.shutdown_governance_coordinator();
    second.shutdown_governance_coordinator();
    drop(first);
    drop(second);

    let reopened = runtime(&root);
    let status = reopened
        .runtime()
        .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest { job_id })
        .expect("reopened status")
        .job;
    assert_eq!(status.status, PostTurnGovernanceJobStatusV2::Succeeded);
    assert!(status.receipt.is_some());
}

#[test]
fn shutdown_during_model_call_prevents_post_shutdown_memory_completion() {
    let root = temp_store("shutdown");
    let runtime = runtime(&root);
    let fake = FakeOpenAiServer::start_with_behavior(0, Duration::from_secs(3));
    configure_model(&runtime, &fake);
    let finalized = runtime
        .runtime()
        .finalize_turn(finalize_request("conversation-stop", "turn-stop"))
        .expect("queued finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");
    let deadline = Instant::now() + Duration::from_secs(10);
    while fake.calls.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "model call did not start");
        std::thread::sleep(Duration::from_millis(10));
    }

    let shutdown_started = Instant::now();
    runtime.shutdown_governance_coordinator();
    assert!(shutdown_started.elapsed() < Duration::from_secs(3));
    std::thread::sleep(Duration::from_secs(2));
    let status = runtime
        .runtime()
        .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest { job_id })
        .expect("status after shutdown")
        .job;
    assert_eq!(status.status, PostTurnGovernanceJobStatusV2::RetryWaiting);
    assert!(status.receipt.is_none());
    let stopped_deadline = Instant::now() + Duration::from_secs(10);
    while runtime.governance_coordinator_report().state != EntryGovernanceCoordinatorState::Stopped
    {
        assert!(
            Instant::now() < stopped_deadline,
            "coordinator reaper did not reach stopped"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
