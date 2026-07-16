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

  if output="$(cargo check --locked -p "$package" --no-default-features --features "$feature_set" 2>&1)"; then
    echo "FAIL: $package feature set should have been rejected: $feature_set" >&2
    exit 1
  fi

  if ! grep -q "$expected" <<<"$output"; then
    echo "FAIL: $package feature set rejected for unexpected reason: $feature_set" >&2
    echo "$output" >&2
    exit 1
  fi
}

cargo test --locked -p bm-http --features server-std --test http_runtime_contract
cargo test --locked -p bm-http --features server-std --test http_backend_contract
cargo test --locked -p bm-http --features server-std --bin bm-http-console
cargo test --locked -p bm-wss --features server-std --test wss_runtime_contract
cargo test --locked -p bm-wss --features server-std --test wss_backend_contract
cargo test --locked -p bm-mcp --features server-stdio --test mcp_runtime_contract
cargo test --locked -p bm-mcp --features server-stdio --test mcp_stdio_contract
cargo test --locked -p bm-mcp --features server-stdio --bin bm-mcp-server
cargo test --locked -p bm-a2a --features bridge-http --test a2a_runtime_contract
cargo test --locked -p bm-a2a --features bridge-http --test a2a_http_contract

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

if rg -n 'pub fn serve_.*stream_from_peer|pub fn serve_http_stream[<(]|pub fn serve_mcp_streamable_http_stream[<(]|pub fn serve_a2a_http_stream[<(]|pub peer_is_loopback:|pub authenticated: bool' \
  crates/{http,mcp,wss,a2a}/src crates/entry/src >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: network adapter exposes caller-forgeable local trust" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if rg -U -n 'pub fn [^{;]*(\bTcpStream\b|Read[[:space:]]*\+[[:space:]]*Write|Write[[:space:]]*\+[[:space:]]*Read)' \
  crates/a2a/src >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: A2A network API must accept EntryAcceptedTcpStream, not bare TCP or Read + Write" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

for required in \
  'pub fn serve_http_accepted_stream' \
  'pub fn handle_http_in_process_request' \
  'pub fn serve_mcp_streamable_http_accepted_stream' \
  'pub fn serve_wss_accepted_stream' \
  'pub fn serve_a2a_http_accepted_stream' \
  'pub fn handle_in_process_request' \
  'pub fn handle_mcp_streamable_http_in_process_request'; do
  rg -F -q "$required" crates/http/src/lib.rs crates/mcp/src/lib.rs crates/wss/src/lib.rs crates/a2a/src/lib.rs || {
    echo "FAIL: explicit transport trust boundary is missing: $required" >&2
    exit 1
  }
done

if rg -n 'A2aBridge[^\n]*EntryAuthDecision|auth: EntryAuthDecision|request_identity_owner: AdapterRequestIdentityOwner' \
  crates/a2a/src >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: A2A bridge must not cache a network principal across requests" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if rg -n 'EntryTransportConfig::all_enabled\(\)' crates/{http,mcp,a2a}/src/bin \
  >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: production transport binary must enable only the transport it serves" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

rg -F -q 'pub fn accept(listener: &TcpListener)' crates/entry/src/accepted_tcp.rs || {
  echo "FAIL: accepted TCP authority must be constructed only by listener accept" >&2
  exit 1
}

if rg -n 'adapter-beetle|source_kind.*beetle|beetle_host|beetle_adapter|beetle_source|qq|feishu|wecom|dingtalk' \
  crates/{http,wss,mcp,a2a}/src crates/{http,wss,mcp,a2a}/tests \
  >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: deployment runtime surface must not contain source-project or product-channel identifiers" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if cargo tree --locked -p bm-entry --no-default-features --features profile-esp-standalone-memory | rg -n 'rusqlite|axum|tokio|hyper|tungstenite' >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: ESP standalone entry must not pull sqlite or server-heavy network deps" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if cargo tree --locked -p bm-entry --no-default-features --features profile-esp-embedded-sdk | rg -n 'rusqlite|axum|tokio|hyper|tungstenite' >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: ESP embedded SDK entry must not pull sqlite or server-heavy network deps" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if cargo tree --locked -p bm-http --no-default-features --features profile-esp-standalone-memory | rg -n 'bm-ollama-transparent|rusqlite|axum|tokio|hyper|tungstenite' >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: ESP standalone HTTP contract must not pull desktop, sqlite, or server-heavy deps" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if cargo tree --locked -p bm-http --no-default-features --features profile-esp-embedded-sdk | rg -n 'bm-ollama-transparent|rusqlite|axum|tokio|hyper|tungstenite' >/tmp/bm-deployment-contract-hit 2>/dev/null; then
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
