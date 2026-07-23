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

require_in_file() {
  local pattern="$1"
  local file="$2"
  if ! rg -Fq -- "$pattern" "$file"; then
    printf 'missing required contract %q in %q\n' "$pattern" "$file" >&2
    return 1
  fi
}

require_in_all() {
  local pattern="$1"
  shift
  local file
  for file in "$@"; do
    require_in_file "$pattern" "$file"
  done
}

bash scripts/check_memory_benchmark_wall.sh
bash scripts/check_inhabited_projection_phase5_cleanup.sh
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_sdk_host_integration_readiness.sh
bash scripts/check_agent_skill_directory_contract.sh
bash scripts/check_memory_write_transaction_contract.sh
bash -n scripts/check_w4_external_noisy_wall_preflight.sh
bash -n scripts/check_w4_external_noisy_wall_operator.sh

cargo test -p bm-sdk --features nonproduction-replay-harness --test projection_audit_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test sdk_runtime_flow runtime_write_recall_project_uses_sdk_entry_only
cargo test -p bm-sdk --features nonproduction-replay-harness --test write_candidate_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test memory_space_migration_contract
cargo test -p bm-sdk --features nonproduction-replay-harness,sqlite-store \
  --test memory_space_migration_contract \
  same_scope_facet_closure_migrates_across_all_store_backends
cargo test -p bm-sdk --features nonproduction-replay-harness --test public_surface
cargo test -p bm-sdk --features nonproduction-replay-harness --test runtime_budget_contract graph_expansion_budget_is_profile_owned_and_not_provider_render_owned
cargo test -p bm-sdk --features nonproduction-replay-harness --test runtime_budget_contract facet_recall_budget_is_profile_owned_and_not_graph_or_render_owned
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract persistent_graph_recall_uses_sdk_owned_production_index_report
cargo test -p bm-core --test memory_facet_contract
cargo test -p bm-store-contract-tests --test mutation_batch_contract memory_facet_index_namespace_requires_read_set_precondition_without_store_semantics
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract eval_recall_reports_facet_stage_for_expanded_miss
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract facet_rank_fusion_preserves_pool_provenance
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract facet_coverage_selection_prioritizes_distinct_canonical_evidence_groups
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract facet_graph_propagation_uses_indexed_graph_anchor_without_full_scan
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract facet_recall_expands_graph_anchor_pool_without_render_growth
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract facet_recall_respects_privacy_scope_and_profile_budget
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract facet_recall_blocks_cross_subject_expanded_metadata_leakage
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract governed_read_filters_private_records_before_source_recall_limit
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract production_delivery_rejects_private_owner_records_before_capsule_render
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract persistent_graph_storage_is_isolated_by_memory_space_and_subject
cargo test -p bm-sdk --features nonproduction-replay-harness --test eval_recall_contract projection_keeps_unicode_capsules_consistent_with_the_character_budget
cargo test -p bm-sdk --features nonproduction-replay-harness --test long_term_memory_control_contract long_term_control_list_and_detail_use_the_governed_runtime_view
cargo test -p bm-sdk --features nonproduction-replay-harness --test memory_write_transaction_contract long_term_extraction_plans_delete_and_upsert_against_one_facet_manifest_state
cargo test -p bm-replay detail_recomputation_independently_validates_sdk_projection_delivery_manifest
cargo test -p bm-replay runner_disk_identity_tracks_exact_build_inputs_lock_and_executable
cargo test -p bm-adapter --test contract project_command_returns_only_the_adapter_projection_contract
cargo test -p bm-adapter --test contract json_adapter_preserves_structured_query_facets_for_recall_and_projection
cargo test -p bm-sdk --features nonproduction-replay-harness --test long_term_memory_control_contract long_term_control_mutation_reports_affected_facet_docs_for_operator_review
cargo test -p bm-core --test subject_registry_contract
cargo test -p bm-core --test soul_non_regression_contract
cargo test -p bm-core --test next_gen_contract temporal_memory_graph_rejects_raw_soul_private_material
cargo test -p bm-sdk --features nonproduction-replay-harness --test single_agent_default_registry_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test shared_fact_governance_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test projection_no_soul_mutation_contract
cargo test -p bm-sdk --features nonproduction-replay-harness --test memory_write_transaction_contract explicit_privacy_transition_updates_owner_facet_and_postings_atomically
cargo test -p bm-sdk --features nonproduction-replay-harness --test replay_import_export continuity_import_preserves_soul_private_without_public_delivery_or_graph_membership
cargo test -p bm-sdk --features nonproduction-replay-harness --test runtime_lifecycle_contract runtime_recover_commits_bundle_owner_facet_soul_and_lifecycle_atomically
cargo test -p bm-sdk --features nonproduction-replay-harness --test runtime_lifecycle_contract runtime_recover_budget_failure_leaves_owner_facet_and_events_unchanged
cargo test -p bm-entry --features nonproduction-replay-harness --test workbench_contract
cargo test -p bm-http --features server-std,nonproduction-replay-harness --test http_console_contract console_workbench

git -C dev-docs diff --check
test -f dev-docs/multi-subject-memory-space-plan.md
test -f crates/core/tests/subject_registry_contract.rs
test -f crates/core/tests/soul_non_regression_contract.rs
test -f crates/sdk/tests/single_agent_default_registry_contract.rs
test -f crates/sdk/tests/shared_fact_governance_contract.rs
test -f crates/sdk/tests/projection_no_soul_mutation_contract.rs
! rg -n "RoleKey|RoleMemoryLane" crates/core/src crates/sdk/src
! rg -n "boss_user|ceo_agent|finance_director_agent|warehouse_manager_agent|CEO|BOSS|财务总监|仓库管理员" crates/core/src crates/sdk/src
rg -Fq "Multi-Subject Memory Space 当前设计真源" dev-docs/README.md
rg -Fq "W0.5 Multi-Subject Memory Space 合同" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq 'shared factual plane 属于 `MemorySpace`' dev-docs/multi-subject-memory-space-plan.md
rg -Fq "Single Agent 最小入口" dev-docs/multi-subject-memory-space-plan.md
rg -Fq "single-agent default registry 已闭合" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "Soul Non-Regression 红线" dev-docs/multi-subject-memory-space-plan.md
rg -Fq "soul non-regression 已闭合首版" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq '`SubjectRegistry` / host metadata / seat binding 不能替代 soul kernel' dev-docs/sdk-host-integration-readiness-plan.md
rg -Fq "主体投影不能直接修改 soul core" dev-docs/soul-and-subject-memory-boundary.md
rg -Fq "graph private soul redaction fixture" dev-docs/multi-subject-memory-space-plan.md
rg -Fq "quickstart 手写 registry 或关系图" dev-docs/sdk-host-integration-readiness-plan.md
rg -Fq "不得出现 Beetle Agent 官方 CBD 角色名" dev-docs/sdk-host-integration-readiness-plan.md
rg -Fq "W1 MemoryBenchmarkReport + fixture matrix + script gate + baseline 已落地" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W2 Soul Kernel 2.0 发布闸已由 SoulKernelBenchmarkJudgeReport + core builders + fixture semantic contract 闭合" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W3 Subject Projection 2.0 发布闸已由 SubjectProjectionBenchmarkJudgeReport + SDK projection report + Gateway audit contract 闭合" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W2-W9 next-gen contract matrix + gate report builders 已落地" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W4-W8 builders 已通过 SDK public surface 暴露" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W3 SubjectProjectionReport 已由 MemoryRuntime::project() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W4 GraphRecallRerankReport 已由 MemoryRuntime::recall() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W4 graph expansion budget" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W4 production recall index" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W4 external noisy wall summary/operator contract" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W4 external noisy stage/index/W4.1 diagnostics" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W4ExternalNoisyW41Diagnostics" dev-docs/next-gen-soul-memory-roadmap.md crates/replay/src/bench.rs crates/replay/src/lib.rs
rg -Fq "w4_external_noisy_wall_w4_1_diagnostics_missing" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md crates/replay/src/bench.rs crates/replay/src/bin/bm-w4-external-noisy-wall.rs
rg -Fq "w4_1_diagnostics" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md dev-docs/next-gen-soul-memory-roadmap.md crates/replay/src/bench.rs
rg -Fq "召回质量优化" dev-docs/next-gen-soul-memory-roadmap.md dev-docs/README.md
rg -Fq "W4.1 Recall Quality Optimization" dev-docs/temporal-memory-graph-plan.md
rg -Fq "W4.3 Evidence Source Safety Pass" dev-docs/temporal-memory-graph-plan.md
require_in_all "governed-memory-facet-index-plan.md" dev-docs/README.md dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "Governed Memory Facet Index / Hybrid Graph Retrieval Implementation Plan" dev-docs/governed-memory-facet-index-plan.md
require_in_all "Evidence-Governed Hybrid Facet Graph Retrieval" dev-docs/governed-memory-facet-index-plan.md dev-docs/README.md dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "exact_facets" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "expanded_facets" dev-docs/governed-memory-facet-index-plan.md
require_in_all "StructuredFacetParser" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md
require_in_all "EntityNormalizer" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md
require_in_all "TemporalAnchorParser" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md
require_in_all "CanonicalEvidenceRef" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md
rg -Fq "canonical_entity_id" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "derived_from_exact_facet_id" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "expansion_rule_id" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "canonical_evidence_group" dev-docs/governed-memory-facet-index-plan.md
require_in_all "RankFusionReport" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md
require_in_all "CoverageSelectionReport" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md
require_in_all "GraphFacetPropagation" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md
rg -Fq "fallback_full_scan=false" dev-docs/governed-memory-facet-index-plan.md
require_in_all "regex/substring" dev-docs/governed-memory-facet-index-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "上桌硬闸" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "FacetReportView" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "FacetIndexRebuildReport" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "HumanFacetSuggestion" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "MemoryFacetIndexDoc" crates/core/src/memory/memory_facet.rs crates/core/tests/memory_facet_contract.rs
rg -Fq "StructuredFacetParser" crates/core/src/memory/memory_facet.rs crates/core/tests/memory_facet_contract.rs
rg -Fq "MemoryFacetRecallIndexReport" crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
rg -Fq "MemoryLongTermAffectedFacetDoc" crates/core/src/memory/long_term_control.rs crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
rg -Fq "affected_facet_docs" crates/core/src/memory/long_term_control.rs crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/long_term_memory_control_contract.rs
rg -Fq "facet_inspector" crates/entry/src/console.rs crates/entry/src/runtime.rs crates/entry/tests/workbench_contract.rs
rg -Fq "obsidian-style-facet-audit-markdown" crates/entry/src/runtime.rs crates/entry/tests/workbench_contract.rs crates/http/tests/http_console_contract.rs
rg -Fq "direct_mutation_allowed" crates/entry/src/console.rs crates/entry/src/runtime.rs crates/entry/tests/workbench_contract.rs
rg -Fq "FacetRecallRuntimeBudget" crates/core/src/budget.rs crates/sdk/src/lib.rs crates/sdk/tests/runtime_budget_contract.rs
rg -Fq "max_facet_index_docs_read" crates/core/src/budget.rs crates/sdk/src/runtime.rs crates/sdk/tests/runtime_budget_contract.rs
rg -Fq "FacetRankFusionReport" crates/core/src/memory/memory_facet.rs crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
rg -Fq "FacetCoverageSelectionReport" crates/core/src/memory/memory_facet.rs crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
rg -Fq "GraphFacetPropagationContext" crates/core/src/memory/next_gen_contract.rs crates/core/src/memory/mod.rs crates/sdk/src/runtime.rs
rg -Fq "facet_exact_score" crates/core/src/memory/next_gen_contract.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "facet_diversity_score" crates/core/src/memory/next_gen_contract.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "facet_temporal_score" crates/core/src/memory/next_gen_contract.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "rank_fusion_report" crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "coverage_selection_report" crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "MemoryEvalRecallAblationReport" crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "sdk_eval_recall_off_run_v1" crates/sdk/src/runtime.rs crates/sdk/tests/eval_recall_contract.rs crates/replay/src/bench.rs crates/replay/tests/memory_benchmark_wall.rs dev-docs/governed-memory-facet-index-plan.md
rg -Fq "memory_facet_indexes" crates/sdk/src/store_internal/platform.rs crates/store-contract-tests/tests/mutation_batch_contract.rs
rg -Fq "facet_index_remap_required" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "report_only_subject_visibility_not_indexed" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_off" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "rank_fusion_off" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "coverage_selection_off" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "LongMemEval-V2" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "From Recall to Forgetting" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "Temporal GraphRAG" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "long_term_memory_generates_governed_facets_from_accepted_fields" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_index_keeps_exact_and_expanded_hierarchical_facets_separate" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_index_uses_canonical_evidence_group_without_collapsing_distinct_sources" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_parser_rejects_regex_only_entity_and_time_facets" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_value_contract_uses_typed_values_not_display_string_splitting" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_rank_fusion_preserves_pool_provenance" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_coverage_selection_prioritizes_distinct_canonical_evidence_groups" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_graph_propagation_uses_indexed_graph_anchor_without_full_scan" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_recall_expands_graph_anchor_pool_without_render_growth" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_recall_respects_privacy_scope_and_profile_budget" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_recall_blocks_cross_subject_expanded_metadata_leakage" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "long_term_control_mutation_updates_facet_index" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "long_term_control_mutation_reports_affected_facet_docs_for_operator_review" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "transcript_mask_redacts_or_blocks_facet_source_refs" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_index_rebuild_reports_orphan_and_schema_failures" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "facet_report_view_redacts_sensitive_metadata_by_default" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "human_facet_suggestion_requires_governed_proposal" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "eval_recall_reports_facet_stage_for_expanded_miss" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "external_noisy_wall_reports_facet_stage_diagnostics" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "external_noisy_wall_requires_facet_ablation_and_no_render_growth" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "quality net" dev-docs/temporal-memory-graph-plan.md || rg -Fq "质量净提升" dev-docs/temporal-memory-graph-plan.md
rg -Fq "candidate pool split" dev-docs/temporal-memory-graph-plan.md
rg -Fq "memory-owned hybrid source retrieval" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "query-aware graph expansion" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "graph_anchor_candidates" dev-docs/temporal-memory-graph-plan.md
rg -Fq "source_candidate_ids" dev-docs/temporal-memory-graph-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
rg -Fq "graph_anchor_candidate_ids" dev-docs/temporal-memory-graph-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
rg -Fq "fallback_recall_candidates_are_query_scored_before_recency" crates/core/src/memory/long_term.rs
rg -Fq "external runner temporal graph anchor binding" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "memory_graph_index_source_anchor_missing" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "write_chunks_creates_indexed_eval_recall_path" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md
rg -Fq "full external noisy wall" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "check_w4_external_noisy_wall_operator: ok" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq -- "--shard-total" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "repeated_external_eval_source_maps_distinct_chunk_topics_without_anchor_collision" dev-docs/temporal-memory-graph-plan.md
rg -Fq "temporal_memory_graph_scores_distinct_external_eval_sources_as_multi_evidence_groups" dev-docs/temporal-memory-graph-plan.md crates/core/tests/next_gen_contract.rs
rg -Fq "source_authority_recognizes_archive_locator_citations" dev-docs/temporal-memory-graph-plan.md crates/core/src/memory/long_term.rs
rg -Fq "w4_external_noisy_summary_with_provenance" dev-docs/temporal-memory-graph-plan.md crates/replay/src/bench.rs crates/replay/src/lib.rs
rg -Fq "w4_external_noisy_wall_stage_diagnostics_missing" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
rg -Fq "W4ExternalNoisyIndexDiagnostics" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs crates/replay/src/lib.rs
rg -Fq "w4_external_noisy_wall_index_diagnostics_missing" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
rg -Fq "shards_valid" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md dev-docs/next-gen-soul-memory-roadmap.md crates/replay/src/bench.rs
rg -Fq "index_no_full_scan" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md dev-docs/next-gen-soul-memory-roadmap.md crates/replay/src/bench.rs scripts/check_w4_external_noisy_wall_operator.sh
rg -Fq "w4_external_noisy_wall_shards_invalid" dev-docs/replay-sandbox-plan.md dev-docs/next-gen-soul-memory-roadmap.md crates/replay/src/bench.rs scripts/check_w4_external_noisy_wall_operator.sh
rg -Fq "w4_external_noisy_wall_index_full_scan_detected" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md dev-docs/next-gen-soul-memory-roadmap.md crates/replay/src/bench.rs scripts/check_w4_external_noisy_wall_operator.sh
rg -Fq "w4_external_noisy_wall_stage_attribution_not_proven" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
rg -Fq "w4_external_noisy_wall_index_effect_not_proven" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
rg -Fq "W4ExternalNoisyFacetAblationDiagnostics" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs crates/replay/src/lib.rs
rg -Fq "method_counts" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs crates/replay/tests/memory_benchmark_wall.rs
rg -Fq "w4_external_noisy_wall_facet_ablation_missing" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs crates/replay/src/bin/bm-w4-external-noisy-wall.rs
rg -Fq "w4_external_noisy_wall_facet_ablation_effect_not_proven" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs crates/replay/src/bin/bm-w4-external-noisy-wall.rs
rg -Fq "w4_external_noisy_wall_render_growth_detected" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs scripts/check_w4_external_noisy_wall_operator.sh
rg -Fq "w4_external_noisy_wall_requires_facet_ablation_and_no_render_growth" crates/replay/tests/memory_benchmark_wall.rs
rg -Fq "w4_external_noisy_wall_requires_every_noisy_split_to_improve_against_w43_baseline" crates/replay/tests/memory_benchmark_wall.rs
rg -Fq "w4_external_noisy_wall_rejects_full_scan_and_wrong_shard_total" crates/replay/tests/memory_benchmark_wall.rs
rg -Fq "w4_external_noisy_wall_passes_only_when_improvement_has_stage_and_index_attribution" crates/replay/tests/memory_benchmark_wall.rs
rg -Fq "bm-w4-external-noisy-wall" dev-docs/temporal-memory-graph-plan.md crates/replay/src/bin/bm-w4-external-noisy-wall.rs scripts/check_w4_external_noisy_wall_operator.sh
rg -Fq "check_w4_external_noisy_wall_operator" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md scripts/check_w4_external_noisy_wall_operator.sh
rg -Fq -- "--merge-suite" dev-docs/temporal-memory-graph-plan.md
rg -Fq "MemoryGraphRecallIndexReport" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
rg -Fq "large_persistent_graph_index_report_explains_anchor_and_expansion_coverage" dev-docs/next-gen-soul-memory-roadmap.md crates/sdk/tests/eval_recall_contract.rs
rg -Fq "persistent_graph_recall_fails_closed_when_loaded_graph_exceeds_profile_budget" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md crates/sdk/tests/eval_recall_contract.rs
rg -Fq "read_json_docs_by_keys" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/sdk/src/store_internal/platform.rs crates/sdk/src/runtime.rs
rg -Fq "memory_graph_nodes_loaded_budget_exceeded" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md dev-docs/replay-sandbox-plan.md crates/sdk/src/runtime.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "memory_graph_edges_loaded_budget_exceeded" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md dev-docs/replay-sandbox-plan.md crates/sdk/src/runtime.rs
rg -Fq "memory_graph_backlinks_loaded_budget_exceeded" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md dev-docs/replay-sandbox-plan.md crates/sdk/src/runtime.rs
rg -Fq "filtered_node_count" dev-docs/next-gen-soul-memory-roadmap.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "unmatched_source_anchor_ids" dev-docs/temporal-memory-graph-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "memory_graph_indexes" dev-docs/temporal-memory-graph-plan.md crates/sdk/src/runtime.rs crates/sdk/src/store_internal/platform.rs
rg -Fq "RuntimeBudgetReport.graph_expansion_budget" dev-docs/temporal-memory-graph-plan.md dev-docs/runtime-budget-refactor-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "GraphRecallExpansionBudget" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md crates/core/src/memory/next_gen_contract.rs crates/sdk/src/lib.rs
rg -Fq "W4 TemporalMemoryGraphGateReport 已由 MemoryRuntime::recall() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W5 ProceduralMemoryPromotionReport 已由 MemoryRuntime::write() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W7 VaultMigrationPreflight 已由 MemoryRuntime/preview_memory_space_migration() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W9 Workbench API map + runtime report + Console 页面已由 EntryRuntime/HTTP console/apps/console 暴露" dev-docs/next-gen-soul-memory-roadmap.md

# P7 production delivery hardening: owner, privacy, exact index, final projection and operator truth.
require_in_all "GovernedLongTermMemoryReadView" dev-docs/long-term-memory-control-surface-plan.md dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/runtime.rs
require_in_all "ChangePrivacy" dev-docs/long-term-memory-control-surface-plan.md dev-docs/governed-memory-facet-index-plan.md crates/core/src/memory/long_term_control.rs
require_in_all "memory_facet_postings" dev-docs/governed-memory-facet-index-plan.md crates/core/src/memory/memory_facet.rs crates/sdk/src/store_internal/platform.rs
require_in_all "QueryFacet" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md crates/core/src/memory/memory_facet.rs crates/sdk/src/runtime.rs
require_in_all "RecallDeliveryOrderingPolicy" dev-docs/governed-memory-facet-index-plan.md crates/core/src/memory/recall_delivery.rs crates/sdk/src/runtime.rs
require_in_all "scoped_memory_graph_storage_key" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md crates/core/src/memory/next_gen_contract.rs
require_in_all "MemoryProjectionDeliveryDigestManifest" dev-docs/governed-memory-facet-index-plan.md dev-docs/inhabited-subject-projection-refactor-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
require_in_all "shared_fact_surface_allowed" dev-docs/governed-memory-facet-index-plan.md dev-docs/inhabited-subject-projection-refactor-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/replay/src/bench.rs
require_in_file ".read_json_docs_by_keys(" crates/sdk/src/runtime.rs
require_in_all "GovernedOpaque" dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/runtime.rs
require_in_all "selected_hit_final_rendered_miss" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
require_in_all "final_projection_integrity" dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
require_in_all "delivery_drop_reason_counts" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
require_in_all "p7_runner_build_inputs_sha256_v2" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
require_in_all "p7_sdk_build_inputs_sha256_v2" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/replay/build.rs crates/replay/src/bench.rs
require_in_file "P7_FROZEN_RUNNER_IDENTITY" crates/replay/src/bench.rs
require_in_file "frozen_runner_identity_contract_is_structurally_valid" crates/replay/src/bench.rs
require_in_all "QueryFacetInput" dev-docs/governed-memory-facet-index-plan.md crates/core/src/memory/memory_facet.rs crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/adapter/src/payload.rs
require_in_all "structured_query_facets" crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/adapter/src/payload.rs crates/adapter/tests/contract.rs
require_in_all "MemoryFacetIndexManifest" dev-docs/governed-memory-facet-index-plan.md dev-docs/long-term-memory-control-surface-plan.md crates/core/src/memory/memory_facet.rs crates/sdk/src/runtime.rs
require_in_all "same_scope_facet_closure_migrates_across_all_store_backends" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/memory_space_migration_contract.rs
require_in_all "manifest_integrity_verified" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/replay/src/bench.rs
require_in_all "candidate_receipts" dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/replay/src/bench.rs
require_in_all "exact_render_match" crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/replay/src/bench.rs
require_in_all "AdapterProjectionReport" dev-docs/governed-memory-facet-index-plan.md crates/adapter/src/contract.rs crates/adapter/src/dispatch.rs
require_in_file "project_command_returns_only_the_adapter_projection_contract" crates/adapter/tests/contract.rs
require_in_all "ui_api" dev-docs/governed-memory-facet-index-plan.md dev-docs/inhabited-subject-projection-refactor-plan.md crates/adapter/src/contract.rs crates/http/src/lib.rs crates/mcp/src/lib.rs
require_in_all "evidence_family_rotation_off" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
require_in_all "baseline_selected_candidate_ids" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/replay/src/bench.rs
require_in_all "off_run_selected_candidate_ids" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/replay/src/bench.rs
require_in_all "baseline_rendered_candidate_ids" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/replay/src/bench.rs
require_in_all "off_run_rendered_candidate_ids" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/replay/src/bench.rs
require_in_all "delivery_contribution_proven" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/replay/src/bench.rs
require_in_all "delivery_affected_candidate_count" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/replay/src/bench.rs
require_in_all "delivery_affected_candidate_occurrences" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs crates/replay/tests/memory_benchmark_wall.rs
require_in_all "used && fact" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md
require_in_file "unused_graph_metadata_is_not_counted_as_used_graph_proof" crates/replay/src/bench.rs
require_in_file "unused_facet_metadata_is_not_counted_as_used_facet_proof" crates/replay/src/bench.rs
require_in_file "不是 release 硬门槛" dev-docs/governed-memory-facet-index-plan.md
require_in_all "--run-id" scripts/check_w4_external_noisy_wall_preflight.sh crates/replay/src/bin/bm-w4-external-noisy-wall.rs
require_in_all "--preflight-report" scripts/check_w4_external_noisy_wall_operator.sh crates/replay/src/bin/bm-w4-external-noisy-wall.rs
require_in_all "preflight-report.json" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md scripts/check_w4_external_noisy_wall_operator.sh crates/replay/src/bin/bm-w4-external-noisy-wall.rs
require_in_all "p7_operator_routes_verifier_publisher_through_retained_launcher" dev-docs/governed-memory-facet-index-plan.md crates/replay/tests/memory_benchmark_wall.rs
require_in_all "p7_linux_real_publisher_and_verifier_use_sealed_execution_authority" dev-docs/governed-memory-facet-index-plan.md crates/replay/tests/memory_benchmark_wall.rs
require_in_all "p7_sealed_execution_identity_binds_memfd_bytes_to_release_manifest" dev-docs/governed-memory-facet-index-plan.md crates/replay/src/bench.rs
require_in_all "p7_retained_launcher_replaces_reserved_execution_authority_environment" dev-docs/governed-memory-facet-index-plan.md crates/replay/src/p7_secure_fs.rs
require_in_all "p7_inherited_execution_authority_rejects_partial_seals_and_wrong_sha" dev-docs/governed-memory-facet-index-plan.md crates/replay/src/p7_secure_fs.rs
require_in_all "check_p7_linux_execution_authority.sh" dev-docs/governed-memory-facet-index-plan.md scripts/check_p7_linux_execution_authority.sh
require_in_all "p7_inherited_execution_authority_rejects_direct_path_and_foreign_fd" dev-docs/governed-memory-facet-index-plan.md crates/replay/src/p7_secure_fs.rs
require_in_all "p7_direct_publisher_fails_before_external_write_without_execution_broker" dev-docs/governed-memory-facet-index-plan.md crates/replay/tests/memory_benchmark_wall.rs
require_in_all "F_GET_SEALS" dev-docs/governed-memory-facet-index-plan.md crates/replay/src/p7_secure_fs.rs
require_in_all "bm-replay" dev-docs/governed-memory-facet-index-plan.md scripts/check_cross_target_compile_gates.sh
! rg -n 'rg .*preflight-report|rg .*operator-report|"p7_provenance_valid": true' scripts/check_w4_external_noisy_wall_preflight.sh scripts/check_w4_external_noisy_wall_operator.sh
! rg -n 'contains\([^)]*capsule\.content' crates/sdk/src
require_in_all "MemoryGraphScopeManifest" dev-docs/temporal-memory-graph-plan.md dev-docs/governed-memory-facet-index-plan.md crates/core/src/memory/next_gen_contract.rs crates/sdk/src/runtime.rs
require_in_all "MemoryGraphNodeMembership" dev-docs/temporal-memory-graph-plan.md crates/core/src/memory/next_gen_contract.rs crates/sdk/src/runtime.rs
require_in_all "MemoryGraphEdgeMembership" dev-docs/temporal-memory-graph-plan.md crates/core/src/memory/next_gen_contract.rs crates/sdk/src/runtime.rs
require_in_all "MemoryGraphBacklinkMembership" dev-docs/temporal-memory-graph-plan.md crates/core/src/memory/next_gen_contract.rs crates/sdk/src/runtime.rs
require_in_all "StoreJsonPrecondition" dev-docs/temporal-memory-graph-plan.md dev-docs/memory-write-transaction-plan.md crates/sdk/src/store_internal/mutation.rs crates/sdk/src/runtime.rs crates/store-contract-tests/tests/mutation_batch_contract.rs
require_in_all "commit_governed_memory_transaction_with_preconditions" dev-docs/temporal-memory-graph-plan.md dev-docs/memory-write-transaction-plan.md crates/sdk/src/store_internal/platform.rs
require_in_all "commit_governed_memory_transaction_with_runtime_budget" dev-docs/memory-write-transaction-plan.md crates/sdk/src/store_internal/platform.rs crates/sdk/src/runtime.rs
require_in_all "commit_governed_memory_transaction_authorized" dev-docs/memory-write-transaction-plan.md crates/sdk/src/store_internal/platform.rs
require_in_all "manifest_contract_verified" dev-docs/temporal-memory-graph-plan.md dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
require_in_all "selected_dependency_chain_verified" dev-docs/temporal-memory-graph-plan.md dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
require_in_all "full_scope_closure_verified" dev-docs/temporal-memory-graph-plan.md dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
require_in_all "graph_selected_dependency_chain_verified_questions" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
require_in_all "graph_read_path_mutation_delta" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/replay/src/bench.rs
require_in_file "graph_v2_persistent_keys_and_dependency_digests_use_explicit_sha256_contract" crates/core/tests/memory_graph_v2_contract.rs
require_in_all "conditional_batch_exact_precondition_serializes_competing_writers_without_lost_update" dev-docs/governed-memory-facet-index-plan.md crates/store-contract-tests/tests/mutation_batch_contract.rs
require_in_all "stale_owner_facet_plan_is_rejected_without_partial_delta" dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/runtime.rs
require_in_all "shared_posting_conflict_requires_complete_replan_before_both_owners_exist" dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/runtime.rs
require_in_all "stale_governance_policy_plan_is_rejected_without_overwrite" dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/runtime.rs
require_in_all "MemoryEvalRecallCandidateEvidenceBinding" dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs
require_in_all "scoped_long_term_memory_storage_key" dev-docs/governed-memory-facet-index-plan.md crates/core/src/memory/long_term.rs crates/sdk/src/runtime.rs
require_in_all "scoped_memory_facet_owner_storage_key" dev-docs/governed-memory-facet-index-plan.md crates/core/src/memory/memory_facet.rs crates/sdk/src/runtime.rs
require_in_all "scoped_control_events_expose_logical_ids_not_physical_storage_keys" dev-docs/governed-memory-facet-index-plan.md crates/store-contract-tests/tests/long_term_memory_control_store_contract.rs
require_in_all "LongTermMemoryControlReadStore" dev-docs/governed-memory-facet-index-plan.md dev-docs/long-term-memory-control-surface-plan.md crates/core/src/memory/long_term_control.rs crates/sdk/src/store_internal/platform.rs crates/sdk/src/runtime.rs
require_in_all "production_long_term_control_mutation_is_not_exposed_by_host_store_surfaces" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/public_surface.rs
require_in_all "production_continuity_and_shared_memory_planners_are_read_only" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/public_surface.rs
require_in_all "explicit_privacy_transition_updates_owner_facet_and_postings_atomically" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/memory_write_transaction_contract.rs
require_in_all "continuity_import_preserves_soul_private_without_public_delivery_or_graph_membership" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/replay_import_export.rs
require_in_all "StoreImmutableReadSession" dev-docs/governed-memory-facet-index-plan.md dev-docs/temporal-memory-graph-plan.md crates/sdk/src/store_internal/transaction.rs crates/sdk/src/store_internal/recall_read.rs
require_in_all "with_recall_immutable_read_session" dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/store_internal/platform.rs crates/sdk/src/runtime.rs crates/store-contract-tests/tests/governed_recall_immutable_session_contract.rs
require_in_all "production_recall_has_no_whole_store_snapshot_or_second_platform_path" dev-docs/governed-memory-facet-index-plan.md crates/store-contract-tests/tests/governed_recall_immutable_session_contract.rs
require_in_all "recall_reports_the_single_immutable_session_read_view_it_consumed" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/runtime_contract.rs
if rg -n "load_governed_recall_snapshot|GovernedRecallSnapshot|ReadOnlySnapshotStoreEngine" crates/sdk crates/store-contract-tests dev-docs; then
  echo "legacy whole-store recall snapshot path must not exist" >&2
  exit 1
fi
require_in_all "governed_transaction_rejects_owner_mutation_without_facet_closure" dev-docs/governed-memory-facet-index-plan.md crates/store-contract-tests/tests/mutation_batch_contract.rs
require_in_all "governed_transaction_rejects_control_mutation_without_audit_closure" dev-docs/governed-memory-facet-index-plan.md crates/store-contract-tests/tests/mutation_batch_contract.rs
require_in_all "governed_transaction_rejects_mismatched_control_audit_binding" dev-docs/governed-memory-facet-index-plan.md crates/store-contract-tests/tests/mutation_batch_contract.rs
require_in_all "raw_graph_batch_cannot_forge_integrity_repair_authority_with_operation_text" dev-docs/governed-memory-facet-index-plan.md crates/store-contract-tests/tests/mutation_batch_contract.rs
require_in_all "GraphRepairAuthority" dev-docs/governed-memory-facet-index-plan.md crates/sdk/src/store_internal/platform.rs crates/sdk/src/runtime.rs
require_in_all "independent_file_open_consistent_read_never_observes_mixed_generation" dev-docs/governed-memory-facet-index-plan.md crates/store-contract-tests/tests/store_concurrency_contract.rs
if rg -Fq "MemoryManageTool" crates; then
  echo "legacy MemoryManageTool mutation path must not exist" >&2
  exit 1
fi
require_in_all "eval_report_canonicalizes_benchmark_locators_before_they_reach_the_public_report" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/memory_graph_v2_contract.rs
require_in_all "evidence_compaction_is_an_owner_mutation_not_a_source_replay" dev-docs/governed-memory-facet-index-plan.md crates/core/tests/long_term_entry_planner_contract.rs
require_in_all "retention_compaction_executor_compacts_metadata_without_deleting_accepted_memory" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/retention_compaction_contract.rs
require_in_all "CompactEvidenceMetadata" dev-docs/governed-memory-facet-index-plan.md dev-docs/memory-write-transaction-plan.md crates/core/src/memory/long_term.rs crates/core/src/memory/hygiene.rs
! rg -n "persistent_graph_prune_mutations|memory_graph\.prune_restricted" crates/sdk/src

p7_ablation_slices=(
  facet_off
  rank_fusion_off
  coverage_selection_off
  delivery_relevance_fusion_off
  evidence_family_rotation_off
  render_capsule_off
  capsule_dedupe_off
)
for slice in "${p7_ablation_slices[@]}"; do
  require_in_all "$slice" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md crates/sdk/src/runtime.rs crates/replay/src/bench.rs
done
! rg -n "coverage_allocator_off|multi_gold_allocator_off" dev-docs crates/core/src crates/sdk/src crates/replay/src scripts/check_memory_benchmark_wall.sh scripts/check_w4_external_noisy_wall_operator.sh
! rg -n "BM_W4_EXTERNAL_EXPECT_BLOCKED|is_expected_current_baseline_block|ExitCode::from\(10\)|expected-block" dev-docs crates/replay/src scripts/check_memory_benchmark_wall.sh scripts/check_w4_external_noisy_wall_operator.sh

require_in_all "delivery_allocator_preserves_distinct_evidence_groups_before_duplicate_rank" dev-docs/governed-memory-facet-index-plan.md crates/core/tests/memory_facet_contract.rs
require_in_all "delivery_allocator_never_backfills_an_exact_group_duplicate" dev-docs/governed-memory-facet-index-plan.md crates/core/tests/memory_facet_contract.rs
require_in_all "projection_consumes_production_evidence_capsules_without_render_budget_growth" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/eval_recall_contract.rs
require_in_all "projection_keeps_unicode_capsules_consistent_with_the_character_budget" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/eval_recall_contract.rs
require_in_all "long_term_control_list_and_detail_use_the_governed_runtime_view" dev-docs/governed-memory-facet-index-plan.md dev-docs/long-term-memory-control-surface-plan.md crates/sdk/tests/long_term_memory_control_contract.rs
require_in_all "governed_read_filters_private_records_before_source_recall_limit" dev-docs/governed-memory-facet-index-plan.md dev-docs/long-term-memory-control-surface-plan.md crates/sdk/tests/eval_recall_contract.rs
require_in_all "persistent_graph_storage_is_isolated_by_memory_space_and_subject" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/eval_recall_contract.rs
require_in_all "long_term_extraction_plans_delete_and_upsert_against_one_facet_manifest_state" dev-docs/governed-memory-facet-index-plan.md crates/sdk/tests/memory_write_transaction_contract.rs
require_in_all "external_noisy_wall_requires_p7_selection_render_and_production_proof" dev-docs/governed-memory-facet-index-plan.md crates/replay/tests/memory_benchmark_wall.rs
require_in_all "external_noisy_wall_rejects_complete_but_unverified_p7_release_evidence" dev-docs/governed-memory-facet-index-plan.md crates/replay/tests/memory_benchmark_wall.rs
require_in_all "detail_recomputation_independently_validates_sdk_projection_delivery_manifest" dev-docs/governed-memory-facet-index-plan.md crates/replay/src/bench.rs
require_in_all "runner_disk_identity_tracks_exact_build_inputs_lock_and_executable" dev-docs/governed-memory-facet-index-plan.md crates/replay/src/bench.rs
require_in_file "capsule_dedupe_assigns_each_exact_evidence_group_to_one_primary_capsule" crates/sdk/tests/eval_recall_contract.rs
require_in_file "explicit_privacy_transition_updates_owner_facet_and_postings_atomically" crates/sdk/tests/memory_write_transaction_contract.rs
require_in_file "delegated_actor_is_not_added_to_shared_memory_owner_subjects" crates/sdk/tests/eval_recall_contract.rs
require_in_file "detail_recomputation_attributes_projection_only_loss_to_final_rendered" crates/replay/src/bench.rs
require_in_file "detail_recomputation_rejects_forged_final_projection_integrity" crates/replay/src/bench.rs
require_in_file "p7_match_gold_groups" crates/replay/src/bench.rs
require_in_all "最大二分匹配" dev-docs/governed-memory-facet-index-plan.md dev-docs/replay-sandbox-plan.md
require_in_file "deterministic_gold_matching_consumes_each_candidate_and_gold_at_most_once" crates/replay/src/bench.rs
require_in_file "p7_augment_gold_match" crates/replay/src/bench.rs
require_in_file "facet_keys_are_isolated_by_mounted_subject_and_reject_empty_subject" crates/core/tests/memory_facet_contract.rs
require_in_file "facet_read_chain_rejects_stale_posting_revision" crates/core/tests/memory_facet_contract.rs
require_in_file "facet_read_chain_rejects_stale_owner_facet_revision" crates/core/tests/memory_facet_contract.rs
require_in_file "projection_digest_proves_duplicate_content_by_candidate_source_id" crates/sdk/tests/eval_recall_contract.rs
require_in_file "evidence_family_rotation_off_keeps_exact_group_deduplication_enabled" crates/core/tests/memory_facet_contract.rs
