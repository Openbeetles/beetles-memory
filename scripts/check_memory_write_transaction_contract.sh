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

if ! command -v rg >/dev/null 2>&1; then
  echo "check_memory_write_transaction_contract: ripgrep (rg) is required" >&2
  exit 1
fi

cargo test -p bm-store-contract-tests --features sqlite-store --test mutation_batch_contract
cargo test -p bm-store-contract-tests --features sqlite-store --test manifest_admission_contract
cargo test -p bm-store-contract-tests --features sqlite-store --test sqlite_store_contract
cargo test -p bm-store-contract-tests --test file_transaction_recovery_contract
cargo test -p bm-store-contract-tests --test file_primitive_concurrency_contract
cargo test -p bm-store-contract-tests --test store_concurrency_contract
cargo test -p bm-store-contract-tests --features sqlite-store --test sqlite_multiprocess_transaction_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test memory_write_transaction_contract
cargo test -p bm-core --test governed_evidence_document_contract
cargo test -p bm-core --test evidence_document_budget_contract
cargo test -p bm-core --test memory_graph_v2_contract
cargo test -p bm-core --test post_image_closure_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test governed_evidence_document_runtime_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test memory_graph_v2_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test archive_restore_contract

candidate_body="$(
  awk '
    /fn write_candidates_transactional/ { in_body = 1 }
    /fn memory_write_transaction_scope/ { if (in_body) { in_body = 0 } }
    in_body { print }
  ' crates/sdk/src/runtime.rs
)"

runtime_source="$(cat crates/sdk/src/runtime.rs)"

if ! grep -q "commit_governed_memory_transaction_with_runtime_budget" <<<"${candidate_body}"; then
  echo "check_memory_write_transaction_contract: Candidates path must commit through the governed Store transaction boundary" >&2
  exit 1
fi

if ! grep -q "plan_governed_shared_memory_in_space" <<<"${candidate_body}"; then
  echo "check_memory_write_transaction_contract: Candidates path must use shared-memory plan builder" >&2
  exit 1
fi

if ! grep -q "plan_runtime_skill_owner_upserts" <<<"${candidate_body}"; then
  echo "check_memory_write_transaction_contract: Candidates path must use the typed runtime-skill owner plan builder" >&2
  exit 1
fi

if grep -q "plan_governed_runtime_skills" <<<"${candidate_body}"; then
  echo "check_memory_write_transaction_contract: Candidates path must not regress to the legacy runtime-skill plan builder" >&2
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
  "plan_governed_owner_facet_index_changes" \
  "plan_long_term_facet_index_upsert_mutations" \
  "plan_long_term_facet_index_mutations_for_store_mutations" \
  "ensure_transcript_lifecycle_has_facet_impact_or_fails_closed" \
  "plan_agent_tool_experience_record" \
  "plan_long_term_control_mutation" \
  "plan_memory_governance_policy_mutation" \
  "plan_long_term_memory_refresh_transactional" \
  "PlanningPrivateGardenStore" \
  "runtime_skill_storage_mutations_to_store_mutations"; do
  if ! grep -q "${required}" <<<"${runtime_source}"; then
    echo "check_memory_write_transaction_contract: missing transactional runtime helper ${required}" >&2
    exit 1
  fi
done

for required in \
  "write_governed_evidence_documents_transactional" \
  "GovernedEvidenceSourceRef" \
  "validate_governed_evidence_source_ref" \
  "TemporalMemoryGraphNodeOwnerRef" \
  "governed_transaction_rejects_evidence_owner_without_typed_source_ref_atomically" \
  "governed_transaction_rejects_unknown_evidence_source_claim_fields_atomically" \
  "snapshot_import_rejects_unknown_evidence_source_ref_fields" \
  "governed_transaction_rejects_evidence_owner_creation_without_graph_closure" \
  "governed_transaction_rejects_unbound_evidence_owner_in_existing_graph" \
  "evidence_source_claim_json_excludes_raw_locator_and_uses_digest_metadata" \
  "snapshot_import_rejects_legacy_evidence_source_ref_shape" \
  "backend_cas_rejects_second_document_claiming_existing_source_identity" \
  "batch_duplicate_source_identity_for_different_documents_fails_closed" \
  "cross_transaction_duplicate_source_identity_fails_closed_with_zero_delta" \
  "concurrent_writers_to_same_source_claim_allow_only_one_commit" \
  "independent_file_store_platforms_allow_one_complete_evidence_source_claim_closure" \
  "independent_sqlite_store_platforms_allow_one_complete_evidence_source_claim_closure" \
  "file_store_rejects_missing_manifest_when_any_persistent_state_exists_without_mutation" \
  "sqlite_store_rejects_missing_schema_when_any_persistent_state_exists_without_mutation" \
  "source_revision_update_atomically_replaces_the_claim" \
  "delete_atomically_removes_evidence_owner_facet_graph_membership_and_writes_lifecycle" \
  "soul_private_evidence_document_write_fails_closed_with_zero_delta" \
  "mixed_batch_with_soul_private_evidence_fails_closed_with_zero_delta" \
  "newer_revision_cannot_remount_an_existing_evidence_owner" \
  "snapshot_round_trip_preserves_same_document_id_in_distinct_memory_spaces" \
  "concurrent_mixed_owner_updates_never_produce_a_torn_recall_snapshot" \
  "recall_snapshot_keeps_long_term_and_evidence_typed_owner_bindings_consistent" \
  "graph_post_image_rejects_evidence_owner_without_node_membership" \
  "public_archive_debug_does_not_expose_raw_snapshot_payloads" \
  "graph_node_identity_is_independent_from_its_typed_governed_owner" \
  "owner_projection_preserves_edge_only_evidence_between_same_owner_anchors"; do
  if ! rg -n "${required}" \
    crates/sdk/src/runtime.rs \
    crates/sdk/src/ops.rs \
    crates/sdk/src/store_internal/platform.rs \
    crates/sdk/tests/governed_evidence_document_runtime_contract.rs \
    crates/sdk/tests/memory_graph_v2_contract.rs \
    crates/sdk/tests/archive_restore_contract.rs \
    crates/core/tests/post_image_closure_contract.rs \
    crates/store-contract-tests/tests/manifest_admission_contract.rs \
    crates/store-contract-tests/tests/mutation_batch_contract.rs >/dev/null; then
    echo "check_memory_write_transaction_contract: missing governed evidence transaction marker ${required}" >&2
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
  "long_term_control_change_scope_persists_visibility_with_owner_and_facet_revision" \
  "transcript_mask_fails_closed_when_facet_source_ref_would_be_redacted" \
  "same_scope_restore_preserves_v6_long_term_facet_and_control_closure" \
  "same_scope_restore_replaces_only_that_scope"; do
  if ! rg -n "${required}" crates/sdk/src/runtime.rs crates/sdk/src/lib.rs crates/sdk/tests/memory_write_transaction_contract.rs crates/sdk/tests/archive_restore_contract.rs >/dev/null; then
    echo "check_memory_write_transaction_contract: missing P2 facet transaction marker ${required}" >&2
    exit 1
  fi
done

for required in \
  "validate_governed_owner_facet_closure" \
  "validate_graph_manifest_closure" \
  "validate_control_audit_closure" \
  "governed_transaction_rejects_owner_mutation_without_facet_closure" \
  "governed_transaction_rejects_control_mutation_without_audit_closure" \
  "governed_transaction_rejects_mismatched_control_audit_binding" \
  "with_recall_immutable_read_session" \
  "production_recall_has_no_whole_store_snapshot_or_second_platform_path"; do
  if ! rg -n "${required}" crates/sdk/src/store_internal/platform.rs crates/store-contract-tests/tests/mutation_batch_contract.rs crates/store-contract-tests/tests/governed_recall_immutable_session_contract.rs >/dev/null; then
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
  "runtime_skill.retire"; do
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

if rg -n "commit_governed_(memory|graph_repair)_transaction" crates \
  --glob '!**/tests/**' \
  --glob '!crates/sdk/src/store_internal/platform.rs' \
  --glob '!crates/sdk/src/store_internal/post_turn_governance.rs' \
  --glob '!crates/sdk/src/runtime.rs'; then
  echo "check_memory_write_transaction_contract: only StorePlatform and SDK transactional kernels/runtime may own governed transaction commits" >&2
  exit 1
fi

echo "check_memory_write_transaction_contract: ok"
