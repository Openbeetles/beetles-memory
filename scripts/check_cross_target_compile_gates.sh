#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
  cat >&2 <<'EOF'
Usage:
  bash scripts/check_cross_target_compile_gates.sh --host-only
  bash scripts/check_cross_target_compile_gates.sh --strict
EOF
}

mode="${1:-}"
case "$mode" in
  --host-only|--strict) ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

profiles=(
  profile-esp-standalone-memory
  profile-esp-embedded-sdk
  profile-linux-device-standalone-memory
  profile-desktop-macos-embedded-sdk
  profile-desktop-windows-embedded-sdk
  profile-server-linux-memory-gateway
  profile-server-linux-dev-full
)

for profile in "${profiles[@]}"; do
  cargo check -p bm-sdk --no-default-features --features "$profile"
done

if [[ "$mode" == "--host-only" ]]; then
  echo "OK: host platform compile gates passed"
  exit 0
fi

gate_file="fixtures/platform/target-gates.json"
if [[ ! -f "$gate_file" ]]; then
  echo "missing target gate fixture: $gate_file" >&2
  exit 1
fi

if ! command -v node >/dev/null 2>&1; then
  echo "check_cross_target_compile_gates requires node to parse target gate fixtures" >&2
  exit 1
fi

gate_rows="$(
  node - "$gate_file" <<'NODE'
const fs = require("fs");
const path = process.argv[2];
const gates = JSON.parse(fs.readFileSync(path, "utf8"));
if (gates.schema !== "beetle-memory.platform.target-gates.v1") {
  throw new Error(`unexpected target gate schema: ${gates.schema}`);
}
for (const gate of gates.gates) {
  console.log([
    gate.profile,
    gate.target,
    gate.package,
    gate.features.join(","),
  ].join("\t"));
}
NODE
)"

target_installed() {
  local target="$1"
  command -v rustup >/dev/null 2>&1 || return 1
  rustup target list --installed 2>/dev/null | grep -Fx "$target" >/dev/null
}

missing_target_report() {
  local profile="$1"
  local target="$2"
  local package="$3"
  local features="$4"
  printf '{\n' >&2
  printf '  "schema": "beetle-memory.platform.target-gate.v1",\n' >&2
  printf '  "status": "missing_toolchain",\n' >&2
  printf '  "target": "%s",\n' "$target" >&2
  printf '  "profile": "%s",\n' "$profile" >&2
  printf '  "required_command": "cargo check -p %s --target %s --no-default-features --features %s"\n' "$package" "$target" "$features" >&2
  printf '}\n' >&2
}

status=0
while IFS=$'\t' read -r profile target package features; do
  [[ -n "$profile" ]] || continue

  if ! target_installed "$target"; then
    missing_target_report "$profile" "$target" "$package" "$features"
    status=1
    continue
  fi

  if ! cargo check -p "$package" --target "$target" --no-default-features --features "$features"; then
    echo "target compile gate failed: profile=$profile target=$target package=$package features=$features" >&2
    status=1
  fi
done <<< "$gate_rows"

if [[ "$status" -ne 0 ]]; then
  exit "$status"
fi

echo "OK: strict target platform compile gates passed"
