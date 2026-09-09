#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -z "${BM_W4_EXTERNAL_BENCH_ROOT:-}" ]]; then
  echo "BM_W4_EXTERNAL_BENCH_ROOT is required" >&2
  exit 2
fi
if [[ ! -d "${BM_W4_EXTERNAL_BENCH_ROOT}/data" ]]; then
  echo "prepare the external benchmark root first" >&2
  exit 2
fi

benchmark_root="$(cd "${BM_W4_EXTERNAL_BENCH_ROOT}" && pwd -P)"

sha256_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d ' ' -f 1
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    echo "shasum or sha256sum is required" >&2
    return 1
  fi
}

fetch_exact() {
  local url="$1"
  local destination="$2"
  local expected_sha256="$3"
  local partial="${destination}.partial"

  mkdir -p "$(dirname "$destination")"
  if [[ -f "$destination" ]]; then
    if [[ "$(sha256_file "$destination")" == "$expected_sha256" ]]; then
      printf 'verified %s\n' "$destination"
      return 0
    fi
    echo "existing dataset digest mismatch for $destination" >&2
    return 1
  fi

  curl --fail --location --continue-at - --output "$partial" "$url"
  local actual_sha256
  actual_sha256="$(sha256_file "$partial")"
  if [[ "$actual_sha256" != "$expected_sha256" ]]; then
    echo "dataset digest mismatch for $destination" >&2
    return 1
  fi
  mv "$partial" "$destination"
  printf 'downloaded %s\n' "$destination"
}

fetch_exact \
  "https://raw.githubusercontent.com/snap-research/locomo/main/data/locomo10.json" \
  "${benchmark_root}/data/locomo10.json" \
  "79fa87e90f04081343b8c8debecb80a9a6842b76a7aa537dc9fdf651ea698ff4"
fetch_exact \
  "https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned/resolve/main/longmemeval_oracle.json" \
  "${benchmark_root}/data/longmemeval_oracle.json" \
  "821a2034d219ab45846873dd14c14f12cfe7776e73527a483f9dac095d38620c"
fetch_exact \
  "https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned/resolve/main/longmemeval_s_cleaned.json" \
  "${benchmark_root}/data/longmemeval_s_cleaned.json" \
  "d6f21ea9d60a0d56f34a05b609c79c88a451d2ae03597821ea3d5a9678c3a442"
fetch_exact \
  "https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned/resolve/main/longmemeval_m_cleaned.json" \
  "${benchmark_root}/data/longmemeval_m_cleaned.json" \
  "9d79e5524794a2e6900a3aa9cb7d9152c5a3e8319c9a87c25494ba1eacee495f"
