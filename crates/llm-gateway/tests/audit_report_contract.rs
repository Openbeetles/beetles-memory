#[cfg(feature = "nonproduction-replay-harness")]
use std::path::PathBuf;
#[cfg(feature = "nonproduction-replay-harness")]
use std::sync::Arc;
#[cfg(feature = "nonproduction-replay-harness")]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "nonproduction-replay-harness")]
use bm_llm_gateway::GatewayAuditConfig;
use bm_llm_gateway::{
    GatewayAuditOutcome, GatewayAuditReport, GatewayAuditStage, GatewayProjectionAuditStatus,
    GatewayScopeResolution,
};
#[cfg(feature = "nonproduction-replay-harness")]
use bm_sdk::{
    default_agent_subject_id, MemoryAuditSink, MemoryClock, MemoryIdentity, MemoryPrivacyPolicy,
    MemoryProjectionRequest, MemoryRuntime, MemoryScope, MemoryStoreHandle, MemoryWriteRequest,
    NoopMemoryAuditSink, PressureLevel, PrivateDocEntry, PrivateDocWorkspace, ProfileId,
    RuntimeLifecycleModeInput, RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
};

#[cfg(feature = "nonproduction-replay-harness")]
mod support;

#[test]
fn gateway_audit_report_has_stages_without_raw_sensitive_payloads() {
    let scope = GatewayScopeResolution::audit_only(
        "owner-default",
        "agent-main",
        "llm.gateway",
        "chat-1",
        "scope resolved without raw path",
    );
    let mut report = GatewayAuditReport::new(
        "audit-1",
        "/v1/chat/completions",
        "zed",
        "local-model",
        scope,
    );
    report.record_stage(GatewayAuditStage::Projection, GatewayAuditOutcome::Skipped);
    report.record_stage(
        GatewayAuditStage::Upstream,
        GatewayAuditOutcome::NotExecuted,
    );
    report.record_stage(GatewayAuditStage::Maintenance, GatewayAuditOutcome::Skipped);

    let json = serde_json::to_string(&report).expect("audit json");

    assert!(json.contains("audit-1"));
    assert!(json.contains("projection"));
    assert!(json.contains("not_executed"));
    assert_eq!(
        report.projection_record.status,
        GatewayProjectionAuditStatus::NotRecorded
    );
    assert_eq!(report.projection_record.reason, "projection_not_attempted");
    assert!(report.projection_record.block.is_none());
    assert!(!json.contains("api_key"));
    assert!(!json.contains("/Users/"));
    assert!(!json.contains("full_request_body"));
    assert!(!json.contains("full_response_body"));
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn raw_projection_audit_records_redacted_final_sdk_projection_to_local_diagnostics() {
    let mounted_subject_id = default_agent_subject_id("agent-main");
    let platform = MemoryStoreHandle::open_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("supported host-native dev-full profile"),
        )
        .expect("store config"),
    )
    .expect("store platform");
    platform
        .replay_harness()
        .seed_private_doc_workspace(
            &mounted_subject_id,
            &PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "RAW_PRIVATE_WORKSPACE_NOTE_DO_NOT_AUDIT".to_string(),
                    updated_at: 1_800_000_000,
                    revision: 1,
                }),
                ..PrivateDocWorkspace::default()
            },
        )
        .expect("private workspace seed");
    platform
        .replay_harness()
        .seed_private_garden_doc(
            &mounted_subject_id,
            "diary/gateway.md",
            "RAW_PRIVATE_GARDEN_NOTE_DO_NOT_AUDIT",
            1_800_000_000,
        )
        .expect("private garden seed");

    let mut privacy = MemoryPrivacyPolicy::standard_private_boundary();
    privacy.private_plane_projection_allowed = true;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("llm.gateway", "chat-a").expect("scope"))
        .store(platform)
        .clock(Arc::new(FixedClock))
        .capability_policy(bm_sdk::MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(privacy)
        .audit_sink(Arc::new(NoopMemoryAuditSink) as Arc<dyn MemoryAuditSink>)
        .build()
        .expect("runtime");
    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "gateway private audit".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");
    assert!(projection
        .provider_payload()
        .system_memory_block()
        .contains("RAW_PRIVATE_WORKSPACE_NOTE_DO_NOT_AUDIT"));

    let diagnostic_dir = unique_audit_dir();
    let config = GatewayAuditConfig {
        record_raw_projection: true,
        raw_projection_diagnostic_path: Some(diagnostic_dir.clone()),
        raw_projection_retention_limit: 1,
        ..GatewayAuditConfig::default()
    };
    let mut report = GatewayAuditReport::new(
        "audit-private",
        "/v1/chat/completions",
        "openai",
        "local",
        GatewayScopeResolution::audit_only(
            "owner-default",
            "agent-main",
            "llm.gateway",
            "chat-a",
            "scope resolved without raw path",
        ),
    );

    report
        .record_projection(&config, projection.report())
        .expect("record projection audit");

    assert_eq!(
        report.projection_record.status,
        GatewayProjectionAuditStatus::Recorded
    );
    assert_eq!(
        report.projection_record.reason,
        "raw_projection_recorded_redacted"
    );
    assert!(report.projection_record.redacted);
    assert_eq!(
        report.projection_record.redacted_source_count,
        projection.report().gateway_audit().redacted_source_count
    );
    assert!(report.projection_record.projection_chars > 0);
    let block = report
        .projection_record
        .block
        .as_deref()
        .expect("redacted projection block");
    assert_eq!(block, projection.report().gateway_audit().block);
    assert!(block.contains("## Subject Mount"), "{block}");
    assert!(
        !block.contains("## Soul Private Runtime Context"),
        "{block}"
    );
    assert!(!block.contains("RAW_PRIVATE_WORKSPACE_NOTE_DO_NOT_AUDIT"));
    assert!(!block.contains("RAW_PRIVATE_GARDEN_NOTE_DO_NOT_AUDIT"));

    let diagnostic_path = PathBuf::from(
        report
            .projection_record
            .local_diagnostic_path
            .as_deref()
            .expect("diagnostic path"),
    );
    let diagnostic_json = std::fs::read_to_string(&diagnostic_path).expect("diagnostic file");
    assert!(diagnostic_json.contains("raw_projection_recorded_redacted"));
    assert!(!diagnostic_json.contains("RAW_PRIVATE_WORKSPACE_NOTE_DO_NOT_AUDIT"));
    assert!(!diagnostic_json.contains("RAW_PRIVATE_GARDEN_NOTE_DO_NOT_AUDIT"));
    let file_count = std::fs::read_dir(&diagnostic_dir)
        .expect("diagnostic dir")
        .count();
    assert!(file_count <= config.raw_projection_retention_limit);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&diagnostic_dir)
                .expect("diagnostic dir metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&diagnostic_path)
                .expect("diagnostic file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    let disabled_config = GatewayAuditConfig {
        record_raw_projection: false,
        ..GatewayAuditConfig::default()
    };
    let mut disabled_report = GatewayAuditReport::new(
        "audit-disabled",
        "/v1/chat/completions",
        "openai",
        "local",
        GatewayScopeResolution::audit_only(
            "owner-default",
            "agent-main",
            "llm.gateway",
            "chat-a",
            "scope resolved without raw path",
        ),
    );
    disabled_report
        .record_projection(&disabled_config, projection.report())
        .expect("record disabled projection audit");
    assert_eq!(
        disabled_report.projection_record.status,
        GatewayProjectionAuditStatus::NotRecorded
    );
    assert_eq!(
        disabled_report.projection_record.reason,
        "raw_projection_recording_disabled"
    );
    assert!(disabled_report.projection_record.block.is_none());
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn procedural_provider_payload_is_exact_zero_in_gateway_audit_and_local_diagnostic() {
    let platform = MemoryStoreHandle::open_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("supported host-native dev-full profile"),
        )
        .expect("store config"),
    )
    .expect("store platform");
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("llm.gateway", "chat-a").expect("scope"))
        .store(platform)
        .clock(Arc::new(FixedClock))
        .capability_policy(bm_sdk::MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink) as Arc<dyn MemoryAuditSink>)
        .build()
        .expect("runtime");
    let procedural_sentinel = "GATEWAY_PROCEDURAL_AUDIT_EXACT_ZERO_SENTINEL";
    let procedural_write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                name: "gateway_procedural_audit_guard".to_string(),
                topic: "gateway procedural audit".to_string(),
                title: "Gateway procedural audit guard".to_string(),
                summary: "Keep provider-only procedural content out of diagnostics.".to_string(),
                content: format!(
                    "1. {procedural_sentinel}\n2. Verify the redacted audit projection."
                ),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-a".to_string()),
                observed_at: 1_800_000_000,
            })],
            owning_scope: support::runtime_skill_subject_scope("agent-main"),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write governed procedure for audit");
    assert!(procedural_write.accepted, "{procedural_write:#?}");

    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "gateway procedural audit".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");
    assert!(
        projection
            .provider_payload()
            .system_memory_block()
            .contains(procedural_sentinel),
        "{:#?}",
        projection.report().procedural_delivery_reports()
    );

    let diagnostic_dir = unique_audit_dir();
    let config = GatewayAuditConfig {
        record_raw_projection: true,
        raw_projection_diagnostic_path: Some(diagnostic_dir.clone()),
        raw_projection_retention_limit: 1,
        ..GatewayAuditConfig::default()
    };
    let mut report = GatewayAuditReport::new(
        "audit-procedural",
        "/v1/chat/completions",
        "openai",
        "local",
        GatewayScopeResolution::audit_only(
            "owner-default",
            "agent-main",
            "llm.gateway",
            "chat-a",
            "scope resolved without raw path",
        ),
    );
    report
        .record_projection(&config, projection.report())
        .expect("record projection audit");

    let block = report
        .projection_record
        .block
        .as_deref()
        .expect("safe gateway audit block");
    assert!(!block.contains(procedural_sentinel));
    let report_json = serde_json::to_string(&report).expect("serialized audit report");
    assert!(!report_json.contains(procedural_sentinel));
    let diagnostic_path = report
        .projection_record
        .local_diagnostic_path
        .as_deref()
        .expect("diagnostic path");
    let diagnostic_json = std::fs::read_to_string(diagnostic_path).expect("diagnostic file");
    assert!(!diagnostic_json.contains(procedural_sentinel));
}

#[cfg(feature = "nonproduction-replay-harness")]
struct FixedClock;

#[cfg(feature = "nonproduction-replay-harness")]
impl MemoryClock for FixedClock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
fn unique_audit_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "bm-llm-gateway-raw-audit-test-{}-{nanos}",
        std::process::id()
    ))
}
