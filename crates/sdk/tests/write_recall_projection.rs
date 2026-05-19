use bm_core::{
    MemoryDomain, MemoryPlane, ProjectionSurface, RecallQuery, RuntimeProfile, WriteCandidate,
    WriteDecision,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn runtime_writes_recalls_and_projects_governed_memory() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let report = runtime.write(
        WriteCandidate::new("user:1", "task:1", "tool result should be reused")
            .source("unit-test")
            .plane_hint(MemoryPlane::Procedural),
    );

    assert_eq!(report.decision, WriteDecision::Accepted);
    assert_eq!(report.domain, Some(MemoryDomain::Program));
    assert_eq!(report.plane, Some(MemoryPlane::Procedural));
    assert!(report.record_id.is_some());
    assert_eq!(report.governance.reason, "accepted");

    let recalled = runtime.recall(
        RecallQuery::new("task:1")
            .domain(MemoryDomain::Program)
            .plane(MemoryPlane::Procedural)
            .limit(4),
    );

    assert_eq!(recalled.selected.len(), 1);
    assert_eq!(recalled.skipped, 0);
    assert_eq!(recalled.profile, RuntimeProfile::DevFull);
    assert_eq!(recalled.selected[0].content, "tool result should be reused");

    let projection = runtime.project(&recalled, ProjectionSurface::Prompt);

    assert_eq!(projection.surface, ProjectionSurface::Prompt);
    assert_eq!(projection.blocks.len(), 1);
    assert_eq!(projection.blocks[0].plane, MemoryPlane::Procedural);
    assert_eq!(projection.blocks[0].content, "tool result should be reused");
}

#[test]
fn esp_profile_rejects_soul_governance_writes() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::EspCompact)
        .store(store)
        .build();

    let report = runtime.write(
        WriteCandidate::new("agent:1", "task:1", "core self revision")
            .source("unit-test")
            .plane_hint(MemoryPlane::SoulGovernance),
    );

    assert_eq!(report.decision, WriteDecision::Rejected);
    assert_eq!(report.record_id, None);
    assert_eq!(report.governance.reason, "profile_rejected");
}

#[test]
fn invalid_candidates_are_rejected_before_store_mutation() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let report = runtime.write(WriteCandidate::new("user:1", "task:1", "   "));

    assert_eq!(report.decision, WriteDecision::Rejected);
    assert_eq!(report.record_id, None);
    assert_eq!(report.governance.reason, "empty_content");

    let recalled = runtime.recall(RecallQuery::new("task:1").limit(10));
    assert!(recalled.selected.is_empty());
}
