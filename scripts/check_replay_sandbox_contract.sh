#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

assert_tree_excludes() {
  local package="$1"
  local feature_set="$2"
  local needle="$3"
  local tree
  if [[ -n "$feature_set" ]]; then
    tree="$(cargo tree -p "$package" --no-default-features --features "$feature_set")"
  else
    tree="$(cargo tree -p "$package" --no-default-features)"
  fi
  if grep -q "$needle" <<<"$tree"; then
    echo "$package feature set unexpectedly includes $needle: ${feature_set:-<none>}" >&2
    exit 1
  fi
}

assert_tree_includes() {
  local package="$1"
  local feature_set="$2"
  local needle="$3"
  local tree
  if [[ -n "$feature_set" ]]; then
    tree="$(cargo tree -p "$package" --no-default-features --features "$feature_set")"
  else
    tree="$(cargo tree -p "$package" --no-default-features)"
  fi
  if ! grep -q "$needle" <<<"$tree"; then
    echo "$package feature set should include $needle: ${feature_set:-<none>}" >&2
    exit 1
  fi
}

cargo fmt --all -- --check
bash scripts/check_profile_matrix.sh

cargo test -p bm-core --test dependency_feature_contract --no-default-features --features profile-server-linux-dev-full,nonproduction-replay-harness
cargo test -p bm-sdk --no-default-features --features nonproduction-replay-harness --test capability_catalog

cargo test -p bm-replay --all-features
cargo test -p bm-evolve --all-features

assert_tree_excludes bm-replay "" "rusqlite"
assert_tree_includes bm-replay "sqlite-store" "rusqlite"
