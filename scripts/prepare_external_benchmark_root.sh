#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -z "${BM_W4_EXTERNAL_BENCH_ROOT:-}" ]]; then
  echo "BM_W4_EXTERNAL_BENCH_ROOT is required" >&2
  exit 2
fi

case "${BM_W4_EXTERNAL_BENCH_ROOT}" in
  /*) ;;
  *)
    echo "BM_W4_EXTERNAL_BENCH_ROOT must be an absolute path" >&2
    exit 2
    ;;
esac

mkdir -p "${BM_W4_EXTERNAL_BENCH_ROOT}"
benchmark_root="$(cd "${BM_W4_EXTERNAL_BENCH_ROOT}" && pwd -P)"
repository_root="$PWD"

case "${benchmark_root}/" in
  "${repository_root}/"*)
    echo "external benchmark root must stay outside the agent-memory repository" >&2
    exit 2
    ;;
esac

mkdir -p \
  "${benchmark_root}/data" \
  "${benchmark_root}/results/runs" \
  "${benchmark_root}/runner" \
  "${benchmark_root}/releases" \
  "${benchmark_root}/authority-probes" \
  "${benchmark_root}/cache/cargo-home" \
  "${benchmark_root}/cache/cargo-target"

printf 'BM_W4_EXTERNAL_BENCH_ROOT=%s\n' "${benchmark_root}"
printf 'BM_P7_RUNNER_SOURCE_ROOT=%s\n' "${benchmark_root}/runner"
printf 'CARGO_HOME=%s\n' "${benchmark_root}/cache/cargo-home"
printf 'CARGO_TARGET_DIR=%s\n' "${benchmark_root}/cache/cargo-target"
