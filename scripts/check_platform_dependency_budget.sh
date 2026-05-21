#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

budget_file="fixtures/platform/dependency-budgets.json"

if [[ ! -f "$budget_file" ]]; then
  echo "missing dependency budget fixture: $budget_file" >&2
  exit 1
fi

if ! command -v node >/dev/null 2>&1; then
  echo "check_platform_dependency_budget requires node to parse JSON budget fixtures" >&2
  exit 1
fi

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

budget_rows="$(
  node - "$budget_file" <<'NODE'
const fs = require("fs");
const path = process.argv[2];
const budget = JSON.parse(fs.readFileSync(path, "utf8"));
if (budget.schema !== "beetle-memory.platform.dependency-budget.v1") {
  throw new Error(`unexpected dependency budget schema: ${budget.schema}`);
}
for (const [profile, entry] of Object.entries(budget.profiles)) {
  console.log([
    profile,
    entry.package,
    entry.features.join(","),
    entry.forbidden_tree_needles.join("|"),
    entry.required_tree_needles.join("|"),
    entry.store_backend,
    String(entry.sqlite_allowed),
    String(entry.server_listener_allowed),
  ].join("\t"));
}
NODE
)"

while IFS=$'\t' read -r profile package features forbidden_needles required_needles store_backend sqlite_allowed server_listener_allowed; do
  [[ -n "$profile" ]] || continue

  tree="$(cargo tree -p "$package" --no-default-features --features "$features")"

  IFS='|' read -r -a forbidden <<< "$forbidden_needles"
  for needle in "${forbidden[@]}"; do
    [[ -n "$needle" ]] || continue
    if grep -q "$needle" <<<"$tree"; then
      echo "$tree" >&2
      fail "$profile dependency tree unexpectedly includes $needle"
    fi
  done

  IFS='|' read -r -a required <<< "$required_needles"
  for needle in "${required[@]}"; do
    [[ -n "$needle" ]] || continue
    if ! grep -q "$needle" <<<"$tree"; then
      echo "$tree" >&2
      fail "$profile dependency tree is missing required dependency $needle"
    fi
  done

  if [[ "$sqlite_allowed" == "false" ]] && grep -q "rusqlite" <<<"$tree"; then
    echo "$tree" >&2
    fail "$profile must not compile rusqlite"
  fi

  if [[ "$store_backend" == "sqlite-store" ]] && ! grep -q "rusqlite" <<<"$tree"; then
    echo "$tree" >&2
    fail "$profile sqlite-store budget must compile rusqlite"
  fi

  if [[ "$server_listener_allowed" == "false" ]] && grep -Eq "tokio|hyper|axum|warp|tungstenite|rumqttc" <<<"$tree"; then
    echo "$tree" >&2
    fail "$profile must not compile server/listener dependencies"
  fi
done <<< "$budget_rows"

adapter_manifests=(
  crates/adapter/Cargo.toml
  crates/cli/Cargo.toml
  crates/http/Cargo.toml
  crates/wss/Cargo.toml
  crates/mqtt/Cargo.toml
  crates/mcp/Cargo.toml
  crates/a2a/Cargo.toml
)

for manifest in "${adapter_manifests[@]}"; do
  if rg -n '^(bm-core|bm-store)[[:space:]]*=' "$manifest" >/tmp/bm-adapter-direct-dep.$$; then
    cat /tmp/bm-adapter-direct-dep.$$ >&2
    rm -f /tmp/bm-adapter-direct-dep.$$
    fail "$manifest must not depend on bm-core or bm-store directly"
  fi
  rm -f /tmp/bm-adapter-direct-dep.$$
done

protocol_manifests=(
  crates/http/Cargo.toml
  crates/wss/Cargo.toml
  crates/mqtt/Cargo.toml
  crates/mcp/Cargo.toml
  crates/a2a/Cargo.toml
)

for manifest in "${protocol_manifests[@]}"; do
  if rg -n '^(tokio|hyper|axum|warp|tungstenite|rumqttc)[[:space:]]*=' "$manifest" >/tmp/bm-protocol-listener-dep.$$; then
    cat /tmp/bm-protocol-listener-dep.$$ >&2
    rm -f /tmp/bm-protocol-listener-dep.$$
    fail "$manifest must not introduce real server/listener dependencies in the contract layer"
  fi
  rm -f /tmp/bm-protocol-listener-dep.$$
done

echo "OK: platform dependency budget checks passed"
