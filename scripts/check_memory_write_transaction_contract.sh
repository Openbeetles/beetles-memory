#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v rg >/dev/null 2>&1; then
  echo "check_memory_write_transaction_contract: ripgrep (rg) is required" >&2
  exit 1
fi

cargo test -p bm-store --test mutation_batch_contract
cargo test -p bm-sdk --test memory_write_transaction_contract

candidate_body="$(
  awk '
    /fn write_candidates_transactional/ { in_body = 1 }
    /fn memory_write_transaction_scope/ { if (in_body) { in_body = 0 } }
    in_body { print }
  ' crates/sdk/src/runtime.rs
)"

runtime_source="$(cat crates/sdk/src/runtime.rs)"

if ! grep -q "commit_mutation_batch" <<<"${candidate_body}"; then
  echo "check_memory_write_transaction_contract: Candidates path must commit through StoreMutationBatch" >&2
  exit 1
fi

if ! grep -q "plan_governed_shared_memory_in_space" <<<"${candidate_body}"; then
  echo "check_memory_write_transaction_contract: Candidates path must use shared-memory plan builder" >&2
  exit 1
fi

if ! grep -q "plan_governed_runtime_skills" <<<"${candidate_body}"; then
  echo "check_memory_write_transaction_contract: Candidates path must use runtime-skill plan builder" >&2
  exit 1
fi

if grep -Eq "write_governed_shared_memory_in_space|write_governed_runtime_skills|record_candidate_derived_memory_refs|record_soul_handoff_derived_memory_refs|finish_lifecycle_success_with_payload" <<<"${candidate_body}"; then
  echo "check_memory_write_transaction_contract: Candidates path contains direct write or post-commit lifecycle helper" >&2
  exit 1
fi

for required in \
  "commit_memory_write_transaction" \
  "commit_memory_mutation_batch" \
  "plan_long_term_extraction_transaction" \
  "plan_agent_tool_experience_record" \
  "plan_long_term_control_mutation" \
  "plan_memory_governance_policy_mutation" \
  "run_long_term_memory_refresh_transactional" \
  "PlanningPrivateGardenStore" \
  "runtime_skill_storage_mutations_to_store_mutations"; do
  if ! grep -q "${required}" <<<"${runtime_source}"; then
    echo "check_memory_write_transaction_contract: missing transactional runtime helper ${required}" >&2
    exit 1
  fi
done

for required in \
  "memory_graph_indexes" \
  "build_memory_graph_recall_index_docs" \
  "MemoryGraphRecallIndexReport" \
  "graph_index_report"; do
  if ! rg -n "${required}" crates/store/src/platform.rs crates/sdk/src/runtime.rs crates/sdk/src/ops.rs crates/sdk/tests/eval_recall_contract.rs >/dev/null; then
    echo "check_memory_write_transaction_contract: missing W4 graph index transaction/report marker ${required}" >&2
    exit 1
  fi
done

for operation in \
  "write.procedural" \
  "write.procedural_promotions" \
  "write.long_term_extraction" \
  "write.agent_tool_usage_feedback" \
  "long_term_control.mutation" \
  "long_term_control.policy" \
  "post_turn.long_term_refresh" \
  "post_turn.private_garden" \
  "runtime_skill.edit" \
  "runtime_skill.set_enabled" \
  "runtime_skill.delete"; do
  if ! grep -q "${operation}" <<<"${runtime_source}"; then
    echo "check_memory_write_transaction_contract: missing transactional operation ${operation}" >&2
    exit 1
  fi
done

if rg -n "write_governed_runtime_skills|write_governed_shared_memory_in_space|apply_long_term_memory_extraction_with_report|write_agent_tool_experience_record|record_long_term_extraction_derived_memory_refs|record_private_garden_derived_memory_refs|append_candidate_derived_memory_ref|delete_skill_record|set_skill_enabled_record|set_skills_order" crates/sdk/src/runtime.rs; then
  echo "check_memory_write_transaction_contract: SDK runtime contains direct write helper bypassing memory transaction" >&2
  exit 1
fi

if rg -n "commit_mutation_batch" crates \
  --glob '!crates/store/src/platform.rs' \
  --glob '!crates/store/tests/mutation_batch_contract.rs' \
  --glob '!crates/sdk/src/runtime.rs'; then
  echo "check_memory_write_transaction_contract: only store and SDK runtime may call commit_mutation_batch" >&2
  exit 1
fi

echo "check_memory_write_transaction_contract: ok"
