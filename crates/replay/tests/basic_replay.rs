use bm_replay::run_basic_replay;

#[test]
fn basic_replay_reproduces_write_recall_projection_chain() {
    let report = run_basic_replay();

    assert_eq!(report.write_accepted, 1);
    assert_eq!(report.recall_selected, 1);
    assert_eq!(report.projection_blocks, 1);
    assert_eq!(report.profile, "DevFull");
}
