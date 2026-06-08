# CLI Usage

The `bm` binary is provided by `bm-cli`. CLI commands go through `bm-entry`, so they exercise the same runtime and store path as protocol deployments.

## Capability Snapshot

```bash
cargo run -p bm-cli --bin bm -- \
  platform capability-snapshot \
  --profile profile-server-linux-memory-gateway
```

This prints the stable platform capability JSON for a profile.

## Memory Commands

General form:

```bash
cargo run -p bm-cli --bin bm -- memory <command> [options]
```

Commands:

| Command | Purpose |
| --- | --- |
| `capabilities` | Print runtime capability catalog. |
| `write-procedural` | Write one procedural memory item. |
| `recall` | Recall memory hits by query. |
| `project` | Render a memory block for model context. |
| `inspect` | Return operator inspection data. |
| `replay` | Inspect turn replay for a chat. |
| `export` | Export a continuity snapshot. |
| `import` | Import a continuity snapshot. |
| `long-term-list` | List accepted long-term memory or filter by topic. |
| `long-term-detail` | Inspect one long-term memory record by `--record-id`. |
| `long-term-delete` | Delete one long-term memory record by `--record-id` and emit tombstone/audit reports. |
| `long-term-policy-suppress` | Add a suppression policy for future preference memory updates with `--topic <pattern>`. |
| `transcript-attr-write` | Write governed transcript turn/message attrs from a JSON request file; requires `--reason`. |
| `skill-list` | List runtime Skill Memory records. |
| `skill-show` | Inspect one runtime Skill Memory record. |
| `skill-edit` | Edit an existing runtime Skill Memory record. |
| `skill-enable` / `skill-disable` | Enable or disable runtime Skill Memory. |
| `skill-delete` | Delete runtime Skill Memory. |
| `close` | Close the runtime and emit lifecycle report. |

Common options:

| Option | Default |
| --- | --- |
| `--profile <profile-feature-id>` | `profile-server-linux-dev-full` |
| `--store-file <path>` | none |
| `--store-sqlite <path>` | none |
| `--store-embedded` | false |
| `--agent <id>` | `agent-main` |
| `--owner <id>` | `owner-default` |
| `--channel <name>` | `local` |
| `--chat <id>` / `--chat-id <id>` | `chat-1` |
| `--query <text>` | empty |
| `--limit <n>` | `8` |
| `--max-len <n>` | `4096` |
| `--record-id <id>` | empty |
| `--topic <text>` | empty |

## Long-Term Memory Control

These commands call the Memory SDK accepted long-term memory control surface only. They do not read or mutate a host-owned local database. `long-term-delete` removes the target from active recall/projection and writes tombstone/audit reports. `long-term-policy-suppress` affects future long-term memory writes and does not retroactively delete accepted memory.

```bash
cargo run -p bm-cli --bin bm -- \
  memory long-term-list \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --query preferred_editor \
  --limit 8
```

```bash
cargo run -p bm-cli --bin bm -- \
  memory long-term-detail \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --record-id ltm-preferred-editor
```

```bash
cargo run -p bm-cli --bin bm -- \
  memory long-term-delete \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --record-id ltm-preferred-editor \
  --reason "user requested deletion"
```

```bash
cargo run -p bm-cli --bin bm -- \
  memory long-term-policy-suppress \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --topic temporary-* \
  --reason "user does not want temporary preferences remembered"
```

## Transcript Attr Write

`transcript-attr-write` is a thin CLI path to `MemoryRuntime::record_transcript_attrs`. It reads a `MemoryTranscriptAttrWriteRequest`-shaped JSON file from `--input` and requires `--reason` for operator audit discipline. The CLI does not construct or interpret attr payloads and does not write the store directly. The response includes `accepted_attrs`, `rejected_attrs`, `redactions_preview`, `profile_budget_applied`, and `audit_event_id`.

```bash
cargo run -p bm-cli --bin bm -- \
  memory transcript-attr-write \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --input /tmp/transcript-attrs.json \
  --reason "record provider-reported per-message usage"
```

Attr JSON must target existing transcript turns/messages. Do not put raw prompts, provider secrets, local file paths, complete attachments, host database payloads, tasks, human gates, capability calls, or artifact records into attrs; use links for owner records and keep values lightweight metadata.

## Runtime Skill Memory Management

These commands manage existing runtime procedural memory records only. They do not execute skills or install plugins. Standard Agent Skill directories remain host-managed; standalone deployments can mount them with `BM_AGENT_SKILL_DIRS`, and the runtime only scans and recalls them read-only.

```bash
cargo run -p bm-cli --bin bm -- \
  memory write-procedural \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --name runtime_skill__release_guard \
  --title "Release guard" \
  --topic release \
  --summary "Verify release artifacts before publishing." \
  --content "1. run gates
2. inspect artifacts
3. dry run publish"
```

```bash
cargo run -p bm-cli --bin bm -- \
  memory skill-edit \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --name runtime_skill__release_guard \
  --title "Release guard" \
  --topic release \
  --summary "Verify release artifacts and changelog before publishing." \
  --content "1. run gates
2. inspect artifacts
3. inspect changelog"
```

```bash
cargo run -p bm-cli --bin bm -- \
  memory skill-list \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --query release
```

## Write And Recall With A File Store

```bash
cargo run -p bm-cli --bin bm -- \
  memory write-procedural \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --name release_guard \
  --topic release \
  --title "Release guard" \
  --summary "Verify release artifacts before publishing." \
  --content "Run examples, platform gates, and publish dry-run."
```

```bash
cargo run -p bm-cli --bin bm -- \
  memory recall \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --query "release artifacts" \
  --limit 4
```

## Project A Memory Block

```bash
cargo run -p bm-cli --bin bm -- \
  memory project \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --query "How should this host release?" \
  --max-len 4096
```

## Export And Import

```bash
cargo run -p bm-cli --bin bm -- \
  memory export \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --output /tmp/chat-1.snapshot.json
```

```bash
cargo run -p bm-cli --bin bm -- \
  memory import \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store-2 \
  --chat chat-2 \
  --input /tmp/chat-1.snapshot.json
```

## Close Runtime

```bash
cargo run -p bm-cli --bin bm -- \
  memory close \
  --profile profile-server-linux-dev-full \
  --store-file /tmp/beetle-memory-store \
  --reason "operator shutdown"
```
