//! Replay-facing contracts for Beetle Memory.

mod bench;
mod fixture;
mod harness;
mod runner;

pub use bench::{
    evaluate_w4_external_noisy_wall, load_memory_benchmark_fixture_dir, run_memory_benchmark_wall,
    run_persona_governance_benchmark_gate, run_recall_benchmark_gate,
    w4_external_noisy_summary_with_provenance, BenchmarkGateReport, MemoryBenchmarkBaseline,
    MemoryBenchmarkClass, MemoryBenchmarkClassCoverage, MemoryBenchmarkEvalRecall,
    MemoryBenchmarkEvalRecallAtK, MemoryBenchmarkEvalRecallDiagnostics,
    MemoryBenchmarkEvalRecallEvidenceRefIndexEntry, MemoryBenchmarkEvalRecallGoldRank,
    MemoryBenchmarkEvalRecallGraphDistanceToGold, MemoryBenchmarkEvalRecallMetrics,
    MemoryBenchmarkEvalRecallStageEvidenceRefs, MemoryBenchmarkEvaluationSource,
    MemoryBenchmarkFailure, MemoryBenchmarkFixture, MemoryBenchmarkMetrics,
    MemoryBenchmarkMissingClass, MemoryBenchmarkMode, MemoryBenchmarkReport,
    MemoryBenchmarkScenario, MemoryBenchmarkSemanticContract, MemoryBenchmarkSemanticCoverage,
    MemoryBenchmarkSemanticDimension, MemoryBenchmarkSemanticFailure, MemoryBenchmarkThresholds,
    SoulKernelBenchmarkJudgeReport, SubjectProjectionBenchmarkJudgeReport,
    W4EvalRecallBenchmarkJudgeReport, W4ExternalNoisyBenchmarkSummary,
    W4ExternalNoisyIndexDiagnostics, W4ExternalNoisyStageHitCounts, W4ExternalNoisySuiteReport,
    W4ExternalNoisyW41Diagnostics, W4ExternalNoisyWallReport,
};
pub use bm_core::memory::{
    inspect_intelligence_replay, ArchiveBenchmarkCase, ArchiveBenchmarkResult,
    IntelligenceReplayAlert, IntelligenceReplayInspection, IntelligenceReplayTurnDigest,
    PersonaContinuityCase, PersonaContinuityResult, PersonaGovernanceReplayCase,
    PersonaGovernanceReplayResult, RecallBenchmarkCase, RecallBenchmarkMetrics,
    RecallBenchmarkResult, RecallSelectionReport, TurnLedgerStore,
};
pub use bm_core::Result;
pub use fixture::{
    ReplayExpectedOutcome, ReplayFailure, ReplayFixture, ReplayOperation, ReplayRunReport,
};
pub use harness::{build_sdk_memory_harness_fixture, run_sdk_memory_harness, MemoryHarnessReport};
pub use runner::{run_replay_fixture, ReplayRunnerConfig};

pub fn inspect_turn_replay(
    store: &dyn TurnLedgerStore,
    chat_id: &str,
    limit: usize,
) -> Result<IntelligenceReplayInspection> {
    inspect_intelligence_replay(store, chat_id, limit)
}
