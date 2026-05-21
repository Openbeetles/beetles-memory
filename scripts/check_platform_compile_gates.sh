#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

usage() {
  cat >&2 <<'EOF'
Usage:
  bash scripts/check_platform_compile_gates.sh
  bash scripts/check_platform_compile_gates.sh --strict-targets
EOF
}

mode="${1:-}"
case "$mode" in
  "" ) target_mode="--host-only" ;;
  --strict-targets ) target_mode="--strict" ;;
  -h|--help )
    usage
    exit 0
    ;;
  * )
    usage
    exit 2
    ;;
esac

cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/check_profile_matrix.sh
bash scripts/check_adapter_communication_contract.sh
bash scripts/check_platform_dependency_budget.sh
bash scripts/emit_platform_capability_snapshots.sh --check
bash scripts/check_cross_target_compile_gates.sh "$target_mode"

echo "OK: platform compile gates passed"
