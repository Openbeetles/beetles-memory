#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

assert_store_tree_excludes() {
  local feature_set="$1"
  local needle="$2"
  local tree
  tree="$(cargo tree -p bm-store --no-default-features --features "$feature_set")"
  if grep -q "$needle" <<<"$tree"; then
    echo "bm-store feature set unexpectedly includes $needle: $feature_set" >&2
    exit 1
  fi
}

assert_store_tree_includes() {
  local feature_set="$1"
  local needle="$2"
  local tree
  tree="$(cargo tree -p bm-store --no-default-features --features "$feature_set")"
  if ! grep -q "$needle" <<<"$tree"; then
    echo "bm-store feature set should include $needle: $feature_set" >&2
    exit 1
  fi
}

cargo fmt --all -- --check
cargo check --workspace
cargo test -p bm-store
cargo test -p bm-store --all-features
cargo test -p bm-store --features sqlite-store
cargo test -p bm-sdk
cargo check -p bm-sdk --features profile-esp-standalone-memory
cargo check -p bm-sdk --features profile-esp-embedded-sdk
cargo check -p bm-sdk --features profile-server-linux-dev-full
cargo test -p bm-sdk --test sdk_runtime_flow --features profile-server-linux-dev-full
assert_store_tree_excludes "embedded-store" "rusqlite"
assert_store_tree_includes "sqlite-store" "rusqlite"
bash scripts/check_profile_matrix.sh
