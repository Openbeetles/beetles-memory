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

bench_root="${BM_W4_EXTERNAL_BENCH_ROOT}"
results_dir="${bench_root}/results/runs/${run_id}"
report_path="${results_dir}/operator-report.json"
tmp_report_path="${results_dir}/operator-report.json.tmp-$$-${RANDOM}"

if [[ -e "$report_path" ]]; then
  echo "operator report already exists and is immutable: $report_path" >&2
  exit 2
fi
if [[ -e "$tmp_report_path" ]]; then
  echo "operator temp report already exists and will not be replaced: $tmp_report_path" >&2
  exit 2
fi

locomo="${results_dir}/locomo.merged.summary.json"
oracle="${results_dir}/longmemeval_oracle.merged.summary.json"
s_cleaned="${results_dir}/longmemeval_s_cleaned.merged.summary.json"
m_cleaned="${results_dir}/longmemeval_m_cleaned.merged.summary.json"

cmd=(
  cargo run -p bm-replay --bin bm-w4-external-noisy-wall --
  --preflight-report "${results_dir}/preflight-report.json"
  --summary "$locomo"
  --summary "$oracle"
  --summary "$s_cleaned"
  --summary "$m_cleaned"
)

set +e
set -o noclobber
"${cmd[@]}" >"$tmp_report_path"
status=$?
set +o noclobber
set -e

if [[ "$status" -ne 0 ]]; then
  cat "$tmp_report_path" >&2 || true
  exit "$status"
fi

mv -n "$tmp_report_path" "$report_path"
if [[ -e "$tmp_report_path" ]]; then
  echo "operator report publish refused to overwrite: $report_path" >&2
  exit 2
fi

echo "check_w4_external_noisy_wall_operator: ok"
