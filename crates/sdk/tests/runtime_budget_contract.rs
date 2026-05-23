mod support;

use bm_sdk::{MemoryProjectionRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput};

use support::{seeded_store_platform, test_runtime};

#[test]
fn projection_render_limit_does_not_cut_source_recall() {
    let platform = seeded_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    let projection = runtime
        .project(MemoryProjectionRequest {
            user_query: "release artifact safety".to_string(),
            system_max_len: 64,
            recent_messages_limit: 1,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("projection");

    assert!(projection.system_memory_block.len() <= 64);
    assert!(
        projection
            .context
            .long_term_memory_text
            .as_deref()
            .unwrap_or_default()
            .contains("Verify release artifacts before publishing."),
        "source assembly must still recall memory when render budget is tiny"
    );
}

#[test]
fn runtime_exposes_compiled_budget_report() {
    let platform = seeded_store_platform(ProfileId::ServerLinuxMemoryGateway);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxMemoryGateway);
    let budget = runtime.runtime_budget();

    assert_eq!(budget.profile, ProfileId::ServerLinuxMemoryGateway);
    assert!(budget.projection_source_budget.context_assembly_max_chars > 0);
    assert!(budget.projection_render_budget.system_block_max_chars > 0);
    assert!(budget.adapter_budget.http_body_max_bytes > 0);
}
