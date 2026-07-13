#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

cargo test -p bm-core --features sqlite-index --test sqlite_index_state_dir_contract
cargo test -p bm-store-contract-tests --test runtime_store_budget_contract
cargo test -p bm-store-contract-tests --test file_store_contract
cargo test -p bm-store-contract-tests --features sqlite-store --test sqlite_store_contract
cargo test -p bm-entry --test runtime_contract entry_runtime
cargo test -p bm-mcp --features server-stdio --bin bm-mcp-server mcp_server
cargo test -p bm-http --features server-std --bin bm-http-console http_console
cargo test -p bm-llm-gateway --no-default-features --features server-async,client-reqwest --bin bm-llm-gateway llm_gateway

if cargo check -p bm-sdk --no-default-features \
  --features profile-server-linux-memory-gateway,nonproduction-replay-harness \
  >/dev/null 2>&1; then
  fail "nonproduction replay harness compiled with a production SDK profile"
fi

for package in bm-desktop bm-cli bm-llm-gateway bm-http bm-wss bm-mcp bm-a2a; do
  if cargo tree -p "$package" -e normal,build,features \
    | rg -q 'nonproduction-replay-harness|bm-replay'; then
    fail "production dependency graph contains replay tooling: $package"
  fi
done

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
