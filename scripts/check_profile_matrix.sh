#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

assert_tree_excludes() {
  local feature_set="$1"
  local needle="$2"
  local tree
  tree="$(cargo tree --locked -p bm-core --no-default-features --features "$feature_set")"
  if grep -q "$needle" <<<"$tree"; then
    echo "profile feature set unexpectedly includes $needle: $feature_set" >&2
    exit 1
  fi
}

assert_tree_includes() {
  local feature_set="$1"
  local needle="$2"
  local tree
  tree="$(cargo tree --locked -p bm-core --no-default-features --features "$feature_set")"
  if ! grep -q "$needle" <<<"$tree"; then
    echo "profile feature set should include $needle: $feature_set" >&2
    exit 1
  fi
}

assert_store_tree_excludes() {
  local feature_set="$1"
  local needle="$2"
  local tree
  tree="$(cargo tree --locked -p bm-sdk --no-default-features --features "$feature_set")"
  if grep -q "$needle" <<<"$tree"; then
    echo "SDK persistence feature set unexpectedly includes $needle: $feature_set" >&2
    exit 1
  fi
}

assert_store_tree_includes() {
  local feature_set="$1"
  local needle="$2"
  local tree
  tree="$(cargo tree --locked -p bm-sdk --no-default-features --features "$feature_set")"
  if ! grep -q "$needle" <<<"$tree"; then
    echo "SDK persistence feature set should include $needle: $feature_set" >&2
    exit 1
  fi
}

assert_feature_set_rejected() {
  local feature_set="$1"
  local expected="$2"
  local output
  if output="$(cargo check --locked -p bm-core --no-default-features --features "$feature_set" 2>&1)"; then
    echo "profile feature set should have been rejected: $feature_set" >&2
    exit 1
  fi
  if ! grep -q "$expected" <<<"$output"; then
    echo "profile feature set rejected for unexpected reason: $feature_set" >&2
    echo "$output" >&2
    exit 1
  fi
}

cargo check --locked -p bm-core --no-default-features --features profile-esp-standalone-memory
cargo test --locked -p bm-core --test profile_capability_catalog --no-default-features --features profile-esp-standalone-memory
cargo test --locked -p bm-core --test dependency_feature_contract --no-default-features --features profile-esp-standalone-memory
assert_tree_excludes "profile-esp-standalone-memory" "rusqlite"
assert_feature_set_rejected "profile-esp-standalone-memory,sqlite-index" "target-esp builds must not enable sqlite-index"

cargo check --locked -p bm-core --no-default-features --features profile-esp-embedded-sdk
cargo test --locked -p bm-core --test profile_capability_catalog --no-default-features --features profile-esp-embedded-sdk
cargo test --locked -p bm-core --test dependency_feature_contract --no-default-features --features profile-esp-embedded-sdk
assert_tree_excludes "profile-esp-embedded-sdk" "rusqlite"
assert_feature_set_rejected "profile-esp-embedded-sdk,sqlite-index" "target-esp builds must not enable sqlite-index"
assert_feature_set_rejected "target-esp,target-server-linux,role-embedded-sdk" "requires at most one target"
assert_feature_set_rejected "target-server-linux,role-embedded-sdk,role-memory-gateway" "requires at most one role"

cargo check --locked -p bm-core --no-default-features --features target-server-linux,role-memory-gateway,sqlite-index
cargo test --locked -p bm-core --test profile_capability_catalog --no-default-features --features target-server-linux,role-memory-gateway,sqlite-index
cargo test --locked -p bm-core --test dependency_feature_contract --no-default-features --features target-server-linux,role-memory-gateway,sqlite-index
assert_tree_includes "target-server-linux,role-memory-gateway,sqlite-index" "rusqlite"
assert_store_tree_excludes "embedded-store" "rusqlite"
assert_store_tree_includes "sqlite-store" "rusqlite"
