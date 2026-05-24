#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo test -p bm-core --test write_candidate_governance_contract
cargo test -p bm-core --test turn_delta_contract
cargo test -p bm-core --test runtime_observation_contract

cargo test -p bm-store --test memory_space_event_scope_contract

cargo test -p bm-sdk --test public_surface
cargo test -p bm-sdk --test host_platform_injection_contract
cargo test -p bm-sdk --test write_candidate_contract
cargo test -p bm-sdk --test memory_space_migration_contract
cargo test -p bm-sdk --test post_turn_deferred_governance_contract
cargo test -p bm-sdk --test post_turn_runtime_contract
cargo test -p bm-sdk --test replay_import_export

bash scripts/check_sdk_profile_contract.sh
