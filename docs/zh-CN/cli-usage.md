# CLI 使用

`bm` binary 由 `bm-cli` 提供。CLI commands 通过 `bm-entry`，因此会走和协议部署相同的 runtime 与 store 路径。

## Capability Snapshot

```bash
cargo run -p bm-cli --bin bm -- \
  platform capability-snapshot \
  --profile profile-server-linux-memory-gateway
```

该命令输出某个 profile 的稳定 platform capability JSON。

## Memory Commands

通用形式：

```bash
cargo run -p bm-cli --bin bm -- memory <command> [options]
```

Commands：

| Command | 用途 |
| --- | --- |
| `capabilities` | 输出 runtime capability catalog。 |
| `write-procedural` | 写入一条 procedural memory。 |
| `recall` | 按 query 召回 memory hits。 |
| `project` | 生成模型上下文 memory block。 |
| `inspect` | 返回 operator inspection 数据。 |
| `replay` | 检查某个 chat 的 turn replay。 |
| `export` | 导出 continuity snapshot。 |
| `import` | 导入 continuity snapshot。 |
| `long-term-list` | 列出或按 topic 查询 accepted long-term memory。 |
| `long-term-detail` | 通过 `--record-id` 查看一条长期记忆详情。 |
| `long-term-delete` | 通过 `--record-id` 删除一条长期记忆并产生 tombstone/audit report。 |
| `long-term-policy-suppress` | 用 `--topic <pattern>` 添加“以后不要记这类偏好”的 suppression policy。 |
| `skill-list` | 列出运行时 Skill 记忆。 |
| `skill-show` | 查看单条运行时 Skill 记忆详情。 |
| `skill-edit` | 编辑已有运行时 Skill 记忆。 |
| `skill-enable` / `skill-disable` | 启用或停用运行时 Skill 记忆。 |
| `skill-delete` | 删除运行时 Skill 记忆。 |
| `close` | 关闭 runtime 并发出 lifecycle report。 |

常用 options：

| Option | 默认值 |
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

## 长期记忆控制

这些命令只调用 Memory SDK 的 accepted long-term memory 控制面，不读取或修改宿主本地数据库。`long-term-delete` 会从 active recall/projection 中移除目标记忆，并写入 tombstone/audit；`long-term-policy-suppress` 影响后续长期记忆写入，不 retroactively 删除已有记忆。

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

## 运行时 Skill 记忆管理

这些命令只管理已经存在的运行时 procedural memory record，不执行 skill，也不提供插件安装器。标准 Agent Skill 目录由宿主自己管理；独立部署时可通过 `BM_AGENT_SKILL_DIRS` 挂载目录，运行时只扫描和召回，不新增、不编辑、不导入。

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

## 用 File Store 写入并召回

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

## 生成 Memory Block

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
