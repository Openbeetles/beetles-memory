use std::process::Command;

#[test]
fn cli_renders_platform_capability_snapshot_for_requested_profile() {
    let output = Command::new(env!("CARGO_BIN_EXE_bm"))
        .args([
            "platform",
            "capability-snapshot",
            "--profile",
            "profile-esp-standalone-memory",
        ])
        .output()
        .expect("run bm cli");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("snapshot json");
    assert_eq!(value["schema"], "beetle-memory.platform.capability.v5");
    assert_eq!(value["profile"], "profile-esp-standalone-memory");
    assert_eq!(value["target"], "target-esp");
    assert_eq!(value["adapter"]["wss"]["client_allowed"], true);
    assert_eq!(
        value["procedural_learning"]["standard_agent_skill_mount"]["profile_allowed"],
        false
    );
    assert_eq!(
        value["procedural_learning"]["agent_tool_registry"]["profile_allowed"],
        false
    );
    assert_eq!(
        value["procedural_learning"]["worker"]["profile_allowed"],
        false
    );
}

#[test]
fn cli_rejects_unknown_platform_profile() {
    let output = Command::new(env!("CARGO_BIN_EXE_bm"))
        .args([
            "platform",
            "capability-snapshot",
            "--profile",
            "profile-unknown-special",
        ])
        .output()
        .expect("run bm cli");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unsupported platform profile"));
}
