use bm_core::{
    ArchiveEvidenceLink, ArchiveRecordLocator, ArchiveRecordSource, ArchiveSearchQuery, Confidence,
    EvidenceState, Freshness, LongTermMemoryKind, MemoryPlane, PromptRecallIntent, RecallQuery,
    RecallWarning, RuntimeProfile, WriteCandidate, WriteDecision,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn long_term_write_merges_same_slot_and_rejects_weaker_updates() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    let first = runtime.write(project_fact("Beetle Memory is implementing S5").observed_at(20));
    assert_eq!(first.decision, WriteDecision::Accepted);

    let weaker = runtime.write(
        project_fact("older weaker content")
            .confidence(Confidence::Low)
            .observed_at(10),
    );
    assert_eq!(weaker.decision, WriteDecision::Rejected);
    assert_eq!(weaker.governance.reason, "lower_confidence_than_existing");

    let merged = runtime.write(
        project_fact("Beetle Memory is implementing S5 with archive kernel").observed_at(30),
    );
    assert_eq!(merged.decision, WriteDecision::Merged);
    assert_eq!(merged.governance.reason, "same_slot_merge");

    let recall = runtime.recall(
        RecallQuery::new("task:s5")
            .identity("agent:s5")
            .intent(PromptRecallIntent::Factual),
    );
    assert_eq!(recall.selected.len(), 1);
    assert!(recall.selected[0].content.contains("archive kernel"));
}

#[test]
fn archive_evidence_searches_selects_and_stays_noncanonical() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    runtime.write(
        WriteCandidate::new(
            "agent:s5",
            "task:s5",
            "archive evidence confirms S5 merge rules",
        )
        .source("archive:transcript:1")
        .plane_hint(MemoryPlane::ArchiveEvidence)
        .topic("S5 evidence")
        .keywords(vec!["archive".to_owned(), "merge".to_owned()])
        .evidence(EvidenceState::ArchiveOnly),
    );
    let promoted = runtime.write(
        WriteCandidate::new("agent:s5", "task:s5", "archive says S5 is true")
            .source("archive:transcript:1")
            .plane_hint(MemoryPlane::SharedFactual),
    );
    assert_eq!(promoted.decision, WriteDecision::Rejected);
    assert_eq!(promoted.governance.reason, "needs_distillation");

    let search = runtime.search_archive(ArchiveSearchQuery::new(
        "task:s5",
        "archive merge",
        RuntimeProfile::DevFull,
    ));
    assert_eq!(search.hits.len(), 1);
    assert_eq!(search.report.hits, 1);

    let block = runtime.select_archive_for_prompt(ArchiveSearchQuery::new(
        "task:s5",
        "archive merge",
        RuntimeProfile::DevFull,
    ));
    assert_eq!(block.lines.len(), 1);

    let recall = runtime.recall(
        RecallQuery::new("task:s5")
            .intent(PromptRecallIntent::Evidence)
            .plane(MemoryPlane::ArchiveEvidence),
    );
    assert_eq!(recall.selected.len(), 1);
    assert!(!recall.selected[0].canonical);
    assert!(recall
        .warnings
        .iter()
        .any(|warning| matches!(warning, RecallWarning::ArchiveEvidenceNotCanonical { .. })));
}

#[test]
fn archive_support_refreshes_existing_fact_without_duplicate_record() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    runtime.write(project_fact("Beetle Memory S5 is active").observed_at(10));
    let refreshed = runtime.write(
        project_fact("Beetle Memory S5 is active")
            .observed_at(20)
            .archive_links(vec![ArchiveEvidenceLink {
                locator: ArchiveRecordLocator {
                    source: ArchiveRecordSource::Transcript,
                    scope: "task:s5".to_owned(),
                    record_id: "archive-1".to_owned(),
                },
                supports: true,
                reason: "transcript supports existing fact".to_owned(),
            }]),
    );

    assert_eq!(refreshed.decision, WriteDecision::Merged);
    assert_eq!(refreshed.governance.reason, "archive_supported");
    let recall = runtime.recall(RecallQuery::new("task:s5").intent(PromptRecallIntent::Factual));
    assert_eq!(recall.selected.len(), 1);
    assert_eq!(recall.selected[0].meta.archive_links.len(), 1);
}

#[test]
fn stale_long_term_memory_is_visible_in_recall_report() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    runtime.write(project_fact("stale S5 note").freshness(Freshness::Stale));
    let recall = runtime.recall(RecallQuery::new("task:s5").intent(PromptRecallIntent::Factual));

    assert!(recall
        .warnings
        .iter()
        .any(|warning| matches!(warning, RecallWarning::StaleLongTermMemory { .. })));
}

fn project_fact(content: &str) -> WriteCandidate {
    WriteCandidate::new("agent:s5", "task:s5", content)
        .source("unit-test")
        .plane_hint(MemoryPlane::SharedFactual)
        .long_term_kind(LongTermMemoryKind::Project)
        .topic("S5 status")
        .confidence(Confidence::Medium)
        .freshness(Freshness::Current)
        .canonical(true)
}
