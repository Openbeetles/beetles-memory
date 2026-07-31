#!/usr/bin/env bash
set -euo pipefail

tool="${0##*/}"
case "$tool" in
  cargo)
    if [[ " $* " == *" xwin check "* ]]; then
      [[ "${XWIN_ARCH:-}" == "x86_64" ]]
      [[ "${XWIN_CACHE_DIR:-}" == /* ]]
    fi
    printf 'FAKE_CARGO\t%s\n' "$*"
    ;;
  rustup)
    if [[ "${1:-}" == "run" ]]; then
      printf 'rustc 1.0.0-fake\n'
    elif [[ "${1:-}" == "target" && "${2:-}" == "list" ]]; then
      printf '%s\n' \
        aarch64-unknown-linux-gnu \
        aarch64-apple-darwin \
        x86_64-pc-windows-msvc \
        x86_64-unknown-linux-gnu
    else
      exit 1
    fi
    ;;
  rustc)
    if [[ "${2:-}" == "--print" && "${3:-}" == "target-list" ]]; then
      printf 'xtensa-esp32s3-espidf\n'
    elif [[ "${2:-}" == "--print" && "${3:-}" == "sysroot" ]]; then
      printf '%s\n' "${BM_TARGET_GATE_FAKE_RUST_SYSROOT:?}"
    else
      exit 1
    fi
    ;;
  aarch64-linux-gnu-gcc)
    if [[ "${1:-}" == "--print-sysroot" ]]; then
      printf '%s\n' "${BM_TARGET_GATE_FAKE_GNU_SYSROOT:?}"
    elif [[ "${1:-}" == "-dumpmachine" ]]; then
      printf 'aarch64-linux-gnu\n'
    else
      exit 1
    fi
    ;;
  x86_64-linux-gnu-gcc)
    if [[ "${1:-}" == "--print-sysroot" ]]; then
      printf '%s\n' "${BM_TARGET_GATE_FAKE_GNU_SYSROOT:?}"
    elif [[ "${1:-}" == "-dumpmachine" ]]; then
      printf 'x86_64-linux-gnu\n'
    else
      exit 1
    fi
    ;;
  aarch64-linux-gnu-ar|x86_64-linux-gnu-ar|cargo-xwin|clang-cl|llvm-lib|lld-link)
    ;;
  *)
    exit 1
    ;;
esac
