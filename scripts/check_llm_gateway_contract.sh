#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/lib_contract_checks.sh"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

case "$(uname -s)" in
  Darwin)
    production_feature="profile-desktop-macos-standalone-memory"
    mismatched_production_feature="profile-server-linux-memory-gateway"
    ;;
  Linux)
    production_feature="profile-server-linux-memory-gateway"
    mismatched_production_feature="profile-desktop-macos-standalone-memory"
    ;;
  *)
    production_feature=""
    mismatched_production_feature=""
    ;;
esac

compile_log="$(mktemp "${TMPDIR:-/tmp}/bm-llm-gateway-profile.XXXXXX")"
trap 'rm -f "$compile_log"' EXIT

assert_gateway_compile_fails() {
  local label="$1"
  local features="$2"
  local expected="$3"
  if cargo check --locked -p bm-llm-gateway --bin bm-llm-gateway \
    --no-default-features --features "$features" >"$compile_log" 2>&1; then
    fail "$label unexpectedly compiled"
  fi
  if ! rg -Fq "$expected" "$compile_log"; then
    cat "$compile_log" >&2
    fail "$label did not fail with the governed compile-time reason"
  fi
}

cargo test --locked -p bm-llm-gateway --no-default-features
cargo test --locked -p bm-llm-gateway --lib --no-default-features \
  --features nonproduction-replay-harness,server-async,client-reqwest
cargo test --locked -p bm-llm-gateway --all-targets --no-default-features --features profile-server-linux-memory-gateway
cargo test --locked -p bm-llm-gateway --all-targets --no-default-features --features profile-desktop-macos-standalone-memory
cargo test --locked -p bm-llm-gateway --no-default-features --features profile-server-linux-dev-full
cargo clippy --locked -p bm-llm-gateway --lib --no-default-features \
  --features nonproduction-replay-harness,server-async,client-reqwest -- -D warnings
cargo clippy --locked -p bm-llm-gateway --tests --no-default-features -- -D warnings

assert_gateway_compile_fails \
  "gateway executable without production profile" \
  "server-async,client-reqwest" \
  "bm-llm-gateway executable requires exactly one production profile"
assert_gateway_compile_fails \
  "gateway executable with dev-full profile" \
  "server-async,client-reqwest,profile-server-linux-dev-full" \
  "bm-llm-gateway executable requires exactly one production profile"
assert_gateway_compile_fails \
  "gateway executable with two production profiles" \
  "server-async,client-reqwest,profile-server-linux-memory-gateway,profile-desktop-macos-standalone-memory" \
  "Beetle Memory feature contract requires at most one target-* feature per build"

if ! rg -Fq 'bm-llm-gateway executable accepts exactly one production profile' \
  crates/llm-gateway/src/bin/bm-llm-gateway.rs; then
  fail "gateway executable does not own its exactly-one production profile compile guard"
fi

if [[ -n "$production_feature" ]]; then
  cargo check --locked -p bm-llm-gateway --bin bm-llm-gateway \
    --no-default-features --features "server-async,client-reqwest,$production_feature"
  assert_gateway_compile_fails \
    "gateway executable with target-mismatched production profile" \
    "server-async,client-reqwest,$mismatched_production_feature" \
    "requires target_os="
else
  assert_gateway_compile_fails \
    "Windows gateway executable with Linux production profile" \
    "server-async,client-reqwest,profile-server-linux-memory-gateway" \
    "requires target_os=linux"
  assert_gateway_compile_fails \
    "Windows gateway executable with macOS production profile" \
    "server-async,client-reqwest,profile-desktop-macos-standalone-memory" \
    "requires target_os=macos"
fi

if cargo check --locked -p bm-desktop --no-default-features >"$compile_log" 2>&1; then
  fail "bm-desktop unexpectedly compiled without its production profile"
fi
if ! rg -Fq 'bm-desktop requires profile-desktop-macos-standalone-memory' "$compile_log"; then
  cat "$compile_log" >&2
  fail "bm-desktop did not fail closed without its production profile"
fi
if [[ "$(uname -s)" == Darwin ]]; then
  cargo check --locked -p bm-desktop --no-default-features \
    --features profile-desktop-macos-standalone-memory
elif cargo check --locked -p bm-desktop --no-default-features \
  --features profile-desktop-macos-standalone-memory >"$compile_log" 2>&1; then
  fail "macOS standalone desktop profile compiled for a non-macOS target"
elif ! rg -Fq 'profile-desktop-macos-standalone-memory requires target_os=macos' "$compile_log"; then
  cat "$compile_log" >&2
  fail "desktop target mismatch did not fail with the governed compile-time reason"
fi

if ! rg -U 'cargo", \[\n\s*"build",\n\s*"--locked"' apps/desktop/scripts/build-sidecars.mjs >/dev/null; then
  fail "desktop release sidecar build must consume the workspace lockfile"
fi
if ! rg -n 'server-async,client-reqwest,profile-desktop-macos-standalone-memory' apps/desktop/scripts/build-sidecars.mjs >/dev/null; then
  fail "desktop release gateway must compile the governed macOS standalone profile"
fi
if ! rg -U '"--release",\n\s*"--target",\n\s*triple' apps/desktop/scripts/build-sidecars.mjs >/dev/null; then
  fail "desktop release gateway binary and Tauri sidecar name must share one target triple"
fi
if ! rg -n 'tauri (dev|build) --features profile-desktop-macos-standalone-memory' apps/desktop/package.json >/dev/null; then
  fail "desktop dev/build commands must select the governed main-process profile"
fi

desktop_profile="profile-desktop-macos-standalone-memory"
desktop_features="$desktop_profile"
sidecar_features="server-async,client-reqwest,$desktop_profile"
desktop_tree="$(cargo tree --locked -e normal,build,features -p bm-desktop --no-default-features --features "$desktop_features")"
sidecar_tree="$(cargo tree --locked -e normal,build,features -p bm-llm-gateway --no-default-features --features "$sidecar_features")"
for tree_name in desktop sidecar; do
  [[ "$tree_name" == desktop ]] && tree="$desktop_tree" || tree="$sidecar_tree"
  if grep -Eq 'nonproduction-replay-harness|bm-replay' <<<"$tree"; then
    echo "$tree" >&2
    fail "$tree_name production feature tree contains replay harness code"
  fi
done

assert_profile_forwarded() {
  local tree_name="$1"
  local root_package="$2"
  local root_features="$3"
  local dependency="$4"
  local inverse_tree
  inverse_tree="$(cargo tree --locked -e features -p "$root_package" --no-default-features \
    --features "$root_features" -i "$dependency")"
  if ! grep -Fq "$dependency feature \"$desktop_profile\"" <<<"$inverse_tree"; then
    echo "$inverse_tree" >&2
    fail "$tree_name feature tree does not forward $desktop_profile to $dependency"
  fi
}

for package in bm-entry bm-adapter bm-sdk bm-core; do
  assert_profile_forwarded desktop bm-desktop "$desktop_features" "$package"
  assert_profile_forwarded sidecar bm-llm-gateway "$sidecar_features" "$package"
done
assert_profile_forwarded desktop bm-desktop "$desktop_features" bm-http
bash scripts/check_llm_gateway_local_openai_smoke.sh
bash scripts/check_llm_gateway_release_integrations.sh

if contract_manifest_has_core_store_dependency crates/llm-gateway/Cargo.toml; then
  fail "bm-llm-gateway must not depend on bm-core or bm-store directly"
fi

if contract_rg_match 'bm_core::|bm_store::' crates/llm-gateway/src crates/llm-gateway/tests; then
  fail "bm-llm-gateway must not import core/store internals"
fi
if contract_rg_match 'MemoryWriteRequest|AdapterOperation::Write|runtime\(\)\.write\(' crates/llm-gateway/src; then
  fail "bm-llm-gateway post-reply maintenance must not bypass SDK maintain with raw writes"
fi
if rg -U 'pub fn (handle_(openai|ollama)_request|serve_(llm_gateway|openai|ollama)_http)[^{]{0,400}config: &GatewayConfig' \
  crates/llm-gateway/src/openai.rs crates/llm-gateway/src/ollama.rs crates/llm-gateway/src/server.rs >/dev/null; then
  fail "gateway request entrypoints must consume the runtime-owned validated configuration"
fi

for manifest in crates/http/Cargo.toml crates/wss/Cargo.toml crates/mcp/Cargo.toml crates/a2a/Cargo.toml; do
  if contract_rg_match 'bm-llm-gateway|bm_llm_gateway' "$manifest"; then
    fail "$manifest must not depend on bm-llm-gateway"
  fi
done

default_tree="$(cargo tree --locked -p bm-llm-gateway --no-default-features)"
if grep -Eq 'tokio|axum|hyper|reqwest|tower|tungstenite' <<<"$default_tree"; then
  echo "$default_tree" >&2
  fail "bm-llm-gateway default dependency tree must not include server/client heavy dependencies"
fi

for production_feature in profile-server-linux-memory-gateway profile-desktop-macos-standalone-memory; do
  production_tree="$(cargo tree --locked -e features -p bm-llm-gateway --no-default-features --features "$production_feature")"
  if grep -Fq 'nonproduction-replay-harness' <<<"$production_tree"; then
    echo "$production_tree" >&2
    fail "bm-llm-gateway production feature $production_feature must not enable nonproduction-replay-harness"
  fi
done

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
