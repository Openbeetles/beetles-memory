mod support;

use bm_sdk::{MemoryProjectionRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput};

use support::{seeded_store_platform, test_runtime_with_scope};

#[test]
fn projection_report_exposes_sdk_owned_source_scope_budget_and_privacy_audit() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = seeded_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "chat-a");

    let report = runtime
        .project(MemoryProjectionRequest {
            user_query: "How should release safety work?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("project");

    assert_eq!(report.audit.operation, "project");
    assert_eq!(report.audit.profile, profile);
    assert_eq!(report.audit.identity.agent_id, "agent-main");
    assert_eq!(report.audit.identity.owner_id, "owner-default");
    assert_eq!(report.audit.scope.channel, "sdk.direct");
    assert_eq!(report.audit.scope.chat_id, "chat-a");
    assert_eq!(report.audit.memory_space_id, "owner-default");
    assert_eq!(report.audit.subject_id, "owner-default");
    assert_eq!(report.audit.conversation_id.as_deref(), Some("chat-a"));
    assert!(report.audit.injected);
    assert_eq!(
        report.audit.system_memory_chars,
        report.system_memory_block.chars().count()
    );
    assert_eq!(
        report.audit.render_budget_chars,
        runtime
            .runtime_budget()
            .projection_render_budget
            .system_block_max_chars
            .min(4096)
    );
    assert_eq!(
        report.audit.source_budget_chars,
        runtime
            .runtime_budget()
            .projection_source_budget
            .context_assembly_max_chars
    );
    assert!(report
        .audit
        .sources
        .iter()
        .any(|source| source.plane == "shared_factual" && source.selected_count > 0));
    assert!(report
        .audit
        .sections
        .iter()
        .any(|section| section.name == "long_term_memory" && section.chars > 0));
    assert_eq!(
        report.audit.private_gate.allowed, false,
        "standard SDK projection policy must not expose private garden by default"
    );
    assert!(report.audit.private_gate.reason.contains("privacy_policy"));
    assert_eq!(report.subject_projection.profile, profile);
    assert_eq!(
        report.subject_projection.projection_id,
        report.audit.projection_id
    );
    assert!(report
        .subject_projection
        .identity_mount
        .contains("agent-main"));
    assert!(report
        .subject_projection
        .relationship_position
        .contains("sdk.direct"));
    assert!(report
        .subject_projection
        .evidence_refs
        .iter()
        .any(|evidence| evidence.contains("long_term_memory")));
    assert!(report
        .subject_projection
        .budget_decisions
        .iter()
        .any(|decision| decision.surface == "prompt"));
    assert!(report
        .subject_projection
        .privacy_decisions
        .iter()
        .any(|decision| !decision.allowed && decision.reason.contains("privacy_policy")));
    assert!(report.subject_projection.validate_contract().accepted);
    assert!(report.projection_faithfulness.passed);
    assert_eq!(report.private_echo_guard.private_echo_count, 0);
    assert!(report.private_echo_guard.passed);
}
