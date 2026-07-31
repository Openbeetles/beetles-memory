#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo() {
  command cargo --locked "$@"
}

usage() {
  cat >&2 <<'EOF'
Usage:
  bash scripts/check_cross_target_compile_gates.sh --preflight
  bash scripts/check_cross_target_compile_gates.sh --host-only
  bash scripts/check_cross_target_compile_gates.sh --strict
EOF
}

mode="${1:-}"
case "$mode" in
  --preflight|--host-only|--strict) ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

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
  node scripts/validate_target_gate_fixture.mjs "$gate_file"
)"

toolchain_available() {
  local toolchain="$1"
  command -v rustup >/dev/null 2>&1 || return 1
  rustup run "$toolchain" rustc --version >/dev/null 2>&1
}

target_std_available() {
  local toolchain="$1"
  local target="$2"
  local mode="$3"
  case "$mode" in
    rustup_component)
      rustup target list --installed --toolchain "$toolchain" 2>/dev/null \
        | grep -Fx "$target" >/dev/null
      ;;
    build_std)
      rustc +"$toolchain" --print target-list 2>/dev/null | grep -Fx "$target" >/dev/null || return 1
      local sysroot
      sysroot="$(rustc +"$toolchain" --print sysroot 2>/dev/null)" || return 1
      [[ -f "$sysroot/lib/rustlib/src/rust/library/Cargo.toml" ]]
      ;;
    *)
      return 1
      ;;
  esac
}

c_toolchain_available() {
  local kind="$1"
  local compiler="$2"
  local archiver="$3"
  local linker="$4"
  local target="$5"
  case "$kind" in
    none)
      return 0
      ;;
    gnu_glibc)
      command -v "$compiler" >/dev/null 2>&1 || return 1
      command -v "$archiver" >/dev/null 2>&1 || return 1
      command -v "$linker" >/dev/null 2>&1 || return 1
      local sysroot
      sysroot="$("$compiler" --print-sysroot 2>/dev/null)" || return 1
      [[ -n "$sysroot" && -d "$sysroot" ]] || return 1
      local machine
      machine="$("$compiler" -dumpmachine 2>/dev/null)" || return 1
      case "$target:$machine" in
        aarch64-unknown-linux-gnu:aarch64*-linux-gnu*|x86_64-unknown-linux-gnu:x86_64*-linux-gnu*)
          ;;
        *)
          return 1
          ;;
      esac
      ;;
    xwin_msvc)
      command -v cargo-xwin >/dev/null 2>&1 || return 1
      command -v "$compiler" >/dev/null 2>&1 || return 1
      command -v "$archiver" >/dev/null 2>&1 || return 1
      command -v "$linker" >/dev/null 2>&1 || return 1
      xwin_cache_available "${XWIN_CACHE_DIR:-}"
      ;;
    *)
      return 1
      ;;
  esac
}

xwin_cache_has_file() {
  local directory="$1"
  local filename="$2"
  [[ -d "$directory" ]] || return 1
  find "$directory" -type f -size +0c -iname "$filename" -print -quit 2>/dev/null \
    | grep -q .
}

xwin_cache_available() {
  local cache_dir="$1"
  [[ "$cache_dir" == /* && -d "$cache_dir" ]] || return 1
  local xwin_dir="$cache_dir/xwin"
  local done_file="$xwin_dir/DONE"
  [[ -s "$done_file" ]] || return 1
  local arches
  IFS= read -r arches <"$done_file" || [[ -n "$arches" ]] || return 1
  [[ " $arches " == *" x86_64 "* ]] || return 1

  xwin_cache_has_file "$xwin_dir/crt/include" "vcruntime.h" || return 1
  xwin_cache_has_file "$xwin_dir/sdk/include/ucrt" "stdio.h" || return 1
  xwin_cache_has_file "$xwin_dir/sdk/include/um" "windows.h" || return 1
  xwin_cache_has_file "$xwin_dir/sdk/include/shared" "winerror.h" || return 1
  xwin_cache_has_file "$xwin_dir/crt/lib/x86_64" "vcruntime.lib" || return 1
  xwin_cache_has_file "$xwin_dir/sdk/lib/um/x86_64" "kernel32.lib" || return 1
  xwin_cache_has_file "$xwin_dir/sdk/lib/ucrt/x86_64" "ucrt.lib"
}

missing_target_report() {
  local profile="$1"
  local target="$2"
  local package="$3"
  local features="$4"
  local rust_toolchain="$5"
  local target_std_mode="$6"
  local build_std="$7"
  local executor_kind="$8"
  local c_toolchain_kind="$9"
  local reason="${10}"
  node scripts/validate_target_gate_fixture.mjs --missing-report \
    "$profile" \
    "$target" \
    "$package" \
    "$features" \
    "$rust_toolchain" \
    "$target_std_mode" \
    "$build_std" \
    "$executor_kind" \
    "$c_toolchain_kind" \
    "$reason" >&2
}

run_target_gate() (
  local profile="$1"
  local target="$2"
  local package="$3"
  local features="$4"
  local rust_toolchain="$5"
  local target_std_mode="$6"
  local build_std="$7"
  local executor_kind="$8"
  local c_toolchain_kind="$9"
  local c_compiler="${10}"
  local c_archiver="${11}"
  local c_linker="${12}"

  if [[ "$c_toolchain_kind" == "gnu_glibc" ]]; then
    local target_key="${target//-/_}"
    export "CC_${target_key}=$c_compiler"
    export "AR_${target_key}=$c_archiver"
    local cargo_target_key
    cargo_target_key="$(printf '%s' "$target_key" | tr '[:lower:]' '[:upper:]')"
    export "CARGO_TARGET_${cargo_target_key}_LINKER=$c_linker"
  fi

  local args=(+"$rust_toolchain")
  case "$executor_kind" in
    cargo)
      args+=(check --locked)
      ;;
    cargo_xwin)
      export XWIN_CROSS_COMPILER=clang-cl
      export XWIN_ARCH=x86_64
      args+=(xwin check --locked)
      ;;
    *)
      return 1
      ;;
  esac
  if [[ "$target_std_mode" == "build_std" ]]; then
    args+=("-Zbuild-std=$build_std")
  fi
  args+=(
    -p "$package"
    --target "$target"
    --no-default-features
    --features "$features"
  )
  command cargo "${args[@]}"
)

run_replay_gate() (
  local target="$1"
  local rust_toolchain="$2"
  local executor_kind="$3"
  local c_toolchain_kind="$4"
  local c_compiler="$5"
  local c_archiver="$6"
  local c_linker="$7"

  if [[ "$c_toolchain_kind" == "gnu_glibc" ]]; then
    local target_key="${target//-/_}"
    export "CC_${target_key}=$c_compiler"
    export "AR_${target_key}=$c_archiver"
    local cargo_target_key
    cargo_target_key="$(printf '%s' "$target_key" | tr '[:lower:]' '[:upper:]')"
    export "CARGO_TARGET_${cargo_target_key}_LINKER=$c_linker"
  fi
  export RUSTFLAGS="-D warnings"
  case "$executor_kind" in
    cargo)
      command cargo +"$rust_toolchain" check --locked -p bm-replay \
        --target "$target" --all-targets --no-default-features
      ;;
    cargo_xwin)
      export XWIN_CROSS_COMPILER=clang-cl
      export XWIN_ARCH=x86_64
      command cargo +"$rust_toolchain" xwin check --locked -p bm-replay \
        --target "$target" --all-targets --no-default-features
      ;;
    *)
      return 1
      ;;
  esac
)

if [[ "$mode" == "--preflight" || "$mode" == "--strict" ]]; then
  preflight_status=0
  while IFS=$'\t' read -r profile target package features rust_toolchain target_std_mode build_std executor_kind c_toolchain_kind c_compiler c_archiver c_linker _replay_all_targets _gateway_check; do
    [[ -n "$profile" ]] || continue

    if ! toolchain_available "$rust_toolchain"; then
      missing_target_report "$profile" "$target" "$package" "$features" \
        "$rust_toolchain" "$target_std_mode" "$build_std" "$executor_kind" "$c_toolchain_kind" \
        "rust_toolchain_unavailable"
      preflight_status=1
      continue
    fi

    if ! target_std_available "$rust_toolchain" "$target" "$target_std_mode"; then
      missing_target_report "$profile" "$target" "$package" "$features" \
        "$rust_toolchain" "$target_std_mode" "$build_std" "$executor_kind" "$c_toolchain_kind" \
        "target_std_unavailable"
      preflight_status=1
      continue
    fi

    if ! c_toolchain_available "$c_toolchain_kind" "$c_compiler" "$c_archiver" "$c_linker" "$target"; then
      missing_target_report "$profile" "$target" "$package" "$features" \
        "$rust_toolchain" "$target_std_mode" "$build_std" "$executor_kind" "$c_toolchain_kind" \
        "c_toolchain_unavailable"
      preflight_status=1
    fi
  done <<< "$gate_rows"

  if [[ "$preflight_status" -ne 0 ]]; then
    exit "$preflight_status"
  fi
  if [[ "$mode" == "--preflight" ]]; then
    echo "OK: strict target toolchain preflight passed"
    exit 0
  fi
fi

while IFS=$'\t' read -r profile _target package features _rust_toolchain _target_std_mode _build_std _executor_kind _c_toolchain_kind _c_compiler _c_archiver _c_linker _replay_all_targets gateway_check; do
  cargo check -p "$package" --no-default-features --features "$features"
  if [[ "$gateway_check" == "true" ]]; then
    cargo check -p bm-llm-gateway --no-default-features --features "$profile"
  fi
done <<< "$gate_rows"

if [[ "$mode" == "--host-only" ]]; then
  echo "OK: host platform compile gates passed"
  exit 0
fi

status=0
while IFS=$'\t' read -r profile target package features rust_toolchain target_std_mode build_std executor_kind c_toolchain_kind c_compiler c_archiver c_linker replay_all_targets _gateway_check; do
  [[ -n "$profile" ]] || continue
  if ! run_target_gate "$profile" "$target" "$package" "$features" \
    "$rust_toolchain" "$target_std_mode" "$build_std" "$executor_kind" "$c_toolchain_kind" \
    "$c_compiler" "$c_archiver" "$c_linker"; then
    echo "target compile gate failed: profile=$profile target=$target package=$package features=$features" >&2
    status=1
  fi

  if [[ "$replay_all_targets" == "true" ]] \
    && ! run_replay_gate "$target" "$rust_toolchain" "$executor_kind" "$c_toolchain_kind" "$c_compiler" "$c_archiver" "$c_linker"; then
    echo "P7 replay target compile gate failed: profile=$profile target=$target" >&2
    status=1
  fi
done <<< "$gate_rows"

if [[ "$status" -ne 0 ]]; then
  exit "$status"
fi

echo "OK: strict target platform compile gates passed"
