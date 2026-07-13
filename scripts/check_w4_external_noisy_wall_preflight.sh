#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -z "${BM_W4_EXTERNAL_BENCH_ROOT:-}" ]]; then
  echo "BM_W4_EXTERNAL_BENCH_ROOT is required for the P7 runner preflight" >&2
  exit 2
fi
if [[ -z "${BM_P7_RUN_ID:-}" ]]; then
  echo "BM_P7_RUN_ID is required for the P7 runner preflight" >&2
  exit 2
fi

cargo run -p bm-replay --bin bm-w4-external-noisy-wall -- \
  --preflight \
  --benchmark-root "$BM_W4_EXTERNAL_BENCH_ROOT" \
  --run-id "$BM_P7_RUN_ID"

echo "check_w4_external_noisy_wall_preflight: ok"
