#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "P7 Linux execution authority gate requires a trusted Linux host" >&2
  exit 1
fi

cargo test --release --locked -p bm-replay --no-default-features --lib \
  p7_secure_fs::tests::p7_retained_launcher_replaces_reserved_execution_authority_environment \
  -- --exact
cargo test --release --locked -p bm-replay --no-default-features --lib \
  p7_secure_fs::tests::p7_concurrent_execution_authority_claim_is_serialized_and_one_time \
  -- --exact
cargo test --release --locked -p bm-replay --no-default-features --lib \
  p7_secure_fs::tests::p7_inherited_execution_authority_rejects_partial_seals_and_wrong_sha \
  -- --exact
cargo test --release --locked -p bm-replay --no-default-features --lib \
  p7_secure_fs::tests::p7_inherited_execution_authority_rejects_direct_path_and_foreign_fd \
  -- --exact
cargo test --release --locked -p bm-replay --no-default-features --lib \
  bench::p7_operator_unit_tests::p7_sealed_execution_identity_binds_memfd_bytes_to_release_manifest \
  -- --ignored --exact
cargo test --release --locked -p bm-replay --no-default-features --test memory_benchmark_wall \
  p7_linux_real_publisher_and_verifier_use_sealed_execution_authority \
  -- --ignored --exact

runner_root="${BM_P7_RUNNER_SOURCE_ROOT:-$(cd .. && pwd)/.beetle-memory-external-bench/runner}"
if [[ ! -f "$runner_root/Cargo.toml" || ! -f "$runner_root/src/main.rs" ]]; then
  echo "P7 Linux execution authority gate requires the repo-external runner source" >&2
  exit 1
fi
runner_root="$(cd "$runner_root" && pwd -P)"
cargo build --release --locked --no-default-features --manifest-path "$runner_root/Cargo.toml"
cargo build --release --locked -p bm-replay --no-default-features --bin bm-p7-retained-launch
runner="$runner_root/target/release/beetle-memory-external-bench-runner"
launcher="$PWD/target/release/bm-p7-retained-launch"
BM_P7_AUTHORITY_PROBE_RUNNER="$runner" \
BM_P7_AUTHORITY_PROBE_LAUNCHER="$launcher" \
  cargo test --release --locked -p bm-replay --no-default-features \
  --test memory_benchmark_wall \
  p7_linux_real_runner_authority_probe_binds_sealed_execution_bytes \
  -- --ignored --exact

echo "check_p7_linux_execution_authority.sh: ok"
