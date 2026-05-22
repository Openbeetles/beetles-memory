#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/lib_contract_checks.sh"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

cargo test -p bm-llm-gateway --no-default-features
cargo test -p bm-llm-gateway --no-default-features --features server-async,client-reqwest
cargo test -p bm-llm-gateway --no-default-features --features profile-server-linux-memory-gateway
cargo test -p bm-llm-gateway --no-default-features --features profile-server-linux-dev-full
cargo clippy -p bm-llm-gateway --all-targets --no-default-features --features server-async,client-reqwest -- -D warnings
bash scripts/check_llm_gateway_local_openai_smoke.sh

if contract_manifest_has_core_store_dependency crates/llm-gateway/Cargo.toml; then
  fail "bm-llm-gateway must not depend on bm-core or bm-store directly"
fi

if contract_rg_match 'bm_core::|bm_store::' crates/llm-gateway/src crates/llm-gateway/tests; then
  fail "bm-llm-gateway must not import core/store internals"
fi
if contract_rg_match 'MemoryWriteRequest|AdapterOperation::Write|runtime\(\)\.write\(' crates/llm-gateway/src; then
  fail "bm-llm-gateway post-reply maintenance must not bypass SDK maintain with raw writes"
fi

for manifest in crates/http/Cargo.toml crates/wss/Cargo.toml crates/mcp/Cargo.toml crates/a2a/Cargo.toml; do
  if contract_rg_match 'bm-llm-gateway|bm_llm_gateway' "$manifest"; then
    fail "$manifest must not depend on bm-llm-gateway"
  fi
done

default_tree="$(cargo tree -p bm-llm-gateway --no-default-features)"
if grep -Eq 'tokio|axum|hyper|reqwest|tower|tungstenite' <<<"$default_tree"; then
  echo "$default_tree" >&2
  fail "bm-llm-gateway default dependency tree must not include server/client heavy dependencies"
fi

if ! rg -n 'name = "bm-llm-gateway"' crates/llm-gateway/Cargo.toml >/dev/null; then
  fail "bm-llm-gateway binary manifest entry is missing"
fi
if ! rg -n 'required-features = \["server-async", "client-reqwest"\]' crates/llm-gateway/Cargo.toml >/dev/null; then
  fail "bm-llm-gateway binary must be gated behind server-async and client-reqwest"
fi
if contract_rg_match '/v1/chat/completions|/api/chat|/api/generate' crates/http/src crates/http/tests; then
  fail "model protocol routes must not be added to bm-http"
fi

echo "check_llm_gateway_contract: ok"
