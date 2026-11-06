mod support;

use bm_sdk::{
    MemoryIdentity, MemoryProjectionRequest, MemoryRecallTemporalOperation, MemoryRuntime,
    MemoryScope, PressureLevel, ProceduralProjectionBindingV1, Result, RuntimeLifecycleModeInput,
    StoreBackendConfig, UserLocalTimeContext, UserLocalTimeProvider,
};
use std::sync::{Arc, Mutex};

// A synthetic trusted host time-zone source; it supplies an offset at the exact
// projection instant rather than a preformatted prompt or a cached wall clock.
struct DeviceZone(Mutex<Option<(i32, &'static str)>>);
impl UserLocalTimeProvider for DeviceZone {
    fn local_time_at(&self, epoch: u64) -> Result<Option<UserLocalTimeContext>> {
        self.0
            .lock()
            .unwrap()
            .map(|(offset, zone)| UserLocalTimeContext::new(epoch, offset, zone))
            .transpose()
    }
}

fn runtime(provider: Arc<dyn UserLocalTimeProvider>) -> MemoryRuntime {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
    )
    .unwrap();
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("clock-agent", "clock-owner").unwrap())
        .scope(MemoryScope::new("local", "clock-contract").unwrap())
        .store(store)
        // 2026-10-29 06:14:00 UTC, 14:14:00 in Asia/Shanghai.
        .clock(support::fixed_clock(1_793_254_440))
        .user_local_time_provider(provider)
        .build()
        .unwrap()
}

fn project(runtime: &MemoryRuntime, budget: usize) -> String {
    try_project(runtime, budget).unwrap()
}

fn try_project(runtime: &MemoryRuntime, budget: usize) -> Result<String> {
    runtime
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Preview,
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "现在几点？".into(),
            system_max_len: budget,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .map(|projection| {
            projection
                .provider_payload()
                .system_memory_block()
                .to_owned()
        })
}

#[test]
fn formal_projection_uses_device_time_with_zone_and_refreshes_without_remount() {
    let zone = Arc::new(DeviceZone(Mutex::new(Some((28_800, "Asia/Shanghai")))));
    let runtime = runtime(zone.clone());
    let first = project(&runtime, 4096);
    assert!(
        first.contains("2026-10-29 14:14:00 UTC+08:00 [Asia/Shanghai]"),
        "{first}"
    );
    assert_eq!(first.matches("The user's local time is").count(), 1);
    assert!(!first.contains("Outer scene now"));
    assert!(!first.contains("06:14:00"));

    *zone.0.lock().unwrap() = Some((-18_000, "America/Bogota"));
    let second = project(&runtime, 4096);
    assert!(
        second.contains("2026-10-29 01:14:00 UTC-05:00 [America/Bogota]"),
        "{second}"
    );
    assert!(!second.contains("Asia/Shanghai"));
    assert!(!second.contains("14:14:00"));
}

#[test]
fn unavailable_device_time_omits_field_in_nonempty_model_payload() {
    let zone = Arc::new(DeviceZone(Mutex::new(None)));
    let runtime = runtime(zone.clone());
    let absent = project(&runtime, 4096);
    assert!(!absent.is_empty());
    for forbidden in [
        "now=",
        "User device local time",
        "User-local time",
        "UTC+",
        "UTC-",
        "Outer scene now",
    ] {
        assert!(!absent.contains(forbidden), "{absent}");
    }
    // Positive control uses the same runtime and source, proving omission is
    // not an empty-store/disabled-projection artifact.
    *zone.0.lock().unwrap() = Some((0, "UTC"));
    let present = project(&runtime, 4096);
    assert!(
        present.contains("2026-10-29 06:14:00 UTC+00:00 [UTC]"),
        "{present}"
    );
    // Losing the device zone must also remove a previously available clock;
    // neither the last snapshot nor UTC is an acceptable fallback.
    *zone.0.lock().unwrap() = None;
    let unavailable_again = project(&runtime, 4096);
    assert!(!unavailable_again.is_empty());
    for forbidden in [
        "now=",
        "User device local time",
        "User-local time",
        "06:14:00",
        "UTC+",
        "UTC-",
        "[UTC]",
    ] {
        assert!(
            !unavailable_again.contains(forbidden),
            "{unavailable_again}"
        );
    }
}

#[test]
fn limited_formal_projection_never_delivers_a_clock_without_timezone() {
    let zone = Arc::new(DeviceZone(Mutex::new(Some((20_700, "Asia/Kathmandu")))));
    let runtime = runtime(zone);
    for budget in [128, 512, 2048] {
        match try_project(&runtime, budget) {
            Ok(payload) => {
                if payload.contains("The user's local time is") {
                    assert!(
                        payload.contains("2026-10-29 11:59:00 UTC+05:45 [Asia/Kathmandu]"),
                        "{payload}"
                    );
                }
            }
            Err(error) => assert_eq!(error.stage(), "memory_projection"),
        }
    }
    assert!(project(&runtime, 4096).contains("UTC+05:45 [Asia/Kathmandu]"));
}

#[test]
fn official_background_job_uses_exact_local_clock_and_drops_it_when_unavailable() {
    use bm_sdk::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct CaptureClock {
        expected: Option<&'static str>,
        saw_background: AtomicBool,
    }
    impl LlmClient for CaptureClock {
        fn chat(
            &self,
            _: &mut dyn LlmHttpClient,
            system: &str,
            messages: &[Message],
            _: Option<&[ToolSpec]>,
            _: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            let content = if system.contains("persona interpretation") {
                r#"{"decision":"no_change"}"#
            } else if system.contains("private garden") {
                "null"
            } else if system.contains("inward autonomy runtime") {
                self.saw_background.store(true, Ordering::SeqCst);
                let prompt = messages
                    .iter()
                    .map(|message| message.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(
                    prompt.contains("World Snapshot"),
                    "must inspect a nonempty real background request"
                );
                if let Some(expected) = self.expected {
                    assert!(
                        prompt.contains(expected),
                        "background ignored trusted local clock: {prompt}"
                    );
                    assert!(
                        !prompt.contains("06:14:00"),
                        "background exposed an unlabelled competing UTC clock"
                    );
                } else {
                    for forbidden in [
                        "now=",
                        "User-local time:",
                        "User device local time:",
                        "14:14:00",
                        "06:14:00",
                        "UTC+08:00",
                        "Local day phase:",
                    ] {
                        assert!(
                            !prompt.contains(forbidden),
                            "unavailable clock leaked stale/default field: {prompt}"
                        );
                    }
                }
                "{}"
            } else {
                "[]"
            };
            Ok(LlmResponse {
                content: content.into(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            })
        }
    }
    struct Port<'a>(&'a CaptureClock);
    impl GovernanceExecutionPort for Port<'_> {
        fn execute(
            &mut self,
            _: &AuthorizedGovernanceEnvelope,
            _: &ImmutableGovernanceExecutionBinding,
            egress: &GovernanceEgressAuthority,
            operation: &mut dyn GovernanceExecutionOperation,
        ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
            egress
                .revalidate_before_egress()
                .map_err(GovernanceExecutionPortFailure::Other)?;
            operation
                .run(&mut support::StaticHttpClient, self.0)
                .map_err(GovernanceExecutionPortFailure::Other)
        }
    }
    let zone = Arc::new(DeviceZone(Mutex::new(Some((28_800, "Asia/Shanghai")))));
    let runtime = Arc::new(runtime(zone.clone()));
    runtime
        .install_governance_binding(MemoryGovernanceBindingInstallRequest {
            source_owner_id: "local-clock-host".into(),
            source_config_id: "single-provider".into(),
            source_revision: 1,
            protocol: PostTurnGovernanceProviderProtocolV1::OllamaNative,
            endpoint: "http://127.0.0.1:11434/api".into(),
            model_id: "synthetic".into(),
            credential_reference: None,
            request_timeout_ms: 30_000,
            max_input_tokens: 8192,
            max_output_tokens: 2048,
            provider_permission_generation: 1,
        })
        .unwrap();
    for (index, expected) in [Some("2026-10-29 14:14:00 UTC+08:00 [Asia/Shanghai]"), None]
        .into_iter()
        .enumerate()
    {
        if expected.is_none() {
            *zone.0.lock().unwrap() = None;
        }
        let turn_id = format!("background-clock-{index}");
        let projection = runtime
            .project(MemoryProjectionRequest {
                binding: ProceduralProjectionBindingV1::Turn {
                    turn_id: turn_id.clone(),
                },
                temporal_operation: MemoryRecallTemporalOperation::Current,
                user_query: "Please reflect on this exchange.".into(),
                system_max_len: 8192,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: vec![],
                tool_registry_refs: vec![],
            })
            .unwrap();
        let mut learning = PostTurnLearningInputV3::empty();
        learning.selection_receipt = Some(projection.report().selection_receipt().unwrap().clone());
        runtime
            .finalize_turn(MemoryTurnFinalizeRequest {
                turn: CanonicalTurnDelta {
                    turn_id,
                    conversation: ConversationScope {
                        channel: "local".into(),
                        chat_id: "clock-contract".into(),
                        conversation_id: Some("clock-contract".into()),
                    },
                    subject: runtime.subject_id().into(),
                    delivery_status: MemoryTurnDeliveryStatus::Delivered,
                    source: MemoryTurnSource {
                        ingress: IngressKind::User,
                        channel: "local".into(),
                        provider: None,
                        protocol: MemoryTurnProtocol::Native,
                        endpoint: None,
                        model_alias: None,
                        model_resolved: None,
                        request_id: None,
                        client_conversation_hint: None,
                    },
                    actor: None,
                    input_messages: vec![TranscriptInputMessage::user(
                        "Please reflect on this exchange.",
                    )],
                    assistant_messages: vec![bm_sdk::TranscriptAssistantMessage::complete(
                        TranscriptInputMessage::assistant(
                            "I will reflect without inventing a time or location.",
                        ),
                    )],
                    tool_observations: vec![],
                    external_content_used: false,
                    candidate_ids: vec![],
                },
                learning,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            })
            .unwrap();
        let capture = CaptureClock {
            expected,
            saw_background: AtomicBool::new(false),
        };
        let outcome = MemoryLearningEngine::attach(runtime.clone())
            .unwrap()
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "background-clock-test".into(),
                    lease_duration_secs: 60,
                },
                &mut Port(&capture),
            )
            .unwrap();
        assert!(
            matches!(outcome, MemoryLearningCycleOutcome::Completed(_)),
            "{outcome:?}"
        );
        assert!(
            capture.saw_background.load(Ordering::SeqCst),
            "official job never sent the actual background prompt"
        );
    }
}
