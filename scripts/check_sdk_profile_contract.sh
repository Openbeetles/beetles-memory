#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test public_surface
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test capability_catalog
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test runtime_contract
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test sdk_runtime_flow
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test replay_import_export

cargo test --locked -p bm-sdk --no-default-features
cargo test --locked -p bm-core --test profile_capability_catalog --no-default-features --features profile-esp-standalone-memory
cargo test --locked -p bm-core --test profile_capability_catalog --no-default-features --features profile-esp-embedded-sdk
bash scripts/check_profile_matrix.sh
