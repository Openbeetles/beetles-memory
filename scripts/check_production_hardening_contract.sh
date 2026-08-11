#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

case "$(uname -s)" in
  Darwin) gateway_production_feature="profile-desktop-macos-standalone-memory" ;;
  Linux) gateway_production_feature="profile-server-linux-memory-gateway" ;;
  *) fail "bm-llm-gateway has no production profile for this hardening target" ;;
esac

cargo test --locked -p bm-core --features sqlite-index --test sqlite_index_state_dir_contract
cargo test --locked -p bm-store-contract-tests --test runtime_store_budget_contract
cargo test --locked -p bm-store-contract-tests --test file_store_contract
cargo test --locked -p bm-store-contract-tests --features sqlite-store --test sqlite_store_contract
cargo test --locked -p bm-entry --test runtime_contract entry_runtime
cargo test --locked -p bm-mcp --features server-stdio --bin bm-mcp-server mcp_server
cargo test --locked -p bm-http --features server-std --bin bm-http-console http_console
cargo check --locked -p bm-llm-gateway --no-default-features \
  --features "server-async,client-reqwest,$gateway_production_feature" \
  --bin bm-llm-gateway

if cargo check --locked -p bm-sdk --no-default-features \
  --features profile-server-linux-memory-gateway,nonproduction-replay-harness \
  >/dev/null 2>&1; then
  fail "nonproduction replay harness compiled with a production SDK profile"
fi

desktop_tree="$(cargo tree --locked -p bm-desktop -e normal,build,features \
  --no-default-features --features profile-desktop-macos-standalone-memory)"
if rg -q 'nonproduction-replay-harness|bm-replay' <<<"$desktop_tree"; then
  fail "production dependency graph contains replay tooling: bm-desktop"
fi

for package in bm-cli bm-llm-gateway bm-http bm-wss bm-mcp bm-a2a; do
  if cargo tree --locked -p "$package" -e normal,build,features \
    | rg -q 'nonproduction-replay-harness|bm-replay'; then
    fail "production dependency graph contains replay tooling: $package"
  fi
done

bash -n scripts/check_release_surface.sh

if rg -n "target/bm-memory-gateway-store|target/bm-http-console-store" \
  scripts docs dev-docs \
  --glob '!scripts/check_production_hardening_contract.sh' \
  --glob '!dev-docs/archive/production-hardening-audit-plan.md'; then
  fail "production docs or gates still mention repository-local target store defaults"
fi

while IFS=: read -r file _ line; do
  if [[ "$file" == "scripts/check_release_surface.sh" \
    && "$line" == *"cargo publish"* \
    && "$line" == *"--dry-run"* ]]; then
    continue
  fi
  fail "--allow-dirty is only permitted for the release-surface package dry-run: $file"
done < <(rg -n --no-heading \
  --glob '!scripts/check_production_hardening_contract.sh' \
  -- '--allow-dirty' scripts || true)

if rg -n 'target/bm-memory-gateway-store|target/bm-http-console-store' crates scripts docs dev-docs \
  --glob '!scripts/check_production_hardening_contract.sh' \
  --glob '!dev-docs/archive/production-hardening-audit-plan.md'; then
  fail "production surface still exposes relative target store paths"
fi

git diff --check
git -C dev-docs diff --check

echo "check_production_hardening_contract: ok"
