use bm_core::memory::{
    run_persona_governance_replay_suite, run_recall_benchmark_suite, PersonaGovernanceReplayCase,
    RecallBenchmarkCase,
};
use bm_core::{Error, Result};
use bm_sdk::ProfileId;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkClass {
    RecallMultisession,
    TemporalUpdate,
    SubjectProjection,
    SoulRegression,
    ProceduralReuse,
    PrivacyRefusal,
}

impl MemoryBenchmarkClass {
    pub const ALL: [Self; 6] = [
        Self::RecallMultisession,
        Self::TemporalUpdate,
        Self::SubjectProjection,
        Self::SoulRegression,
        Self::ProceduralReuse,
        Self::PrivacyRefusal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RecallMultisession => "recall_multisession",
            Self::TemporalUpdate => "temporal_update",
            Self::SubjectProjection => "subject_projection",
            Self::SoulRegression => "soul_regression",
            Self::ProceduralReuse => "procedural_reuse",
            Self::PrivacyRefusal => "privacy_refusal",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkMode {
    Compact,
    Full,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkFixture {
    #[serde(default)]
    pub schema: String,
    pub fixture_id: String,
    pub class: MemoryBenchmarkClass,
    pub profile: ProfileId,
    pub mode: MemoryBenchmarkMode,
    #[serde(default)]
    pub evaluation_source: MemoryBenchmarkEvaluationSource,
    pub description: String,
    #[serde(default)]
    pub scenario: MemoryBenchmarkScenario,
    pub metrics: MemoryBenchmarkMetrics,
    pub thresholds: MemoryBenchmarkThresholds,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkEvaluationSource {
    #[default]
    ContractBaseline,
    RuntimeReplay,
    GoldenJudge,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkScenario {
    #[serde(default)]
    pub user_goal: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub expected_surfaces: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkMetrics {
    pub accuracy_bps: u16,
    pub evidence_precision_bps: u16,
    pub projection_faithfulness_bps: u16,
    pub privacy_violation_count: u32,
    pub stale_memory_false_positive_count: u32,
    pub procedural_reuse_success_bps: u16,
    pub soul_regression_count: u32,
    pub latency_ms: u32,
    pub token_budget: u32,
    pub memory_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkThresholds {
    #[serde(default)]
    pub min_accuracy_bps: u16,
    #[serde(default)]
    pub min_evidence_precision_bps: u16,
    #[serde(default)]
    pub min_projection_faithfulness_bps: u16,
    #[serde(default)]
    pub max_privacy_violation_count: u32,
    #[serde(default)]
    pub max_stale_memory_false_positive_count: u32,
    #[serde(default)]
    pub min_procedural_reuse_success_bps: u16,
    #[serde(default)]
    pub max_soul_regression_count: u32,
    #[serde(default)]
    pub max_latency_ms: Option<u32>,
    #[serde(default)]
    pub max_token_budget: Option<u32>,
    #[serde(default)]
    pub max_memory_bytes: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkBaseline {
    pub accuracy_bps: u16,
    pub evidence_precision_bps: u16,
    pub projection_faithfulness_bps: u16,
    pub privacy_violation_count: u32,
    pub stale_memory_false_positive_count: u32,
    pub procedural_reuse_success_bps: u16,
    pub soul_regression_count: u32,
    pub latency_ms: u32,
    pub token_budget: u32,
    pub memory_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkClassCoverage {
    pub class: MemoryBenchmarkClass,
    pub compact_fixtures: usize,
    pub full_fixtures: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkMissingClass {
    pub class: MemoryBenchmarkClass,
    pub mode: MemoryBenchmarkMode,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkFailure {
    pub fixture_id: String,
    pub class: MemoryBenchmarkClass,
    pub mode: MemoryBenchmarkMode,
    pub profile: ProfileId,
    pub stage: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkReport {
    pub suite: String,
    pub total_fixtures: usize,
    pub passed_fixtures: usize,
    pub baseline: MemoryBenchmarkBaseline,
    pub class_coverage: Vec<MemoryBenchmarkClassCoverage>,
    pub missing_classes: Vec<MemoryBenchmarkMissingClass>,
    pub failures: Vec<MemoryBenchmarkFailure>,
    pub passed: bool,
}

pub fn load_memory_benchmark_fixture_dir(
    root: impl AsRef<Path>,
) -> Result<Vec<MemoryBenchmarkFixture>> {
    let mut files = Vec::new();
    collect_json_files(root.as_ref(), &mut files)?;
    files.sort();

    let mut fixtures = Vec::with_capacity(files.len());
    for file in files {
        let body = fs::read_to_string(&file)
            .map_err(|source| Error::io("memory_benchmark_fixture_read", source))?;
        let fixture = serde_json::from_str::<MemoryBenchmarkFixture>(&body).map_err(|source| {
            Error::Other {
                source: Box::new(source),
                stage: "memory_benchmark_fixture_parse",
            }
        })?;
        fixtures.push(fixture);
    }
    Ok(fixtures)
}

pub fn run_memory_benchmark_wall(fixtures: &[MemoryBenchmarkFixture]) -> MemoryBenchmarkReport {
    let class_coverage = MemoryBenchmarkClass::ALL
        .iter()
        .copied()
        .map(|class| MemoryBenchmarkClassCoverage {
            class,
            compact_fixtures: fixtures
                .iter()
                .filter(|fixture| {
                    fixture.class == class && fixture.mode == MemoryBenchmarkMode::Compact
                })
                .count(),
            full_fixtures: fixtures
                .iter()
                .filter(|fixture| {
                    fixture.class == class && fixture.mode == MemoryBenchmarkMode::Full
                })
                .count(),
        })
        .collect::<Vec<_>>();

    let mut missing_classes = Vec::new();
    for coverage in &class_coverage {
        if coverage.compact_fixtures == 0 {
            missing_classes.push(MemoryBenchmarkMissingClass {
                class: coverage.class,
                mode: MemoryBenchmarkMode::Compact,
            });
        }
        if coverage.full_fixtures == 0 {
            missing_classes.push(MemoryBenchmarkMissingClass {
                class: coverage.class,
                mode: MemoryBenchmarkMode::Full,
            });
        }
    }

    let failures = fixtures
        .iter()
        .flat_map(validate_memory_benchmark_fixture)
        .collect::<Vec<_>>();
    let failed_fixture_count = fixtures
        .iter()
        .filter(|fixture| {
            failures
                .iter()
                .any(|failure| failure.fixture_id == fixture.fixture_id)
        })
        .count();

    MemoryBenchmarkReport {
        suite: "memory_benchmark_wall".to_string(),
        total_fixtures: fixtures.len(),
        passed_fixtures: fixtures.len().saturating_sub(failed_fixture_count),
        baseline: calculate_memory_benchmark_baseline(fixtures),
        class_coverage,
        missing_classes,
        passed: failures.is_empty(),
        failures,
    }
    .with_missing_class_gate()
}

fn collect_json_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(root).map_err(|source| Error::io("memory_benchmark_fixture_list", source))?
    {
        let entry =
            entry.map_err(|source| Error::io("memory_benchmark_fixture_list_entry", source))?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            files.push(path);
        }
    }
    Ok(())
}

fn validate_memory_benchmark_fixture(
    fixture: &MemoryBenchmarkFixture,
) -> Vec<MemoryBenchmarkFailure> {
    let mut failures = Vec::new();
    push_min_bps_failure(
        &mut failures,
        fixture,
        "accuracy_bps",
        fixture.metrics.accuracy_bps,
        fixture.thresholds.min_accuracy_bps,
    );
    push_min_bps_failure(
        &mut failures,
        fixture,
        "evidence_precision_bps",
        fixture.metrics.evidence_precision_bps,
        fixture.thresholds.min_evidence_precision_bps,
    );
    push_min_bps_failure(
        &mut failures,
        fixture,
        "projection_faithfulness_bps",
        fixture.metrics.projection_faithfulness_bps,
        fixture.thresholds.min_projection_faithfulness_bps,
    );
    push_max_u32_failure(
        &mut failures,
        fixture,
        "privacy_violation_count",
        fixture.metrics.privacy_violation_count,
        fixture.thresholds.max_privacy_violation_count,
    );
    push_max_u32_failure(
        &mut failures,
        fixture,
        "stale_memory_false_positive_count",
        fixture.metrics.stale_memory_false_positive_count,
        fixture.thresholds.max_stale_memory_false_positive_count,
    );
    push_min_bps_failure(
        &mut failures,
        fixture,
        "procedural_reuse_success_bps",
        fixture.metrics.procedural_reuse_success_bps,
        fixture.thresholds.min_procedural_reuse_success_bps,
    );
    push_max_u32_failure(
        &mut failures,
        fixture,
        "soul_regression_count",
        fixture.metrics.soul_regression_count,
        fixture.thresholds.max_soul_regression_count,
    );
    if let Some(max_latency_ms) = fixture.thresholds.max_latency_ms {
        push_max_u32_failure(
            &mut failures,
            fixture,
            "latency_ms",
            fixture.metrics.latency_ms,
            max_latency_ms,
        );
    }
    if let Some(max_token_budget) = fixture.thresholds.max_token_budget {
        push_max_u32_failure(
            &mut failures,
            fixture,
            "token_budget",
            fixture.metrics.token_budget,
            max_token_budget,
        );
    }
    if let Some(max_memory_bytes) = fixture.thresholds.max_memory_bytes {
        if fixture.metrics.memory_bytes > max_memory_bytes {
            failures.push(memory_benchmark_failure(
                fixture,
                "memory_bytes",
                format!(
                    "expected at most {}, got {}",
                    max_memory_bytes, fixture.metrics.memory_bytes
                ),
            ));
        }
    }
    failures
}

fn calculate_memory_benchmark_baseline(
    fixtures: &[MemoryBenchmarkFixture],
) -> MemoryBenchmarkBaseline {
    if fixtures.is_empty() {
        return MemoryBenchmarkBaseline::default();
    }
    let len = fixtures.len() as u32;
    MemoryBenchmarkBaseline {
        accuracy_bps: average_bps(fixtures.iter().map(|fixture| fixture.metrics.accuracy_bps)),
        evidence_precision_bps: average_bps(
            fixtures
                .iter()
                .map(|fixture| fixture.metrics.evidence_precision_bps),
        ),
        projection_faithfulness_bps: average_bps(
            fixtures
                .iter()
                .map(|fixture| fixture.metrics.projection_faithfulness_bps),
        ),
        privacy_violation_count: fixtures
            .iter()
            .map(|fixture| fixture.metrics.privacy_violation_count)
            .sum(),
        stale_memory_false_positive_count: fixtures
            .iter()
            .map(|fixture| fixture.metrics.stale_memory_false_positive_count)
            .sum(),
        procedural_reuse_success_bps: average_bps(
            fixtures
                .iter()
                .map(|fixture| fixture.metrics.procedural_reuse_success_bps),
        ),
        soul_regression_count: fixtures
            .iter()
            .map(|fixture| fixture.metrics.soul_regression_count)
            .sum(),
        latency_ms: fixtures
            .iter()
            .map(|fixture| fixture.metrics.latency_ms)
            .sum::<u32>()
            / len,
        token_budget: fixtures
            .iter()
            .map(|fixture| fixture.metrics.token_budget)
            .sum::<u32>()
            / len,
        memory_bytes: fixtures
            .iter()
            .map(|fixture| fixture.metrics.memory_bytes)
            .sum::<u64>()
            / fixtures.len() as u64,
    }
}

fn average_bps(values: impl Iterator<Item = u16>) -> u16 {
    let mut total = 0u32;
    let mut count = 0u32;
    for value in values {
        total += u32::from(value);
        count += 1;
    }
    if count == 0 {
        return 0;
    }
    (total / count) as u16
}

fn push_min_bps_failure(
    failures: &mut Vec<MemoryBenchmarkFailure>,
    fixture: &MemoryBenchmarkFixture,
    stage: &str,
    got: u16,
    min: u16,
) {
    if got < min {
        failures.push(memory_benchmark_failure(
            fixture,
            stage,
            format!("expected at least {min}, got {got}"),
        ));
    }
}

fn push_max_u32_failure(
    failures: &mut Vec<MemoryBenchmarkFailure>,
    fixture: &MemoryBenchmarkFixture,
    stage: &str,
    got: u32,
    max: u32,
) {
    if got > max {
        failures.push(memory_benchmark_failure(
            fixture,
            stage,
            format!("expected at most {max}, got {got}"),
        ));
    }
}

fn memory_benchmark_failure(
    fixture: &MemoryBenchmarkFixture,
    stage: impl Into<String>,
    reason: impl Into<String>,
) -> MemoryBenchmarkFailure {
    MemoryBenchmarkFailure {
        fixture_id: fixture.fixture_id.clone(),
        class: fixture.class,
        mode: fixture.mode,
        profile: fixture.profile,
        stage: stage.into(),
        reason: reason.into(),
    }
}

impl MemoryBenchmarkReport {
    fn with_missing_class_gate(mut self) -> Self {
        if !self.missing_classes.is_empty() {
            self.passed = false;
        }
        self
    }
}
