#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo() {
  local subcommand="$1"
  shift
  if [[ "$subcommand" == "fmt" ]]; then
    command cargo fmt "$@"
  else
    local has_locked=0
    local has_no_default_features=0
    local arg
    for arg in "$@"; do
      [[ "$arg" == "--locked" ]] && has_locked=1
      [[ "$arg" == "--no-default-features" ]] && has_no_default_features=1
    done
    if [[ "$has_locked" -eq 1 && "$has_no_default_features" -eq 1 ]]; then
      command cargo "$subcommand" "$@"
    elif [[ "$has_locked" -eq 1 ]]; then
      command cargo "$subcommand" --no-default-features "$@"
    elif [[ "$has_no_default_features" -eq 1 ]]; then
      command cargo "$subcommand" --locked "$@"
    else
      command cargo "$subcommand" --locked --no-default-features "$@"
    fi
  fi
}
export -f cargo

cargo test -p bm-core --test write_candidate_governance_contract
cargo test -p bm-core --test turn_delta_contract
cargo test -p bm-core --test runtime_observation_contract

cargo test -p bm-store-contract-tests --test memory_space_event_scope_contract

cargo test -p bm-sdk --features nonproduction-replay-harness --test public_surface
cargo test -p bm-sdk --features nonproduction-replay-harness --test host_platform_injection_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test write_candidate_contract
cargo test -p bm-core --test subject_registry_contract
cargo test -p bm-core --test soul_non_regression_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test single_agent_default_registry_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test shared_fact_governance_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test projection_no_soul_mutation_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test archive_restore_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test beetle_derived_archive_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test post_turn_deferred_governance_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test post_turn_runtime_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test projection_audit_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test retention_compaction_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test replay_import_export

cargo test -p bm-entry --test bearer_principal_contract
cargo test -p bm-http --features server-std --test http_auth_contract
cargo test -p bm-http --features server-std --test http_scope_contract
cargo test -p bm-http --features server-std --test operator_metrics_contract
cargo test -p bm-llm-gateway --test body_budget_contract
cargo test -p bm-llm-gateway --test server_auth_contract
cargo test -p bm-mcp --features server-stdio --test mcp_scope_contract
cargo test -p bm-wss --features server-std --test wss_scope_contract

cargo test -p bm-replay --test sdk_host_beetle_derived_archive_replay_contract

bash scripts/check_sdk_profile_contract.sh

! rg -n "RoleKey|RoleMemoryLane" crates/core/src crates/sdk/src
! rg -n "boss_user|ceo_agent|finance_director_agent|warehouse_manager_agent|CEO|BOSS|财务总监|仓库管理员" crates/core/src crates/sdk/src
test -f fixtures/sdk-host-readiness/generic-rust-host/host-turn-lifecycle.json
test -f fixtures/sdk-host-readiness/beetle-derived/host-turn-lifecycle.json
rg -n "Host Turn Lifecycle|Archive Import And Replay|Host Forbidden Zones" docs/en/integration.md
rg -n "宿主回合生命周期|Archive Import 与 Replay|宿主禁区" docs/zh-CN/integration.md
rg -n "beetle-derived|generic-rust-host" fixtures/sdk-host-readiness/README.md
