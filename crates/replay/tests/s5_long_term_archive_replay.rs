use bm_replay::run_s5_replay;

#[test]
fn s5_replay_covers_long_term_archive_and_extraction_paths() {
    let report = run_s5_replay();

    assert_eq!(report.inserted, 1);
    assert_eq!(report.replaced, 1);
    assert_eq!(report.rejected, 1);
    assert_eq!(report.archive_hits, 1);
    assert_eq!(report.deleted, 1);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("ArchiveEvidenceNotCanonical")));
}
