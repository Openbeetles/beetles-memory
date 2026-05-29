#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

bash scripts/check_memory_benchmark_wall.sh
bash scripts/check_inhabited_projection_phase5_cleanup.sh
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_sdk_host_integration_readiness.sh
bash scripts/check_agent_skill_directory_contract.sh

cargo test -p bm-sdk --test projection_audit_contract
cargo test -p bm-sdk --test sdk_runtime_flow runtime_write_recall_project_uses_sdk_entry_only
cargo test -p bm-sdk --test write_candidate_contract
cargo test -p bm-sdk --test memory_space_migration_contract
cargo test -p bm-sdk --test public_surface next_gen_builders_are_sdk_public_without_adapter_ownership
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
rg -Fq "W4 TemporalMemoryGraphGateReport 已由 MemoryRuntime::recall() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W5 ProceduralMemoryPromotionReport 已由 MemoryRuntime::write() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W7 VaultMigrationPreflight 已由 MemoryRuntime/preview_memory_space_migration() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W9 Workbench API map + runtime report + Console 页面已由 EntryRuntime/HTTP console/apps/console 暴露" dev-docs/next-gen-soul-memory-roadmap.md
