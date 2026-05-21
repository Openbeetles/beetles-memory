#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo test -p bm-core --test runtime_lifecycle_contract
cargo test -p bm-store --test runtime_lifecycle_event_contract
cargo test -p bm-store --features sqlite-store --test runtime_lifecycle_event_contract
cargo test -p bm-sdk --test capability_catalog
cargo test -p bm-sdk --test runtime_lifecycle_contract
cargo test -p bm-sdk --test sdk_runtime_flow
cargo test -p bm-sdk --test store_opening_contract
bash scripts/check_profile_matrix.sh
