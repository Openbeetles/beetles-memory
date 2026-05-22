#!/usr/bin/env bash

if ! command -v rg >/dev/null 2>&1; then
  echo "contract checks require ripgrep (rg)" >&2
  exit 1
fi

contract_rg_match() {
  local pattern="$1"
  shift

  local hit_file
  local err_file
  local status

  hit_file="$(mktemp "${TMPDIR:-/tmp}/bm-contract-hit.XXXXXX")"
  err_file="$(mktemp "${TMPDIR:-/tmp}/bm-contract-err.XXXXXX")"

  if rg -n -- "$pattern" "$@" >"$hit_file" 2>"$err_file"; then
    cat "$hit_file" >&2
    rm -f "$hit_file" "$err_file"
    return 0
  else
    status=$?
  fi

  if [[ "$status" -eq 1 ]]; then
    rm -f "$hit_file" "$err_file"
    return 1
  fi

  cat "$err_file" >&2
  rm -f "$hit_file" "$err_file"
  echo "FAIL: rg search failed with status $status for pattern: $pattern" >&2
  exit "$status"
}

contract_manifest_has_package_dependency() {
  local manifest="$1"
  local package_pattern="$2"
  local pattern
  local patterns=(
    "^[[:space:]]*(${package_pattern})([.][A-Za-z0-9_-]+)?[[:space:]]*="
    "^[[:space:]]*\"(${package_pattern})\"([.][A-Za-z0-9_-]+)?[[:space:]]*="
    "^[[:space:]]*'(${package_pattern})'([.][A-Za-z0-9_-]+)?[[:space:]]*="
    "^[[:space:]]*\\[(dependencies|dev-dependencies|build-dependencies)\\.(${package_pattern})\\]"
    "^[[:space:]]*\\[(dependencies|dev-dependencies|build-dependencies)\\.\"(${package_pattern})\"\\]"
    "^[[:space:]]*\\[(dependencies|dev-dependencies|build-dependencies)\\.'(${package_pattern})'\\]"
    "^[[:space:]]*\\[target\\..*\\.(dependencies|dev-dependencies|build-dependencies)\\.(${package_pattern})\\]"
    "^[[:space:]]*\\[target\\..*\\.(dependencies|dev-dependencies|build-dependencies)\\.\"(${package_pattern})\"\\]"
    "^[[:space:]]*\\[target\\..*\\.(dependencies|dev-dependencies|build-dependencies)\\.'(${package_pattern})'\\]"
    "package[[:space:]]*=[[:space:]]*\"(${package_pattern})\""
    "package[[:space:]]*=[[:space:]]*'(${package_pattern})'"
  )

  for pattern in "${patterns[@]}"; do
    if contract_rg_match "$pattern" "$manifest"; then
      return 0
    fi
  done

  return 1
}

contract_manifest_has_core_store_dependency() {
  contract_manifest_has_package_dependency "$1" 'bm-core|bm-store'
}

contract_manifest_has_protocol_listener_dependency() {
  contract_manifest_has_package_dependency "$1" 'tokio|hyper|axum|warp|tungstenite'
}
