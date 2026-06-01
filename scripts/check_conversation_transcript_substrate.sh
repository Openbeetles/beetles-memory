#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

required_files=(
  "dev-docs/conversation-transcript-substrate-plan.md"
  "dev-docs/conversation-transcript-governance-hardening-plan.md"
  "dev-docs/README.md"
  "docs/en/api.md"
  "docs/zh-CN/api.md"
  "docs/en/replay-and-migration.md"
  "docs/zh-CN/replay-and-migration.md"
  "docs/en/README.md"
  "docs/zh-CN/README.md"
)

for file in "${required_files[@]}"; do
  if [[ ! -s "$file" ]]; then
    echo "missing transcript substrate release-surface file: $file" >&2
    exit 1
  fi
done

require_fixed() {
  local needle="$1"
  shift
  if ! rg -F -q "$needle" "$@"; then
    echo "missing required transcript substrate term: $needle" >&2
    printf '  files: %s\n' "$*" >&2
    exit 1
  fi
}

require_regex() {
  local needle="$1"
  shift
  if ! rg -q "$needle" "$@"; then
    echo "missing required transcript substrate pattern: $needle" >&2
    printf '  files: %s\n' "$*" >&2
    exit 1
  fi
}

require_fixed "conversation-transcript-substrate-plan.md" dev-docs/README.md
require_fixed "conversation-transcript-governance-hardening-plan.md" dev-docs/README.md dev-docs/conversation-transcript-substrate-plan.md
require_fixed "release-surface docs" dev-docs/README.md dev-docs/conversation-transcript-substrate-plan.md
require_fixed "Memory Evidence System" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "host-ref visibility" dev-docs/conversation-transcript-governance-hardening-plan.md
require_fixed "redaction report" dev-docs/conversation-transcript-governance-hardening-plan.md
require_fixed "derived refs" dev-docs/conversation-transcript-governance-hardening-plan.md
require_fixed "lifecycle impact" dev-docs/conversation-transcript-governance-hardening-plan.md
require_fixed "Conversation Transcript Substrate" docs/en/README.md docs/zh-CN/README.md docs/en/api.md docs/zh-CN/api.md
require_fixed "ConversationKey" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_fixed "HostOpaqueRef" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_fixed "RedactedTranscriptSlice" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "TranscriptRedactionReportItem" docs/en/api.md docs/zh-CN/api.md crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs
require_fixed "TranscriptEvidenceRef" docs/en/api.md docs/zh-CN/api.md crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs
require_fixed "DerivedMemoryRef" docs/en/api.md docs/zh-CN/api.md crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs
require_fixed "TranscriptTurnPage" dev-docs/conversation-transcript-governance-hardening-plan.md docs/en/api.md docs/zh-CN/api.md crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs
require_fixed "TranscriptRepairReport" dev-docs/conversation-transcript-governance-hardening-plan.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md crates/core/src/memory/transcript.rs crates/sdk/src/lib.rs
require_fixed "HostRefLabel" dev-docs/conversation-transcript-governance-hardening-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md crates/core/src/memory/transcript.rs crates/core/tests/conversation_transcript_contract.rs
require_fixed "MissingSourceMessage" dev-docs/conversation-transcript-governance-hardening-plan.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md crates/core/src/memory/transcript.rs crates/store/tests/conversation_transcript_store_contract.rs
require_fixed "next_cursor" docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "has_more" docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "lifecycle_request_without_affected_turns_reports_noop" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "host_ref_label_is_redacted_for_non_owner_views" dev-docs/conversation-transcript-governance-hardening-plan.md crates/core/tests/conversation_transcript_contract.rs
require_fixed "transcript_replay_export_page_requests_are_public" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/public_surface.rs
require_fixed "conversation_transcript_derived_ref" crates/store/src/platform.rs crates/store/tests/conversation_transcript_store_contract.rs
require_fixed "TranscriptBackedSessionStore" crates/sdk/src/runtime.rs dev-docs/conversation-transcript-governance-hardening-plan.md
require_fixed "with_conversation_id" crates/sdk/src/runtime.rs docs/en/api.md docs/zh-CN/api.md
require_fixed "SoulCandidateHandoff" crates/core/src/memory/transcript.rs crates/sdk/src/runtime.rs crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "redacted_host_refs" crates/core/src/memory/transcript.rs crates/core/tests/conversation_transcript_contract.rs
require_fixed "transcript_commit" crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "TranscriptLifecycleRequest" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryTranscriptCommitRequest" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryTranscriptReplayRequest" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryTranscriptLifecycleRequest" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryTranscriptExportRequest" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryTranscriptRepairRequest" docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md crates/sdk/src/ops.rs crates/sdk/src/runtime.rs crates/sdk/src/lib.rs
require_fixed "MemoryRuntime::finalize_turn_and_maintain" docs/en/api.md docs/zh-CN/api.md
require_fixed "memory_space_id + channel_id + conversation_id" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "HostUi" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_fixed "ModelContext" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_regex "chat_id.*legacy|legacy.*chat_id" docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_regex "current|当前|follow-up|后续" docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_regex "not .*task system|不是宿主任务系统" docs/en/api.md docs/zh-CN/api.md
require_regex "fail closed|fail closed" docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_regex "affected_turns=0.*changed=false|changed=false.*affected_turns=0" docs/en/api.md docs/zh-CN/api.md

require_fixed "long_term_extraction_records_transcript_derived_ref_for_lifecycle_impact" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "automatic_post_turn_extraction_records_transcript_derived_ref_for_lifecycle_impact" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "candidate_write_records_only_second_stage_accepted_derived_refs" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "record_long_term_extraction_derived_memory_refs" crates/sdk/src/runtime.rs
require_fixed "SessionMessageRecord" dev-docs/conversation-transcript-governance-hardening-plan.md crates/core/src/memory/mod.rs crates/core/src/memory/archive_search.rs
require_fixed "transcript_ref" dev-docs/conversation-transcript-governance-hardening-plan.md crates/core/src/memory/mod.rs crates/sdk/src/runtime.rs
require_fixed "live_archive_transcript_hit_preserves_structured_transcript_evidence_ref" dev-docs/conversation-transcript-governance-hardening-plan.md crates/core/src/memory/archive_search.rs
require_fixed "private_garden_self_work_records_private_garden_derived_refs_without_raw_content" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "record_private_garden_derived_memory_refs" crates/sdk/src/runtime.rs
require_fixed "lifecycle_report_sanitizes_host_refs_for_operator_view" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "host_ref_redactions" docs/en/api.md docs/zh-CN/api.md crates/core/src/memory/transcript.rs crates/sdk/src/runtime.rs
require_fixed "filter_host_refs_for_transcript_view" crates/core/src/memory/transcript.rs crates/sdk/src/runtime.rs
require_fixed "recall_inspect_and_maintenance_do_not_fallback_to_session_shadow_after_transcript_mask" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "transcript_backed_projection_honors_recent_message_limit" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "fresh_runtime_does_not_fallback_to_session_shadow_after_transcript_mask" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "fresh_runtime_does_not_fallback_to_session_shadow_after_transcript_raw_delete" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "fresh_runtime_fails_closed_when_transcript_alias_is_corrupt" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "conversation_transcript_key_unavailable" crates/sdk/src/runtime.rs
require_fixed "conversation_transcript_alias" docs/en/api.md docs/zh-CN/api.md crates/store/src/platform.rs crates/store/tests/conversation_transcript_store_contract.rs crates/sdk/src/lib.rs
require_fixed "conversation_transcript_derived_ref" docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md crates/sdk/src/lib.rs crates/sdk/tests/conversation_transcript_runtime_contract.rs
require_fixed "transcript_governance_budget_is_profile_owned" dev-docs/conversation-transcript-governance-hardening-plan.md
require_fixed "TranscriptGovernanceBudget" dev-docs/conversation-transcript-governance-hardening-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md crates/core/src/budget.rs crates/sdk/src/lib.rs
require_fixed "profile_budget_applied" dev-docs/conversation-transcript-governance-hardening-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md crates/core/src/memory/transcript.rs crates/sdk/src/runtime.rs crates/sdk/tests/runtime_budget_contract.rs
require_fixed "transcript_report_budgets_limit_derived_refs_and_repair_issues" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/runtime_budget_contract.rs
require_fixed "transcript_replay_budget_limits_visible_host_refs_and_redaction_items" dev-docs/conversation-transcript-governance-hardening-plan.md crates/sdk/tests/runtime_budget_contract.rs
require_fixed "repair_transcript" docs/en/api.md docs/zh-CN/api.md crates/sdk/src/runtime.rs crates/sdk/tests/runtime_budget_contract.rs
require_fixed "orphan derived ref" dev-docs/conversation-transcript-governance-hardening-plan.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_fixed "corrupt transcript record" dev-docs/conversation-transcript-governance-hardening-plan.md docs/en/replay-and-migration.md
require_fixed "corrupt transcript records" docs/zh-CN/replay-and-migration.md
require_fixed "mismatched source key" dev-docs/conversation-transcript-governance-hardening-plan.md docs/en/replay-and-migration.md
require_fixed "mismatched source keys" docs/zh-CN/replay-and-migration.md
require_fixed "duplicate sequence/cursor" dev-docs/conversation-transcript-governance-hardening-plan.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md

host_product_forbidden_pattern="TaskRoomProjection|HumanGate|ClarificationRequest|Clarification UI|RoleKey|CEO|BOSS|Task\\.status|TaskRecord|联系人 TAB|任务详情|灰色推理块|runtime progress|runtime progress card|evidence card|capability card|capability proposal"

if rg -n "$host_product_forbidden_pattern" \
  docs/en docs/zh-CN | rg -v "不|不得|not|forbidden|禁区"; then
  echo "public docs appear to expose host/product task semantics" >&2
  exit 1
fi

if rg -n "$host_product_forbidden_pattern" \
  crates/core/src crates/sdk/src crates/store/src; then
  echo "core/sdk/store appear to expose host/product task semantics" >&2
  exit 1
fi

if rg -n "production-ready transcript|transcript.*release-ready|transcript.*ready-to-ship" \
  docs/en docs/zh-CN dev-docs/conversation-transcript-substrate-plan.md; then
  echo "docs appear to over-claim transcript implementation status" >&2
  exit 1
fi

bash -n "$0"

echo "OK: conversation transcript substrate release-surface docs gate passed"
