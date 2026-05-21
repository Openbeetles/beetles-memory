use bm_core::memory::{
    run_persona_governance_replay_suite, run_recall_benchmark_suite, PersonaGovernanceReplayCase,
    RecallBenchmarkCase,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BenchmarkGateReport {
    pub suite: String,
    pub cases: usize,
    pub passed_cases: usize,
    pub failed_cases: Vec<String>,
    pub passed: bool,
}

pub fn run_recall_benchmark_gate(cases: &[RecallBenchmarkCase]) -> BenchmarkGateReport {
    let results = run_recall_benchmark_suite(cases);
    let failed_cases = results
        .iter()
        .filter(|result| !result.passed)
        .map(|result| result.case_name.to_string())
        .collect::<Vec<_>>();
    BenchmarkGateReport {
        suite: "recall".to_string(),
        cases: results.len(),
        passed_cases: results.len().saturating_sub(failed_cases.len()),
        passed: failed_cases.is_empty(),
        failed_cases,
    }
}

pub fn run_persona_governance_benchmark_gate(
    cases: &[PersonaGovernanceReplayCase],
) -> BenchmarkGateReport {
    let results = run_persona_governance_replay_suite(cases);
    let failed_cases = results
        .iter()
        .filter(|result| !result.passed)
        .map(|result| result.case_name.to_string())
        .collect::<Vec<_>>();
    BenchmarkGateReport {
        suite: "persona_governance".to_string(),
        cases: results.len(),
        passed_cases: results.len().saturating_sub(failed_cases.len()),
        passed: failed_cases.is_empty(),
        failed_cases,
    }
}
