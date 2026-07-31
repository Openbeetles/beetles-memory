#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

require_fixed() {
  local needle="$1"
  shift
  if ! rg -F -q "$needle" "$@"; then
    echo "missing required long-term memory control term: $needle" >&2
    printf '  files: %s\n' "$*" >&2
    exit 1
  fi
}

require_regex() {
  local needle="$1"
  shift
  if ! rg -q "$needle" "$@"; then
    echo "missing required long-term memory control pattern: $needle" >&2
    printf '  files: %s\n' "$*" >&2
    exit 1
  fi
}

required_files=(
  "dev-docs/long-term-memory-control-surface-plan.md"
  "docs/en/api.md"
  "docs/zh-CN/api.md"
  "docs/en/deployment.md"
  "docs/zh-CN/deployment.md"
  "docs/en/cli-usage.md"
  "docs/zh-CN/cli-usage.md"
  "docs/en/integration.md"
  "docs/zh-CN/integration.md"
  "docs/en/replay-and-archive.md"
  "docs/zh-CN/replay-and-archive.md"
  "docs/en/operator-guide.md"
  "docs/zh-CN/operator-guide.md"
  "docs/en/profiles.md"
  "docs/zh-CN/profiles.md"
)

for file in "${required_files[@]}"; do
  if [[ ! -s "$file" ]]; then
    echo "missing long-term memory control file: $file" >&2
    exit 1
  fi
done

require_fixed "long-term-memory-control-surface-plan.md" dev-docs/README.md
require_fixed "MemoryLongTermMutationRequest" crates/sdk/src/ops.rs crates/sdk/src/lib.rs docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryLongTermPolicyRequest" crates/sdk/src/ops.rs crates/sdk/src/lib.rs docs/en/api.md docs/zh-CN/api.md
require_fixed "mutate_long_term_memory" crates/sdk/src/runtime.rs crates/sdk/tests/long_term_memory_control_contract.rs docs/en/api.md docs/zh-CN/api.md docs/en/integration.md docs/zh-CN/integration.md
require_fixed "mutate_memory_governance_policy" crates/sdk/src/runtime.rs docs/en/api.md docs/zh-CN/api.md docs/en/operator-guide.md docs/zh-CN/operator-guide.md
require_fixed "MemoryGovernancePolicyMutation" crates/core/src/memory/long_term_control.rs crates/sdk/src/ops.rs crates/sdk/tests/long_term_memory_control_contract.rs
require_fixed "long_term_control_mutation" crates/sdk/src/capability.rs fixtures/platform/capabilities
require_fixed "long_term_control_bulk_forget" crates/sdk/src/capability.rs fixtures/platform/capabilities docs/en/profiles.md docs/zh-CN/profiles.md
require_fixed "LongTermMemoryControlReadStore" crates/core/src/memory/long_term_control.rs crates/sdk/src/store_internal/platform.rs crates/store-contract-tests/tests/long_term_memory_control_store_contract.rs crates/sdk/src/runtime.rs
require_fixed "LongTermMemoryControlStore" crates/core/src/memory/long_term_control.rs crates/core/tests/long_term_memory_control_contract.rs crates/sdk/src/runtime.rs
require_fixed "tombstone" crates/core/src/memory/long_term_control.rs crates/sdk/src/store_internal/platform.rs crates/sdk/tests/long_term_memory_control_contract.rs docs/en/api.md docs/zh-CN/api.md
require_fixed "forget_by_query" crates/core/src/memory/long_term_control.rs crates/core/tests/long_term_memory_control_contract.rs crates/sdk/tests/long_term_memory_control_contract.rs docs/en/api.md docs/zh-CN/api.md
require_fixed "suppression policy" docs/en/api.md docs/zh-CN/api.md dev-docs/long-term-memory-control-surface-plan.md
require_fixed "TranscriptDerivedRef" crates/core/src/memory/long_term_control.rs crates/sdk/tests/long_term_memory_control_contract.rs
require_fixed "DerivedMemoryRef" docs/en/replay-and-archive.md docs/zh-CN/replay-and-archive.md crates/sdk/tests/long_term_memory_control_contract.rs
require_fixed "HostUi" docs/en/api.md docs/zh-CN/api.md docs/en/integration.md docs/zh-CN/integration.md
require_fixed "/memory/long-term/list" docs/en/deployment.md docs/zh-CN/deployment.md crates/http/src/lib.rs
require_fixed "memory_long_term_list" docs/en/deployment.md docs/zh-CN/deployment.md crates/mcp/src/lib.rs
require_fixed "command.long_term.list" docs/en/deployment.md docs/zh-CN/deployment.md crates/wss/src/lib.rs
require_fixed "memory_long_term_list_request" docs/en/deployment.md docs/zh-CN/deployment.md crates/a2a/src/lib.rs
require_regex "shadow memory|shadow memory|shadow memory|shadow memory|shadow memory" docs/en/api.md docs/zh-CN/api.md docs/en/integration.md docs/zh-CN/integration.md
require_regex "runtime skill.*not|运行时 Skill.*不是|Runtime Skill.*not" docs/en/api.md docs/zh-CN/api.md docs/en/integration.md docs/zh-CN/integration.md
require_regex "Transcript lifecycle.*not automatically|Transcript lifecycle.*不会自动|transcript lifecycle.*does not automatically" docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-archive.md docs/zh-CN/replay-and-archive.md

host_product_forbidden_pattern="RoleKey|TaskRoomProjection|ClarificationRequest|HumanGate|Task\\.status|TaskRecord|CEO|BOSS|财务总监|仓库管理员"

if rg -n "$host_product_forbidden_pattern" crates/core/src crates/sdk/src; then
  echo "core/sdk/store contain host product semantics in long-term memory control surface" >&2
  exit 1
fi

if rg -n "local SQLite memory|fake memory|Agent SQLite|host-owned shadow memory editor" crates/core/src crates/sdk/src; then
  echo "core/sdk/store appear to expose host-owned shadow memory wording" >&2
  exit 1
fi

forbidden_control_mutation_surface="fn long_term_memory_control_store\\(|impl LongTermMemoryControlStore for StorePlatform|pub struct ScopedLongTermMemoryControlStore|pub fn scoped_long_term_memory_control_store\\("
if rg -n "$forbidden_control_mutation_surface" crates/core/src/platform crates/sdk/src/store_internal; then
  echo "host/store expose direct long-term control mutation capability" >&2
  exit 1
fi

cargo test --locked -p bm-core --test long_term_memory_control_contract
cargo test --locked -p bm-store-contract-tests --test long_term_memory_control_store_contract
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test public_surface
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test capability_catalog
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test platform_capability_snapshot_shape
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test platform_capability_snapshots
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test long_term_memory_control_contract
cargo test --locked -p bm-adapter --test contract
cargo test --locked -p bm-http --test http_contract
cargo test --locked -p bm-mcp --test mcp_contract
cargo test --locked -p bm-wss --test wss_contract
cargo test --locked -p bm-a2a --test a2a_contract
cargo test --locked -p bm-cli --test cli_contract
cargo test --locked -p bm-http --features server-std --test http_runtime_contract
cargo test --locked -p bm-mcp --features server-stdio --test mcp_runtime_contract
cargo test --locked -p bm-wss --features server-std --test wss_runtime_contract
cargo test --locked -p bm-a2a --features bridge-http --test a2a_runtime_contract

bash scripts/emit_platform_capability_snapshots.sh --check
bash -n "$0"

echo "OK: long-term memory control surface gate passed"
