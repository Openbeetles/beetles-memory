use bm_bridge_beetle::{BeetleMemorySource, MigrationPlanner};
use bm_core::{MemoryDomain, MemoryPlane};

#[test]
fn bridge_creates_source_provenance_without_beetle_product_logic() {
    let source = BeetleMemorySource::new("beetle-shared-factual", "record-1", "stable fact")
        .origin_path("src/memory/legacy.rs");

    let plan = MigrationPlanner::default().plan(source);

    assert_eq!(plan.source.system, "beetle-shared-factual");
    assert_eq!(plan.target_domain, MemoryDomain::Program);
    assert_eq!(plan.target_plane, MemoryPlane::SharedFactual);
    assert_eq!(plan.candidate.content, "stable fact");
    assert_eq!(plan.candidate.source.as_deref(), Some("beetle:record-1"));
}
