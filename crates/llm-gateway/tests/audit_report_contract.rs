use bm_llm_gateway::{
    GatewayAuditOutcome, GatewayAuditReport, GatewayAuditStage, GatewayScopeResolution,
};

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
    assert!(!json.contains("api_key"));
    assert!(!json.contains("raw_projection"));
    assert!(!json.contains("/Users/"));
    assert!(!json.contains("full_request_body"));
    assert!(!json.contains("full_response_body"));
}
