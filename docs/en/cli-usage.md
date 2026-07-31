# CLI Usage

The `bm` binary is provided by `bm-cli`. CLI commands go through `bm-entry`, so they exercise the same runtime and store path as protocol deployments.

## Capability Snapshot

```bash
cargo run --locked -p bm-cli --bin bm --no-default-features \
  --features profile-server-linux-memory-gateway -- \
  platform capability-snapshot \
  --profile profile-server-linux-memory-gateway
```

This prints the stable platform capability JSON for a profile.

## Memory Commands

General form:

```bash
cargo run --locked -p bm-cli --bin bm --no-default-features --features <profile-feature-id> -- \
  memory <command> --profile <profile-feature-id> [options]
```

`--profile` is a required deployment contract. It must match both the profile feature enabled for the build and the real host target; the CLI no longer selects dev-full implicitly.

For the examples below, set `BM_HOST_PROFILE` to the one dev-full profile that matches the machine compiling and running the command, then use the helper. Linux is not the generic local-development truth source.

```bash
# macOS:  profile-desktop-macos-dev-full
# Windows: profile-desktop-windows-dev-full
# Linux:  profile-server-linux-dev-full
export BM_HOST_PROFILE=profile-desktop-macos-dev-full
bm() {
  cargo run --locked -p bm-cli --bin bm --no-default-features \
    --features "$BM_HOST_PROFILE" -- "$@"
}
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
| `long-term-list` | List accepted long-term memory or filter by topic. |
| `long-term-detail` | Inspect one long-term memory record by `--record-id`. |
| `long-term-delete` | Delete one long-term memory record by `--record-id` and emit tombstone/audit reports. |
| `long-term-policy-suppress` | Add a suppression policy for future preference memory updates with `--topic <pattern>`. |
| `transcript-attr-write` | Write governed transcript turn/message attrs from a JSON request file; requires `--reason`. |
| `skill-list` | List runtime Skill Memory records. |
| `skill-show` | Inspect one runtime Skill Memory record. |
| `skill-edit` | Edit an existing runtime Skill Memory record. |
| `skill-enable` / `skill-disable` | Enable or disable runtime Skill Memory. |
| `skill-retire` | Append a retired revision while preserving owner lineage; no physical deletion. |
| `close` | Close the runtime and emit lifecycle report. |

Common options:

| Option | Default |
| --- | --- |
| `--profile <profile-feature-id>` | required; no default |
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
bm \
  memory long-term-list \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --query preferred_editor \
  --limit 8
```

```bash
bm \
  memory long-term-detail \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --record-id ltm-preferred-editor
```

```bash
bm \
  memory long-term-delete \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --record-id ltm-preferred-editor \
  --reason "user requested deletion"
```

```bash
bm \
  memory long-term-policy-suppress \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --topic temporary-* \
  --reason "user does not want temporary preferences remembered"
```

## Transcript Attr Write

`transcript-attr-write` is a thin CLI path to `MemoryRuntime::record_transcript_attrs`. It reads a `MemoryTranscriptAttrWriteRequest`-shaped JSON file from `--input` and requires `--reason` for operator audit discipline. The CLI does not construct or interpret attr payloads and does not write the store directly. The response includes `accepted_attrs`, `rejected_attrs`, `redactions_preview`, `profile_budget_applied`, and `audit_event_id`.

```bash
bm \
  memory transcript-attr-write \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --input /tmp/transcript-attrs.json \
  --reason "record provider-reported per-message usage"
```

Attr JSON must target existing transcript turns/messages. Do not put raw prompts, provider secrets, local file paths, complete attachments, host database payloads, tasks, human gates, capability calls, or artifact records into attrs; use links for owner records and keep values lightweight metadata.

## Runtime Skill Memory Management

These commands manage existing runtime procedural memory records only. They do not execute skills or install plugins. Standard Agent Skill directories remain host-managed; standalone deployments can mount them with `BM_AGENT_SKILL_DIRS`, and the runtime only scans and recalls them read-only.

```bash
bm \
  memory write-procedural \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --runtime-skill-subject agent:agent-main \
  --replay-candidate-ref cli:release_guard \
  --verification-receipt-digest sha256:<64-hex> \
  --runtime-skill-privacy shared-with-subject \
  --name runtime_skill__release_guard \
  --title "Release guard" \
  --topic release \
  --summary "Verify release artifacts before publishing." \
  --content "1. run gates
2. inspect artifacts
3. dry run publish"
```

Run `skill-list` with the same owning scope (`--runtime-skill-subject <subject-id>` or `--runtime-skill-shared-program`) first and read `ownerId` plus `locator.owner_revision_ref.owner_revision`. Every later mutation must continue from the previous response's `currentLocator`; the CLI does not translate a name into an owner.

```bash
bm \
  memory skill-list \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --runtime-skill-subject agent:agent-main \
  --query release
```

```bash
bm \
  memory skill-edit \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --runtime-skill-subject agent:agent-main \
  --runtime-skill-owner-id <owner-id> \
  --runtime-skill-owner-revision <revision> \
  --title "Release guard" \
  --topic release \
  --summary "Verify release artifacts and changelog before publishing." \
  --content "1. run gates
2. inspect artifacts
3. inspect changelog"
```

## Write And Recall With A File Store

```bash
bm \
  memory write-procedural \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --runtime-skill-subject agent:agent-main \
  --replay-candidate-ref cli:file_store_release_guard \
  --verification-receipt-digest sha256:<64-hex> \
  --runtime-skill-privacy shared-with-subject \
  --name release_guard \
  --topic release \
  --title "Release guard" \
  --summary "Verify release artifacts before publishing." \
  --content "Run examples, platform gates, and publish dry-run."
```

```bash
bm \
  memory recall \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --query "release artifacts" \
  --limit 4
```

## Project A Memory Block

```bash
bm \
  memory project \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --chat chat-1 \
  --query "How should this host release?" \
  --max-len 4096
```

## Archive Boundary

The CLI does not expose generic memory `export` or `import` commands. Governed replacement uses the SDK's typed memory-space scope and archive contract described in [Replay And Archive](replay-and-archive.md); continuity snapshots remain internal Soul-recovery payloads.

## Close Runtime

```bash
bm \
  memory close \
  --profile "$BM_HOST_PROFILE" \
  --store-file /tmp/beetle-memory-store \
  --reason "operator shutdown"
```
