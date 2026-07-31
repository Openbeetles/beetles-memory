use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
use bm_sdk::{
    MemoryArchiveScope, MemoryCapabilityPolicy, MemoryPrivacyPolicy,
    MemorySpacePrivateMaterialPolicy, StoreBackendConfig,
};

mod support;

fn config() -> EntryRuntimeConfig {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let profile = support::host_production_profile();
    EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "workbench-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "console".to_string(),
            chat_id: "workbench-chat".to_string(),
        },
        store: StoreBackendConfig::in_memory(profile)
            .expect("store config")
            .with_fsync(false),
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
            "facet_inspector",
            "projection_inspector",
            "soul_health",
            "procedural_evolution",
            "replay_diff",
            "archive_restore",
        ]
    );
    #[cfg(feature = "nonproduction-replay-harness")]
    assert!(map.missing_report_apis.is_empty());
    #[cfg(not(feature = "nonproduction-replay-harness"))]
    assert_eq!(
        map.missing_report_apis,
        vec!["sdk.replay.memory_benchmark_report".to_string()]
    );
    assert!(map
        .surfaces
        .iter()
        .all(|surface| !surface.private_raw_allowed));
    assert!(map
        .surfaces
        .iter()
        .any(|surface| surface.report_api == "sdk.project.subject_projection"));
    let facet_surface = map
        .surfaces
        .iter()
        .find(|surface| surface.surface_id == "facet_inspector")
        .expect("facet inspector surface");
    assert_eq!(facet_surface.report_api, "sdk.recall.facet_index_report");
    assert!(!facet_surface.private_raw_allowed);
}

#[test]
fn workbench_report_exposes_real_runtime_reports_without_private_raw_surfaces() {
    let runtime = EntryRuntime::open(config()).expect("runtime");

    let report = runtime.console_workbench_report();

    assert_eq!(report.api_map.surfaces.len(), 8);
    assert_eq!(report.facet_inspector.status.status, "ready");
    assert!(report.facet_inspector.report_only);
    assert!(!report.facet_inspector.direct_mutation_allowed);
    assert_eq!(
        report.facet_inspector.audit_markdown_format,
        "obsidian-style-facet-audit-markdown"
    );
    #[cfg(feature = "nonproduction-replay-harness")]
    {
        assert!(report.benchmark_wall.report.is_some());
        let benchmark = report
            .benchmark_wall
            .report
            .as_ref()
            .expect("benchmark report");
        assert!(benchmark.passed);
        assert!(benchmark.soul_kernel_judge.release_gate_passed);
        assert!(benchmark.subject_projection_judge.release_gate_passed);
    }
    #[cfg(not(feature = "nonproduction-replay-harness"))]
    {
        assert!(report.benchmark_wall.report.is_none());
        assert_eq!(report.benchmark_wall.status.status, "limited");
        assert_eq!(
            report.benchmark_wall.status.reason,
            "replay_harness_not_compiled"
        );
    }
    assert_eq!(report.recall_inspector.query, "workbench memory inspection");
    assert!(!report.recall_inspector.high_confidence_projection_allowed);
    assert_eq!(
        report.projection_inspector.raw_private_violation_count, 0,
        "console workbench must not surface private raw echoes"
    );
    assert!(!report.projection_inspector.foreground_disclosure_allowed);
    assert!(report.projection_inspector.disclosure_integrity_passed);
    let MemoryArchiveScope::Subject {
        memory_space_id,
        mounted_subject_id,
    } = &report.archive_restore.scope
    else {
        panic!("entry workbench archive must use Subject scope");
    };
    assert!(!memory_space_id.is_empty());
    assert!(!mounted_subject_id.is_empty());
    assert_eq!(
        report.archive_restore.private_material_policy,
        MemorySpacePrivateMaterialPolicy::ExcludePrivate
    );
    assert_eq!(report.archive_restore.status.status, "ready");
    let root = report
        .archive_restore
        .archive_root
        .as_ref()
        .expect("successful typed archive export must expose its governed root");
    assert_eq!(root.scope, report.archive_restore.scope);
    assert_eq!(
        root.private_material_policy,
        report.archive_restore.private_material_policy
    );
    assert_eq!(root.closure_sha256.len(), 64);
    assert!(root
        .closure_sha256
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit()));
    let json = serde_json::to_value(&report).expect("serialize workbench report");
    assert!(json.get("archiveRestore").is_some());
    assert!(json.get("vaultMigration").is_none());
}
