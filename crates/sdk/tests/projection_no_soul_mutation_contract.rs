mod support;

use bm_core::memory::{board_subject_scope_id, SelfAuthoredCore};
use bm_core::platform::Platform as _;
use bm_sdk::{MemoryProjectionRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput};

use support::{empty_store_platform, test_runtime_with_scope};

#[test]
fn projection_composer_does_not_mutate_soul_or_private_surfaces() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    platform
        .self_authored_core_store()
        .set(
            board_subject_scope_id(),
            &SelfAuthoredCore {
                identity_anchor: "stable soul core".to_string(),
                default_response_mode: "direct work mode".to_string(),
                self_preservation_doctrine: "never expose private raw material".to_string(),
                ..SelfAuthoredCore::default()
            },
        )
        .expect("seed core");
    platform
        .private_garden_store()
        .write(
            "chat-a",
            "journal/today.md",
            "raw private note that projection may read but must not rewrite",
            1_800_000_000,
        )
        .expect("seed private garden");

    let before_core = platform
        .self_authored_core_store()
        .get(board_subject_scope_id())
        .expect("read core");
    let before_private = platform
        .private_garden_store()
        .list("chat-a", 16)
        .expect("list private garden");
    let before_ledger = platform
        .core_revision_ledger_store()
        .get(board_subject_scope_id())
        .expect("read ledger");

    let runtime = test_runtime_with_scope(platform.clone(), profile, "sdk.direct", "chat-a");
    runtime
        .project(MemoryProjectionRequest {
            user_query: "Summarize the current work context.".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    assert_eq!(
        platform
            .self_authored_core_store()
            .get(board_subject_scope_id())
            .expect("read core after"),
        before_core
    );
    assert_eq!(
        platform
            .private_garden_store()
            .list("chat-a", 16)
            .expect("list private garden after"),
        before_private
    );
    assert_eq!(
        platform
            .core_revision_ledger_store()
            .get(board_subject_scope_id())
            .expect("read ledger after"),
        before_ledger
    );
}
