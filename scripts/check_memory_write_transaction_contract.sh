#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v rg >/dev/null 2>&1; then
  echo "check_memory_write_transaction_contract: ripgrep (rg) is required" >&2
  exit 1
fi

cargo test -p bm-store-contract-tests --test mutation_batch_contract
cargo test -p bm-store-contract-tests --test file_transaction_recovery_contract
cargo test -p bm-store-contract-tests --test file_primitive_concurrency_contract
cargo test -p bm-store-contract-tests --test store_concurrency_contract
cargo test -p bm-store-contract-tests --features sqlite-store --test sqlite_multiprocess_transaction_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test memory_write_transaction_contract

candidate_body="$(
  awk '
    /fn write_candidates_transactional/ { in_body = 1 }
    /fn memory_write_transaction_scope/ { if (in_body) { in_body = 0 } }
    in_body { print }
  ' crates/sdk/src/runtime.rs
)"

runtime_source="$(cat crates/sdk/src/runtime.rs)"

if ! grep -q "commit_governed_memory_transaction_with_preconditions" <<<"${candidate_body}"; then
  echo "check_memory_write_transaction_contract: Candidates path must commit through the governed Store transaction boundary" >&2
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
  "plan_long_term_facet_index_changes" \
  "plan_long_term_facet_index_upsert_mutations" \
  "plan_long_term_facet_index_mutations_for_store_mutations" \
  "ensure_transcript_lifecycle_has_facet_impact_or_fails_closed" \
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
  "build_memory_graph_persistence_plan" \
  "MemoryGraphScopeManifest" \
  "append_graph_owner_cascade_mutations" \
  "MemoryGraphRecallIndexReport" \
  "graph_index_report"; do
  if ! rg -n "${required}" crates/sdk/src/store_internal/platform.rs crates/sdk/src/runtime.rs crates/sdk/src/ops.rs crates/sdk/tests/eval_recall_contract.rs >/dev/null; then
    echo "check_memory_write_transaction_contract: missing W4 graph index transaction/report marker ${required}" >&2
    exit 1
  fi
done

for required in \
  "rejected_candidate_does_not_write_recallable_facet_index" \
  "long_term_extraction_delete_removes_facet_index_in_same_transaction" \
  "long_term_extraction_plans_delete_and_upsert_against_one_facet_manifest_state" \
  "long_term_control_correct_updates_facet_index_revision_in_same_transaction" \
  "long_term_control_delete_removes_facet_index_in_same_transaction" \
  "long_term_control_supersede_replaces_owner_facet_index_in_same_transaction" \
  "long_term_control_change_scope_updates_facet_and_reports_visibility_not_indexed" \
  "report_only_subject_visibility_not_indexed" \
  "transcript_mask_fails_closed_when_facet_source_ref_would_be_redacted" \
  "memory_space_migration_fails_closed_when_snapshot_contains_facet_index" \
  "facet_index_remap_required"; do
  if ! rg -n "${required}" crates/sdk/src/runtime.rs crates/sdk/src/lib.rs crates/sdk/tests/memory_write_transaction_contract.rs crates/sdk/tests/memory_space_migration_contract.rs >/dev/null; then
    echo "check_memory_write_transaction_contract: missing P2 facet transaction marker ${required}" >&2
    exit 1
  fi
done

for required in \
  "validate_long_term_owner_facet_closure" \
  "validate_graph_manifest_closure" \
  "validate_control_audit_closure" \
  "governed_transaction_rejects_owner_mutation_without_facet_closure" \
  "governed_transaction_rejects_control_mutation_without_audit_closure" \
  "governed_transaction_rejects_mismatched_control_audit_binding" \
  "load_governed_recall_snapshot" \
  "governed_recall_snapshot_is_immutable_and_rejects_writes"; do
  if ! rg -n "${required}" crates/sdk/src/store_internal/platform.rs crates/store-contract-tests/tests/mutation_batch_contract.rs crates/store-contract-tests/tests/governed_recall_snapshot_contract.rs >/dev/null; then
    echo "check_memory_write_transaction_contract: missing governed Store boundary marker ${required}" >&2
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

if rg -n "commit_mutation_batch(_with_preconditions)?" crates --glob '!**/tests/**'; then
  echo "check_memory_write_transaction_contract: legacy raw batch commit API must not exist in production code" >&2
  exit 1
fi

if rg -n "commit_governed_memory_transaction(_with_preconditions)?" crates \
  --glob '!**/tests/**' \
  --glob '!crates/sdk/src/store_internal/platform.rs' \
  --glob '!crates/sdk/src/runtime.rs'; then
  echo "check_memory_write_transaction_contract: only StorePlatform and SDK runtime may own governed transaction commits" >&2
  exit 1
fi

echo "check_memory_write_transaction_contract: ok"
