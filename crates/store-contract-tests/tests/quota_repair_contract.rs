use bm_sdk::nonproduction_replay_harness::StoreRepairReport;

#[test]
fn quota_violation_reports_pressure_without_authorizing_host_deletion() {
    let report = StoreRepairReport::quota_pressure("long_term", 12, 10, "import", true);

    assert!(report.checked);
    assert!(!report.repaired);
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.contains("quota_exceeded")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.contains("host_deletion_allowed=false")));
}

#[test]
fn clean_repair_report_is_fail_closed_by_default() {
    let report = StoreRepairReport::clean();
    assert!(report.checked);
    assert!(!report.repaired);
    assert!(report.findings.is_empty());
}
