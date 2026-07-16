#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v rg >/dev/null 2>&1; then
  echo "check_llm_gateway_profile_budget: ripgrep (rg) is required" >&2
  exit 1
fi

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_no_heavy_deps() {
  local label="$1"
  local tree="$2"
  if grep -Eq 'tokio|axum|hyper|reqwest|tower|tungstenite' <<<"$tree"; then
    echo "$tree" >&2
    fail "$label must not compile server/client heavy dependencies"
  fi
}

assert_no_heavy_deps "bm-llm-gateway default" "$(cargo tree --locked -p bm-llm-gateway --no-default-features)"
assert_no_heavy_deps "bm-llm-gateway server memory gateway profile" "$(cargo tree --locked -p bm-llm-gateway --no-default-features --features profile-server-linux-memory-gateway)"
assert_no_heavy_deps "bm-llm-gateway macOS standalone profile" "$(cargo tree --locked -p bm-llm-gateway --no-default-features --features profile-desktop-macos-standalone-memory)"
assert_no_heavy_deps "bm-llm-gateway server dev full profile" "$(cargo tree --locked -p bm-llm-gateway --no-default-features --features profile-server-linux-dev-full)"

if rg -n 'profile-esp-|profile-desktop-macos-embedded-sdk|profile-desktop-windows-embedded-sdk|profile-linux-device-standalone-memory' crates/llm-gateway/Cargo.toml >/tmp/bm-llm-gateway-profile-hit.$$; then
  cat /tmp/bm-llm-gateway-profile-hit.$$ >&2
  rm -f /tmp/bm-llm-gateway-profile-hit.$$
  fail "bm-llm-gateway must not expose compact/device/desktop embedded profile features"
fi
rm -f /tmp/bm-llm-gateway-profile-hit.$$

compact_checks=(
  "bm-sdk profile-esp-standalone-memory"
  "bm-sdk profile-esp-embedded-sdk"
  "bm-entry profile-esp-standalone-memory"
  "bm-entry profile-esp-embedded-sdk"
  "bm-entry profile-desktop-macos-embedded-sdk"
  "bm-entry profile-desktop-windows-embedded-sdk"
  "bm-http profile-esp-standalone-memory"
  "bm-http profile-esp-embedded-sdk"
  "bm-http profile-desktop-macos-embedded-sdk"
  "bm-http profile-desktop-windows-embedded-sdk"
)

for row in "${compact_checks[@]}"; do
  package="${row%% *}"
  features="${row#* }"
  tree="$(cargo tree --locked -p "$package" --no-default-features --features "$features")"
  if grep -Eq 'bm-llm-gateway|bm-ollama-transparent|reqwest|axum|hyper|tower' <<<"$tree"; then
    echo "$tree" >&2
    fail "$package $features must not compile gateway/desktop/server-client dependencies"
  fi
done

echo "check_llm_gateway_profile_budget: ok"
