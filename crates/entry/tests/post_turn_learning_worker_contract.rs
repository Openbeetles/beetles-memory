#![cfg(feature = "governance-model-client-std")]

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bm_entry::{
    EntryAuthConfig, EntryGovernanceModelAuthMode, EntryGovernanceModelConfigUpdate,
    EntryGovernanceModelProtocol, EntryIdempotencyConfig, EntryIdentity, EntryRuntime,
    EntryRuntimeConfig, EntryScope, EntryTransportConfig, MemoryLearningAttachmentStatusRequest,
    MemoryLearningServiceStatusRequest,
};

fn service_report(runtime: &EntryRuntime) -> bm_entry::MemoryLearningServiceReport {
    let authority = runtime
        .learning_service_status_authority()
        .expect("service status authority");
    runtime
        .memory_learning_service_report(MemoryLearningServiceStatusRequest { authority })
        .expect("service status")
}

fn attachment_report(runtime: &EntryRuntime) -> bm_entry::MemoryLearningAttachmentReport {
    let authority = runtime
        .runtime()
        .learning_attachment_status_authority()
        .expect("attachment status authority");
    runtime
        .memory_learning_attachment_report(MemoryLearningAttachmentStatusRequest { authority })
        .expect("attachment status")
}
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
            conversation_id: None,
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
            assistant_messages: vec![bm_sdk::TranscriptAssistantMessage::complete(
                TranscriptInputMessage::assistant("记下了。"),
            )],
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        learning: bm_sdk::PostTurnLearningInputV3::empty(),
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

fn wait_for_completed_job_reports(runtimes: &[&EntryRuntime], expected: u64) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let completed = runtimes.iter().fold(0_u64, |total, runtime| {
            total.saturating_add(service_report(runtime).completed_jobs)
        });
        if completed >= expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "durable success was not reflected by the learning service report"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct FakeOpenAiServer {
    address: String,
    calls: Arc<AtomicUsize>,
    malformed_long_term_responses: Arc<AtomicUsize>,
    response_status: Arc<AtomicUsize>,
    persona_calls: Arc<AtomicUsize>,
    method_calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl FakeOpenAiServer {
    fn start() -> Self {
        Self::start_with_options(0, Duration::ZERO, false, false, false)
    }

    fn start_with_behavior(failures: usize, response_delay: Duration) -> Self {
        Self::start_with_options(failures, response_delay, false, false, false)
    }

    fn start_with_malformed_long_term_output() -> Self {
        Self::start_with_options(0, Duration::ZERO, true, false, false)
    }

    fn start_with_auth_rejection() -> Self {
        Self::start_with_options(0, Duration::ZERO, false, true, false)
    }

    fn start_with_permission_denial() -> Self {
        let server = Self::start_with_options(0, Duration::ZERO, false, false, false);
        server.response_status.store(403, Ordering::Release);
        server
    }

    fn start_with_learning() -> Self {
        Self::start_with_options(0, Duration::ZERO, false, false, true)
    }

    fn start_with_options(
        failures: usize,
        response_delay: Duration,
        malformed_long_term_output: bool,
        auth_rejected: bool,
        production_learning: bool,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake model");
        listener
            .set_nonblocking(true)
            .expect("nonblocking fake model");
        let address = listener.local_addr().expect("fake address").to_string();
        let calls = Arc::new(AtomicUsize::new(0));
        let malformed_long_term_responses = Arc::new(AtomicUsize::new(0));
        let response_status = Arc::new(AtomicUsize::new(if auth_rejected { 401 } else { 0 }));
        let persona_calls = Arc::new(AtomicUsize::new(0));
        let method_calls = Arc::new(AtomicUsize::new(0));
        let thread_persona_calls = Arc::clone(&persona_calls);
        let thread_method_calls = Arc::clone(&method_calls);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_calls = Arc::clone(&calls);
        let thread_malformed_responses = Arc::clone(&malformed_long_term_responses);
        let thread_response_status = Arc::clone(&response_status);
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
                        let forced_status = thread_response_status.load(Ordering::Acquire);
                        if matches!(forced_status, 401 | 403) {
                            let reason = if forced_status == 401 {
                                "Unauthorized"
                            } else {
                                "Forbidden"
                            };
                            let response = format!(
                                "HTTP/1.1 {forced_status} {reason}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                            );
                            stream
                                .write_all(response.as_bytes())
                                .expect("fake authorization rejection");
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
                        let content = if production_learning {
                            pl3_model_response(&body, &thread_persona_calls, &thread_method_calls)
                        } else if body.contains("private garden") {
                            "null".to_string()
                        } else if malformed_long_term_output
                            && body.contains("You extract durable memory updates")
                        {
                            thread_malformed_responses.fetch_add(1, Ordering::SeqCst);
                            "not-json".to_string()
                        } else {
                            "[]".to_string()
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
            malformed_long_term_responses,
            response_status,
            persona_calls,
            method_calls,
            stop,
            handle: Some(handle),
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}/v1", self.address)
    }

    fn allow_requests(&self) {
        self.response_status.store(0, Ordering::Release);
    }
}

fn configure_model(runtime: &EntryRuntime, fake: &FakeOpenAiServer) {
    configure_model_with_auth(
        runtime,
        fake,
        EntryGovernanceModelAuthMode::LocalUnauthenticated,
    );
}

fn configure_model_with_auth(
    runtime: &EntryRuntime,
    fake: &FakeOpenAiServer,
    auth_mode: EntryGovernanceModelAuthMode,
) {
    runtime
        .console_update_governance_model(EntryGovernanceModelConfigUpdate {
            enabled: true,
            protocol: EntryGovernanceModelProtocol::OpenAiCompatible,
            endpoint: fake.endpoint(),
            model: "fixture-memory-model".to_string(),
            auth_mode,
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
    const CREDENTIAL_ENV: &str = "BM_TEST_PL2_AUTH_REJECTED_TOKEN";
    std::env::set_var(CREDENTIAL_ENV, "synthetic-secret-never-logged");
    configure_model_with_auth(
        &runtime,
        &fake,
        EntryGovernanceModelAuthMode::CredentialEnv {
            credential_env: CREDENTIAL_ENV.to_string(),
        },
    );
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
        Some("governance_credential_rejected")
    );
    let block = job
        .execution_block_authority
        .as_ref()
        .expect("credential rejection must persist exact recovery authority");
    assert_eq!(
        block.typed_block_reason,
        bm_sdk::PostTurnGovernanceExecutionBlockReasonV1::CredentialRejected
    );
    assert_eq!(block.credential_generation, Some(1));
    assert!(block.credential_ref_safe_id.is_some());
    assert!(job.receipt.is_none());
    assert!(job.next_attempt_at.is_none());
    let report = service_report(&runtime);
    let attachment = attachment_report(&runtime);
    assert_eq!(report.blocked_jobs, 1);
    assert_eq!(report.retrying_jobs, 0);
    assert_eq!(attachment.state, "blocked");
    assert_eq!(attachment.last_job_id.as_deref(), Some(job_id.as_str()));
    let calls = fake.calls.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(fake.calls.load(Ordering::SeqCst), calls);
    assert_eq!(service_report(&runtime).blocked_jobs, 1);
    std::env::remove_var(CREDENTIAL_ENV);
}

#[test]
fn provider_permission_change_resumes_only_the_exact_blocked_binding_generation() {
    let root = temp_store("permission-recovery");
    let runtime = runtime(&root);
    let fake = FakeOpenAiServer::start_with_permission_denial();
    configure_model(&runtime, &fake);
    let finalized = runtime
        .runtime()
        .finalize_turn(finalize_request(
            "conversation-permission-recovery",
            "turn-permission-recovery",
        ))
        .expect("queued finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");

    wait_for_job(
        &runtime,
        &job_id,
        PostTurnGovernanceJobStatusV2::BlockedPolicy,
    );
    let blocked = runtime
        .runtime()
        .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("permission blocked status")
        .job;
    let block = blocked
        .execution_block_authority
        .as_ref()
        .expect("permission block authority");
    assert_eq!(
        block.typed_block_reason,
        bm_sdk::PostTurnGovernanceExecutionBlockReasonV1::ProviderPermissionDenied
    );
    assert_eq!(block.provider_permission_generation, Some(1));
    assert!(runtime
        .governance_provider_permission_changed(1, 1, "permission-operation-stale")
        .is_err());
    assert_eq!(
        runtime
            .runtime()
            .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest {
                job_id: job_id.clone(),
            })
            .expect("still blocked")
            .job
            .status,
        PostTurnGovernanceJobStatusV2::BlockedPolicy
    );

    fake.allow_requests();
    runtime
        .governance_provider_permission_changed(1, 2, "permission-operation-2")
        .expect("advanced permission generation resumes exact job");
    wait_for_job(&runtime, &job_id, PostTurnGovernanceJobStatusV2::Succeeded);
    let succeeded = runtime
        .runtime()
        .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest { job_id })
        .expect("permission recovered status")
        .job;
    assert_eq!(
        succeeded.last_provider_permission_recovery_generation,
        Some(2)
    );
    assert!(succeeded.receipt.is_some());
}

#[test]
fn missing_credential_has_zero_network_and_changed_generation_resumes_same_job() {
    let root = temp_store("credential-recovery");
    let runtime = runtime(&root);
    let fake = FakeOpenAiServer::start();
    const CREDENTIAL_ENV: &str = "BM_TEST_PL2_MISSING_TOKEN";
    const GENERATION_ENV: &str = "BM_TEST_PL2_MISSING_TOKEN_GENERATION";
    std::env::remove_var(CREDENTIAL_ENV);
    std::env::remove_var(GENERATION_ENV);
    configure_model_with_auth(
        &runtime,
        &fake,
        EntryGovernanceModelAuthMode::CredentialEnv {
            credential_env: CREDENTIAL_ENV.to_string(),
        },
    );
    let finalized = runtime
        .runtime()
        .finalize_turn(finalize_request(
            "conversation-credential-recovery",
            "turn-credential-recovery",
        ))
        .expect("queued finalize");
    let job_id = finalized.memory_consolidation.job_id.expect("job id");

    wait_for_job(
        &runtime,
        &job_id,
        PostTurnGovernanceJobStatusV2::BlockedConfiguration,
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    let blocked = runtime
        .runtime()
        .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest {
            job_id: job_id.clone(),
        })
        .expect("blocked status")
        .job;
    assert_eq!(
        blocked
            .execution_block_authority
            .as_ref()
            .map(|authority| authority.typed_block_reason),
        Some(bm_sdk::PostTurnGovernanceExecutionBlockReasonV1::CredentialMissing)
    );

    std::env::set_var(CREDENTIAL_ENV, "synthetic-recovered-secret");
    std::env::set_var(GENERATION_ENV, "1");
    runtime
        .governance_credential_changed(CREDENTIAL_ENV, 1, "credential-recovery-operation-1")
        .expect("credential generation resumes exact blocked job");
    wait_for_job(&runtime, &job_id, PostTurnGovernanceJobStatusV2::Succeeded);
    assert!(fake.calls.load(Ordering::SeqCst) >= 1);
    let succeeded = runtime
        .runtime()
        .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest { job_id })
        .expect("succeeded status")
        .job;
    assert!(succeeded.receipt.is_some());
    std::env::remove_var(CREDENTIAL_ENV);
    std::env::remove_var(GENERATION_ENV);
}

#[test]
fn learning_service_blocks_without_config_then_recovers_and_completes_the_same_job() {
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
        let report = service_report(&runtime);
        if report.blocked_jobs > 0 {
            break report;
        }
        assert!(
            Instant::now() < report_deadline,
            "learning service report did not observe the durable blocked transition"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    std::thread::sleep(Duration::from_millis(200));
    let settled_report = service_report(&runtime);
    assert!(
        settled_report.cycles <= blocked_report.cycles.saturating_add(1),
        "an unchanged blocked job must not create a hot loop"
    );
    assert_eq!(settled_report.blocked_jobs, blocked_report.blocked_jobs);
    let fake = FakeOpenAiServer::start();
    configure_model(&runtime, &fake);

    wait_for_job(&runtime, &job_id, PostTurnGovernanceJobStatusV2::Succeeded);
    wait_for_completed_job_reports(&[&runtime], 1);
    let report = service_report(&runtime);
    let attachment = attachment_report(&runtime);
    assert_eq!(report.completed_jobs, 1);
    assert_eq!(attachment.last_job_id.as_deref(), Some(job_id.as_str()));
    assert!(fake.calls.load(Ordering::SeqCst) >= 1);
    runtime
        .shutdown_memory_learning_service(Instant::now() + Duration::from_secs(2))
        .expect("bounded shutdown");
    assert_eq!(service_report(&runtime).state, "stopped");
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
    assert_eq!(service_report(&runtime).retrying_jobs, 1);
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
    assert!(
        fake.malformed_long_term_responses.load(Ordering::SeqCst) > 0,
        "a real long-term extraction request must receive the malformed response"
    );
}

#[test]
fn two_entry_learning_services_share_one_backend_claim_and_one_terminal_receipt() {
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
    wait_for_completed_job_reports(&[&first, &second], 1);
    assert_eq!(
        service_report(&first)
            .completed_jobs
            .saturating_add(service_report(&second).completed_jobs),
        1,
        "backend CAS must leave only one learning service completion winner"
    );
    first
        .shutdown_memory_learning_service(Instant::now() + Duration::from_secs(2))
        .expect("first shutdown");
    second
        .shutdown_memory_learning_service(Instant::now() + Duration::from_secs(2))
        .expect("second shutdown");
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
    let shutdown =
        runtime.shutdown_memory_learning_service(Instant::now() + Duration::from_millis(100));
    assert!(
        shutdown.is_err(),
        "active call must honor the short deadline"
    );
    assert!(shutdown_started.elapsed() < Duration::from_secs(3));
    wait_for_job(
        &runtime,
        &job_id,
        PostTurnGovernanceJobStatusV2::RetryWaiting,
    );
    let status = runtime
        .runtime()
        .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest { job_id })
        .expect("status after shutdown")
        .job;
    assert_eq!(status.status, PostTurnGovernanceJobStatusV2::RetryWaiting);
    assert!(status.receipt.is_none());
    runtime
        .shutdown_memory_learning_service(Instant::now() + Duration::from_secs(10))
        .expect("worker joins after in-flight response is fenced");
    assert_eq!(service_report(&runtime).state, "stopped");
}

const PL3_OBSERVATION: &str =
    "I consistently value honest uncertainty and repair grounded in evidence.";
const PL3_GROWTH: &str = "evidence-grounded candor before certainty";
const PL3_MATERIAL: &str = "Calculate a checksum and compare it with the authenticated expected checksum before accepting a file.";

fn pl3_model_response(
    body: &str,
    persona_calls: &AtomicUsize,
    method_calls: &AtomicUsize,
) -> String {
    let wire: serde_json::Value = serde_json::from_str(body).unwrap();
    let messages = wire["messages"].as_array().unwrap();
    let system = messages
        .iter()
        .filter(|m| m["role"] == "system")
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let user = messages
        .iter()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if system.contains("persona interpretation") {
        persona_calls.fetch_add(1, Ordering::SeqCst);
        let input: serde_json::Value = serde_json::from_str(&user).unwrap();
        let observation = input["observations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|observation| observation["text"] == PL3_OBSERVATION)
            .unwrap();
        assert!(
            !user.contains(PL3_GROWTH),
            "derived personality must not be relabelled as canonical observation"
        );
        serde_json::json!({"decision":"growth", "scope":"subject", "priority_order":[], "value_orientation":PL3_GROWTH,
            "relationship_posture":"", "supporting_observations":[{"message_id":observation["message_id"],"quote":PL3_OBSERVATION}],
            "counter_observations":[]}).to_string()
    } else if system.contains("method learning extraction") {
        method_calls.fetch_add(1, Ordering::SeqCst);
        assert!(user.contains(PL3_MATERIAL));
        assert!(
            !user.contains(PL3_GROWTH),
            "method extraction cannot consume Soul private growth"
        );
        serde_json::json!([{"task_signature":"verify_file", "procedure":"1. Calculate the checksum.\n2. Compare with the authenticated expected checksum and reject mismatches.",
            "constraints":["Use an authenticated expected checksum."], "source_material_ids":["verified-material"]}]).to_string()
    } else if system.contains("compact private self-model") {
        assert!(user.contains(PL3_GROWTH));
        serde_json::json!({"continuity_anchor":"I preserve evidence-grounded candor before certainty.","value_orientation":PL3_GROWTH}).to_string()
    } else if system.contains("private garden") {
        "null".into()
    } else if system.contains("inward autonomy runtime") {
        "{}".into()
    } else {
        "[]".into()
    }
}

fn pl3_governor(
    runtime: &bm_sdk::MemoryRuntime,
    store: bm_sdk::MemoryStoreHandle,
) -> bm_sdk::MemoryRuntime {
    let mut scope = runtime.scoped_runtime().clone();
    scope.actor_subject_id = runtime
        .subject_registry()
        .system_governor()
        .unwrap()
        .subject_id
        .clone();
    bm_sdk::MemoryRuntime::builder()
        .identity(runtime.identity().clone())
        .scope(runtime.scope().clone())
        .store(store)
        .subject_registry(runtime.subject_registry().clone())
        .subject_relationship_graph(runtime.config().subject_relationship_graph.clone())
        .subject_id(runtime.subject_id())
        .scoped_runtime(scope)
        .build()
        .unwrap()
}

fn pl3_registry() -> bm_sdk::AgentToolRegistrySnapshot {
    bm_sdk::AgentToolRegistrySnapshot::compact(
        "service-learning-tools",
        "host",
        vec![bm_sdk::AgentToolDescriptor::compact(
            "verify_file",
            "Verify file",
            "checksum-schema",
        )],
        1,
    )
}

fn pl3_material_capability(
    runtime: &bm_sdk::MemoryRuntime,
    governor: &bm_sdk::MemoryRuntime,
) -> bm_sdk::MemoryProceduralSubmissionCapability {
    use bm_sdk::*;
    let registry = pl3_registry();
    runtime
        .upsert_agent_tool_registry(registry.clone())
        .unwrap();
    governor
        .upsert_agent_tool_registry(registry.clone())
        .unwrap();
    let registered = governor
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "register-service-material-producer".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: ProceduralProducerSpecV1 {
                binding_id: "service-material-producer".into(),
                scope: ProceduralProducerScopeV1 {
                    memory_space_id: runtime.memory_space_id().into(),
                    mounted_subject_id: runtime.subject_id().into(),
                    channel_id: runtime.scope().channel.clone(),
                    chat_id: runtime.scope().chat_id.clone(),
                },
                principal: ProceduralProducerPrincipalV1::LocalCapability {
                    capability_id: "service-material-observer".into(),
                },
                source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
                claims: ProceduralProducerClaimsV1 {
                    method_learning_materials: true,
                    method_adoptions: false,
                    method_task_evaluations: false,
                    execution_facts: false,
                    method_declarations: false,
                    usage_feedback: false,
                    source_classifications: vec![ProceduralSourceSensitivity::NonPrivate],
                },
                tools: vec![ProceduralProducerToolV1 {
                    registry_ref: registry.registry_ref(),
                    tool_id: "verify_file".into(),
                    schema_fingerprint: "checksum-schema".into(),
                }],
                source_config_ref: "synthetic-local-service".into(),
                method_evaluators: vec![],
            },
        })
        .unwrap();
    governor
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap()
}

fn pl3_projection(
    runtime: &bm_sdk::MemoryRuntime,
    turn: Option<&str>,
) -> bm_sdk::MemoryProjectionOutput {
    use bm_sdk::*;
    runtime
        .project(MemoryProjectionRequest {
            binding: turn.map_or(ProceduralProjectionBindingV1::Preview, |turn| {
                ProceduralProjectionBindingV1::Turn {
                    turn_id: turn.into(),
                }
            }),
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "How do you approach uncertain claims? verify_file".into(),
            system_max_len: 12000,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![],
            tool_registry_refs: vec![pl3_registry().registry_ref()],
        })
        .unwrap()
}

fn pl3_service_turn(
    runtime: &bm_sdk::MemoryRuntime,
    capability: &bm_sdk::MemoryProceduralSubmissionCapability,
    ordinal: usize,
) {
    use bm_sdk::*;
    let turn_id = format!("service-learning-turn-{ordinal}");
    let signed = pl3_projection(runtime, Some(&turn_id));
    let mut learning = PostTurnLearningInputV3::empty();
    learning.selection_receipt = Some(signed.report().selection_receipt().unwrap().clone());
    let mut observations = vec![TranscriptInputMessage::user(PL3_OBSERVATION)];
    if ordinal == 0 {
        observations.insert(0, TranscriptInputMessage::user(PL3_MATERIAL));
        learning.method_learning_inputs = vec![MethodLearningInputV1 {
            registry_ref: pl3_registry().registry_ref(),
            tool_id: "verify_file".into(),
            schema_fingerprint: "checksum-schema".into(),
            materials: vec![MethodLearningMaterialV1 {
                material_id: "verified-material".into(),
                purpose: MethodLearningMaterialPurposeV1::TaskConstraint,
                source: MethodLearningMaterialSourceV1::CanonicalInputMessage { input_index: 0 },
                text: PL3_MATERIAL.into(),
                sensitivity: ProceduralSourceSensitivity::NonPrivate,
                external_content: false,
            }],
        }];
    }
    let finalized = runtime
        .finalize_turn_with_procedural_evidence(
            capability,
            MemoryTurnFinalizeRequest {
                turn: CanonicalTurnDelta {
                    turn_id,
                    conversation: ConversationScope {
                        channel: runtime.scope().channel.clone(),
                        chat_id: runtime.scope().chat_id.clone(),
                        conversation_id: Some(runtime.scope().conversation_id_or_chat_id().into()),
                    },
                    subject: runtime.subject_id().into(),
                    delivery_status: MemoryTurnDeliveryStatus::Delivered,
                    source: MemoryTurnSource {
                        ingress: IngressKind::User,
                        channel: runtime.scope().channel.clone(),
                        provider: None,
                        protocol: MemoryTurnProtocol::Native,
                        endpoint: None,
                        model_alias: None,
                        model_resolved: None,
                        request_id: None,
                        client_conversation_hint: None,
                    },
                    actor: None,
                    input_messages: observations,
                    assistant_messages: vec![bm_sdk::TranscriptAssistantMessage::complete(
                        TranscriptInputMessage::assistant(
                            "I will distinguish uncertainty from evidence.",
                        ),
                    )],
                    tool_observations: vec![],
                    external_content_used: false,
                    candidate_ids: vec![],
                },
                learning,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .unwrap();
    let job_id = finalized.memory_consolidation.job_id.unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = runtime
            .governance_job_status(MemoryGovernanceJobStatusRequest {
                job_id: job_id.clone(),
            })
            .unwrap()
            .job;
        if job.status == PostTurnGovernanceJobStatusV2::Succeeded {
            assert!(job.receipt.is_some());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "official service stalled: {job:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn assert_pl3_service_result(
    runtime: &bm_sdk::MemoryRuntime,
    other: &bm_sdk::MemoryRuntime,
    fake: &FakeOpenAiServer,
) {
    use bm_sdk::*;
    let query = MemoryMethodTrialRequest {
        tool: ProceduralProducerToolV1 {
            registry_ref: pl3_registry().registry_ref(),
            tool_id: "verify_file".into(),
            schema_fingerprint: "checksum-schema".into(),
        },
        max_candidates: 4,
    };
    let learned = runtime.discover_method_trials(query.clone()).unwrap();
    assert!(
        !learned.candidates.is_empty(),
        "official service never persisted extracted method"
    );
    assert!(learned
        .candidates
        .iter()
        .all(|value| value.status == AgentToolExperienceStatus::Candidate));
    assert!(learned.candidates[0]
        .procedure
        .contains("reject mismatches"));
    assert!(
        other
            .discover_method_trials(query)
            .unwrap()
            .candidates
            .is_empty(),
        "another subject read the method candidate"
    );
    let projected = pl3_projection(runtime, None);
    assert!(projected
        .provider_payload()
        .system_memory_block()
        .contains(PL3_GROWTH));
    assert!(
        !projected
            .provider_payload()
            .system_memory_block()
            .contains(&learned.candidates[0].procedure),
        "unadopted method was prematurely delivered"
    );
    assert!(!pl3_projection(other, None)
        .provider_payload()
        .system_memory_block()
        .contains(PL3_GROWTH));
    let safe = runtime
        .read_subject_soul(SubjectSoulReadRequestV1 {
            target_subject_id: runtime.subject_id().into(),
            selector: SubjectSoulReadSelectorV1::Current,
            view: SubjectSoulReadViewV1::OperatorSafe,
        })
        .unwrap();
    assert!(!serde_json::to_string(&safe).unwrap().contains(PL3_GROWTH));
    assert!(fake.persona_calls.load(Ordering::SeqCst) >= 4);
    assert!(fake.method_calls.load(Ordering::SeqCst) > 0);
}

fn pl3_other_subject(
    runtime: &bm_sdk::MemoryRuntime,
    store: bm_sdk::MemoryStoreHandle,
) -> Arc<bm_sdk::MemoryRuntime> {
    use bm_sdk::*;
    let mut registry = runtime.subject_registry().clone();
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("service-other"),
            "Other subject",
        ))
        .unwrap();
    let other = Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("service-other", &runtime.identity().owner_id).unwrap())
            .scope(MemoryScope::new(runtime.scope().channel.clone(), "other-service-chat").unwrap())
            .store(store)
            .subject_registry(registry)
            .build()
            .unwrap(),
    );
    other.upsert_agent_tool_registry(pl3_registry()).unwrap();
    other
}

#[test]
fn entry_official_service_learns_methods_and_persona_from_canonical_sources() {
    let root = temp_store("pl3-entry-consumer");
    let entry = runtime(&root);
    let fake = FakeOpenAiServer::start_with_learning();
    configure_model(&entry, &fake);
    let store = bm_sdk::MemoryStoreHandle::open(
        StoreBackendConfig::file(&root, support::host_production_profile())
            .unwrap()
            .with_fsync(false),
    )
    .unwrap();
    let governor = pl3_governor(entry.runtime(), store.clone());
    let capability = pl3_material_capability(entry.runtime(), &governor);
    for ordinal in 0..4 {
        pl3_service_turn(entry.runtime(), &capability, ordinal);
    }
    let other = pl3_other_subject(entry.runtime(), store);
    assert_pl3_service_result(entry.runtime(), &other, &fake);
    entry
        .shutdown_memory_learning_service(Instant::now() + Duration::from_secs(2))
        .unwrap();
}

#[test]
fn attached_official_service_learns_on_existing_multi_subject_runtime() {
    use bm_entry::*;
    use bm_sdk::*;
    struct Binding(String);
    impl GovernanceBindingSource for Binding {
        fn current_binding(&self) -> bm_sdk::Result<Option<GovernanceProviderBinding>> {
            Ok(Some(GovernanceProviderBinding {
                source_owner_id: "pl3-host".into(),
                source_config_id: "single-provider".into(),
                source_revision: 1,
                protocol: EntryGovernanceModelProtocol::OpenAiCompatible,
                endpoint: self.0.clone(),
                model_id: "synthetic".into(),
                credential_reference: None,
                request_timeout_ms: 5000,
                max_input_tokens: 8192,
                max_output_tokens: 2048,
                provider_permission_generation: 1,
            }))
        }
    }
    struct NoSecret;
    impl GovernanceCredentialResolver for NoSecret {
        fn resolve(
            &self,
            _: &GovernanceCredentialRequest,
        ) -> bm_sdk::Result<ResolvedGovernanceCredential> {
            panic!("loopback needs no credentials")
        }
    }
    let fake = FakeOpenAiServer::start_with_learning();
    let store = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(support::host_production_profile()).unwrap(),
    )
    .unwrap();
    let mut registry =
        SubjectRegistry::single_agent_default("service-owner", "service-agent").unwrap();
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("service-other"),
            "Other subject",
        ))
        .unwrap();
    let runtime = Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("service-agent", "service-owner").unwrap())
            .scope(MemoryScope::new("desktop.embedded", "service-chat").unwrap())
            .store(store.clone())
            .subject_registry(registry)
            .build()
            .unwrap(),
    );
    let other = pl3_other_subject(&runtime, store.clone());
    let governor = pl3_governor(&runtime, store.clone());
    let other_governor = pl3_governor(&other, store);
    let capability = pl3_material_capability(&runtime, &governor);
    let (service, _attachment) = MemoryLearningService::builder(runtime.clone())
        .control_authorities(governor.learning_service_control_authorities().unwrap())
        .binding_source(Arc::new(Binding(fake.endpoint())))
        .credential_resolver(Arc::new(NoSecret))
        .start()
        .unwrap();
    let _other_attachment = service
        .attach_runtime(
            other.clone(),
            other_governor
                .learning_service_control_authorities()
                .unwrap(),
        )
        .unwrap();
    for ordinal in 0..4 {
        pl3_service_turn(&runtime, &capability, ordinal);
    }
    assert_pl3_service_result(&runtime, &other, &fake);
    service
        .shutdown(Instant::now() + Duration::from_secs(2))
        .unwrap();
}
