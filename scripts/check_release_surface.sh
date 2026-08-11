#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

cargo() {
  command cargo --locked "$@"
}
export -f cargo

gate_tmp="$(mktemp -d "${TMPDIR:-/tmp}/bm-release-surface.XXXXXX")"
cleanup() {
  rm -rf "$gate_tmp"
}
trap cleanup EXIT

readonly RELEASE_SURFACE_MIN_AVAILABLE_KIB=$((32 * 1024 * 1024))
release_surface_available_kib="$(df -Pk "$gate_tmp" | awk 'END { print $4 }')"
if [[ ! "$release_surface_available_kib" =~ ^[0-9]+$ ]]; then
  echo "release surface storage probe did not return canonical available KiB" >&2
  exit 1
fi
if ((release_surface_available_kib < RELEASE_SURFACE_MIN_AVAILABLE_KIB)); then
  printf 'release surface requires at least 32 GiB free on the temporary target filesystem; available=%s KiB path=%s\n' \
    "$release_surface_available_kib" "$gate_tmp" >&2
  exit 1
fi

export CARGO_TARGET_DIR="$gate_tmp/cargo-target"
export CARGO_INCREMENTAL=0

ignored_before="$gate_tmp/ignored-before.txt"
ignored_after="$gate_tmp/ignored-after.txt"
git ls-files --others --ignored --exclude-standard | sort > "$ignored_before"

required_docs=(
  "docs/README.md"
  "docs/en/api.md"
  "docs/en/cli-usage.md"
  "docs/en/deployment.md"
  "docs/en/getting-started.md"
  "docs/en/llm-gateway-integrations.md"
  "docs/en/profiles.md"
  "docs/en/store-backends.md"
  "docs/en/replay-and-archive.md"
  "docs/en/adapters.md"
  "docs/en/operator-guide.md"
  "docs/en/release-checklist.md"
  "docs/zh-CN/api.md"
  "docs/zh-CN/cli-usage.md"
  "docs/zh-CN/deployment.md"
  "docs/zh-CN/getting-started.md"
  "docs/zh-CN/llm-gateway-integrations.md"
  "docs/zh-CN/profiles.md"
  "docs/zh-CN/store-backends.md"
  "docs/zh-CN/replay-and-archive.md"
  "docs/zh-CN/adapters.md"
  "docs/zh-CN/operator-guide.md"
  "docs/zh-CN/release-checklist.md"
  "dev-docs/deployment-runtime-plan.md"
  "dev-docs/entry-runtime-plan.md"
  "dev-docs/archive/production-hardening-audit-plan.md"
  "dev-docs/release-surface-plan.md"
  "dev-docs/agent-tool-experience-registry-plan.md"
)

for doc in "${required_docs[@]}"; do
  if [[ ! -s "$doc" ]]; then
    echo "missing release surface document: $doc" >&2
    exit 1
  fi
done

current_persistence_truth_docs=(
  "dev-docs/code-quality-governance.md"
  "dev-docs/config-console-plan.md"
  "dev-docs/entry-runtime-plan.md"
  "dev-docs/release-surface-plan.md"
  "dev-docs/runtime-lifecycle-plan.md"
  "dev-docs/agent-context-substrate-refactor-plan.md"
  "dev-docs/conversation-transcript-attribute-plane-plan.md"
  "dev-docs/conversation-transcript-governance-hardening-plan.md"
  "dev-docs/conversation-transcript-substrate-plan.md"
  "dev-docs/deployment-runtime-plan.md"
  "dev-docs/llm-gateway-production-implementation-plan.md"
  "dev-docs/adapter-communication-plan.md"
  "dev-docs/sdk-profile-contract-plan.md"
  "dev-docs/post-turn-memory-governance-plan.md"
  "dev-docs/memory-write-transaction-plan.md"
  "dev-docs/temporal-memory-graph-plan.md"
)

if rg -n -P 'crates/store/(?:src|tests)|cargo (?:test|clippy|tree|doc)[^\n]*-p bm-store(?!-contract-tests)|bm_store::StorePlatform|bm-store::StorePlatform|-> bm-store(?!-contract-tests)' \
  "${current_persistence_truth_docs[@]}"; then
  echo "current persistence truth docs retain the deleted public store owner" >&2
  exit 1
fi

if rg -n 'EntryStoreConfig|StoreBackendKind|\.profile\(' \
  README.md README.zh-CN.md docs/en docs/zh-CN; then
  echo "release surface contains removed store/profile API" >&2
  exit 1
fi

for doc in README.md README.zh-CN.md docs/en/api.md docs/zh-CN/api.md; do
  rg -F -q 'structured_query_facets' "$doc" || {
    echo "release surface omits required structured query facets: $doc" >&2
    exit 1
  }
done

for doc in docs/en/api.md docs/zh-CN/api.md; do
  rg -F -q 'MemoryWriteRequest::GovernedEvidenceDocuments' "$doc" || {
    echo "release surface omits governed evidence document writes: $doc" >&2
    exit 1
  }
  rg -F -q 'MemoryEvidenceDocumentReadRequest' "$doc" || {
    echo "release surface omits governed evidence document reads: $doc" >&2
    exit 1
  }
  rg -F -q 'MemoryRuntime::read_governed_evidence_documents(request)' "$doc" || {
    echo "release surface uses a stale governed evidence read method: $doc" >&2
    exit 1
  }
  rg -F -q '`memory_space_id`, `document_ids`' "$doc" || {
    echo "release surface uses stale governed evidence read fields: $doc" >&2
    exit 1
  }
done

if rg -n 'read_evidence_documents|`owner_refs`, `view`' docs/en/api.md docs/zh-CN/api.md; then
  echo "release surface retains the removed governed evidence read contract" >&2
  exit 1
fi

for doc in docs/en/cli-usage.md docs/zh-CN/cli-usage.md; do
  rg -F -q 'BM_HOST_PROFILE' "$doc" || {
    echo "CLI usage omits the host-native profile selection contract: $doc" >&2
    exit 1
  }
  if rg -n -- '--profile profile-server-linux-dev-full' "$doc"; then
    echo "CLI usage treats Linux dev-full as the generic local profile: $doc" >&2
    exit 1
  fi
done

for doc in docs/en/deployment.md docs/zh-CN/deployment.md; do
  if rg -n 'Local CLI/operator process.*profile-server-linux-dev-full|本地 CLI/operator 进程.*profile-server-linux-dev-full' "$doc"; then
    echo "deployment guide treats Linux dev-full as the generic local profile: $doc" >&2
    exit 1
  fi
done

for marker in \
  'typed `(memory_space_id, mounted_subject_id)` projection' \
  '不是 whole-store snapshot 加标签' \
  '当前 dev 不保留旧 evidence/source-claim/family/export schema' \
  'MemoryRuntime::read_governed_evidence_documents(' \
  '所有 Cargo 执行使用 `--locked`'; do
  rg -F -q "$marker" dev-docs/governed-memory-facet-index-plan.md || {
    echo "P7 truth source omits audited release decision: $marker" >&2
    exit 1
  }
done

locked_cargo_gates=(
  scripts/check_release_surface.sh
  scripts/check_cross_target_compile_gates.sh
  scripts/check_memory_write_transaction_contract.sh
  scripts/check_next_gen_memory_plan.sh
  scripts/check_runtime_budget_contracts.sh
  scripts/check_sdk_host_integration_readiness.sh
  scripts/emit_platform_capability_snapshots.sh
)

gate_enforces_locked_cargo() {
  local gate="$1"
  if rg -F -q 'command cargo --locked "$@"' "$gate"; then
    return 0
  fi
  rg -F -q 'local has_locked=0' "$gate" \
    && rg -F -q '[[ "$arg" == "--locked" ]] && has_locked=1' "$gate" \
    && rg -F -q 'command cargo "$subcommand" --locked "$@"' "$gate" \
    && rg -F -q 'command cargo "$subcommand" --locked --no-default-features "$@"' "$gate"
}

for gate in "${locked_cargo_gates[@]}"; do
  gate_enforces_locked_cargo "$gate" || {
    echo "Cargo gate does not enforce Cargo.lock: $gate" >&2
    exit 1
  }
done

for gate in scripts/*.sh; do
  if gate_enforces_locked_cargo "$gate"; then
    continue
  fi
  if rg -n '(^|[[:space:]])cargo (check|test|clippy|build|tree|run|doc|publish)([[:space:]]|$)' "$gate" \
    | rg -v -- '--locked'; then
    echo "Cargo gate contains an unlocked command: $gate" >&2
    exit 1
  fi
done

if rg -n \
  '当前仍是 whole-space snapshot|当前迁移粒度仍是 whole-space snapshot|migration apply 仍需真实 subject key / scope remap|facet_migration_remap_required_fails_closed|memory_space_migration_fails_closed_when_snapshot_contains_facet_index' \
  dev-docs/README.md \
  dev-docs/archive/sdk-host-integration-readiness-plan.md \
  dev-docs/governed-memory-facet-index-plan.md \
  dev-docs/multi-subject-memory-space-plan.md \
  scripts/check_memory_write_transaction_contract.sh; then
  echo "active truth or gate retains the superseded whole-store/remap contract" >&2
  exit 1
fi

rg -F -q 'MEMORY_FACET_SCHEMA_VERSION = 4' \
  dev-docs/governed-memory-facet-index-plan.md
rg -F -q 'source identity claim，schema 直接为 3' \
  dev-docs/governed-memory-facet-index-plan.md

for public_entry_doc in \
  docs/en/deployment.md docs/zh-CN/deployment.md \
  docs/en/adapters.md docs/zh-CN/adapters.md; do
  rg -q 'StoreBackendConfig::' "$public_entry_doc" || {
    echo "entry runtime public example is not bound to StoreBackendConfig: $public_entry_doc" >&2
    exit 1
  }
done

for doc in "${required_docs[@]}"; do
  if [[ "$doc" == dev-docs/* ]]; then
    rg -q "$(basename "$doc")" dev-docs/README.md README.md || {
      echo "release surface dev-doc is not indexed: $doc" >&2
      exit 1
    }
  else
    rg -q "$doc" README.md || {
      echo "release surface public doc is not indexed in README: $doc" >&2
      exit 1
    }
  fi
done

examples=(
  "examples/rust-sdk-embedded/Cargo.toml"
  "examples/server-runtime/Cargo.toml"
  "examples/linux-device/Cargo.toml"
  "examples/esp-standalone-memory/Cargo.toml"
  "examples/esp-embedded-sdk/Cargo.toml"
)

example_tmp_root="$gate_tmp/example-repo"
mkdir -p "$example_tmp_root/examples"
awk '
  /^members = \[/ {
    print
    print "    \"examples/rust-sdk-embedded\","
    print "    \"examples/server-runtime\","
    print "    \"examples/linux-device\","
    print "    \"examples/esp-standalone-memory\","
    print "    \"examples/esp-embedded-sdk\","
    next
  }
  { print }
' "$ROOT/Cargo.toml" > "$example_tmp_root/Cargo.toml"
cp "$ROOT/Cargo.lock" "$example_tmp_root/Cargo.lock"
ln -s "$ROOT/crates" "$example_tmp_root/crates"
ln -s "$ROOT/apps" "$example_tmp_root/apps"

prepare_example_manifest() {
  local manifest="$1"
  local example_dir
  local example_name
  local tmp_example

  example_dir="$(dirname "$manifest")"
  example_name="$(basename "$example_dir")"
  tmp_example="$example_tmp_root/examples/$example_name"
  mkdir -p "$tmp_example"
  sed '/^\[workspace\]$/d' "$ROOT/$manifest" > "$tmp_example/Cargo.toml"
  case "$example_name" in
    rust-sdk-embedded|esp-embedded-sdk)
      example_lock_dependencies=' "bm-sdk",'
      ;;
    linux-device)
      example_lock_dependencies=$' "bm-adapter",\n "bm-entry",\n "bm-sdk",'
      ;;
    server-runtime)
      example_lock_dependencies=$' "bm-entry",\n "bm-http",\n "bm-sdk",'
      ;;
    esp-standalone-memory)
      example_lock_dependencies=$' "bm-adapter",\n "bm-entry",\n "bm-sdk",\n "bm-wss",'
      ;;
    *)
      echo "release surface example lock owner is missing: $example_name" >&2
      exit 1
      ;;
  esac
  printf '\n[[package]]\nname = "bm-example-%s"\nversion = "0.1.0"\ndependencies = [\n%s\n]\n' \
    "$example_name" "$example_lock_dependencies" >> "$example_tmp_root/Cargo.lock"
  if [[ -d "$ROOT/$example_dir/src" ]]; then
    cp -R "$ROOT/$example_dir/src" "$tmp_example/src"
  fi
}

for manifest in "${examples[@]}"; do
  prepare_example_manifest "$manifest"
done

for manifest in "${examples[@]}"; do
  example_name="$(basename "$(dirname "$manifest")")"
  CARGO_TARGET_DIR="$gate_tmp/cargo-target" \
    cargo check -q --locked -p "bm-example-$example_name" --manifest-path "$example_tmp_root/Cargo.toml"
done

case "$(uname -s)" in
  Darwin)
    host_examples=(rust-sdk-embedded)
    ;;
  Linux)
    host_examples=(server-runtime linux-device)
    ;;
  *)
    host_examples=()
    ;;
esac
for example_name in "${host_examples[@]}"; do
  CARGO_TARGET_DIR="$gate_tmp/cargo-target" \
    cargo run -q --locked -p "bm-example-$example_name" --manifest-path "$example_tmp_root/Cargo.toml"
done

publishable=(
  "bm-core"
  "bm-sdk"
  "bm-replay"
  "bm-evolve"
  "bm-adapter"
  "bm-entry"
  "bm-cli"
  "bm-llm-gateway"
  "bm-ollama-transparent"
  "bm-http"
  "bm-wss"
  "bm-mcp"
  "bm-a2a"
)

cargo doc --locked --no-deps --no-default-features \
  -p bm-core \
  -p bm-sdk \
  -p bm-replay \
  -p bm-evolve \
  -p bm-adapter \
  -p bm-entry \
  -p bm-cli \
  -p bm-llm-gateway \
  -p bm-ollama-transparent \
  -p bm-http \
  -p bm-wss \
  -p bm-mcp \
  -p bm-a2a

bash scripts/emit_platform_capability_snapshots.sh --check
bash scripts/check_entry_runtime_contract.sh
bash scripts/check_deployment_runtime_contract.sh
bash scripts/check_next_gen_memory_plan.sh
bash scripts/check_memory_benchmark_wall.sh
bash scripts/check_long_term_memory_control_surface.sh
bash scripts/check_conversation_transcript_attr_plane.sh
bash scripts/check_production_hardening_contract.sh

for needle in \
  "Agent Tool Registry" \
  "agent_tool_hints" \
  "no_governed_tool_experience" \
  "host_execution_required" \
  "/agent-tool-registries/{id}"
do
  rg -F -q "$needle" docs/en/api.md docs/zh-CN/api.md dev-docs/agent-tool-experience-registry-plan.md
done

publish_dry_run() {
  local crate="$1"
  local extra=()

  case "$crate" in
    bm-core)
      ;;
    bm-sdk)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      ;;
    bm-replay)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      ;;
    bm-evolve)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      ;;
    bm-adapter)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      ;;
    bm-entry)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      extra+=(--config 'patch.crates-io.bm-replay.path="crates/replay"')
      extra+=(--config 'patch.crates-io.bm-adapter.path="crates/adapter"')
      ;;
    bm-cli|bm-llm-gateway)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      extra+=(--config 'patch.crates-io.bm-replay.path="crates/replay"')
      extra+=(--config 'patch.crates-io.bm-adapter.path="crates/adapter"')
      extra+=(--config 'patch.crates-io.bm-entry.path="crates/entry"')
      ;;
    bm-http)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      extra+=(--config 'patch.crates-io.bm-replay.path="crates/replay"')
      extra+=(--config 'patch.crates-io.bm-adapter.path="crates/adapter"')
      extra+=(--config 'patch.crates-io.bm-entry.path="crates/entry"')
      extra+=(--config 'patch.crates-io.bm-ollama-transparent.path="crates/ollama-transparent"')
      ;;
    bm-wss|bm-mcp|bm-a2a)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      extra+=(--config 'patch.crates-io.bm-adapter.path="crates/adapter"')
      extra+=(--config 'patch.crates-io.bm-entry.path="crates/entry"')
      ;;
    bm-ollama-transparent)
      ;;
    *)
      echo "missing publish dry-run patch mapping: $crate" >&2
      exit 1
      ;;
  esac

  if ((${#extra[@]} == 0)); then
    cargo publish --locked --dry-run --allow-dirty -p "$crate"
  else
    cargo publish --locked --dry-run --allow-dirty -p "$crate" "${extra[@]}"
  fi
}

for crate in "${publishable[@]}"; do
  publish_dry_run "$crate"
done

if rg -n "adapter-beetle|source_kind.*beetle|nested_beetle_error|beetle_host|beetle_adapter|beetle_source|default_host.*beetle" \
  docs examples crates README.md; then
  echo "release surface contains source-project-bound public wording" >&2
  exit 1
fi

if rg -n "workflow runner|skill marketplace|管理控制台|TLS certificate|TLS 证书|TLS termination|TLS 终止" \
  docs examples crates README.md | rg -v "不|不能|不启动|not|Red Lines|Drift"; then
  echo "release surface appears to include out-of-scope runtime surface" >&2
  exit 1
fi

git ls-files --others --ignored --exclude-standard | sort > "$ignored_after"
if ! diff -u "$ignored_before" "$ignored_after"; then
  echo "release surface gate changed ignored repository artifacts" >&2
  exit 1
fi

echo "OK: release surface gate passed"
