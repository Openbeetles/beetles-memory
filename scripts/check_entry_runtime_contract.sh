#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v rg >/dev/null 2>&1; then
  echo "check_entry_runtime_contract: ripgrep (rg) is required" >&2
  exit 1
fi

cargo test --locked -p bm-entry
cargo test --locked -p bm-cli --test cli_contract
cargo test --locked -p bm-http --features server-std --test http_runtime_contract
cargo test --locked -p bm-wss --features server-std --test wss_runtime_contract
cargo test --locked -p bm-mcp --features server-stdio --test mcp_runtime_contract
cargo test --locked -p bm-a2a --features bridge-http --test a2a_runtime_contract

entry_consumers=(
  crates/cli
  crates/http
  crates/wss
  crates/mcp
  crates/a2a
)

for crate in "${entry_consumers[@]}"; do
  if rg -n 'bm_core::|bm_store::|crates/core|crates/store' "$crate/src" >/tmp/bm-entry-contract-hit 2>/dev/null; then
    echo "FAIL: entry consumer must not import core/store directly: $crate" >&2
    cat /tmp/bm-entry-contract-hit >&2
    exit 1
  fi
  if rg -n '(^|\s)(bm-core|bm-store)\s*=' "$crate/Cargo.toml" >/tmp/bm-entry-contract-hit 2>/dev/null; then
    echo "FAIL: entry consumer manifest must not depend on bm-core or bm-store directly: $crate" >&2
    cat /tmp/bm-entry-contract-hit >&2
    exit 1
  fi
done

if rg -n 'adapter-beetle|source_kind.*beetle|beetle_host|beetle_adapter|beetle_source|qq|feishu|wecom|dingtalk' \
  crates/entry/src crates/entry/tests crates/{cli,http,wss,mcp,a2a}/src crates/{cli,http,wss,mcp,a2a}/tests \
  >/tmp/bm-entry-contract-hit 2>/dev/null; then
  echo "FAIL: entry runtime surface must not contain source-project or product-channel identifiers" >&2
  cat /tmp/bm-entry-contract-hit >&2
  exit 1
fi

if cargo tree --locked -p bm-entry --no-default-features --features profile-esp-standalone-memory | rg -n 'rusqlite|axum|tokio' >/tmp/bm-entry-contract-hit 2>/dev/null; then
  echo "FAIL: ESP standalone entry must not pull sqlite or server-heavy network deps" >&2
  cat /tmp/bm-entry-contract-hit >&2
  exit 1
fi

if cargo tree --locked -p bm-entry --no-default-features --features profile-esp-embedded-sdk | rg -n 'rusqlite|axum|tokio' >/tmp/bm-entry-contract-hit 2>/dev/null; then
  echo "FAIL: ESP embedded SDK entry must not pull sqlite or server-heavy network deps" >&2
  cat /tmp/bm-entry-contract-hit >&2
  exit 1
fi

rm -f /tmp/bm-entry-contract-hit
echo "check_entry_runtime_contract: ok"
