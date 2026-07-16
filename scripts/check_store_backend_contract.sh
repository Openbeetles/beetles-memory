#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo() {
  command cargo --locked "$@"
}
export -f cargo

assert_store_tree_excludes() {
  local feature_set="$1"
  local needle="$2"
  local tree
  tree="$(cargo tree --locked -p bm-sdk --no-default-features --features "$feature_set")"
  if grep -q "$needle" <<<"$tree"; then
    echo "bm-sdk persistence feature set unexpectedly includes $needle: $feature_set" >&2
    exit 1
  fi
}

assert_store_tree_includes() {
  local feature_set="$1"
  local needle="$2"
  local tree
  tree="$(cargo tree --locked -p bm-sdk --no-default-features --features "$feature_set")"
  if ! grep -q "$needle" <<<"$tree"; then
    echo "bm-sdk persistence feature set should include $needle: $feature_set" >&2
    exit 1
  fi
}

cargo fmt --all -- --check
cargo check --workspace --exclude bm-desktop
cargo test --locked -p bm-store-contract-tests --test file_store_contract file_store_maps_long_logical_keys_to_profile_bounded_physical_paths
cargo test --locked -p bm-store-contract-tests --test conversation_transcript_store_contract file_snapshot_export_import_preserves_long_transcript_keys_and_attrs
cargo test --locked -p bm-store-contract-tests --all-features --test governed_evidence_exact_read_contract
cargo test --locked -p bm-store-contract-tests
cargo test --locked -p bm-store-contract-tests --all-features
cargo test --locked -p bm-store-contract-tests --features sqlite-store
cargo test --locked -p bm-sdk
cargo check --locked -p bm-sdk --features profile-esp-standalone-memory
cargo check --locked -p bm-sdk --features profile-esp-embedded-sdk
cargo check --locked -p bm-sdk --features profile-server-linux-dev-full
cargo test --locked -p bm-sdk --features nonproduction-replay-harness,profile-server-linux-dev-full --test sdk_runtime_flow

assert_store_tree_excludes "embedded-store" "rusqlite"
assert_store_tree_includes "sqlite-store" "rusqlite"
bash scripts/check_profile_matrix.sh
