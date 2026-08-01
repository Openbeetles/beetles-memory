#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/beetle-target-gate-dispatch.XXXXXX")"
trap 'rm -rf -- "$tmp_root"' EXIT

fake_bin="$tmp_root/bin"
fake_rust_sysroot="$tmp_root/rust-sysroot"
fake_gnu_sysroot="$tmp_root/gnu-sysroot"
fake_xwin_cache="$tmp_root/xwin-cache"
output="$tmp_root/output.log"
mkdir -p "$fake_bin"
mkdir -p "$fake_rust_sysroot/lib/rustlib/src/rust/library"
mkdir -p "$fake_gnu_sysroot"
mkdir -p "$fake_xwin_cache/xwin/crt/include"
mkdir -p "$fake_xwin_cache/xwin/sdk/include/ucrt"
mkdir -p "$fake_xwin_cache/xwin/sdk/include/um"
mkdir -p "$fake_xwin_cache/xwin/sdk/include/shared"
mkdir -p "$fake_xwin_cache/xwin/crt/lib/x86_64"
mkdir -p "$fake_xwin_cache/xwin/sdk/lib/um/x86_64"
mkdir -p "$fake_xwin_cache/xwin/sdk/lib/ucrt/x86_64"
touch "$fake_rust_sysroot/lib/rustlib/src/rust/library/Cargo.toml"
for required_file in \
  "$fake_xwin_cache/xwin/crt/include/vcruntime.h" \
  "$fake_xwin_cache/xwin/sdk/include/ucrt/stdio.h" \
  "$fake_xwin_cache/xwin/sdk/include/um/Windows.h" \
  "$fake_xwin_cache/xwin/sdk/include/shared/winerror.h" \
  "$fake_xwin_cache/xwin/crt/lib/x86_64/vcruntime.lib" \
  "$fake_xwin_cache/xwin/sdk/lib/um/x86_64/kernel32.lib" \
  "$fake_xwin_cache/xwin/sdk/lib/ucrt/x86_64/ucrt.lib"
do
  printf 'fixture\n' >"$required_file"
done
printf 'x86_64' >"$fake_xwin_cache/xwin/DONE"

fake_tool="$PWD/scripts/tests/fixtures/fake_target_tool.sh"
for tool in \
  cargo \
  rustup \
  rustc \
  aarch64-linux-gnu-gcc \
  aarch64-linux-gnu-ar \
  x86_64-linux-gnu-gcc \
  x86_64-linux-gnu-ar \
  cargo-xwin \
  clang-cl \
  llvm-lib \
  lld-link
do
  ln -s "$fake_tool" "$fake_bin/$tool"
done

BM_TARGET_GATE_FAKE_RUST_SYSROOT="$fake_rust_sysroot" \
BM_TARGET_GATE_FAKE_GNU_SYSROOT="$fake_gnu_sysroot" \
XWIN_CACHE_DIR="$fake_xwin_cache" \
PATH="$fake_bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin" \
  bash scripts/check_cross_target_compile_gates.sh --strict >"$output"

[[ "$(grep -c '^FAKE_CARGO' "$output")" -eq 26 ]]
[[ "$(grep -c $'^FAKE_CARGO\t--locked check -p bm-sdk' "$output")" -eq 11 ]]
[[ "$(grep -c $'^FAKE_CARGO\t--locked check -p bm-llm-gateway' "$output")" -eq 3 ]]
[[ "$(grep -c 'xwin check --locked -p bm-sdk' "$output")" -eq 2 ]]
[[ "$(grep -c 'xwin check --locked -p bm-replay' "$output")" -eq 1 ]]
[[ "$(grep -c -- '--target x86_64-unknown-linux-gnu' "$output")" -eq 4 ]]
[[ "$(grep -c -- '--target aarch64-unknown-linux-gnu' "$output")" -eq 1 ]]
[[ "$(grep -c -- '--target xtensa-esp32s3-espidf' "$output")" -eq 2 ]]
grep -Fx 'OK: strict target platform compile gates passed' "$output" >/dev/null
if grep -F 'WindowsSdkDir' "$output" >/dev/null; then
  echo "fake dispatch must not consume WindowsSdkDir" >&2
  exit 1
fi

for source in \
  scripts/check_cross_target_compile_gates.sh \
  scripts/validate_target_gate_fixture.mjs \
  fixtures/platform/target-gates.json
do
  if grep -F 'WindowsSdkDir' "$source" >/dev/null; then
    echo "target gate source must not read WindowsSdkDir: $source" >&2
    exit 1
  fi
done

for missing_tool in cargo-xwin clang-cl llvm-lib lld-link
do
  missing_output="$tmp_root/missing-$missing_tool.log"
  mv "$fake_bin/$missing_tool" "$tmp_root/$missing_tool.disabled"
  set +e
  WindowsSdkDir=/does/not/exist \
  BM_TARGET_GATE_FAKE_RUST_SYSROOT="$fake_rust_sysroot" \
  BM_TARGET_GATE_FAKE_GNU_SYSROOT="$fake_gnu_sysroot" \
  XWIN_CACHE_DIR="$fake_xwin_cache" \
  PATH="$fake_bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin" \
    bash scripts/check_cross_target_compile_gates.sh --preflight \
      >"$missing_output" 2>&1
  missing_status="$?"
  set -e
  mv "$tmp_root/$missing_tool.disabled" "$fake_bin/$missing_tool"

  [[ "$missing_status" -eq 1 ]]
  [[ "$(grep -c '"reason":"c_toolchain_unavailable"' "$missing_output")" -eq 2 ]]
  [[ "$(grep -c '"executor_kind":"cargo_xwin"' "$missing_output")" -eq 2 ]]
  if grep -F 'FAKE_CARGO' "$missing_output" >/dev/null; then
    echo "preflight failure must occur before Cargo: $missing_tool" >&2
    exit 1
  fi
done

strict_missing_output="$tmp_root/strict-missing.log"
mv "$fake_bin/cargo-xwin" "$tmp_root/cargo-xwin.disabled"
set +e
WindowsSdkDir=/does/not/exist \
BM_TARGET_GATE_FAKE_RUST_SYSROOT="$fake_rust_sysroot" \
BM_TARGET_GATE_FAKE_GNU_SYSROOT="$fake_gnu_sysroot" \
XWIN_CACHE_DIR="$fake_xwin_cache" \
PATH="$fake_bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin" \
  bash scripts/check_cross_target_compile_gates.sh --strict \
    >"$strict_missing_output" 2>&1
strict_missing_status="$?"
set -e
[[ "$strict_missing_status" -eq 1 ]]
if grep -F 'FAKE_CARGO' "$strict_missing_output" >/dev/null; then
  echo "strict preflight failure must occur before host/target Cargo" >&2
  exit 1
fi
mv "$tmp_root/cargo-xwin.disabled" "$fake_bin/cargo-xwin"

assert_xwin_cache_preflight_fails() {
  local case_name="$1"
  local cache_mode="$2"
  local cache_output="$tmp_root/cache-missing-$case_name.log"

  set +e
  if [[ "$cache_mode" == "unset" ]]; then
    env -u XWIN_CACHE_DIR \
      WindowsSdkDir=/does/not/exist \
      BM_TARGET_GATE_FAKE_RUST_SYSROOT="$fake_rust_sysroot" \
      BM_TARGET_GATE_FAKE_GNU_SYSROOT="$fake_gnu_sysroot" \
      PATH="$fake_bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin" \
        bash scripts/check_cross_target_compile_gates.sh --preflight \
          >"$cache_output" 2>&1
  else
    WindowsSdkDir=/does/not/exist \
    BM_TARGET_GATE_FAKE_RUST_SYSROOT="$fake_rust_sysroot" \
    BM_TARGET_GATE_FAKE_GNU_SYSROOT="$fake_gnu_sysroot" \
    XWIN_CACHE_DIR="$cache_mode" \
    PATH="$fake_bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin" \
      bash scripts/check_cross_target_compile_gates.sh --preflight \
        >"$cache_output" 2>&1
  fi
  local cache_status="$?"
  set -e

  [[ "$cache_status" -eq 1 ]]
  [[ "$(grep -c '"reason":"c_toolchain_unavailable"' "$cache_output")" -eq 2 ]]
  if grep -F 'FAKE_CARGO' "$cache_output" >/dev/null; then
    echo "xwin cache preflight failure must occur before Cargo: $case_name" >&2
    exit 1
  fi
  while IFS= read -r report
  do
    node -e 'JSON.parse(process.argv[1])' "$report"
  done <"$cache_output"
}

assert_xwin_cache_preflight_fails unset unset
assert_xwin_cache_preflight_fails relative relative-cache

mv "$fake_xwin_cache/xwin/DONE" "$tmp_root/DONE.disabled"
assert_xwin_cache_preflight_fails missing-done "$fake_xwin_cache"
mv "$tmp_root/DONE.disabled" "$fake_xwin_cache/xwin/DONE"

printf 'aarch64' >"$fake_xwin_cache/xwin/DONE"
assert_xwin_cache_preflight_fails wrong-arch "$fake_xwin_cache"
printf 'x86_64' >"$fake_xwin_cache/xwin/DONE"

cache_case=0
for required_path in \
  crt/include/vcruntime.h \
  sdk/include/ucrt/stdio.h \
  sdk/include/um/Windows.h \
  sdk/include/shared/winerror.h \
  crt/lib/x86_64/vcruntime.lib \
  sdk/lib/um/x86_64/kernel32.lib \
  sdk/lib/ucrt/x86_64/ucrt.lib
do
  cache_case="$((cache_case + 1))"
  mv \
    "$fake_xwin_cache/xwin/$required_path" \
    "$tmp_root/cache-file-$cache_case.disabled"
  assert_xwin_cache_preflight_fails "missing-file-$cache_case" "$fake_xwin_cache"
  mv \
    "$tmp_root/cache-file-$cache_case.disabled" \
    "$fake_xwin_cache/xwin/$required_path"
done

echo "OK: cross-target compile gate dispatch tests passed"
