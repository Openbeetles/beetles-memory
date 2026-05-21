use bm_core::memory::{
    RecallBenchmarkCase, RecallCandidate, RecallPlane, RecallQuery, RecallScoreBreakdown,
    RecallSelectionReport,
};
use bm_replay::run_recall_benchmark_gate;

#[test]
fn recall_benchmark_gate_reports_regression_failures() {
    let passing = RecallBenchmarkCase {
        name: "runtime_skill_recall",
        plane: RecallPlane::RuntimeSkill,
        report: RecallSelectionReport {
            plane: RecallPlane::RuntimeSkill,
            query: RecallQuery {
                plane: RecallPlane::RuntimeSkill,
                raw_query: "release artifact".to_string(),
                requested_limit: 2,
                ..RecallQuery::default()
            },
            candidates: vec![candidate("skill-a", true), candidate("skill-b", false)],
            selected_ids: vec!["skill-a".to_string()],
            selected_count: 1,
            candidate_count: 2,
            ..RecallSelectionReport::default()
        },
        relevant_candidate_ids: vec!["skill-a".to_string()],
        expected_top_candidate_id: Some("skill-a".to_string()),
        top_k: 2,
        min_recall_at_k: 1.0,
        min_precision_at_k: 0.5,
        min_mrr: 1.0,
        min_ndcg: 1.0,
    };
    let failing = RecallBenchmarkCase {
        name: "runtime_skill_miss",
        expected_top_candidate_id: Some("missing".to_string()),
        ..passing.clone()
    };

    let report = run_recall_benchmark_gate(&[passing, failing]);

    assert!(!report.passed);
    assert_eq!(report.cases, 2);
    assert_eq!(report.passed_cases, 1);
    assert_eq!(report.failed_cases, vec!["runtime_skill_miss".to_string()]);
}

fn candidate(id: &str, selected: bool) -> RecallCandidate {
    RecallCandidate {
        plane: RecallPlane::RuntimeSkill,
        candidate_id: id.to_string(),
        title: id.to_string(),
        excerpt: "release artifact guard".to_string(),
        selected,
        score: RecallScoreBreakdown {
            total_score: if selected { 100 } else { 10 },
            ..RecallScoreBreakdown::default()
        },
        ..RecallCandidate::default()
    }
}
