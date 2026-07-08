#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

require_in_file() {
  local pattern="$1"
  local file="$2"
  rg -Fq "$pattern" "$file"
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

cargo test -p bm-sdk --test projection_audit_contract
cargo test -p bm-sdk --test sdk_runtime_flow runtime_write_recall_project_uses_sdk_entry_only
cargo test -p bm-sdk --test write_candidate_contract
cargo test -p bm-sdk --test memory_space_migration_contract
cargo test -p bm-sdk --test public_surface next_gen_builders_are_sdk_public_without_adapter_ownership
cargo test -p bm-sdk --test runtime_budget_contract graph_expansion_budget_is_profile_owned_and_not_provider_render_owned
cargo test -p bm-sdk --test runtime_budget_contract facet_recall_budget_is_profile_owned_and_not_graph_or_render_owned
cargo test -p bm-sdk --test eval_recall_contract persistent_graph_recall_uses_sdk_owned_production_index_report
cargo test -p bm-core --test memory_facet_contract
cargo test -p bm-store --test mutation_batch_contract memory_facet_index_namespace_is_admitted_without_store_semantics
cargo test -p bm-sdk --test eval_recall_contract eval_recall_reports_facet_stage_for_expanded_miss
cargo test -p bm-sdk --test eval_recall_contract facet_rank_fusion_preserves_pool_provenance
cargo test -p bm-sdk --test eval_recall_contract facet_coverage_selection_prioritizes_distinct_canonical_evidence_groups
cargo test -p bm-sdk --test eval_recall_contract facet_graph_propagation_uses_indexed_graph_anchor_without_full_scan
cargo test -p bm-sdk --test eval_recall_contract facet_recall_expands_graph_anchor_pool_without_render_growth
cargo test -p bm-sdk --test eval_recall_contract facet_recall_respects_privacy_scope_and_profile_budget
cargo test -p bm-sdk --test eval_recall_contract facet_recall_blocks_cross_subject_expanded_metadata_leakage
cargo test -p bm-sdk --test long_term_memory_control_contract long_term_control_mutation_reports_affected_facet_docs_for_operator_review
cargo test -p bm-core --test subject_registry_contract
cargo test -p bm-core --test soul_non_regression_contract
cargo test -p bm-core --test next_gen_contract temporal_memory_graph_rejects_raw_soul_private_material
cargo test -p bm-sdk --test single_agent_default_registry_contract
cargo test -p bm-sdk --test shared_fact_governance_contract
cargo test -p bm-sdk --test projection_no_soul_mutation_contract
cargo test -p bm-entry --features replay-harness --test workbench_contract
cargo test -p bm-http --features server-std,replay-harness --test http_console_contract console_workbench

git -C dev-docs diff --check
test -f dev-docs/multi-subject-memory-space-plan.md
test -f crates/core/tests/subject_registry_contract.rs
test -f crates/core/tests/soul_non_regression_contract.rs
test -f crates/sdk/tests/single_agent_default_registry_contract.rs
test -f crates/sdk/tests/shared_fact_governance_contract.rs
test -f crates/sdk/tests/projection_no_soul_mutation_contract.rs
! rg -n "RoleKey|RoleMemoryLane" crates/core/src crates/store/src crates/sdk/src
! rg -n "boss_user|ceo_agent|finance_director_agent|warehouse_manager_agent|CEO|BOSS|财务总监|仓库管理员" crates/core/src crates/store/src crates/sdk/src
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
rg -Fq "memory_facet_indexes" crates/store/src/platform.rs crates/store/tests/mutation_batch_contract.rs
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
rg -Fq "facet_migration_remap_required_fails_closed" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "eval_recall_reports_facet_stage_for_expanded_miss" dev-docs/governed-memory-facet-index-plan.md
rg -Fq "eval_recall_reports_real_off_run_facet_ablation_method" dev-docs/governed-memory-facet-index-plan.md
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
rg -Fq "read_json_docs_by_keys" dev-docs/temporal-memory-graph-plan.md dev-docs/replay-sandbox-plan.md crates/store/src/platform.rs crates/sdk/src/runtime.rs
rg -Fq "memory_graph_nodes_loaded_budget_exceeded" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md dev-docs/replay-sandbox-plan.md crates/sdk/src/runtime.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "memory_graph_edges_loaded_budget_exceeded" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md dev-docs/replay-sandbox-plan.md crates/sdk/src/runtime.rs
rg -Fq "memory_graph_backlinks_loaded_budget_exceeded" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md dev-docs/replay-sandbox-plan.md crates/sdk/src/runtime.rs
rg -Fq "filtered_node_count" dev-docs/next-gen-soul-memory-roadmap.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "unmatched_source_anchor_ids" dev-docs/temporal-memory-graph-plan.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/eval_recall_contract.rs
rg -Fq "memory_graph_indexes" dev-docs/temporal-memory-graph-plan.md crates/sdk/src/runtime.rs crates/store/src/platform.rs
rg -Fq "RuntimeBudgetReport.graph_expansion_budget" dev-docs/temporal-memory-graph-plan.md dev-docs/runtime-budget-refactor-plan.md dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "GraphRecallExpansionBudget" dev-docs/temporal-memory-graph-plan.md dev-docs/next-gen-soul-memory-roadmap.md crates/core/src/memory/next_gen_contract.rs crates/sdk/src/lib.rs
rg -Fq "W4 TemporalMemoryGraphGateReport 已由 MemoryRuntime::recall() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W5 ProceduralMemoryPromotionReport 已由 MemoryRuntime::write() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W7 VaultMigrationPreflight 已由 MemoryRuntime/preview_memory_space_migration() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W9 Workbench API map + runtime report + Console 页面已由 EntryRuntime/HTTP console/apps/console 暴露" dev-docs/next-gen-soul-memory-roadmap.md
