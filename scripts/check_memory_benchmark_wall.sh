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
test -f fixtures/memory-benchmark-wall/soul-regression/compact-baseline.json
test -f fixtures/memory-benchmark-wall/soul-regression/full-baseline.json
test -f fixtures/memory-benchmark-wall/procedural-reuse/compact-baseline.json
test -f fixtures/memory-benchmark-wall/procedural-reuse/full-baseline.json
test -f fixtures/memory-benchmark-wall/privacy-refusal/compact-baseline.json
test -f fixtures/memory-benchmark-wall/privacy-refusal/full-baseline.json

for needle in \
  MemoryBenchmarkReport \
  check_memory_benchmark_wall \
  recall_multisession \
  temporal_update \
  subject_projection \
  soul_regression \
  procedural_reuse \
  privacy_refusal
do
  rg -q "$needle" dev-docs/next-gen-soul-memory-roadmap.md crates/replay scripts fixtures/memory-benchmark-wall
done
