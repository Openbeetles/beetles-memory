use bm_core::{
    build_archive_evidence_block, search_archive_records, select_archive_hits_for_prompt,
    ArchiveRecord, ArchiveRecordLocator, ArchiveRecordSource, ArchiveSearchBackendKind,
    ArchiveSearchQuery, RuntimeProfile, SourceKind, SourceRef,
};

#[test]
fn archive_search_reports_hits_scores_and_source_stats() {
    let records = vec![
        archive_record(
            "a1",
            ArchiveRecordSource::Transcript,
            "S5 archive kernel stores evidence",
        ),
        archive_record("a2", ArchiveRecordSource::DailyNote, "unrelated daily note"),
    ];
    let query = ArchiveSearchQuery::new("task:s5", "archive evidence", RuntimeProfile::DevFull);

    let result = search_archive_records(&query, &records, ArchiveSearchBackendKind::StoreScan);

    assert_eq!(result.hits.len(), 2);
    assert_eq!(result.hits[0].record_id, "a1");
    assert!(result.hits[0].score.total > 0);
    assert_eq!(result.report.candidates, 2);
    assert_eq!(result.report.hits, 2);
    assert!(result
        .report
        .source_stats
        .iter()
        .any(|stat| stat.source == ArchiveRecordSource::Transcript && stat.hits == 1));
}

#[test]
fn archive_selector_dedupes_and_builds_noncanonical_evidence_block() {
    let records = vec![
        archive_record(
            "a1",
            ArchiveRecordSource::Transcript,
            "archive evidence proves S5",
        ),
        archive_record(
            "a2",
            ArchiveRecordSource::Transcript,
            "archive evidence proves S5",
        ),
        archive_record(
            "a3",
            ArchiveRecordSource::TurnLog,
            "archive evidence comes from turn log",
        ),
    ];
    let query = ArchiveSearchQuery::new("task:s5", "archive evidence", RuntimeProfile::DevFull);
    let result = search_archive_records(&query, &records, ArchiveSearchBackendKind::StoreScan);
    let report = result.report.clone();
    let selection = select_archive_hits_for_prompt(result.hits, RuntimeProfile::DevFull);

    assert_eq!(selection.report.selected, 2);
    assert_eq!(selection.report.skipped_by_similarity, 1);

    let block = build_archive_evidence_block(report, selection);
    assert_eq!(block.lines.len(), 2);
    assert!(block.lines[0].contains("a1") || block.lines[0].contains("a3"));
}

fn archive_record(id: &str, source: ArchiveRecordSource, content: &str) -> ArchiveRecord {
    ArchiveRecord {
        id: id.to_owned(),
        locator: ArchiveRecordLocator {
            source,
            scope: "task:s5".to_owned(),
            record_id: id.to_owned(),
        },
        title: format!("archive {id}"),
        content: content.to_owned(),
        cues: vec!["archive".to_owned(), "evidence".to_owned()],
        observed_at: Some(10),
        source_ref: SourceRef::new(SourceKind::ArchiveEvidence, format!("archive:{id}")),
    }
}
