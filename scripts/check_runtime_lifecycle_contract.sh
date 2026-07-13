#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo test -p bm-core --test runtime_lifecycle_contract
cargo test -p bm-store-contract-tests --test runtime_lifecycle_event_contract
cargo test -p bm-store-contract-tests --features sqlite-store --test runtime_lifecycle_event_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test capability_catalog
cargo test -p bm-sdk --features nonproduction-replay-harness --test runtime_lifecycle_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test sdk_runtime_flow
cargo test -p bm-sdk --features nonproduction-replay-harness --test store_opening_contract
bash scripts/check_profile_matrix.sh
