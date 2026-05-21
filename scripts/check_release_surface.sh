#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

required_docs=(
  "docs/api.md"
  "docs/sdk-quickstart.md"
  "docs/profile-matrix.md"
  "docs/store-backends.md"
  "docs/replay-migration.md"
  "docs/adapter-contract.md"
  "docs/operator-inspection.md"
  "docs/release-checklist.md"
  "dev-docs/release-surface-plan.md"
)

for doc in "${required_docs[@]}"; do
  if [[ ! -s "$doc" ]]; then
    echo "missing release surface document: $doc" >&2
    exit 1
  fi
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
  "examples/memory-gateway/Cargo.toml"
)

for manifest in "${examples[@]}"; do
  cargo run -q --manifest-path "$manifest"
done

publishable=(
  "bm-core"
  "bm-store"
  "bm-sdk"
  "bm-replay"
  "bm-evolve"
  "bm-adapter"
  "bm-cli"
  "bm-http"
  "bm-wss"
  "bm-mqtt"
  "bm-mcp"
  "bm-a2a"
)

cargo doc --no-deps --no-default-features \
  -p bm-core \
  -p bm-store \
  -p bm-sdk \
  -p bm-replay \
  -p bm-evolve \
  -p bm-adapter \
  -p bm-cli \
  -p bm-http \
  -p bm-wss \
  -p bm-mqtt \
  -p bm-mcp \
  -p bm-a2a

bash scripts/emit_platform_capability_snapshots.sh --check

publish_dry_run() {
  local crate="$1"
  local extra=()

  case "$crate" in
    bm-core)
      ;;
    bm-store)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      ;;
    bm-sdk)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-store.path="crates/store"')
      ;;
    bm-replay)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-store.path="crates/store"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      ;;
    bm-evolve)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-store.path="crates/store"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      ;;
    bm-adapter)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-store.path="crates/store"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      ;;
    bm-cli|bm-http|bm-wss|bm-mqtt|bm-mcp|bm-a2a)
      extra+=(--config 'patch.crates-io.bm-core.path="crates/core"')
      extra+=(--config 'patch.crates-io.bm-store.path="crates/store"')
      extra+=(--config 'patch.crates-io.bm-sdk.path="crates/sdk"')
      extra+=(--config 'patch.crates-io.bm-adapter.path="crates/adapter"')
      ;;
    *)
      echo "missing publish dry-run patch mapping: $crate" >&2
      exit 1
      ;;
  esac

  if ((${#extra[@]} == 0)); then
    cargo publish --dry-run --allow-dirty -p "$crate"
  else
    cargo publish --dry-run --allow-dirty -p "$crate" "${extra[@]}"
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

if rg -n "workflow runner|skill marketplace|管理控制台|真实网络 listener|real network listener" \
  docs examples crates README.md | rg -v "不|不能|不启动|not|Red Lines|Drift"; then
  echo "release surface appears to include out-of-scope runtime surface" >&2
  exit 1
fi

echo "OK: release surface gate passed"
