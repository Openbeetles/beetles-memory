//! Replay-facing contracts for Beetle Memory.

mod bench;
mod fixture;
mod harness;
mod runner;

pub use bench::{
    run_persona_governance_benchmark_gate, run_recall_benchmark_gate, BenchmarkGateReport,
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
