use bm_core::memory::{
    run_persona_governance_replay_suite, run_recall_benchmark_suite, PersonaGovernanceReplayCase,
    RecallBenchmarkCase,
};
use bm_core::{Error, Result};
use bm_sdk::ProfileId;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
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
    AgentToolExperience,
}

impl MemoryBenchmarkClass {
    pub const ALL: [Self; 7] = [
        Self::RecallMultisession,
        Self::TemporalUpdate,
        Self::SubjectProjection,
        Self::SoulRegression,
        Self::ProceduralReuse,
        Self::PrivacyRefusal,
        Self::AgentToolExperience,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RecallMultisession => "recall_multisession",
            Self::TemporalUpdate => "temporal_update",
            Self::SubjectProjection => "subject_projection",
            Self::SoulRegression => "soul_regression",
            Self::ProceduralReuse => "procedural_reuse",
            Self::PrivacyRefusal => "privacy_refusal",
            Self::AgentToolExperience => "agent_tool_experience",
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
    #[serde(default)]
    pub semantic_contract: MemoryBenchmarkSemanticContract,
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkSemanticDimension {
    ProjectionShape,
    PrivacyRuntimeSemantics,
    SoulLifeSemantics,
    WorkIntegritySemantics,
    AgentToolExperienceSemantics,
}

impl MemoryBenchmarkSemanticDimension {
    pub const ALL: [Self; 5] = [
        Self::ProjectionShape,
        Self::PrivacyRuntimeSemantics,
        Self::SoulLifeSemantics,
        Self::WorkIntegritySemantics,
        Self::AgentToolExperienceSemantics,
    ];
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkSemanticContract {
    #[serde(default)]
    pub dimensions: Vec<MemoryBenchmarkSemanticDimension>,
    #[serde(default)]
    pub provided_keys: Vec<String>,
    #[serde(default)]
    pub required_keys: Vec<String>,
    #[serde(default)]
    pub forbidden_keys: Vec<String>,
    #[serde(default)]
    pub observed_markers: Vec<String>,
    #[serde(default)]
    pub required_markers: Vec<String>,
    #[serde(default)]
    pub forbidden_markers: Vec<String>,
}

impl MemoryBenchmarkSemanticContract {
    fn is_empty(&self) -> bool {
        self.dimensions.is_empty()
            && self.provided_keys.is_empty()
            && self.required_keys.is_empty()
            && self.forbidden_keys.is_empty()
            && self.observed_markers.is_empty()
            && self.required_markers.is_empty()
            && self.forbidden_markers.is_empty()
    }
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
pub struct MemoryBenchmarkSemanticCoverage {
    pub dimension: MemoryBenchmarkSemanticDimension,
    pub fixture_count: usize,
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
pub struct MemoryBenchmarkSemanticFailure {
    pub fixture_id: String,
    pub dimension: Option<MemoryBenchmarkSemanticDimension>,
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
    pub semantic_coverage: Vec<MemoryBenchmarkSemanticCoverage>,
    pub soul_kernel_judge: SoulKernelBenchmarkJudgeReport,
    pub subject_projection_judge: SubjectProjectionBenchmarkJudgeReport,
    pub agent_tool_experience_judge: AgentToolExperienceBenchmarkJudgeReport,
    pub failures: Vec<MemoryBenchmarkFailure>,
    pub semantic_failures: Vec<MemoryBenchmarkSemanticFailure>,
    pub passed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulKernelBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub growth_proposal_contract_covered: bool,
    pub regression_suite_covered: bool,
    pub feedback_report_covered: bool,
    pub compact_digest_covered: bool,
    pub no_roleplay_gate_passed: bool,
    pub life_slot_gate_passed: bool,
    pub work_integrity_gate_passed: bool,
    pub privacy_zero_gate_passed: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubjectProjectionBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub projection_report_covered: bool,
    pub budget_compiler_covered: bool,
    pub faithfulness_gate_passed: bool,
    pub private_disclosure_integrity_gate_passed: bool,
    pub gateway_raw_audit_redaction_covered: bool,
    pub raw_audit_disabled_reason_covered: bool,
    pub cross_surface_consistency_passed: bool,
    pub benchmark_judge_attached: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentToolExperienceBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub no_experience_empty_hints_covered: bool,
    pub governed_experience_hint_covered: bool,
    pub schema_drift_rejection_covered: bool,
    pub private_observation_not_public_covered: bool,
    pub gateway_no_cold_route_covered: bool,
    pub compact_registry_forbidden_covered: bool,
    pub host_execution_boundary_covered: bool,
    pub blocked_reasons: Vec<String>,
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

    let semantic_coverage = MemoryBenchmarkSemanticDimension::ALL
        .iter()
        .copied()
        .map(|dimension| MemoryBenchmarkSemanticCoverage {
            dimension,
            fixture_count: fixtures
                .iter()
                .filter(|fixture| fixture.semantic_contract.dimensions.contains(&dimension))
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

    let mut semantic_failures = fixtures
        .iter()
        .flat_map(validate_memory_benchmark_semantics)
        .collect::<Vec<_>>();
    for coverage in &semantic_coverage {
        if coverage.fixture_count == 0 {
            semantic_failures.push(memory_benchmark_suite_semantic_failure(
                Some(coverage.dimension),
                "semantic_dimension_coverage",
                "expected at least one fixture covering semantic dimension",
            ));
        }
    }

    let failures = fixtures
        .iter()
        .flat_map(validate_memory_benchmark_fixture)
        .collect::<Vec<_>>();
    let mut failed_fixture_ids = failures
        .iter()
        .map(|failure| failure.fixture_id.as_str())
        .collect::<BTreeSet<_>>();
    failed_fixture_ids.extend(
        semantic_failures
            .iter()
            .filter(|failure| failure.fixture_id != "__suite__")
            .map(|failure| failure.fixture_id.as_str()),
    );
    let failed_fixture_count = failed_fixture_ids.len();
    let soul_kernel_judge = build_soul_kernel_benchmark_judge(fixtures);
    let subject_projection_judge = build_subject_projection_benchmark_judge(fixtures);
    let agent_tool_experience_judge = build_agent_tool_experience_benchmark_judge(fixtures);
    let passed = failures.is_empty()
        && semantic_failures.is_empty()
        && soul_kernel_judge.release_gate_passed
        && subject_projection_judge.release_gate_passed
        && agent_tool_experience_judge.release_gate_passed;

    MemoryBenchmarkReport {
        suite: "memory_benchmark_wall".to_string(),
        total_fixtures: fixtures.len(),
        passed_fixtures: fixtures.len().saturating_sub(failed_fixture_count),
        baseline: calculate_memory_benchmark_baseline(fixtures),
        class_coverage,
        missing_classes,
        semantic_coverage,
        soul_kernel_judge,
        subject_projection_judge,
        agent_tool_experience_judge,
        failures,
        semantic_failures,
        passed,
    }
    .with_missing_class_gate()
}

fn build_soul_kernel_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> SoulKernelBenchmarkJudgeReport {
    let soul_fixtures = fixtures
        .iter()
        .filter(|fixture| fixture.class == MemoryBenchmarkClass::SoulRegression)
        .collect::<Vec<_>>();
    let fixture_ids = soul_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let growth_proposal_contract_covered = any_fixture_has_key_or_surface(
        &soul_fixtures,
        "soul_growth_proposal",
        "SoulGrowthProposal",
    );
    let regression_suite_covered = any_fixture_has_key_or_surface(
        &soul_fixtures,
        "soul_regression_suite",
        "SoulRegressionSuite",
    );
    let feedback_report_covered = any_fixture_has_key_or_surface(
        &soul_fixtures,
        "soul_feedback_report",
        "SoulFeedbackReport",
    );
    let compact_digest_covered =
        any_fixture_has_key_or_surface(&soul_fixtures, "soul_compact_digest", "SoulCompactDigest");
    let no_roleplay_gate_passed = any_fixture_has_key(&soul_fixtures, "roleplay_prompt_rejected")
        && !any_fixture_has_marker(&soul_fixtures, "append persona prompt")
        && !any_fixture_has_marker(&soul_fixtures, "just pretend to be");
    let life_slot_gate_passed = any_fixture_has_key(&soul_fixtures, "soul_life_facets")
        && any_fixture_has_key(&soul_fixtures, "self_owned_update_candidates");
    let work_integrity_gate_passed = any_fixture_has_key(&soul_fixtures, "work_integrity_covenant")
        || any_fixture_has_surface(&soul_fixtures, "Work Integrity Covenant");
    let privacy_zero_gate_passed = fixtures.iter().all(|fixture| {
        fixture.metrics.privacy_violation_count <= fixture.thresholds.max_privacy_violation_count
            && fixture.metrics.soul_regression_count <= fixture.thresholds.max_soul_regression_count
    });

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        growth_proposal_contract_covered,
        "soul_growth_proposal_contract_missing",
    );
    push_missing(
        &mut blocked_reasons,
        regression_suite_covered,
        "soul_regression_suite_missing",
    );
    push_missing(
        &mut blocked_reasons,
        feedback_report_covered,
        "soul_feedback_report_missing",
    );
    push_missing(
        &mut blocked_reasons,
        compact_digest_covered,
        "soul_compact_digest_missing",
    );
    push_missing(
        &mut blocked_reasons,
        no_roleplay_gate_passed,
        "no_roleplay_host_mount_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        life_slot_gate_passed,
        "soul_life_slot_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        work_integrity_gate_passed,
        "work_integrity_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        privacy_zero_gate_passed,
        "soul_privacy_zero_gate_failed",
    );

    SoulKernelBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        growth_proposal_contract_covered,
        regression_suite_covered,
        feedback_report_covered,
        compact_digest_covered,
        no_roleplay_gate_passed,
        life_slot_gate_passed,
        work_integrity_gate_passed,
        privacy_zero_gate_passed,
        blocked_reasons,
    }
}

fn build_agent_tool_experience_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> AgentToolExperienceBenchmarkJudgeReport {
    let tool_fixtures = fixtures
        .iter()
        .filter(|fixture| fixture.class == MemoryBenchmarkClass::AgentToolExperience)
        .collect::<Vec<_>>();
    let fixture_ids = tool_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let no_experience_empty_hints_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_no_experience_empty_hints")
            && any_fixture_has_marker(&tool_fixtures, "no_governed_tool_experience");
    let governed_experience_hint_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_governed_hint")
            && any_fixture_has_key(&tool_fixtures, "agent_tool_hints");
    let schema_drift_rejection_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_registry_fingerprint_mismatch")
            || any_fixture_has_key(&tool_fixtures, "agent_tool_experience_stale_schema");
    let private_observation_not_public_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_private_observation_excluded")
            && any_fixture_has_marker(&tool_fixtures, "private observation not projected");
    let gateway_no_cold_route_covered =
        any_fixture_has_key(&tool_fixtures, "gateway_host_tools_no_cold_route")
            && any_fixture_has_marker(&tool_fixtures, "host fallback required");
    let compact_registry_forbidden_covered = tool_fixtures.iter().any(|fixture| {
        fixture.mode == MemoryBenchmarkMode::Compact
            && any_fixture_has_key(&[*fixture], "agent_tool_registry_forbidden_by_profile")
    });
    let host_execution_boundary_covered =
        any_fixture_has_key(&tool_fixtures, "host_execution_required")
            && any_fixture_has_surface(&tool_fixtures, "HostToolRegistry");

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        no_experience_empty_hints_covered,
        "agent_tool_no_experience_empty_hints_missing",
    );
    push_missing(
        &mut blocked_reasons,
        governed_experience_hint_covered,
        "agent_tool_governed_hint_missing",
    );
    push_missing(
        &mut blocked_reasons,
        schema_drift_rejection_covered,
        "agent_tool_schema_drift_rejection_missing",
    );
    push_missing(
        &mut blocked_reasons,
        private_observation_not_public_covered,
        "agent_tool_private_observation_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        gateway_no_cold_route_covered,
        "gateway_host_tools_no_cold_route_missing",
    );
    push_missing(
        &mut blocked_reasons,
        compact_registry_forbidden_covered,
        "agent_tool_compact_registry_forbidden_missing",
    );
    push_missing(
        &mut blocked_reasons,
        host_execution_boundary_covered,
        "agent_tool_host_execution_boundary_missing",
    );

    AgentToolExperienceBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        no_experience_empty_hints_covered,
        governed_experience_hint_covered,
        schema_drift_rejection_covered,
        private_observation_not_public_covered,
        gateway_no_cold_route_covered,
        compact_registry_forbidden_covered,
        host_execution_boundary_covered,
        blocked_reasons,
    }
}

fn build_subject_projection_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> SubjectProjectionBenchmarkJudgeReport {
    let projection_fixtures = fixtures
        .iter()
        .filter(|fixture| {
            fixture.class == MemoryBenchmarkClass::SubjectProjection
                || fixture.class == MemoryBenchmarkClass::PrivacyRefusal
        })
        .collect::<Vec<_>>();
    let fixture_ids = projection_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let projection_report_covered =
        any_fixture_has_key_or_surface(
            &projection_fixtures,
            "subject_projection_report",
            "SubjectProjectionReport",
        ) || any_fixture_has_surface(&projection_fixtures, "MemoryProjectionReport");
    let budget_compiler_covered = any_fixture_has_key(&projection_fixtures, "budget_decisions")
        || any_fixture_has_key(&projection_fixtures, "projection_budget_compiler")
        || any_fixture_has_surface(&projection_fixtures, "ProjectionBudgetCompiler");
    let faithfulness_gate_passed =
        projection_fixtures.iter().all(|fixture| {
            fixture.metrics.projection_faithfulness_bps
                >= fixture.thresholds.min_projection_faithfulness_bps
        }) && (any_fixture_has_key(&projection_fixtures, "projection_faithfulness_check")
            || any_fixture_has_surface(&projection_fixtures, "ProjectionFaithfulnessCheck"));
    let private_disclosure_integrity_gate_passed =
        any_fixture_has_key(&projection_fixtures, "private_disclosure_integrity_report")
            || any_fixture_has_key(&projection_fixtures, "private_raw_absent")
            || any_fixture_has_surface(&projection_fixtures, "PrivateDisclosureIntegrityReport");
    let gateway_raw_audit_redaction_covered =
        any_fixture_has_key(&projection_fixtures, "gateway_raw_audit_redacted")
            || any_fixture_has_key(&projection_fixtures, "redacted_private_envelope")
            || any_fixture_has_surface(&projection_fixtures, "RawProjectionAudit");
    let raw_audit_disabled_reason_covered =
        any_fixture_has_key(&projection_fixtures, "raw_audit_disabled_reason")
            || any_fixture_has_marker(
                &projection_fixtures,
                "raw audit unavailable reason when disabled",
            )
            || any_fixture_has_marker(&projection_fixtures, "raw_projection_recording_disabled");
    let cross_surface_consistency_passed =
        any_fixture_has_key(&projection_fixtures, "cross_surface_consistency")
            && all_projection_fixtures_have_required_surface_context(&projection_fixtures);
    let benchmark_judge_attached = projection_fixtures.iter().any(|fixture| {
        matches!(
            fixture.evaluation_source,
            MemoryBenchmarkEvaluationSource::RuntimeReplay
                | MemoryBenchmarkEvaluationSource::GoldenJudge
        )
    });

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        projection_report_covered,
        "subject_projection_report_missing",
    );
    push_missing(
        &mut blocked_reasons,
        budget_compiler_covered,
        "projection_budget_compiler_missing",
    );
    push_missing(
        &mut blocked_reasons,
        faithfulness_gate_passed,
        "projection_faithfulness_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        private_disclosure_integrity_gate_passed,
        "private_disclosure_integrity_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        gateway_raw_audit_redaction_covered,
        "gateway_raw_audit_redaction_missing",
    );
    push_missing(
        &mut blocked_reasons,
        raw_audit_disabled_reason_covered,
        "raw_audit_disabled_reason_missing",
    );
    push_missing(
        &mut blocked_reasons,
        cross_surface_consistency_passed,
        "cross_surface_consistency_missing",
    );
    push_missing(
        &mut blocked_reasons,
        benchmark_judge_attached,
        "runtime_or_golden_benchmark_judge_missing",
    );

    SubjectProjectionBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        projection_report_covered,
        budget_compiler_covered,
        faithfulness_gate_passed,
        private_disclosure_integrity_gate_passed,
        gateway_raw_audit_redaction_covered,
        raw_audit_disabled_reason_covered,
        cross_surface_consistency_passed,
        benchmark_judge_attached,
        blocked_reasons,
    }
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

fn validate_memory_benchmark_semantics(
    fixture: &MemoryBenchmarkFixture,
) -> Vec<MemoryBenchmarkSemanticFailure> {
    let contract = &fixture.semantic_contract;
    if contract.is_empty() {
        return Vec::new();
    }

    let mut failures = Vec::new();
    if contract.dimensions.is_empty() {
        failures.push(memory_benchmark_semantic_failure(
            fixture,
            None,
            "semantic_dimension",
            "semantic contract must declare at least one gate dimension",
        ));
    }

    for required_key in &contract.required_keys {
        if !contract.provided_keys.contains(required_key) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_required_key",
                format!("missing required key {required_key}"),
            ));
        }
    }
    for forbidden_key in &contract.forbidden_keys {
        if contract.provided_keys.contains(forbidden_key) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_forbidden_key",
                format!("forbidden key {forbidden_key} is present"),
            ));
        }
    }
    for required_marker in &contract.required_markers {
        if !contract.observed_markers.contains(required_marker) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_required_marker",
                format!("missing required marker {required_marker}"),
            ));
        }
    }
    for forbidden_marker in &contract.forbidden_markers {
        if contract.observed_markers.contains(forbidden_marker) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_forbidden_marker",
                format!("forbidden marker {forbidden_marker} is present"),
            ));
        }
    }

    failures
}

fn any_fixture_has_key(fixtures: &[&MemoryBenchmarkFixture], key: &str) -> bool {
    fixtures.iter().any(|fixture| {
        fixture
            .semantic_contract
            .provided_keys
            .iter()
            .chain(fixture.semantic_contract.required_keys.iter())
            .any(|candidate| candidate == key)
    })
}

fn any_fixture_has_surface(fixtures: &[&MemoryBenchmarkFixture], surface: &str) -> bool {
    fixtures.iter().any(|fixture| {
        fixture
            .scenario
            .expected_surfaces
            .iter()
            .any(|candidate| candidate == surface)
    })
}

fn any_fixture_has_key_or_surface(
    fixtures: &[&MemoryBenchmarkFixture],
    key: &str,
    surface: &str,
) -> bool {
    any_fixture_has_key(fixtures, key) || any_fixture_has_surface(fixtures, surface)
}

fn any_fixture_has_marker(fixtures: &[&MemoryBenchmarkFixture], marker: &str) -> bool {
    fixtures.iter().any(|fixture| {
        fixture
            .semantic_contract
            .observed_markers
            .iter()
            .chain(fixture.semantic_contract.required_markers.iter())
            .any(|candidate| candidate == marker)
    })
}

fn all_projection_fixtures_have_required_surface_context(
    fixtures: &[&MemoryBenchmarkFixture],
) -> bool {
    fixtures.iter().all(|fixture| {
        !fixture.scenario.expected_surfaces.is_empty()
            && !fixture.scenario.evidence_refs.is_empty()
            && fixture.metrics.projection_faithfulness_bps
                >= fixture.thresholds.min_projection_faithfulness_bps
            && fixture.metrics.privacy_violation_count
                <= fixture.thresholds.max_privacy_violation_count
    })
}

fn push_missing(blocked_reasons: &mut Vec<String>, condition: bool, reason: &str) {
    if !condition {
        blocked_reasons.push(reason.to_string());
    }
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

fn memory_benchmark_semantic_failure(
    fixture: &MemoryBenchmarkFixture,
    dimension: Option<MemoryBenchmarkSemanticDimension>,
    stage: impl Into<String>,
    reason: impl Into<String>,
) -> MemoryBenchmarkSemanticFailure {
    MemoryBenchmarkSemanticFailure {
        fixture_id: fixture.fixture_id.clone(),
        dimension,
        stage: stage.into(),
        reason: reason.into(),
    }
}

fn memory_benchmark_suite_semantic_failure(
    dimension: Option<MemoryBenchmarkSemanticDimension>,
    stage: impl Into<String>,
    reason: impl Into<String>,
) -> MemoryBenchmarkSemanticFailure {
    MemoryBenchmarkSemanticFailure {
        fixture_id: "__suite__".to_string(),
        dimension,
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
