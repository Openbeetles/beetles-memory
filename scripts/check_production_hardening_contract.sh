#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

cargo test -p bm-core --features sqlite-index --test sqlite_index_state_dir_contract
cargo test -p bm-store --test runtime_store_budget_contract
cargo test -p bm-store --test file_store_contract
cargo test -p bm-store --features sqlite-store --test sqlite_store_contract
cargo test -p bm-entry --test runtime_contract entry_runtime
cargo test -p bm-mcp --features server-stdio --bin bm-mcp-server mcp_server
cargo test -p bm-http --features server-std --bin bm-http-console http_console
cargo test -p bm-llm-gateway --no-default-features --features server-async,client-reqwest --bin bm-llm-gateway llm_gateway

bash -n scripts/check_release_surface.sh

if rg -n "target/bm-memory-gateway-store|target/bm-http-console-store|--allow-dirty" \
  scripts docs dev-docs \
  --glob '!scripts/check_production_hardening_contract.sh' \
  --glob '!dev-docs/production-hardening-audit-plan.md'; then
  fail "production docs or gates still mention repository-local target store defaults or --allow-dirty"
fi

if rg -n 'target/bm-memory-gateway-store|target/bm-http-console-store' crates scripts docs dev-docs \
  --glob '!scripts/check_production_hardening_contract.sh' \
  --glob '!dev-docs/production-hardening-audit-plan.md'; then
  fail "production surface still exposes relative target store paths"
fi

git diff --check
git -C dev-docs diff --check

echo "check_production_hardening_contract: ok"
