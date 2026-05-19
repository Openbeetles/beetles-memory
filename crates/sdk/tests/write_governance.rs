use bm_core::{
    MemoryDomain, MemoryPlane, RecallQuery, RuntimeProfile, SourceKind, WriteCandidate,
    WriteDecision,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn governed_write_rejects_invalid_raw_and_profile_blocked_candidates() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::EspCompact)
        .store(store)
        .build();

    let empty = runtime.write(WriteCandidate::new("agent:1", "task:1", "  "));
    assert_eq!(empty.decision, WriteDecision::Rejected);
    assert_eq!(empty.governance.reason, "empty_content");

    let missing_source = runtime.write(WriteCandidate::new("agent:1", "task:1", "Beetle Memory"));
    assert_eq!(missing_source.decision, WriteDecision::Rejected);
    assert_eq!(missing_source.governance.reason, "missing_source");

    let raw_payload = runtime.write(
        WriteCandidate::new("agent:1", "task:1", r#"{"raw":"tool payload"}"#).source("unit-test"),
    );
    assert_eq!(raw_payload.decision, WriteDecision::Rejected);
    assert_eq!(raw_payload.governance.reason, "raw_payload_or_log");

    let soul = runtime.write(
        WriteCandidate::new("agent:1", "task:1", "self core revision")
            .source("unit-test")
            .plane_hint(MemoryPlane::SoulGovernance),
    );
    assert_eq!(soul.decision, WriteDecision::Rejected);
    assert_eq!(soul.governance.reason, "profile_rejected");
}

#[test]
fn governed_write_accepts_factual_memory_with_profile_report() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let report = runtime.write(
        WriteCandidate::new("agent:1", "task:s1", "项目名称是 Beetle Memory").source("operator"),
    );

    assert_eq!(report.decision, WriteDecision::Accepted);
    assert_eq!(report.domain, Some(MemoryDomain::Program));
    assert_eq!(report.plane, Some(MemoryPlane::SharedFactual));
    assert_eq!(report.profile, Some(RuntimeProfile::DevFull));

    let recalled = runtime.recall(RecallQuery::new("task:s1").plane(MemoryPlane::SharedFactual));
    assert_eq!(recalled.selected.len(), 1);
    assert!(recalled.skipped.is_empty());
}

#[test]
fn archive_evidence_cannot_be_promoted_to_factual_without_distillation() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let report = runtime.write(
        WriteCandidate::new("agent:1", "task:s1", "archive hit says maybe true")
            .source("archive:hit-1")
            .plane_hint(MemoryPlane::SharedFactual),
    );

    assert_eq!(report.decision, WriteDecision::Rejected);
    assert_eq!(report.governance.reason, "needs_distillation");
}

#[test]
fn host_specific_source_prefixes_do_not_become_kernel_variants() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let report = runtime.write(
        WriteCandidate::new("agent:1", "task:s1", "host source remains adapter-owned")
            .source("legacy-host:memory-sample"),
    );

    assert_eq!(report.decision, WriteDecision::Accepted);
    assert_eq!(
        report.source.expect("accepted write has source").kind,
        SourceKind::AdapterEvent
    );
}
