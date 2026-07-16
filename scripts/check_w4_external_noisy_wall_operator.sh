#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -z "${BM_W4_EXTERNAL_BENCH_ROOT:-}" ]]; then
  echo "BM_W4_EXTERNAL_BENCH_ROOT is required for the explicit W4 external noisy wall operator" >&2
  exit 2
fi
if [[ -z "${BM_P7_RUN_ID:-}" ]]; then
  echo "BM_P7_RUN_ID is required for the explicit W4 external noisy wall operator" >&2
  exit 2
fi

valid_run_id() {
  local run_id="$1"
  local LC_ALL=C
  [[ "$run_id" != "." \
    && "$run_id" != ".." \
    && "$run_id" =~ ^[A-Za-z0-9._-]+$ ]]
}

run_id="${BM_P7_RUN_ID}"
if ! valid_run_id "$run_id"; then
  echo "BM_P7_RUN_ID must match ASCII [A-Za-z0-9._-]+ and must not be . or .." >&2
  exit 2
fi
if [[ "${BM_P7_VERIFY_MAX_RSS:-0}" != "0" && "${BM_P7_VERIFY_MAX_RSS:-0}" != "1" ]]; then
  echo "BM_P7_VERIFY_MAX_RSS must be 0 or 1" >&2
  exit 2
fi

bench_root="$(realpath -- "${BM_W4_EXTERNAL_BENCH_ROOT}")"
results_dir="${bench_root}/results/runs/${run_id}"

locomo="${results_dir}/locomo.merged.summary.json"
oracle="${results_dir}/longmemeval_oracle.merged.summary.json"
s_cleaned="${results_dir}/longmemeval_s_cleaned.merged.summary.json"
m_cleaned="${results_dir}/longmemeval_m_cleaned.merged.summary.json"

target_root="${CARGO_TARGET_DIR:-target}"
case "$target_root" in
  /*) ;;
  *) target_root="$PWD/$target_root" ;;
esac
cargo build --release --locked -p bm-replay \
  --bin bm-w4-external-noisy-wall --bin bm-p7-retained-launch
publisher="$(realpath -- "${target_root}/release/bm-w4-external-noisy-wall")"
launcher="$(realpath -- "${target_root}/release/bm-p7-retained-launch")"
verifier="$("$publisher" --publish-verifier-release --benchmark-root "$bench_root")"
if [[ -z "$verifier" || "$verifier" == *$'\n'* || "$(realpath -- "$verifier")" != "$verifier" ]]; then
  echo "P7 verifier publisher must return one canonical release path" >&2
  exit 2
fi

if [[ "${BM_P7_VERIFY_MAX_RSS:-0}" == "1" ]]; then
  cmd=(
    "$launcher" --executable "$verifier" --
    --verify-max-rss
    --benchmark-root "$bench_root"
    --run-id "$run_id"
  )
else
  cmd=(
    "$launcher" --executable "$verifier" --
    --preflight-report "${results_dir}/preflight-report.json"
    --summary "$locomo"
    --summary "$oracle"
    --summary "$s_cleaned"
    --summary "$m_cleaned"
  )
fi

set +e
"${cmd[@]}"
status=$?
set -e

if [[ "$status" -ne 0 ]]; then
  exit "$status"
fi

if [[ "${BM_P7_VERIFY_MAX_RSS:-0}" != "1" ]]; then
  echo "check_w4_external_noisy_wall_operator: ok"
fi
