#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo test -p bm-sdk --test public_surface
cargo test -p bm-sdk --test capability_catalog
cargo test -p bm-sdk --test runtime_contract
cargo test -p bm-sdk --test sdk_runtime_flow
cargo test -p bm-sdk --test replay_import_export

cargo test -p bm-sdk --no-default-features
cargo test -p bm-core --test profile_capability_catalog --no-default-features --features profile-esp-standalone-memory
cargo test -p bm-core --test profile_capability_catalog --no-default-features --features profile-esp-embedded-sdk
bash scripts/check_profile_matrix.sh
