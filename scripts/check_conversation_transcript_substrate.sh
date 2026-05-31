#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

required_files=(
  "dev-docs/conversation-transcript-substrate-plan.md"
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
require_fixed "release-surface docs" dev-docs/README.md dev-docs/conversation-transcript-substrate-plan.md
require_fixed "Memory Evidence System" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "Conversation Transcript Substrate" docs/en/README.md docs/zh-CN/README.md docs/en/api.md docs/zh-CN/api.md
require_fixed "ConversationKey" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_fixed "HostOpaqueRef" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_fixed "RedactedTranscriptSlice" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "TranscriptLifecycleRequest" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryTranscriptCommitRequest" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryTranscriptReplayRequest" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryTranscriptLifecycleRequest" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryTranscriptExportRequest" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "MemoryRuntime::finalize_turn_and_maintain" docs/en/api.md docs/zh-CN/api.md
require_fixed "memory_space_id + channel_id + conversation_id" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md
require_fixed "HostUi" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_fixed "ModelContext" dev-docs/conversation-transcript-substrate-plan.md docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_regex "chat_id.*legacy|legacy.*chat_id" docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_regex "current|当前|follow-up|后续" docs/en/api.md docs/zh-CN/api.md docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md
require_regex "not .*task system|不是宿主任务系统" docs/en/api.md docs/zh-CN/api.md
require_regex "fail closed|fail closed" docs/en/replay-and-migration.md docs/zh-CN/replay-and-migration.md

if rg -n "TaskRoomProjection|HumanGate|ClarificationRequest|RoleKey|CEO|BOSS|联系人 TAB|任务详情|灰色推理块|runtime progress card|evidence card|capability proposal" \
  docs/en docs/zh-CN | rg -v "不|不得|not|forbidden|禁区"; then
  echo "public docs appear to expose host/product task semantics" >&2
  exit 1
fi

if rg -n "production-ready transcript|transcript.*release-ready|transcript.*ready-to-ship" \
  docs/en docs/zh-CN dev-docs/conversation-transcript-substrate-plan.md; then
  echo "docs appear to over-claim transcript implementation status" >&2
  exit 1
fi

bash -n "$0"

echo "OK: conversation transcript substrate release-surface docs gate passed"
