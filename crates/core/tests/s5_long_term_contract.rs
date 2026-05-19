use bm_core::{
    canonicalize_long_term_draft, inspect_long_term_merge, ArchiveEvidenceLink,
    ArchiveRecordLocator, ArchiveRecordSource, Confidence, EvidenceState, Freshness,
    LongTermMemoryDraft, LongTermMemoryKind, LongTermWriteAction, LongTermWriteReason, MemoryPlane,
    MemoryRecord, SourceKind, SourceRef,
};

#[test]
fn long_term_slot_and_defaults_are_stable() {
    let draft = canonicalize_long_term_draft(draft("Project Status", "Beetle Memory is S5"));

    assert_eq!(
        draft.slot().stable_id(),
        "Project:agent:test:task:s5:project status"
    );
    assert!(draft.canonical);
    assert_eq!(draft.confidence, Confidence::Medium);
    assert_eq!(draft.freshness, Freshness::Unknown);
}

#[test]
fn lower_confidence_and_older_revision_do_not_replace_existing_slot() {
    let existing = existing_record(
        "mem-1",
        "Project Status",
        "newer high confidence content",
        Confidence::High,
        Some(20),
    );
    let mut incoming = draft("Project Status", "older low confidence content");
    incoming.confidence = Confidence::Low;
    incoming.observed_at = Some(10);

    let report = inspect_long_term_merge(Some(&existing), &incoming);

    assert_eq!(report.action, LongTermWriteAction::Rejected);
    assert_eq!(
        report.reason,
        LongTermWriteReason::LowerConfidenceThanExisting
    );
    assert_eq!(report.existing_record_id.as_deref(), Some("mem-1"));
}

#[test]
fn same_slot_archive_support_refreshes_without_duplicate_fact() {
    let existing = existing_record(
        "mem-1",
        "Project Status",
        "Beetle Memory is in S5",
        Confidence::Medium,
        Some(10),
    );
    let mut incoming = draft("Project Status", "Beetle Memory is in S5");
    incoming.archive_links.push(ArchiveEvidenceLink {
        locator: ArchiveRecordLocator {
            source: ArchiveRecordSource::Transcript,
            scope: "task:s5".to_owned(),
            record_id: "archive-1".to_owned(),
        },
        supports: true,
        reason: "matching transcript evidence".to_owned(),
    });

    let report = inspect_long_term_merge(Some(&existing), &incoming);

    assert_eq!(report.action, LongTermWriteAction::Refreshed);
    assert_eq!(report.reason, LongTermWriteReason::ArchiveSupported);
    assert_eq!(report.archive_support_count, 1);
}

fn draft(topic: &str, content: &str) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind: LongTermMemoryKind::Project,
        identity: "agent:test".to_owned(),
        scope: "task:s5".to_owned(),
        topic: topic.to_owned(),
        content: content.to_owned(),
        keywords: vec!["Beetle".to_owned(), "memory".to_owned()],
        source: SourceRef::new(SourceKind::Manual, "unit-test"),
        evidence: EvidenceState::Supported,
        confidence: Confidence::Medium,
        freshness: Freshness::Unknown,
        observed_at: Some(10),
        canonical: true,
        archive_links: Vec::new(),
    }
}

fn existing_record(
    id: &str,
    topic: &str,
    content: &str,
    confidence: Confidence,
    observed_at: Option<u64>,
) -> MemoryRecord {
    let draft = LongTermMemoryDraft {
        confidence,
        observed_at,
        ..draft(topic, content)
    };
    MemoryRecord {
        id: id.to_owned(),
        identity: "agent:test".to_owned(),
        scope: "task:s5".to_owned(),
        content: content.to_owned(),
        source: "unit-test".to_owned(),
        domain: MemoryPlane::SharedFactual.domain(),
        plane: MemoryPlane::SharedFactual,
        meta: draft.into_meta(1),
    }
}
