#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -z "${BM_W4_EXTERNAL_BENCH_ROOT:-}" ]]; then
  echo "BM_W4_EXTERNAL_BENCH_ROOT is required for the explicit W4 external noisy wall operator" >&2
  exit 2
fi

bench_root="${BM_W4_EXTERNAL_BENCH_ROOT}"
results_dir="${bench_root}/results"
runner_source="${bench_root}/runner/src/main.rs"
report_path="${BM_W4_EXTERNAL_REPORT_PATH:-/tmp/bm-w4-external-noisy-wall.operator-report.json}"

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "missing required W4 external noisy operator input: $path" >&2
    exit 2
  fi
}

sha256_file() {
  shasum -a 256 "$1" | awk '{print $1}'
}

locomo="${results_dir}/locomo.merged.summary.json"
oracle="${results_dir}/longmemeval_oracle.merged.summary.json"
s_cleaned="${results_dir}/longmemeval_s_cleaned.merged.summary.json"
m_cleaned="${results_dir}/longmemeval_m_cleaned.merged.summary.json"

require_file "$runner_source"
require_file "$locomo"
require_file "$oracle"
require_file "$s_cleaned"
require_file "$m_cleaned"

runner_hash="$(sha256_file "$runner_source")"

cmd=(
  cargo run -p bm-replay --bin bm-w4-external-noisy-wall --
  --runner-source-sha256 "$runner_hash"
  --summary "$(sha256_file "$locomo")" "$locomo"
  --summary "$(sha256_file "$oracle")" "$oracle"
  --summary "$(sha256_file "$s_cleaned")" "$s_cleaned"
  --summary "$(sha256_file "$m_cleaned")" "$m_cleaned"
)

set +e
"${cmd[@]}" >"$report_path"
status=$?
set -e

if [[ "${BM_W4_EXTERNAL_EXPECT_BLOCKED:-}" == "1" ]]; then
  if [[ "$status" -ne 10 ]]; then
    cat "$report_path" >&2 || true
    echo "expected current W4 external noisy wall to be blocked only by expected baseline reasons, got exit $status" >&2
    exit 1
  fi
  rg -q '"provenance_attached": true' "$report_path"
  rg -q 'w4_external_noisy_wall_improvement_not_proven' "$report_path"
  rg -q 'w4_external_noisy_wall_stage_attribution_not_proven' "$report_path"
  rg -q 'w4_external_noisy_wall_index_effect_not_proven' "$report_path"
  if rg -q '"stage_diagnostics_attached": false' "$report_path"; then
    rg -q 'w4_external_noisy_wall_stage_diagnostics_missing' "$report_path"
  fi
  if rg -q '"index_diagnostics_attached": false' "$report_path"; then
    rg -q 'w4_external_noisy_wall_index_diagnostics_missing' "$report_path"
  fi
  ! rg -q 'w4_external_noisy_wall_provenance_missing' "$report_path"
  echo "check_w4_external_noisy_wall_operator: baseline blocked as expected"
  exit 0
fi

if [[ "$status" -ne 0 ]]; then
  cat "$report_path" >&2 || true
  exit "$status"
fi

echo "check_w4_external_noisy_wall_operator: ok"
