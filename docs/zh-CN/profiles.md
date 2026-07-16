# Profile 矩阵

Profile 把目标平台和运行角色绑定在一起。它控制 feature selection、store 选择、adapter visibility、validation scope 和 capability snapshot。

## Profile Matrix

| Profile feature | 目标 | 角色 | Store 姿态 | Adapter 姿态 |
| --- | --- | --- | --- | --- |
| `profile-esp-standalone-memory` | ESP | standalone memory | embedded 或 in-memory | capability policy 允许的 compact entry surface 和本地/client transport |
| `profile-esp-embedded-sdk` | ESP | embedded SDK | embedded 或 in-memory | 默认 in-process SDK |
| `profile-linux-device-standalone-memory` | Linux device | standalone memory | file 或 sqlite | 本地/device entry surface |
| `profile-desktop-macos-standalone-memory` | macOS | standalone desktop app | file 或 sqlite | in-process Tauri command、可选本地 transport，以及透明 Ollama 使用的 loopback LLM Gateway |
| `profile-desktop-macos-embedded-sdk` | macOS | embedded SDK | file、sqlite 或 in-memory | in-process SDK 加本地 entry surface |
| `profile-desktop-macos-dev-full` | macOS | nonproduction development full | sqlite、file 或 in-memory | 完整 adapter、LLM gateway、replay 与 benchmark validation surface |
| `profile-desktop-windows-embedded-sdk` | Windows | embedded SDK | file、sqlite 或 in-memory | in-process SDK 加本地 entry surface |
| `profile-desktop-windows-dev-full` | Windows | nonproduction development full | sqlite、file 或 in-memory | 完整 adapter、LLM gateway、replay 与 benchmark validation surface |
| `profile-server-linux-memory-gateway` | Linux server | memory gateway | sqlite 或 file | 允许 HTTP、WebSocket、MCP、A2A 与 LLM gateway server surface；运行时 visible 仍取决于 capability policy 和 transport config |
| `profile-server-linux-dev-full` | Linux server | development full profile | sqlite、file 或 in-memory | 允许完整 adapter、LLM gateway server 和 replay validation surface；运行时 visible 仍取决于 capability policy 和 transport config |

## 命名

开发者使用 `profile-*` convenience features，例如 `profile-server-linux-memory-gateway`。实现上，每个 profile 会映射到一个 `target-*` feature 和一个 `role-*` feature。Platform capability snapshot 文件同样使用 `profile-*` 名称。

```bash
cargo run --locked -p bm-cli --bin bm --no-default-features \
  --features profile-esp-standalone-memory -- \
  platform capability-snapshot \
  --profile profile-esp-standalone-memory
```

## 编译规则

- 每次构建最多选择一个 `target-*` feature。
- 每次构建最多选择一个 `role-*` feature。
- ESP profile 不能启用 `sqlite-index`。
- ESP profile 可以使用 `embedded` 或 `in-memory` store backend。
- ESP profile 会拒绝 `file` 和 `sqlite` store backend。
- Linux device、desktop 和 server profile 在启用对应 profile/store feature 后可以使用 sqlite。
- 所有 `*-dev-full` 都会编译 nonproduction replay harness，只能匹配当前真实 host target，不能成为生产或嵌入式默认 profile。
- `llm_gateway_server` 属于 server Linux memory gateway、承载本机透明 Ollama 的 macOS standalone-memory profile 与三种 dev-full profile。ESP、device 与 desktop embedded SDK profile 不暴露这个 entry surface。Gateway 启动绑定前仍必须执行 runtime capability view；仅有 catalog 许可不构成启动授权。
- Profile catalog 表达某个 surface 是否被该 profile 允许。运行时 `EntryCapabilityView.visible` 是 profile allowed、enabled capability policy 和 `EntryTransportConfig` 三者共同生效后的结果。
- 长期记忆控制能力分为 `long_term_control_inspect`、`long_term_control_mutation`、`long_term_control_policy` 和 `long_term_control_bulk_forget`。所有 profile 都应能暴露 targeted inspect/mutation/policy 或返回结构化拒绝；destructive bulk forget 只在 desktop/server 等有足够 operator surface 的 profile 可见，ESP compact profile 默认隐藏。

## Snapshot Fixtures

Platform capability fixtures 位于 `fixtures/platform/capabilities/`。刷新或检查命令：

这些 fixtures 是 strict policy snapshot；strict policy 默认关闭 communication adapter，因此 server profile 可以出现 `entry.llm_gateway_server.server_allowed=true` 但 `visible=false`。这表示 profile 允许该 server surface，但当前 snapshot policy 没有把它打开。

```bash
bash scripts/emit_platform_capability_snapshots.sh --write
bash scripts/emit_platform_capability_snapshots.sh --check
```

具备全部目标工具链的 release 环境应运行：

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```
