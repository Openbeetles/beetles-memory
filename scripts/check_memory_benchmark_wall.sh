#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all -- --check
bash scripts/check_profile_matrix.sh

cargo test -p bm-core --test next_gen_contract
cargo test -p bm-replay --test benchmark_gate
cargo test -p bm-replay --test memory_benchmark_wall
cargo test -p bm-replay --all-features

test -f fixtures/memory-benchmark-wall/README.md
test -f fixtures/memory-benchmark-wall/recall-multisession/compact-baseline.json
test -f fixtures/memory-benchmark-wall/recall-multisession/full-baseline.json
test -f fixtures/memory-benchmark-wall/temporal-update/compact-baseline.json
test -f fixtures/memory-benchmark-wall/temporal-update/full-baseline.json
test -f fixtures/memory-benchmark-wall/temporal-update/w4-eval-recall-persistent-graph-full.json
test -f fixtures/memory-benchmark-wall/subject-projection/compact-baseline.json
test -f fixtures/memory-benchmark-wall/subject-projection/full-baseline.json
test -f fixtures/memory-benchmark-wall/subject-projection/inhabited-subject-mount-compact.json
test -f fixtures/memory-benchmark-wall/subject-projection/inhabited-subject-mount-full.json
test -f fixtures/memory-benchmark-wall/subject-projection/protected-private-runtime-envelope-full.json
test -f fixtures/memory-benchmark-wall/soul-regression/compact-baseline.json
test -f fixtures/memory-benchmark-wall/soul-regression/full-baseline.json
test -f fixtures/memory-benchmark-wall/soul-regression/no-roleplay-host-mount-full.json
test -f fixtures/memory-benchmark-wall/soul-regression/soul-life-slot-continuity-full.json
test -f fixtures/memory-benchmark-wall/soul-regression/work-integrity-no-obstruction-full.json
test -f fixtures/memory-benchmark-wall/procedural-reuse/compact-baseline.json
test -f fixtures/memory-benchmark-wall/procedural-reuse/full-baseline.json
test -f fixtures/memory-benchmark-wall/privacy-refusal/compact-baseline.json
test -f fixtures/memory-benchmark-wall/privacy-refusal/full-baseline.json
test -f fixtures/memory-benchmark-wall/privacy-refusal/private-disclosure-adjudication-full.json
test -f fixtures/memory-benchmark-wall/privacy-refusal/no-final-llm-privacy-judge-full.json
test -f fixtures/memory-benchmark-wall/privacy-refusal/disclosure-protocol-in-main-runtime-full.json
test -f fixtures/memory-benchmark-wall/privacy-refusal/raw-audit-redacted-private-envelope-full.json
test -f fixtures/memory-benchmark-wall/agent-tool-experience/agent-tool-registry-forbidden-compact.json
test -f fixtures/memory-benchmark-wall/agent-tool-experience/no-experience-empty-hints-full.json
test -f fixtures/memory-benchmark-wall/agent-tool-experience/governed-experience-hint-full.json
test -f fixtures/memory-benchmark-wall/agent-tool-experience/schema-drift-stales-experience-full.json
test -f fixtures/memory-benchmark-wall/agent-tool-experience/private-observation-not-public-full.json
test -f fixtures/memory-benchmark-wall/agent-tool-experience/gateway-host-tools-no-cold-route-full.json
test -f scripts/check_w4_external_noisy_wall_operator.sh

needles=(
  "MemoryBenchmarkReport"
  "SoulKernelBenchmarkJudgeReport"
  "SubjectProjectionBenchmarkJudgeReport"
  "AgentToolExperienceBenchmarkJudgeReport"
  "W4EvalRecallBenchmarkJudgeReport"
  "W4ExternalNoisyWallReport"
  "W4ExternalNoisyStageHitCounts"
  "W4ExternalNoisyIndexDiagnostics"
  "MemoryBenchmarkSemanticFailure"
  "check_memory_benchmark_wall"
  "recall_multisession"
  "temporal_update"
  "subject_projection"
  "soul_regression"
  "procedural_reuse"
  "privacy_refusal"
  "agent_tool_experience"
  "semantic_contract"
  "subject_mount"
  "source_authority"
  "protected private runtime context"
  "no final second LLM judge"
  "Work Integrity Covenant"
  "roleplay prompt rejected"
  "redacted private envelope"
  "soul_growth_proposal"
  "soul_feedback_report"
  "soul_compact_digest"
  "cross_surface_consistency"
  "raw_audit_disabled_reason"
  "agent_tool_hints"
  "no_governed_tool_experience"
  "host_execution_required"
  "agent_tool_registry_forbidden_by_profile"
  "w4_eval_recall"
  "W4EvalRecallSemantics"
  "source_expanded_selected_split_covered"
  "evaluate_w4_external_noisy_wall"
  "w4_external_noisy_summary_with_provenance"
  "w4_external_noisy_wall_improvement_not_proven"
  "w4_external_noisy_wall_stage_diagnostics_missing"
  "w4_external_noisy_wall_index_diagnostics_missing"
  "w4_external_noisy_wall_stage_attribution_not_proven"
  "w4_external_noisy_wall_index_effect_not_proven"
  "stage_diagnostics_attached"
  "index_diagnostics_attached"
  "stage_attributed_improvement_proven"
  "index_effect_proven"
  "stage_hit_counts"
  "index_diagnostics"
)

for needle in "${needles[@]}"; do
  rg -q "$needle" dev-docs/next-gen-soul-memory-roadmap.md crates/replay scripts fixtures/memory-benchmark-wall
done

operator_needles=(
  "BM_W4_EXTERNAL_BENCH_ROOT"
  "BM_W4_EXTERNAL_EXPECT_BLOCKED"
  "bm-w4-external-noisy-wall"
  "runner/src/main.rs"
  "locomo.merged.summary.json"
  "longmemeval_oracle.merged.summary.json"
  "longmemeval_s_cleaned.merged.summary.json"
  "longmemeval_m_cleaned.merged.summary.json"
  "shasum -a 256"
)

for needle in "${operator_needles[@]}"; do
  rg -q "$needle" scripts/check_w4_external_noisy_wall_operator.sh
done

! rg -q "jsonl|data/|invalid-pre-runner-fix|runner/target" scripts/check_w4_external_noisy_wall_operator.sh
