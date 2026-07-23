//! Replay-facing contracts for Beetle Memory.

mod bench;
mod fixture;
mod harness;
mod p7_process;
mod p7_secure_fs;
mod runner;

pub use bench::{
    attach_p7_soul_regression_gate, attach_p7_verifier_performance,
    attest_p7_current_verifier_execution, bind_p7_verifier_identity,
    evaluate_w4_external_noisy_wall, finalize_w4_external_noisy_release_report,
    load_memory_benchmark_fixture_dir, p7_cohort_admission_creation_sequence,
    p7_producer_semantic_source_manifest, p7_release_gate_plan, p7_release_gate_source_fingerprint,
    p7_release_gate_source_manifest, p7_release_gate_source_manifest_with_receipt,
    preflight_p7_runner_release_with_frozen, preflight_p7_runner_release_with_frozen_and_receipt,
    publish_p7_verifier_release, run_memory_benchmark_wall, run_p7_soul_regression_gate,
    run_persona_governance_benchmark_gate, run_recall_benchmark_gate,
    validate_p7_cohort_admission_contract, validate_p7_runner_preflight_report,
    validate_p7_runner_preflight_report_with_frozen, verify_p7_cohort_admission,
    verify_p7_cohort_admission_with_receipt, verify_p7_maximum_rss_evidence,
    verify_p7_maximum_rss_evidence_with_receipt, verify_p7_merged_resume_with_receipt,
    verify_p7_preflight_artifact_with_receipt, verify_p7_published_release_bundle_with_receipt,
    verify_p7_release_gate_plan, verify_p7_shard_bundle_with_receipt,
    verify_p7_shard_set_with_receipt, verify_p7_verifier_release_manifest_with_receipt,
    verify_p7_wall_cohort_evidence_with_receipt, verify_p7_wall_input_context,
    verify_p7_wall_input_context_with_authority, w4_external_noisy_summary_with_provenance,
    BenchmarkGateReport, MemoryBenchmarkBaseline, MemoryBenchmarkClass,
    MemoryBenchmarkClassCoverage, MemoryBenchmarkEvalRecall, MemoryBenchmarkEvalRecallAtK,
    MemoryBenchmarkEvalRecallDiagnostics, MemoryBenchmarkEvalRecallEvidenceRefIndexEntry,
    MemoryBenchmarkEvalRecallGoldRank, MemoryBenchmarkEvalRecallGraphDistanceToGold,
    MemoryBenchmarkEvalRecallMetrics, MemoryBenchmarkEvalRecallStageEvidenceRefs,
    MemoryBenchmarkEvaluationSource, MemoryBenchmarkFailure, MemoryBenchmarkFixture,
    MemoryBenchmarkMetrics, MemoryBenchmarkMissingClass, MemoryBenchmarkMode,
    MemoryBenchmarkReport, MemoryBenchmarkScenario, MemoryBenchmarkSemanticContract,
    MemoryBenchmarkSemanticCoverage, MemoryBenchmarkSemanticDimension,
    MemoryBenchmarkSemanticFailure, MemoryBenchmarkThresholds, P7ArtifactLifecycleReceipt,
    P7CohortAdmission, P7CohortAdmissionStage, P7CohortAdmissionStep, P7EvaluationApplicability,
    P7FrozenRunnerIdentity, P7MaximumRssEvidence, P7MaximumRssMeasurementReport,
    P7MeasuredArtifactIdentity, P7MergedBundleCommit, P7MergedProvenance, P7ProducerExecutionKind,
    P7ProducerIdentity, P7PublishedReleaseIdentity, P7QuestionEvaluationContract, P7QuestionType,
    P7RecordedProducerIdentity, P7ReleaseEnvironmentAttestation, P7ReleaseGateAttestation,
    P7ReleaseGateOwner, P7ReleaseGatePlan, P7ReleaseGatePlanStep, P7ReleaseGateReceipt,
    P7ReleaseMetadata, P7ReleaseSourceManifest, P7ReleaseSourceManifestEntry,
    P7ReleaseSourceManifestEntryKind, P7ReleaseToolIdentity, P7RunnerBuildIdentity,
    P7RunnerPreflightReport, P7ShardBundleCommit, P7ShardBundleExpectation, P7ShardBundleState,
    P7ShardProducerProvenance, P7SoulRegressionCommandReceipt, P7SoulRegressionGateReport,
    P7VerificationReceipt, P7VerifiedPreflightArtifact, P7VerifiedPublishedReleaseBundle,
    P7VerifiedShardBundle, P7VerifiedWallCohortEvidence, P7VerifiedWallInputContext,
    P7VerifierExecutionAuthority, P7VerifierIdentity, P7VerifierPerformanceReport,
    P7VerifierReleaseManifest, P7VerifierReleasePublishReport, SoulKernelBenchmarkJudgeReport,
    SubjectProjectionBenchmarkJudgeReport, W4EvalRecallBenchmarkJudgeReport,
    W4ExternalNoisyBenchmarkSummary, W4ExternalNoisyFacetAblationDiagnostics,
    W4ExternalNoisyIndexDiagnostics, W4ExternalNoisyP7LossDiagnostics,
    W4ExternalNoisyP7ProductionDeliveryDiagnostics, W4ExternalNoisyP7ShardDigest,
    W4ExternalNoisyStageHitCounts, W4ExternalNoisySuiteReport, W4ExternalNoisyW41Diagnostics,
    W4ExternalNoisyWallReport, P7_COHORT_ADMISSION_FILE_NAME, P7_COHORT_ADMISSION_SCHEMA_VERSION,
    P7_DETAIL_SCHEMA_VERSION, P7_FROZEN_RUNNER_IDENTITY_RELATIVE_PATH,
    P7_MAXIMUM_RSS_MEASUREMENT_FILE_NAME, P7_MAXIMUM_RSS_MEASUREMENT_SCHEMA_VERSION,
    P7_MAXIMUM_RSS_REPORT_FILE_NAME, P7_MERGED_BUNDLE_COMMIT_SCHEMA_VERSION,
    P7_MERGED_PROVENANCE_SCHEMA_VERSION, P7_PRODUCER_SEMANTIC_SOURCE_FINGERPRINT_CONTRACT,
    P7_PRODUCER_SEMANTIC_SOURCE_MANIFEST_SCHEMA_VERSION, P7_RELEASE_GATE_ATTESTATION_FILE_NAME,
    P7_RELEASE_GATE_ATTESTATION_SCHEMA_VERSION, P7_RELEASE_GATE_ORCHESTRATOR_CONTRACT,
    P7_RELEASE_GATE_PLAN_SCHEMA_VERSION, P7_RELEASE_GATE_SOURCE_FINGERPRINT_CONTRACT,
    P7_RELEASE_GATE_SOURCE_MANIFEST_FILE_NAME, P7_RELEASE_GATE_SOURCE_MANIFEST_SCHEMA_VERSION,
    P7_RELEASE_METADATA_FILE_NAME, P7_RELEASE_METADATA_SCHEMA_VERSION,
    P7_REQUIRED_RELEASE_GATE_IDS, P7_RUNNER_PREFLIGHT_SCHEMA_VERSION,
    P7_SHARD_BUNDLE_COMMIT_SCHEMA_VERSION, P7_SHARD_PRODUCER_PROVENANCE_SCHEMA_VERSION,
    P7_VERIFIER_RELEASES_DIR, P7_VERIFIER_RELEASE_MANIFEST_FILE_NAME,
    P7_VERIFIER_RELEASE_MANIFEST_SCHEMA_VERSION,
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
pub use p7_process::{
    exec_p7_retained_executable, run_p7_bounded_command, run_p7_bounded_retained_executable,
    run_p7_retained_executable, P7ProcessLimits, P7ProcessOutput, P7ProcessReceipt,
    P7ProcessTermination,
};
pub use p7_secure_fs::{
    P7ArtifactPublishOutcome, P7AuthorityBoundArtifactTransaction,
    P7AuthorityBoundReleaseTransaction, P7BundleWriteGuard, P7ContentIdentity,
    P7DirectoryInstallError, P7ProcessExecutionAuthority, P7RetainedDirectoryOwner, P7RetainedFile,
};
pub use runner::{run_replay_fixture, ReplayRunnerConfig};

pub fn inspect_turn_replay(
    store: &dyn TurnLedgerStore,
    chat_id: &str,
    limit: usize,
) -> Result<IntelligenceReplayInspection> {
    inspect_intelligence_replay(store, chat_id, limit)
}
