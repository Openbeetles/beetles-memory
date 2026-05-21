#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v rg >/dev/null 2>&1; then
  echo "check_deployment_runtime_contract: ripgrep (rg) is required" >&2
  exit 1
fi

cargo test -p bm-http --features server-std --test http_runtime_contract
cargo test -p bm-http --features server-std --test http_backend_contract
cargo test -p bm-wss --features server-std --test wss_runtime_contract
cargo test -p bm-wss --features server-std --test wss_backend_contract
cargo test -p bm-mqtt --features bridge-std --test mqtt_runtime_contract
cargo test -p bm-mqtt --features bridge-std --test mqtt_backend_contract
cargo test -p bm-mcp --features server-stdio --test mcp_runtime_contract
cargo test -p bm-mcp --features server-stdio --test mcp_stdio_contract
cargo test -p bm-a2a --features bridge-http --test a2a_runtime_contract
cargo test -p bm-a2a --features bridge-http --test a2a_http_contract

removed_axum_feature="server""-axum"
removed_mqtt_feature="bridge""-rumqttc"

if rg -n "${removed_axum_feature}|${removed_mqtt_feature}" \
  Cargo.toml README.md docs crates examples scripts \
  >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: deployment runtime surface still contains removed placeholder feature names" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

for crate in crates/http crates/wss crates/mqtt crates/mcp crates/a2a; do
  if rg -n 'bm_core::|bm_store::|crates/core|crates/store' "$crate/src" >/tmp/bm-deployment-contract-hit 2>/dev/null; then
    echo "FAIL: transport backend must not import core/store directly: $crate" >&2
    cat /tmp/bm-deployment-contract-hit >&2
    exit 1
  fi
  if rg -n '(^|\s)(bm-core|bm-store)\s*=' "$crate/Cargo.toml" >/tmp/bm-deployment-contract-hit 2>/dev/null; then
    echo "FAIL: transport backend manifest must not depend on bm-core or bm-store directly: $crate" >&2
    cat /tmp/bm-deployment-contract-hit >&2
    exit 1
  fi
done

if rg -n 'adapter-beetle|source_kind.*beetle|beetle_host|beetle_adapter|beetle_source|qq|feishu|wecom|dingtalk' \
  crates/{http,wss,mqtt,mcp,a2a}/src crates/{http,wss,mqtt,mcp,a2a}/tests \
  >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: deployment runtime surface must not contain source-project or product-channel identifiers" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if cargo tree -p bm-entry --no-default-features --features profile-esp-standalone-memory | rg -n 'rusqlite|axum|tokio|rumqttc|hyper|tungstenite' >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: ESP standalone entry must not pull sqlite or server-heavy network deps" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

if cargo tree -p bm-entry --no-default-features --features profile-esp-embedded-sdk | rg -n 'rusqlite|axum|tokio|rumqttc|hyper|tungstenite' >/tmp/bm-deployment-contract-hit 2>/dev/null; then
  echo "FAIL: ESP embedded SDK entry must not pull sqlite or server-heavy network deps" >&2
  cat /tmp/bm-deployment-contract-hit >&2
  exit 1
fi

rm -f /tmp/bm-deployment-contract-hit
echo "check_deployment_runtime_contract: ok"
