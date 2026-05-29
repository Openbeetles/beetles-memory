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

needles=(
  "MemoryBenchmarkReport"
  "SoulKernelBenchmarkJudgeReport"
  "SubjectProjectionBenchmarkJudgeReport"
  "AgentToolExperienceBenchmarkJudgeReport"
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
)

for needle in "${needles[@]}"; do
  rg -q "$needle" dev-docs/next-gen-soul-memory-roadmap.md crates/replay scripts fixtures/memory-benchmark-wall
done
