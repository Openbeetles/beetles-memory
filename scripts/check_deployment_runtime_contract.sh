#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/lib_contract_checks.sh"

assert_feature_set_rejected() {
  local package="$1"
  local feature_set="$2"
  local expected="$3"
  local output

  if output="$(cargo check -p "$package" --no-default-features --features "$feature_set" 2>&1)"; then
    echo "FAIL: $package feature set should have been rejected: $feature_set" >&2
    exit 1
  fi

  if ! grep -q "$expected" <<<"$output"; then
    echo "FAIL: $package feature set rejected for unexpected reason: $feature_set" >&2
    echo "$output" >&2
    exit 1
  fi
}

cargo test -p bm-http --features server-std --test http_runtime_contract
cargo test -p bm-http --features server-std --test http_backend_contract
cargo test -p bm-wss --features server-std --test wss_runtime_contract
cargo test -p bm-wss --features server-std --test wss_backend_contract
cargo test -p bm-mcp --features server-stdio --test mcp_runtime_contract
cargo test -p bm-mcp --features server-stdio --test mcp_stdio_contract
cargo test -p bm-a2a --features bridge-http --test a2a_runtime_contract
cargo test -p bm-a2a --features bridge-http --test a2a_http_contract

removed_axum_feature="server""-axum"

if rg -n "${removed_axum_feature}" \
  Cargo.toml README.md docs crates examples scripts \
  >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: deployment runtime surface still contains removed placeholder feature names" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

for crate in crates/http crates/wss crates/mcp crates/a2a crates/llm-gateway; do
  if rg -n 'bm_core::|bm_store::|crates/core|crates/store' "$crate/src" >/tmp/bm-deployment-contract-hit 2>/dev/null; then
    echo "FAIL: transport backend must not import core/store directly: $crate" >&2
    cat /tmp/bm-deployment-contract-hit >&2
    exit 1
  fi
  if contract_manifest_has_core_store_dependency "$crate/Cargo.toml"; then
    echo "FAIL: transport backend manifest must not depend on bm-core or bm-store directly: $crate" >&2
    exit 1
  fi
done

if rg -n 'adapter-beetle|source_kind.*beetle|beetle_host|beetle_adapter|beetle_source|qq|feishu|wecom|dingtalk' \
  crates/{http,wss,mcp,a2a}/src crates/{http,wss,mcp,a2a}/tests \
  >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: deployment runtime surface must not contain source-project or product-channel identifiers" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if cargo tree -p bm-entry --no-default-features --features profile-esp-standalone-memory | rg -n 'rusqlite|axum|tokio|hyper|tungstenite' >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: ESP standalone entry must not pull sqlite or server-heavy network deps" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if cargo tree -p bm-entry --no-default-features --features profile-esp-embedded-sdk | rg -n 'rusqlite|axum|tokio|hyper|tungstenite' >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: ESP embedded SDK entry must not pull sqlite or server-heavy network deps" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if cargo tree -p bm-http --no-default-features --features profile-esp-standalone-memory | rg -n 'bm-ollama-transparent|rusqlite|axum|tokio|hyper|tungstenite' >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: ESP standalone HTTP contract must not pull desktop, sqlite, or server-heavy deps" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if cargo tree -p bm-http --no-default-features --features profile-esp-embedded-sdk | rg -n 'bm-ollama-transparent|rusqlite|axum|tokio|hyper|tungstenite' >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: ESP embedded SDK HTTP contract must not pull desktop, sqlite, or server-heavy deps" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

assert_feature_set_rejected bm-http "server-std,profile-esp-standalone-memory" "server-std is forbidden for ESP profiles"
assert_feature_set_rejected bm-http "server-std,profile-esp-embedded-sdk" "server-std is forbidden for ESP profiles"
assert_feature_set_rejected bm-wss "server-std,profile-esp-standalone-memory" "server-std is forbidden for ESP profiles"
assert_feature_set_rejected bm-wss "server-std,profile-esp-embedded-sdk" "server-std is forbidden for ESP profiles"
assert_feature_set_rejected bm-mcp "server-stdio,profile-esp-standalone-memory" "server-stdio is forbidden for ESP profiles"
assert_feature_set_rejected bm-mcp "server-stdio,profile-esp-embedded-sdk" "server-stdio is forbidden for ESP profiles"
assert_feature_set_rejected bm-a2a "bridge-http,profile-esp-standalone-memory" "bridge-http is forbidden for ESP profiles"
assert_feature_set_rejected bm-a2a "bridge-http,profile-esp-embedded-sdk" "bridge-http is forbidden for ESP profiles"

rm -f /tmp/bm-deployment-contract-hit
echo "check_deployment_runtime_contract: ok"
