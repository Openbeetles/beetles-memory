#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_sdk::{
    primary_human_subject_id, MemoryProjectionRequest, PressureLevel, RuntimeLifecycleModeInput,
    SubjectSoulFoundingCharterSeedV1, SubjectSoulProvisionIntentV1,
};

use support::{empty_store_platform, seeded_store_platform, test_runtime_with_scope};

#[test]
fn projection_report_exposes_sdk_owned_safe_budget_and_privacy_audit() {
    let profile = support::host_test_profile();
    let platform = seeded_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "chat-a");
    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "projection-audit-soul".to_string(),
            human_actor_subject_id: primary_human_subject_id("owner-default"),
            charter: Box::new(
                SubjectSoulFoundingCharterSeedV1 {
                    identity_anchor: Some("mounted inhabited subject".to_string()),
                    character_tendencies: vec!["work-first direct reply".to_string()],
                    priority_constitution: vec!["complete the user task".to_string()],
                    non_negotiables: vec!["protect private inner material".to_string()],
                    default_response_mode: Some("work-first direct reply".to_string()),
                    default_initiative_posture: Some(
                        "continue without theatrical drift".to_string(),
                    ),
                    default_relationship_posture: Some("engineering collaborator".to_string()),
                    boundary_doctrine: None,
                    truth_seeking_commitment: None,
                    self_preservation_doctrine: Some("protect private inner material".to_string()),
                    repair_doctrine: None,
                    change_principle: None,
                }
                .canonicalize()
                .expect("canonical audit Soul seed"),
            ),
            source_asserted_at: Some(1_700_000_000),
        })
        .expect("seed typed Soul through lifecycle owner");

    let report = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "How should release safety work?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    let safe = report.report();
    let audit = safe.audit();
    assert!(!audit.projection_id.is_empty());
    assert!(audit.injected);
    assert_eq!(
        safe.gateway_audit().provider_projection_chars,
        report
            .provider_payload()
            .system_memory_block()
            .chars()
            .count()
    );
    assert_eq!(
        audit.render_budget_chars,
        runtime
            .runtime_budget()
            .projection_render_budget
            .system_block_max_chars
            .min(4096)
    );
    assert_eq!(
        audit.source_budget_chars,
        runtime
            .runtime_budget()
            .projection_source_budget
            .context_assembly_max_chars
    );
    assert!(
        !audit.runtime_private_context_allowed,
        "standard SDK projection policy must not load private runtime depth by default"
    );
    assert!(
        !audit.foreground_disclosure_allowed,
        "foreground disclosure must be separate from runtime-private access"
    );
    assert!(audit.private_gate_reason.contains("privacy_policy"));
    assert!(audit.evidence_ref_count > 0);
    assert!(audit.budget_decision_count > 0);
    assert!(audit.privacy_decision_count > 0);
    assert!(audit.faithfulness_passed);
    assert!(audit.disclosure_integrity_passed);
    assert_eq!(audit.raw_private_violation_count, 0);
    assert!(safe.ui_api_chars() > 0);
    assert_eq!(
        safe.ui_api_chars(),
        safe.ui_api_projection().chars().count()
    );
    for forbidden_marker in [
        "private_raw:",
        "private-garden-raw:",
        "private garden raw:",
        "<private_raw>",
    ] {
        assert!(
            !report
                .provider_payload()
                .system_memory_block()
                .to_ascii_lowercase()
                .contains(forbidden_marker),
            "projection leaked {forbidden_marker}"
        );
        assert!(!safe
            .gateway_audit()
            .block
            .to_ascii_lowercase()
            .contains(forbidden_marker));
    }
}

#[test]
fn projection_runtime_envelope_replaces_flat_internal_sections() {
    let profile = support::host_test_profile();
    let platform = seeded_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "chat-a");

    let report = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "Prepare the release checklist without drifting into roleplay.".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    for required_heading in [
        "## LLM Runtime Projection Envelope",
        "## Subject Mount",
        "## Governed Memory Evidence",
        "## Boundary And Disclosure Protocol",
        "## Runtime Constraints",
        "## Work Integrity Covenant",
    ] {
        assert!(
            report
                .provider_payload()
                .system_memory_block()
                .contains(required_heading),
            "{}",
            report.provider_payload().system_memory_block()
        );
    }
    for forbidden_heading in [
        "## Self State",
        "## Inner Life",
        "## Private Garden",
        "## Inner Workspace",
        "## Autonomy Strategy",
        "## Outer Voice",
        "## Mental Privacy Boundary",
    ] {
        assert!(
            !report
                .provider_payload()
                .system_memory_block()
                .contains(forbidden_heading),
            "{} must not appear in runtime envelope:\n{}",
            forbidden_heading,
            report.provider_payload().system_memory_block()
        );
    }
    assert_eq!(
        report.report().gateway_audit().projection_id,
        report.report().audit().projection_id
    );
    assert!(report.report().audit().faithfulness_passed);
    assert!(report.report().audit().disclosure_integrity_passed);
    assert!(!report
        .report()
        .ui_api_projection()
        .contains("## Soul Private Runtime Context"));
}

#[test]
fn projection_report_exposes_disclosure_integrity_for_runtime_surfaces() {
    let profile = support::host_test_profile();
    let platform = seeded_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "chat-a");

    let report = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "Prepare the release checklist.".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    assert_eq!(report.report().audit().raw_private_violation_count, 0);
    assert!(report.report().audit().disclosure_integrity_passed);
    assert!(report.report().audit().faithfulness_passed);
    assert_eq!(report.report().audit().unsupported_claim_count, 0);
    assert!(report.report().gateway_audit().redacted);
}

#[test]
fn empty_store_projection_degrades_subject_mount_without_inventing_personality() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "empty-chat");

    let report = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "Summarize what you know before answering.".to_string(),
            system_max_len: 2048,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    assert!(
        report
            .provider_payload()
            .system_memory_block()
            .contains("## Subject Mount"),
        "{}",
        report.provider_payload().system_memory_block()
    );
    assert!(
        report
            .provider_payload()
            .system_memory_block()
            .contains("subject_mount_degraded"),
        "{}",
        report.provider_payload().system_memory_block()
    );
    assert!(!report
        .provider_payload()
        .system_memory_block()
        .to_ascii_lowercase()
        .contains("pretend"));
    assert!(report.report().audit().faithfulness_passed);
}

#[test]
fn empty_store_greeting_projection_does_not_leak_identity_meta_terms() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "empty-chat");

    let report = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "你好".to_string(),
            system_max_len: 2048,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    let lower = report
        .provider_payload()
        .system_memory_block()
        .to_ascii_lowercase();
    for forbidden in [
        "roleplay",
        "personality",
        "model identity",
        "memory helper",
        "assistant self-description",
        "relationship theater",
        "training provenance",
        "user-facing identity",
        "ai assistant",
        "模拟角色",
        "角色扮演",
        "人设",
        "人格",
        "ai 助手",
        "人工智能模型",
    ] {
        assert!(
            !lower.contains(forbidden),
            "{forbidden} leaked into clean greeting projection:\n{}",
            report.provider_payload().system_memory_block()
        );
    }
}
