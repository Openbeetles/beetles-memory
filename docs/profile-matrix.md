# Profile Matrix

Profile 是 Beetle Memory 的裁剪单位。目标平台、运行角色、store backend、adapter visibility 和 validation 能力都必须通过 profile 固定，不能由某个宿主项目临时解释。

| Profile feature | Target | Role | 默认 store | Adapter 姿态 | SQLite | Replay harness |
| --- | --- | --- | --- | --- | --- | --- |
| `profile-esp-standalone-memory` | ESP | standalone memory | embedded | compact `bm-entry` + CLI/local frame + WSS/MQTT client | 禁止 | compact validation |
| `profile-esp-embedded-sdk` | ESP | embedded SDK | embedded | SDK in-process，默认不启用 listener | 禁止 | compact validation |
| `profile-linux-device-standalone-memory` | Linux device | standalone memory | file / sqlite | `bm-entry` + CLI / loopback HTTP/WebSocket/MQTT/Webhook backend | 允许 | device validation |
| `profile-desktop-macos-embedded-sdk` | macOS | embedded SDK | file / sqlite / in-memory | SDK in-process + local loopback entry | 允许 | host validation |
| `profile-desktop-windows-embedded-sdk` | Windows | embedded SDK | file / sqlite / in-memory | SDK in-process + local loopback entry | 允许 | host validation |
| `profile-server-linux-memory-gateway` | Linux server | memory gateway | sqlite / file | `bm-entry` + HTTP/Webhook/WebSocket/MQTT/MCP/A2A deployment backend | 允许 | gateway validation |
| `profile-server-linux-dev-full` | Linux server | dev full | sqlite / file / in-memory | 全 deployment backend + adapter + CLI inspection | 允许 | full replay suite |

## Compile Rules

- 一个构建只能选择一个 target feature。
- 一个构建只能选择一个 role feature。
- ESP profile 不允许启用 `sqlite-index`，也不允许使用 sqlite store。
- server / desktop / Linux device profile 可以启用 sqlite index 和 sqlite store。
- `profile-server-linux-dev-full` 是开发全量 profile，不能被当成 ESP 或嵌入式默认 profile。
- `entry` capability 是 adapter capability 的运行时投影。server listener、client bridge 和 local CLI 必须同时满足 profile 允许、feature 编译、配置启用和隐私门禁。

## Snapshot Source

Release gate 使用 `fixtures/platform/capabilities/*.json` 作为稳定 profile capability snapshot。刷新命令：

```bash
bash scripts/emit_platform_capability_snapshots.sh --write
bash scripts/emit_platform_capability_snapshots.sh --check
```

目标工具链齐全的 CI / release 环境还必须运行：

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```
