#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

required_files=(
  "dev-docs/conversation-transcript-attribute-plane-plan.md"
  "dev-docs/README.md"
  "docs/en/api.md"
  "docs/zh-CN/api.md"
  "docs/en/replay-and-archive.md"
  "docs/zh-CN/replay-and-archive.md"
  "docs/en/cli-usage.md"
  "docs/zh-CN/cli-usage.md"
  "docs/en/operator-guide.md"
  "docs/zh-CN/operator-guide.md"
)

for file in "${required_files[@]}"; do
  if [[ ! -s "$file" ]]; then
    echo "missing transcript attr release-surface file: $file" >&2
    exit 1
  fi
done

require_fixed() {
  local needle="$1"
  shift
  if ! rg -F -q "$needle" "$@"; then
    echo "missing required transcript attr term: $needle" >&2
    printf '  files: %s\n' "$*" >&2
    exit 1
  fi
}

require_regex() {
  local needle="$1"
  shift
  if ! rg -q "$needle" "$@"; then
    echo "missing required transcript attr pattern: $needle" >&2
    printf '  files: %s\n' "$*" >&2
    exit 1
  fi
}

require_fixed "conversation-transcript-attribute-plane-plan.md" dev-docs/README.md
require_fixed "TranscriptAttrEnvelope" crates/core/src/memory/transcript.rs crates/core/src/memory/mod.rs crates/sdk/src/lib.rs docs/en/api.md docs/zh-CN/api.md
require_fixed "TranscriptAttrScope" crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs dev-docs/conversation-transcript-attribute-plane-plan.md
require_fixed "TranscriptAttrValueKind" crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs docs/en/api.md docs/zh-CN/api.md
require_fixed "TranscriptAttrSource" crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs dev-docs/conversation-transcript-attribute-plane-plan.md
require_fixed "TranscriptAttrGovernance" crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs dev-docs/conversation-transcript-attribute-plane-plan.md
require_fixed "TranscriptAttrLink" crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs dev-docs/conversation-transcript-attribute-plane-plan.md
require_fixed "TranscriptAttrWriteReport" crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs
require_fixed "attrs: Vec<TranscriptAttrEnvelope>" crates/core/src/memory/transcript.rs
require_fixed "attr_id" crates/core/src/memory/transcript.rs docs/en/replay-and-archive.md docs/zh-CN/replay-and-archive.md
require_fixed "attr_key" crates/core/src/memory/transcript.rs docs/en/replay-and-archive.md docs/zh-CN/replay-and-archive.md
require_fixed "AttrVisibility" crates/core/src/memory/transcript.rs
require_fixed "AttrValueBudget" crates/core/src/memory/transcript.rs
require_fixed "AttrLifecyclePolicy" crates/core/src/memory/transcript.rs
require_fixed "conversation_transcript_attr" crates/sdk/src/store_internal/platform.rs crates/sdk/src/lib.rs docs/en/api.md docs/zh-CN/api.md docs/en/operator-guide.md docs/zh-CN/operator-guide.md
require_fixed "upsert_transcript_attrs" crates/core/src/memory/transcript.rs crates/sdk/src/store_internal/platform.rs
require_fixed "list_transcript_attrs" crates/core/src/memory/transcript.rs crates/sdk/src/store_internal/platform.rs
require_fixed "inspect_repair_records" crates/core/src/memory/transcript.rs crates/sdk/src/store_internal/platform.rs
require_fixed "MissingAttrTargetTurn" crates/core/src/memory/transcript.rs docs/en/replay-and-archive.md docs/zh-CN/replay-and-archive.md
require_fixed "MissingAttrTargetMessage" crates/core/src/memory/transcript.rs docs/en/replay-and-archive.md docs/zh-CN/replay-and-archive.md
require_fixed "MemoryTranscriptAttrWriteRequest" crates/sdk/src/ops.rs crates/sdk/src/lib.rs docs/en/api.md docs/zh-CN/api.md docs/en/cli-usage.md docs/zh-CN/cli-usage.md
require_fixed "MemoryTranscriptAttrWriteReport" crates/sdk/src/ops.rs crates/sdk/src/lib.rs docs/en/api.md docs/zh-CN/api.md
require_fixed "redactions_preview" crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/cli/src/lib.rs crates/http/src/lib.rs crates/wss/src/lib.rs crates/mcp/src/lib.rs crates/a2a/src/lib.rs docs/en/api.md docs/zh-CN/api.md docs/en/cli-usage.md docs/zh-CN/cli-usage.md
require_fixed "profile_budget_applied" crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/cli/src/lib.rs crates/http/src/lib.rs crates/wss/src/lib.rs crates/mcp/src/lib.rs crates/a2a/src/lib.rs docs/en/api.md docs/zh-CN/api.md
require_fixed "audit_event_id" crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/cli/src/lib.rs crates/http/src/lib.rs crates/wss/src/lib.rs crates/mcp/src/lib.rs crates/a2a/src/lib.rs docs/en/api.md docs/zh-CN/api.md docs/en/cli-usage.md docs/zh-CN/cli-usage.md
require_fixed "record_transcript_attrs" crates/sdk/src/runtime.rs crates/sdk/tests/conversation_transcript_runtime_contract.rs docs/en/api.md docs/zh-CN/api.md docs/en/operator-guide.md docs/zh-CN/operator-guide.md
require_fixed "max_attrs_per_turn" crates/core/src/budget.rs crates/sdk/src/runtime.rs
require_fixed "max_attrs_per_message" crates/core/src/budget.rs crates/sdk/src/runtime.rs crates/sdk/tests/runtime_budget_contract.rs
require_fixed "transcript_attr_budget_limits_visible_message_attrs_and_reports_redaction" crates/sdk/tests/runtime_budget_contract.rs
require_fixed "TranscriptRedactionReason::AttrValueBudget" crates/sdk/src/runtime.rs crates/sdk/tests/runtime_budget_contract.rs
require_fixed "AdapterOperation::TranscriptAttrWrite" crates/adapter/src/contract.rs crates/adapter/src/dispatch.rs crates/http/src/lib.rs crates/wss/src/lib.rs crates/mcp/src/lib.rs crates/a2a/src/lib.rs crates/cli/src/lib.rs
require_fixed "memory_transcript_attr_write" crates/mcp/src/lib.rs crates/mcp/tests/mcp_contract.rs
require_fixed "/memory/transcript/attrs" crates/http/src/lib.rs crates/http/tests/http_contract.rs
require_fixed "command.transcript.attrs" crates/wss/src/lib.rs crates/wss/tests/wss_contract.rs
require_fixed "memory_transcript_attr_write_request" crates/a2a/src/lib.rs crates/a2a/tests/a2a_contract.rs
require_fixed "transcript-attr-write" crates/cli/src/lib.rs crates/cli/tests/cli_contract.rs docs/en/cli-usage.md docs/zh-CN/cli-usage.md
require_fixed "runtime_records_transcript_attrs_and_replays_host_ui_message_usage" crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "memory_space_export_redacts_raw_conversation_transcript_by_default" crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "store_persists_transcript_attrs_and_replays_visible_message_attrs" crates/store-contract-tests/tests/conversation_transcript_store_contract.rs
require_fixed "store_replay_fails_closed_while_repair_reports_corrupt_transcript_attr_records" crates/store-contract-tests/tests/conversation_transcript_store_contract.rs
require_fixed "transcript_attrs_are_filtered_per_replay_view_and_attached_to_message" crates/core/tests/conversation_transcript_contract.rs
require_fixed "transcript_attrs_obey_mask_and_delete_raw_lifecycle_policies" crates/core/tests/conversation_transcript_contract.rs
require_fixed "host.beetle_agent.model_usage" dev-docs/conversation-transcript-attribute-plane-plan.md
require_fixed "not a host business object store" docs/en/api.md
require_fixed "不是宿主业务对象库" docs/zh-CN/api.md
require_fixed "raw prompts" docs/en/api.md docs/en/cli-usage.md
require_fixed "raw prompt" docs/zh-CN/api.md docs/zh-CN/cli-usage.md
require_fixed "provider secrets" docs/en/api.md docs/en/cli-usage.md
require_fixed "provider secret" docs/zh-CN/api.md docs/zh-CN/cli-usage.md
require_fixed "local file paths" docs/en/cli-usage.md
require_fixed "本地真实文件路径" docs/zh-CN/api.md docs/zh-CN/cli-usage.md

require_regex "HostUi.*attrs|attrs.*HostUi" docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-archive.md docs/zh-CN/replay-and-archive.md
require_regex "ModelContext.*attrs|attrs.*ModelContext" docs/en/api.md docs/zh-CN/api.md
require_regex "Export.*export_allowed=true|export_allowed=true.*Export" docs/en/api.md docs/zh-CN/api.md
require_regex "DeleteRaw.*attrs|attrs.*DeleteRaw" dev-docs/conversation-transcript-attribute-plane-plan.md docs/en/api.md docs/zh-CN/api.md
require_regex "OperatorAuditOnlyAfterMask.*redacted|OperatorAuditOnlyAfterMask.*脱敏" docs/en/operator-guide.md docs/zh-CN/operator-guide.md docs/en/api.md docs/zh-CN/api.md
require_regex "dry-run|Dry-run|dry_run" docs/en/api.md docs/zh-CN/api.md docs/en/operator-guide.md docs/zh-CN/operator-guide.md
require_regex "Task|TaskDelegation|HumanGate|CapabilityCall|ArtifactRecord|FileWorkspace" docs/en/api.md docs/zh-CN/api.md dev-docs/conversation-transcript-attribute-plane-plan.md

for crate in bm-adapter bm-cli bm-http bm-wss bm-mcp bm-a2a; do
  require_fixed "transcript_attr" "crates/${crate#bm-}/tests"/*.rs
done

if rg -n 'TaskRecord|TaskDelegationLifecycleEvent|PolicyDecision|HumanGate|CapabilityCall|ArtifactRecord|FileWorkspace' \
  crates/core/src/memory/transcript.rs crates/sdk/src/runtime.rs crates/sdk/src/ops.rs | rg -v 'TranscriptAttr|DerivedMemory|MemoryLongTermTarget'; then
  echo "core/store/sdk appear to put host business owner records into transcript attr implementation" >&2
  exit 1
fi

store_attr_scan="$(
  sed -n '/fn upsert_transcript_attrs/,/^    fn append_derived_memory_ref/p' crates/sdk/src/store_internal/platform.rs
  sed -n '/fn transcript_attr_rejection/,/^impl ScopedLongTermMemoryStore/p' crates/sdk/src/store_internal/platform.rs
)"
if printf '%s\n' "$store_attr_scan" | rg -n 'TaskRecord|TaskDelegationLifecycleEvent|PolicyDecision|HumanGate|CapabilityCall|ArtifactRecord|FileWorkspace' \
  | rg -v 'TranscriptAttr|DerivedMemory|MemoryLongTermTarget'; then
  echo "store transcript attr implementation appears to reference host business owner records" >&2
  exit 1
fi

if rg -n 'max_attrs_per_turn|max_attrs_per_message' crates/sdk/src/store_internal; then
  echo "StorePlatform must not own transcript attr profile budgets" >&2
  exit 1
fi

bash -n "$0"

echo "OK: conversation transcript attr plane release-surface docs gate passed"
