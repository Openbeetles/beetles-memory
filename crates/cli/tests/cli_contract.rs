use bm_cli::{command_specs, render_capabilities};
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
    assert!(output.contains("\"lifecycle\""));
    assert!(output.contains("\"validation\""));
    assert!(!output.contains("private_garden_raw"));
    assert!(!output.contains("subject_state_raw"));
    assert!(!output.contains("soul_governance_raw"));
}
