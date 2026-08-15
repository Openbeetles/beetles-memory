#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

fail() {
  echo "P5 cleanup contract failed: $*" >&2
  exit 1
}

expect_fail() {
  local output_file="$1"
  shift
  if "$@" >"$output_file" 2>&1; then
    cat "$output_file" >&2
    fail "command unexpectedly succeeded: $*"
  fi
}

test -f scripts/reset_development_runtime_memory.sh

if rg -n "sdk_projection_text_parts|render_sdk_projection_block|legacy_flat_projection|flat_projection_compat|use_flat_projection" crates/sdk/src crates/llm-gateway/src; then
  fail "old flat projection renderer or compatibility switch is still present"
fi

if rg -n "没有 commit 当前 user/assistant turn|GatewayMaintenancePlan 只把|MemoryRuntime::maintain\(\) 会读取" dev-docs/post-turn-memory-governance-plan.md; then
  fail "post-turn plan still treats the closed gateway commit gap as current"
fi
rg -Fq "project -> upstream -> finalize_turn" dev-docs/post-turn-memory-governance-plan.md

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/bm-p5-reset-contract.XXXXXX")"
trap 'chmod -R u+w "$tmp_root" 2>/dev/null || true; rm -rf "$tmp_root"' EXIT
out="$tmp_root/out.txt"

store="$tmp_root/runtime-store"
mkdir -p "$store/kv/session" "$store/blob/private_garden" "$store/events" "$store/snapshots"
printf '{"schema_id":"beetle_memory_store_v1"}\n' >"$store/manifest.json"
printf '{"turn":"old"}\n' >"$store/kv/session/chat.json"
printf '{"event":"old"}\n' >"$store/events/events.jsonl"

dry_output="$(bash scripts/reset_development_runtime_memory.sh --target "$store" --backend file 2>&1)"
printf '%s\n' "$dry_output" >"$out"
grep -Fq "mode=dry-run" "$out"
grep -Fq "backend=file" "$out"
grep -Fq "target=$store" "$out"
grep -Fq "before_entries=" "$out"
test -f "$store/kv/session/chat.json"
test -f "$store/events/events.jsonl"

expect_fail "$out" bash scripts/reset_development_runtime_memory.sh --backend file
expect_fail "$out" bash scripts/reset_development_runtime_memory.sh --target "$store" --backend file --apply
expect_fail "$out" bash scripts/reset_development_runtime_memory.sh --target "$PWD" --backend file
expect_fail "$out" bash scripts/reset_development_runtime_memory.sh --target "$PWD/dev-docs" --backend file
expect_fail "$out" bash scripts/reset_development_runtime_memory.sh --target "$PWD/fixtures" --backend file

apply_output="$(bash scripts/reset_development_runtime_memory.sh --target "$store" --backend file --apply --confirm-development-reset 2>&1)"
printf '%s\n' "$apply_output" >"$out"
grep -Fq "mode=apply" "$out"
grep -Fq "backend=file" "$out"
grep -Fq "target=$store" "$out"
grep -Fq "backup_path=" "$out"
grep -Fq "after_entries=0" "$out"
test -d "$store"
test ! -e "$store/kv/session/chat.json"
test ! -e "$store/events/events.jsonl"

sqlite="$tmp_root/memory.sqlite3"
printf 'sqlite-store' >"$sqlite"
printf 'wal' >"$sqlite-wal"
printf 'shm' >"$sqlite-shm"

sqlite_dry_output="$(bash scripts/reset_development_runtime_memory.sh --target "$sqlite" --backend sqlite 2>&1)"
printf '%s\n' "$sqlite_dry_output" >"$out"
grep -Fq "mode=dry-run" "$out"
grep -Fq "backend=sqlite" "$out"
grep -Fq "target=$sqlite" "$out"
test -f "$sqlite"
test -f "$sqlite-wal"
test -f "$sqlite-shm"

sqlite_apply_output="$(bash scripts/reset_development_runtime_memory.sh --target "$sqlite" --backend sqlite --apply --confirm-development-reset 2>&1)"
printf '%s\n' "$sqlite_apply_output" >"$out"
grep -Fq "mode=apply" "$out"
grep -Fq "backend=sqlite" "$out"
grep -Fq "backup_path=" "$out"
test ! -e "$sqlite"
test ! -e "$sqlite-wal"
test ! -e "$sqlite-shm"

cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test projection_audit_contract empty_store_projection_degrades_subject_mount_without_inventing_personality
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test projection_audit_contract empty_store_greeting_projection_does_not_leak_identity_meta_terms
cargo test --locked -p bm-sdk --features nonproduction-replay-harness --test sdk_runtime_flow runtime_projection_includes_private_planes_when_policy_allows_it
