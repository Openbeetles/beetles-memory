#[test]
fn s6_replay_covers_procedural_skill_paths() {
    let report = bm_replay::run_s6_replay();

    assert_eq!(report.user_provided_inserted, 1);
    assert_eq!(report.import_quarantined, 1);
    assert_eq!(report.import_adopted, 1);
    assert_eq!(report.runtime_rejected, 1);
    assert_eq!(report.runtime_accepted, 1);
    assert_eq!(report.outcome_updates, 2);
    assert!(report.recall_selected >= 1);
    assert!(report.projection_blocks >= 1);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("quarantined")));
}
