#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v rg >/dev/null 2>&1; then
  echo "check_adapter_communication_contract: ripgrep (rg) is required" >&2
  exit 1
fi

adapter_crates=(
  crates/adapter
  crates/cli
  crates/http
  crates/wss
  crates/mcp
  crates/a2a
)

for crate in "${adapter_crates[@]}"; do
  if rg -n 'bm_core::|bm_store::|crates/core|crates/store' "$crate/src" >/tmp/bm-adapter-contract-hit 2>/dev/null; then
    echo "FAIL: adapter crate must not import core/store directly: $crate" >&2
    cat /tmp/bm-adapter-contract-hit >&2
    exit 1
  fi
  if rg -n '(^|\s)(bm-core|bm-store)\s*=' "$crate/Cargo.toml" >/tmp/bm-adapter-contract-hit 2>/dev/null; then
    echo "FAIL: adapter crate manifest must not depend on bm-core or bm-store directly: $crate" >&2
    cat /tmp/bm-adapter-contract-hit >&2
    exit 1
  fi
done

if rg -n 'adapter-beetle|source_kind.*beetle|route.*beetle|topic.*beetle|tool.*beetle|qq|feishu|wecom|dingtalk' crates/{adapter,cli,http,wss,mcp,a2a}/src crates/{adapter,cli,http,wss,mcp,a2a}/tests >/tmp/bm-adapter-contract-hit 2>/dev/null; then
  echo "FAIL: adapter public surface must not contain source-project or product-channel identifiers" >&2
  cat /tmp/bm-adapter-contract-hit >&2
  exit 1
fi

if ! rg -n 'max_frame_bytes|WssBudget|payload budget|frame budget' crates/wss/tests crates/wss/src >/dev/null; then
  echo "FAIL: WSS adapter must have frame/payload budget contract" >&2
  exit 1
fi

echo "check_adapter_communication_contract: ok"
