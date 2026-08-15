#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo() {
  command cargo --locked "$@"
}

if ! command -v rg >/dev/null 2>&1; then
  echo "check_runtime_budget_contracts: ripgrep (rg) is required" >&2
  exit 1
fi

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

if [[ ! -f crates/core/src/resource.rs || ! -f crates/core/src/budget.rs ]]; then
  fail "runtime resource and budget owners must live in bm-core"
fi

if rg -n '^[[:space:]]*\.profile[[:space:]]*\(' crates examples --glob '*.rs'; then
  fail "MemoryRuntimeBuilder must inherit profile authority from MemoryStoreHandle"
fi

if rg -n 'RuntimeBudgetReport::static_for_profile' \
  crates/entry crates/llm-gateway crates/wss crates/adapter examples; then
  fail "runtime consumers must use the opened store/runtime report instead of recompiling it"
fi

if rg -n '\bEntryStoreConfig\b' crates/entry crates/llm-gateway crates/wss examples; then
  fail "Entry must consume validated StoreBackendConfig values instead of owning a mutable store DTO"
fi

if rg -n '\.store\.(backend|data_path|fsync)[[:space:]]*=|StoreBackendConfig[[:space:]]*\{' \
  crates/entry crates/llm-gateway crates/wss examples --glob '*.rs'; then
  fail "Entry/Gateway/WSS/examples must construct StoreBackendConfig through validated APIs"
fi

if rg -n 'nonproduction_benchmark_store_capacity|with_nonproduction_benchmark_store_capacity' \
  crates/sdk/src/store_internal/config.rs; then
  fail "StoreBackendConfig must not carry benchmark capacity; use feature-gated store preparation requirements"
fi

if rg -n 'pub (use [^;]*|fn [^(]*\()RuntimeResourceProbeRegistration|pub fn open_with_resource_probe_registration' \
  crates/sdk/src; then
  fail "SDK public surface must not expose arbitrary runtime resource probe registration"
fi

for symbol in \
  compile_runtime_budget \
  RuntimeBudgetAuthority \
  RuntimeBudgetInput \
  StaticPlatformManifest \
  probe_host_runtime_resource \
  HostRuntimeResourceProbe \
  RuntimeResourceProbeRegistration \
  StaticRuntimeResourceProbe \
  UnavailableRuntimeResourceProbe; do
  if rg -n "\\b${symbol}\\b" crates/sdk/src/lib.rs; then
    fail "SDK facade must not re-export runtime authority construction material: $symbol"
  fi
done

if rg -n 'SimulatedProfileResourceProbe|simulated_profile_probe|open_simulated_(store_platform|memory_store)' \
  crates/sdk crates/store-contract-tests crates/replay; then
  fail "Store/SDK/runtime authority tests must use attested probes, not simulated resource snapshots"
fi

if rg -n 'ServerLinuxDevFull' \
  crates/llm-gateway/src/config.rs crates/llm-gateway/src/bin examples; then
  fail "production gateway and examples must not default to ServerLinuxDevFull"
fi

if rg -n 'ProfileId::(ServerLinux|DesktopMacos|DesktopWindows)DevFull' \
  crates/entry/tests crates/llm-gateway/tests crates/wss/tests crates/adapter/tests examples; then
  fail "consumer tests must use host-native dev-full only inside the nonproduction harness"
fi

for profile in \
  profile-server-linux-dev-full \
  profile-desktop-macos-dev-full \
  profile-desktop-windows-dev-full; do
  for manifest in \
    crates/adapter/Cargo.toml \
    crates/entry/Cargo.toml \
    crates/wss/Cargo.toml \
    crates/llm-gateway/Cargo.toml; do
    if ! rg -n "^${profile}[[:space:]]*=" "$manifest" >/dev/null; then
      fail "$manifest must expose the host-native dev-full profile closure: $profile"
    fi
  done
done

if rg -n 'bm-sdk/profile-desktop-(macos|windows)-dev-full' \
  crates/entry/Cargo.toml crates/wss/Cargo.toml; then
  fail "Entry and WSS desktop dev-full profiles must forward through bm-adapter"
fi

assert_default_tree_is_production() {
  local package="$1"
  local tree
  tree="$(cargo tree -p "$package" --no-default-features --edges normal,build,features)"
  if grep -Eq 'nonproduction-replay-harness|bm-replay' <<<"$tree"; then
    echo "$tree" >&2
    fail "$package default dependency graph must remain production-only"
  fi
}

assert_dev_full_forwards_sdk_harness() {
  local package="$1"
  local feature="$2"
  local tree
  tree="$(cargo tree -p "$package" --no-default-features --features "$feature" \
    --edges features -i bm-sdk)"
  if ! grep -F 'bm-sdk feature "nonproduction-replay-harness"' <<<"$tree" >/dev/null; then
    echo "$tree" >&2
    fail "$package/$feature must forward bm-sdk/nonproduction-replay-harness"
  fi
}

assert_profile_forwards_through_adapter() {
  local package="$1"
  local feature="$2"
  local tree
  tree="$(cargo tree -p "$package" --no-default-features --features "$feature" \
    --edges features -i bm-adapter)"
  if ! grep -F "bm-adapter feature \"$feature\"" <<<"$tree" >/dev/null; then
    echo "$tree" >&2
    fail "$package/$feature must select the matching bm-adapter profile feature"
  fi
}

assert_gateway_profile_forwards_through_entry() {
  local feature="$1"
  local tree
  tree="$(cargo tree -p bm-llm-gateway --no-default-features --features "$feature" \
    --edges features -i bm-entry)"
  if ! grep -F "bm-entry feature \"$feature\"" <<<"$tree" >/dev/null; then
    echo "$tree" >&2
    fail "bm-llm-gateway/$feature must select the matching bm-entry profile feature"
  fi
}

for package in bm-entry bm-llm-gateway bm-wss bm-adapter; do
  assert_default_tree_is_production "$package"
done

for feature in \
  profile-server-linux-dev-full \
  profile-desktop-macos-dev-full \
  profile-desktop-windows-dev-full; do
  for package in bm-adapter bm-entry bm-wss bm-llm-gateway; do
    assert_dev_full_forwards_sdk_harness "$package" "$feature"
  done
  for package in bm-entry bm-wss; do
    assert_profile_forwards_through_adapter "$package" "$feature"
  done
  assert_gateway_profile_forwards_through_entry "$feature"
done

assert_gateway_profile_forwards_through_entry profile-desktop-macos-standalone-memory

if rg -n 'JSON_BODY_MAX_BYTES' crates/http crates/wss crates/llm-gateway crates/sdk crates/core; then
  fail "HTTP must consume RuntimeBudgetReport.adapter_budget instead of JSON_BODY_MAX_BYTES"
fi

if rg -n 'WssBudget::(esp_standalone|server_gateway)' crates/wss crates/http crates/llm-gateway; then
  fail "WSS profile budgets must come from RuntimeBudgetReport"
fi

if rg -n 'GatewayProjectionConfig\s*\{[^}]*system_max_len|projection\.system_max_len' crates/llm-gateway; then
  fail "GatewayProjectionConfig must not own system_max_len"
fi

if rg -n 'GatewayMaintenanceConfig|maintenance\.(user|max|reply).*_(chars|bytes|max)' crates/llm-gateway; then
  fail "GatewayMaintenanceConfig must not be reintroduced; automatic governance is Entry-owned"
fi

if rg -n 'acquire_budget_lease|execute_with_budget_lease' \
  crates/llm-gateway/src/openai.rs \
  crates/llm-gateway/src/ollama.rs \
  crates/llm-gateway/src/server.rs; then
  fail "LLM gateway handlers and servers must enter through the single GatewayRuntime request lease owner"
fi

for file in \
  crates/llm-gateway/src/openai.rs \
  crates/llm-gateway/src/ollama.rs \
  crates/llm-gateway/src/server.rs; do
  if ! rg -n 'execute_request|_in_budget_lease' "$file" >/dev/null; then
    fail "LLM gateway request path must borrow the canonical active request lease: $file"
  fi
done

if rg -n 'Projection Budget|投影预算' apps/console/src crates/entry/src; then
  fail "UI/API must not call last_projection_chars a projection budget"
fi

if ! rg -n 'projection_source_budget' crates/sdk/src/runtime.rs >/dev/null; then
  fail "SDK projection source assembly must consume projection_source_budget"
fi

if ! rg -n 'projection_render_budget|projection_render_chars_for_request' crates/sdk/src/runtime.rs crates/llm-gateway/src >/dev/null; then
  fail "projection render must consume projection_render_budget"
fi

for file in crates/core/src/budget.rs crates/sdk/src/lib.rs; do
  if ! rg -n 'GraphExpansionRuntimeBudget' "$file" >/dev/null; then
    fail "graph expansion budget must be owned by RuntimeBudgetReport and exported by SDK"
  fi
  if ! rg -n 'TranscriptGovernanceBudget' "$file" >/dev/null; then
    fail "transcript governance ceilings must be owned by RuntimeBudgetReport and exported by SDK"
  fi
done

if ! rg -n 'graph_expansion_budget' crates/core/src/budget.rs crates/sdk/src/runtime.rs >/dev/null; then
  fail "W4 graph expansion must consume RuntimeBudgetReport.graph_expansion_budget"
fi

for file in crates/core/src/budget.rs crates/sdk/src/runtime.rs; do
  if ! rg -n 'transcript_governance_budget' "$file" >/dev/null; then
    fail "SDK transcript replay/lifecycle/repair must consume transcript_governance_budget"
  fi
done

if rg -n 'profile.*transcript_page|transcript_page_size|host_refs_per_turn|max_attrs_per_turn|max_attrs_per_message|derived_refs_per_report|repair_issues_per_report' crates/sdk/src/store_internal; then
  fail "StorePlatform must not own transcript governance profile budgets"
fi

if ! rg -n 'profiles_have_distinct_budget_reports' crates/core/src/budget.rs >/dev/null; then
  fail "runtime budget tests must cover profile-specific compiled reports"
fi

if ! rg -n 'transcript_governance_budget_is_profile_owned_and_runtime_enforced' crates/sdk/tests/runtime_budget_contract.rs >/dev/null; then
  fail "runtime budget tests must cover transcript governance budget ownership"
fi

if ! rg -n 'provider_limit_only_caps_render_budget' crates/core/src/budget.rs >/dev/null; then
  fail "runtime budget tests must cover W4 graph expansion budget ownership"
fi

if ! rg -n 'evidence_document_exact_read_closes_one_admitted_transaction' crates/core/src/budget.rs >/dev/null; then
  fail "runtime budget tests must keep evidence transaction admission and exact snapshot read aligned"
fi

if rg -n 'pub fn runtime_budget\s*\(mut self|with_runtime_store_budget' crates/sdk/src; then
  fail "production SDK must not accept caller-owned RuntimeBudgetReport or raw store budget overrides"
fi

if rg -n 'with_runtime_budget_input|nonproduction_runtime_budget_override' \
  crates/sdk/src crates/entry/src; then
  fail "production raw budget input and whole-report nonproduction injection must not exist"
fi

if rg -n '^[[:space:]]*pub[[:space:]]+(backend|profile|memory_system_kind|event_scope|data_path|path_budget|repair_policy|fsync|lock_timeout|schema_id|capacity):' \
  crates/sdk/src/store_internal/config.rs; then
  fail "StoreBackendConfig authority fields must not be caller-mutable"
fi

if ! rg -n 'open_runtime_budget_authority' \
  crates/sdk/src/store_internal/config.rs crates/sdk/src/store_internal/platform.rs >/dev/null \
  || ! rg -n 'prepare_for_nonproduction_harness|NonproductionStorePreparation' \
    crates/sdk/src/store.rs crates/sdk/src/store_internal/platform.rs >/dev/null; then
  fail "Store preparation must compile one authority/report before opening production or benchmark capacity"
fi

for owner in \
  crates/core/src/budget.rs \
  crates/sdk/src/store_internal/config.rs \
  crates/sdk/src/runtime.rs; do
  if ! rg -n '#\[cfg\(feature = "nonproduction-replay-harness"\)\]' "$owner" >/dev/null; then
    fail "nonproduction budget controls must be feature-gated in $owner"
  fi
done

if ! rg -n 'NonproductionRuntimeBudgetLimits|compile_nonproduction_runtime_budget' \
  crates/core/src/budget.rs crates/sdk/src/runtime.rs >/dev/null \
  || ! rg -n 'try_with_nonproduction_store_budget_limit' \
    crates/sdk/src/store_internal/config.rs >/dev/null \
  || ! rg -n 'open_with_benchmark_store_capacity' \
    crates/sdk/src/store.rs crates/sdk/src/store_internal/platform.rs >/dev/null; then
  fail "nonproduction controls must use typed semantic limits and two-stage store preparation"
fi

cargo test -p bm-core --features nonproduction-replay-harness budget::tests
cargo test -p bm-sdk --features nonproduction-replay-harness --lib runtime_budget_admission_tests
cargo test -p bm-sdk --features nonproduction-replay-harness --test runtime_budget_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test memory_write_transaction_contract
cargo test -p bm-store-contract-tests --features embedded-store,sqlite-store \
  --test runtime_store_budget_contract

echo "check_runtime_budget_contracts: ok"
