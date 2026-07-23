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
  --no-default-features \
  --bin bm-w4-external-noisy-wall --bin bm-p7-retained-launch
bench_root="$(realpath -- "${BM_W4_EXTERNAL_BENCH_ROOT}")"
publisher="$(realpath -- "${target_root}/release/bm-w4-external-noisy-wall")"
launcher="$(realpath -- "${target_root}/release/bm-p7-retained-launch")"
verifier="$("$launcher" --executable "$publisher" -- \
  --publish-verifier-release --benchmark-root "$bench_root")"
if [[ -z "$verifier" || "$verifier" == *$'\n'* || "$(realpath -- "$verifier")" != "$verifier" ]]; then
  echo "P7 verifier publisher must return one canonical release path" >&2
  exit 2
fi

"$launcher" --executable "$verifier" -- \
  --preflight \
  --benchmark-root "$bench_root" \
  --run-id "$BM_P7_RUN_ID"

echo "check_w4_external_noisy_wall_preflight: ok" >&2
