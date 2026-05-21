use bm_cli::{command_specs, render_capabilities, run_cli};
use bm_sdk::{resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId};

#[test]
fn command_catalog_covers_adapter_plan_without_core_store_bypass() {
    let commands: Vec<_> = command_specs().iter().map(|spec| spec.name).collect();
    assert_eq!(
        commands,
        vec![
            "capabilities",
            "inspect",
            "recall",
            "project",
            "replay",
            "export",
            "import",
            "write-procedural",
            "skill-list",
            "skill-show",
            "skill-import",
            "skill-edit",
            "skill-enable",
            "skill-disable",
            "skill-delete",
            "close",
        ]
    );

    let manifest = std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")))
        .expect("manifest");
    let dependencies = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or_default()
        .split('[')
        .next()
        .unwrap_or_default();
    assert!(!dependencies.contains("bm-core"));
    assert!(!dependencies.contains("bm-store"));
    assert!(dependencies.contains("bm-adapter"));
}

#[test]
fn memory_cli_skill_management_uses_entry_runtime_facade() {
    let root = unique_temp_dir("bm-cli-skill-management");
    let store = root.to_string_lossy().to_string();

    let imported = run_cli(
        [
            "memory",
            "skill-import",
            "--profile",
            "profile-server-linux-dev-full",
            "--store-file",
            &store,
            "--agent",
            "cli-skill-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--title",
            "Release guard",
            "--topic",
            "release",
            "--summary",
            "Check release artifacts before publishing.",
            "--content",
            "1. run gates\n2. inspect artifacts\n3. dry run publish",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("skill import");
    let imported_json: serde_json::Value = serde_json::from_str(&imported).expect("import json");
    assert_eq!(imported_json["status"], "accepted");
    assert_eq!(imported_json["mutation"]["accepted"], true);

    let list = run_cli(
        [
            "memory",
            "skill-list",
            "--profile",
            "profile-server-linux-dev-full",
            "--store-file",
            &store,
            "--agent",
            "cli-skill-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--query",
            "release",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("skill list");
    let list_json: serde_json::Value = serde_json::from_str(&list).expect("list json");
    assert_eq!(list_json["skills"]["total"], 1);
    assert_eq!(list_json["skills"]["skills"][0]["origin"], "user_provided");

    let disabled = run_cli(
        [
            "memory",
            "skill-disable",
            "--profile",
            "profile-server-linux-dev-full",
            "--store-file",
            &store,
            "--agent",
            "cli-skill-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--name",
            "runtime_skill__release",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("skill disable");
    let disabled_json: serde_json::Value = serde_json::from_str(&disabled).expect("disable json");
    assert_eq!(disabled_json["mutation"]["accepted"], true);

    let deleted = run_cli(
        [
            "memory",
            "skill-delete",
            "--profile",
            "profile-server-linux-dev-full",
            "--store-file",
            &store,
            "--agent",
            "cli-skill-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--name",
            "runtime_skill__release",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("skill delete");
    let deleted_json: serde_json::Value = serde_json::from_str(&deleted).expect("delete json");
    assert_eq!(deleted_json["mutation"]["accepted"], true);
}

#[test]
fn capabilities_output_contains_runtime_validation_and_adapter_catalog() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    policy.adapter.http_enabled = true;
    let catalog = resolve_memory_capabilities(
        ProfileId::ServerLinuxMemoryGateway,
        &policy,
        &MemoryPrivacyPolicy::standard_private_boundary(),
    )
    .expect("catalog");

    let output = render_capabilities(&catalog).expect("json");
    assert!(output.contains("\"profile\""));
    assert!(output.contains("\"adapter\""));
    assert!(output.contains("\"entry\""));
    assert!(output.contains("\"lifecycle\""));
    assert!(output.contains("\"validation\""));
    assert!(!output.contains("private_garden_raw"));
    assert!(!output.contains("subject_state_raw"));
    assert!(!output.contains("soul_governance_raw"));
}

#[test]
fn memory_cli_write_then_recall_uses_entry_runtime_store() {
    let root = unique_temp_dir("bm-cli-entry-runtime");
    let store = root.to_string_lossy().to_string();

    let write = run_cli([
        "memory",
        "write-procedural",
        "--profile",
        "profile-server-linux-dev-full",
        "--store-file",
        &store,
        "--agent",
        "cli-agent",
        "--owner",
        "owner-default",
        "--channel",
        "local",
        "--chat",
        "chat-1",
        "--name",
        "runtime_skill__cli_entry",
        "--topic",
        "cli-entry",
        "--title",
        "CLI entry runtime",
        "--summary",
        "CLI writes procedural memory through bm-entry.",
        "--content",
        "1. Open EntryRuntime with an explicit profile and store.\n2. Normalize source, auth, and idempotency metadata.\n3. Dispatch the AdapterCommand through the SDK runtime.\n4. Return only the adapter report envelope.",
    ]
    .into_iter()
    .map(str::to_string))
    .expect("write");
    let write_json: serde_json::Value = serde_json::from_str(&write).expect("write json");
    assert_eq!(write_json["status"], "accepted");
    assert_eq!(write_json["operation"], "write.procedural");
    assert_eq!(write_json["accepted"], true);

    let recall = run_cli(
        [
            "memory",
            "recall",
            "--profile",
            "profile-server-linux-dev-full",
            "--store-file",
            &store,
            "--agent",
            "cli-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--query",
            "entry runtime cli",
            "--limit",
            "4",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("recall");
    let recall_json: serde_json::Value = serde_json::from_str(&recall).expect("recall json");
    assert_eq!(recall_json["status"], "accepted");
    assert!(recall_json["procedural_hits"]
        .as_array()
        .is_some_and(|hits| !hits.is_empty()));
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

#[test]
fn memory_cli_binary_can_reopen_file_store_across_processes() {
    let root = unique_temp_dir("bm-cli-binary-entry-runtime");
    let store = root.to_string_lossy().to_string();

    let write = std::process::Command::new(env!("CARGO_BIN_EXE_bm"))
        .args([
            "memory",
            "write-procedural",
            "--profile",
            "profile-server-linux-dev-full",
            "--store-file",
            &store,
            "--agent",
            "cli-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--name",
            "runtime_skill__cli_binary_entry",
            "--topic",
            "cli-entry",
            "--title",
            "CLI binary entry runtime",
            "--summary",
            "CLI binary writes procedural memory through bm-entry.",
            "--content",
            "1. Open EntryRuntime from the CLI binary.\n2. Persist through the configured file store.\n3. Reopen the same store from a second process.\n4. Recall the procedural memory through SDK dispatch.",
        ])
        .output()
        .expect("write command");
    assert!(
        write.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&write.stderr)
    );

    let recall = std::process::Command::new(env!("CARGO_BIN_EXE_bm"))
        .args([
            "memory",
            "recall",
            "--profile",
            "profile-server-linux-dev-full",
            "--store-file",
            &store,
            "--agent",
            "cli-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--query",
            "cli binary entry runtime",
            "--limit",
            "4",
        ])
        .output()
        .expect("recall command");
    assert!(
        recall.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recall.stderr)
    );

    let value: serde_json::Value = serde_json::from_slice(&recall.stdout).expect("recall json");
    assert!(value["procedural_hits"]
        .as_array()
        .is_some_and(|hits| !hits.is_empty()));
}
