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
