use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn config() -> EntryRuntimeConfig {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "workbench-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "console".to_string(),
            chat_id: "workbench-chat".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_disabled().with_cli(true),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 16 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    }
}

#[test]
fn workbench_api_map_is_entry_owned_and_private_raw_closed() {
    let runtime = EntryRuntime::open(config()).expect("runtime");

    let map = runtime.console_workbench_api_map();

    let surface_ids = map
        .surfaces
        .iter()
        .map(|surface| surface.surface_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        surface_ids,
        vec![
            "home",
            "recall_inspector",
            "projection_inspector",
            "soul_health",
            "procedural_evolution",
            "replay_diff",
            "vault_migration",
        ]
    );
    assert!(map.missing_report_apis.is_empty());
    assert!(map
        .surfaces
        .iter()
        .all(|surface| !surface.private_raw_allowed));
    assert!(map
        .surfaces
        .iter()
        .any(|surface| surface.report_api == "sdk.project.subject_projection"));
}

#[test]
fn workbench_report_exposes_real_runtime_reports_without_private_raw_surfaces() {
    let runtime = EntryRuntime::open(config()).expect("runtime");

    let report = runtime.console_workbench_report();

    assert_eq!(report.api_map.surfaces.len(), 7);
    assert!(report.benchmark_wall.report.is_some());
    assert!(
        report
            .benchmark_wall
            .report
            .as_ref()
            .expect("benchmark report")
            .passed
    );
    assert_eq!(report.recall_inspector.query, "workbench memory inspection");
    assert!(!report.recall_inspector.high_confidence_projection_allowed);
    assert_eq!(
        report.projection_inspector.private_echo_count, 0,
        "console workbench must not surface private raw echoes"
    );
    assert!(report.projection_inspector.private_echo_guard_passed);
    assert!(report.vault_migration.preflight_passed);
}
