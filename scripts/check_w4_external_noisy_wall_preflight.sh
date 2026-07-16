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

target_root="${CARGO_TARGET_DIR:-target}"
case "$target_root" in
  /*) ;;
  *) target_root="$PWD/$target_root" ;;
esac
cargo build --release --locked -p bm-replay \
  --bin bm-w4-external-noisy-wall --bin bm-p7-retained-launch
operator_bin="$(realpath -- "${target_root}/release/bm-w4-external-noisy-wall")"

"$operator_bin" \
  --preflight \
  --benchmark-root "$BM_W4_EXTERNAL_BENCH_ROOT" \
  --run-id "$BM_P7_RUN_ID"

echo "check_w4_external_noisy_wall_preflight: ok" >&2
