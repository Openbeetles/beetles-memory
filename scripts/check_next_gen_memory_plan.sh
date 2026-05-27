#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

bash scripts/check_memory_benchmark_wall.sh
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_sdk_host_integration_readiness.sh

cargo test -p bm-sdk --test projection_audit_contract
cargo test -p bm-sdk --test sdk_runtime_flow runtime_write_recall_project_uses_sdk_entry_only
cargo test -p bm-sdk --test write_candidate_contract
cargo test -p bm-sdk --test memory_space_migration_contract
cargo test -p bm-sdk --test public_surface next_gen_builders_are_sdk_public_without_adapter_ownership
cargo test -p bm-entry --test workbench_contract
cargo test -p bm-http --features server-std --test http_console_contract console_workbench

git -C dev-docs diff --check
rg -Fq "W1 MemoryBenchmarkReport + fixture matrix + script gate + baseline 已落地" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W2-W9 next-gen contract matrix + gate report builders 已落地" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W4-W8 builders 已通过 SDK public surface 暴露" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W3 SubjectProjectionReport 已由 MemoryRuntime::project() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W4 GraphRecallRerankReport 已由 MemoryRuntime::recall() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W4 TemporalMemoryGraphGateReport 已由 MemoryRuntime::recall() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W5 ProceduralMemoryPromotionReport 已由 MemoryRuntime::write() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W7 VaultMigrationPreflight 已由 MemoryRuntime/preview_memory_space_migration() 返回" dev-docs/next-gen-soul-memory-roadmap.md
rg -Fq "W9 Workbench API map + runtime report + Console 页面已由 EntryRuntime/HTTP console/apps/console 暴露" dev-docs/next-gen-soul-memory-roadmap.md
