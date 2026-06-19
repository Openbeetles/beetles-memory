#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

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

if rg -n 'JSON_BODY_MAX_BYTES' crates/http crates/wss crates/llm-gateway crates/sdk crates/store crates/core; then
  fail "HTTP must consume RuntimeBudgetReport.adapter_budget instead of JSON_BODY_MAX_BYTES"
fi

if rg -n 'WssBudget::(esp_standalone|server_gateway)' crates/wss crates/http crates/llm-gateway; then
  fail "WSS profile budgets must come from RuntimeBudgetReport"
fi

if rg -n 'GatewayProjectionConfig\s*\{[^}]*system_max_len|projection\.system_max_len' crates/llm-gateway; then
  fail "GatewayProjectionConfig must not own system_max_len"
fi

if rg -n 'GatewayMaintenanceConfig\s*\{[^}]*_(max|bytes|chars)|maintenance\.(user|max|reply).*_(chars|bytes|max)' crates/llm-gateway; then
  fail "GatewayMaintenanceConfig must not own maintenance accumulator budgets"
fi

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

if rg -n 'profile.*transcript_page|transcript_page_size|host_refs_per_turn|max_attrs_per_turn|max_attrs_per_message|derived_refs_per_report|repair_issues_per_report' crates/store/src; then
  fail "StorePlatform must not own transcript governance profile budgets"
fi

if ! rg -n 'eight_profiles_have_distinct_budget_reports' crates/core/src/budget.rs >/dev/null; then
  fail "runtime budget tests must cover profile-specific compiled reports"
fi

if ! rg -n 'transcript_governance_budget_is_profile_owned_and_runtime_enforced' crates/sdk/tests/runtime_budget_contract.rs >/dev/null; then
  fail "runtime budget tests must cover transcript governance budget ownership"
fi

if ! rg -n 'graph_expansion_budget_is_profile_owned_and_not_provider_render_owned' crates/sdk/tests/runtime_budget_contract.rs >/dev/null; then
  fail "runtime budget tests must cover W4 graph expansion budget ownership"
fi

echo "check_runtime_budget_contracts: ok"
