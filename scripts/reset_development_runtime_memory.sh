#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
repo_root="$(pwd -P)"

usage() {
  cat <<'EOF'
Usage:
  bash scripts/reset_development_runtime_memory.sh --target <path> [--backend auto|file|sqlite] [--dry-run]
  bash scripts/reset_development_runtime_memory.sh --target <path> [--backend auto|file|sqlite] --apply --confirm-development-reset

This is a development-only operator tool. It resets the active runtime memory
store path by moving the current data to a timestamped backup. Dry-run is the
default. No data is moved unless both --apply and --confirm-development-reset
are present.
EOF
}

fail() {
  echo "reset_development_runtime_memory: $*" >&2
  exit 1
}

target=""
backend="auto"
mode="dry-run"
confirmed="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      shift
      [[ $# -gt 0 ]] || fail "--target requires a path"
      target="$1"
      ;;
    --backend)
      shift
      [[ $# -gt 0 ]] || fail "--backend requires auto, file, or sqlite"
      backend="$1"
      ;;
    --dry-run)
      mode="dry-run"
      ;;
    --apply)
      mode="apply"
      ;;
    --confirm-development-reset)
      confirmed="true"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
  shift
done

[[ -n "$target" ]] || fail "--target is required"
case "$backend" in
  auto|file|sqlite) ;;
  *) fail "--backend must be auto, file, or sqlite" ;;
esac
if [[ "$mode" == "apply" && "$confirmed" != "true" ]]; then
  fail "--apply requires --confirm-development-reset"
fi

canonical_path() {
  local path="$1"
  if [[ -d "$path" ]]; then
    (cd "$path" && pwd -P)
    return
  fi
  local parent
  local base
  parent="$(dirname "$path")"
  base="$(basename "$path")"
  [[ -d "$parent" ]] || fail "target parent does not exist: $parent"
  printf '%s/%s\n' "$(cd "$parent" && pwd -P)" "$base"
}

[[ -e "$target" ]] || fail "target does not exist: $target"
target_abs="$(canonical_path "$target")"

refuse_target() {
  local path="$1"
  [[ "$path" != "/" ]] || fail "refusing to reset filesystem root"
  [[ "$path" != "$HOME" ]] || fail "refusing to reset HOME"
  [[ "$path" != "$repo_root" ]] || fail "refusing to reset repository root"

  case "$path" in
    "$repo_root/.git"|"$repo_root/.git/"*) fail "refusing to reset git metadata" ;;
    "$repo_root/dev-docs"|"$repo_root/dev-docs/"*) fail "refusing to reset dev-docs truth source" ;;
    "$repo_root/fixtures"|"$repo_root/fixtures/"*) fail "refusing to reset fixtures truth source" ;;
    "$repo_root/crates"|"$repo_root/crates/"*) fail "refusing to reset source crates" ;;
    "$repo_root/apps"|"$repo_root/apps/"*) fail "refusing to reset app source" ;;
    "$repo_root/scripts"|"$repo_root/scripts/"*) fail "refusing to reset scripts" ;;
    "$repo_root/Cargo.toml"|"$repo_root/Cargo.lock") fail "refusing to reset source files" ;;
  esac

  if [[ "$path" == "$repo_root/"* && "$path" != "$repo_root/target/"* ]]; then
    fail "repository-local reset targets must live under target/"
  fi
}

detect_backend() {
  if [[ "$backend" != "auto" ]]; then
    printf '%s\n' "$backend"
  elif [[ -d "$target_abs" ]]; then
    printf '%s\n' "file"
  elif [[ -f "$target_abs" ]]; then
    printf '%s\n' "sqlite"
  else
    fail "cannot detect backend for target: $target"
  fi
}

refuse_target "$target_abs"
backend="$(detect_backend)"

count_file_store_entries() {
  find "$1" -mindepth 1 | wc -l | tr -d '[:space:]'
}

validate_file_store_target() {
  [[ -d "$target_abs" ]] || fail "file backend target must be a directory"
  local evidence=0
  for name in manifest.json events kv blob snapshots; do
    if [[ -e "$target_abs/$name" ]]; then
      evidence=$((evidence + 1))
    fi
  done
  [[ "$evidence" -ge 2 ]] || fail "target does not look like a Beetle Memory file store"
}

count_sqlite_entries() {
  local count=0
  for suffix in "" "-wal" "-shm"; do
    if [[ -e "$target_abs$suffix" ]]; then
      count=$((count + 1))
    fi
  done
  printf '%s\n' "$count"
}

validate_sqlite_target() {
  [[ -f "$target_abs" ]] || fail "sqlite backend target must be a file"
  case "$target_abs" in
    *.sqlite|*.sqlite3|*.db) ;;
    *) fail "sqlite target must use .sqlite, .sqlite3, or .db extension" ;;
  esac
}

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"

case "$backend" in
  file)
    validate_file_store_target
    before_entries="$(count_file_store_entries "$target_abs")"
    printf 'mode=%s\n' "$mode"
    printf 'backend=file\n'
    printf 'target=%s\n' "$target"
    printf 'canonical_target=%s\n' "$target_abs"
    printf 'before_entries=%s\n' "$before_entries"
    if [[ "$mode" == "dry-run" ]]; then
      printf 'action=none\n'
      printf 'confirmation_required=--apply --confirm-development-reset\n'
      exit 0
    fi
    backup_path="${target_abs}.reset-backup-${timestamp}"
    if [[ -e "$backup_path" ]]; then
      backup_path="${backup_path}-$$"
    fi
    mv "$target_abs" "$backup_path"
    mkdir -p "$target_abs"
    after_entries="$(count_file_store_entries "$target_abs")"
    printf 'backup_path=%s\n' "$backup_path"
    printf 'after_entries=%s\n' "$after_entries"
    ;;
  sqlite)
    validate_sqlite_target
    before_entries="$(count_sqlite_entries)"
    printf 'mode=%s\n' "$mode"
    printf 'backend=sqlite\n'
    printf 'target=%s\n' "$target"
    printf 'canonical_target=%s\n' "$target_abs"
    printf 'before_entries=%s\n' "$before_entries"
    if [[ "$mode" == "dry-run" ]]; then
      printf 'action=none\n'
      printf 'confirmation_required=--apply --confirm-development-reset\n'
      exit 0
    fi
    backup_path="${target_abs}.reset-backup-${timestamp}"
    if [[ -e "$backup_path" ]]; then
      backup_path="${backup_path}-$$"
    fi
    mkdir -p "$backup_path"
    for suffix in "" "-wal" "-shm"; do
      if [[ -e "$target_abs$suffix" ]]; then
        mv "$target_abs$suffix" "$backup_path/$(basename "$target_abs$suffix")"
      fi
    done
    after_entries="$(count_sqlite_entries)"
    printf 'backup_path=%s\n' "$backup_path"
    printf 'after_entries=%s\n' "$after_entries"
    ;;
esac
