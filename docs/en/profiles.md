# Profiles

Profiles bind a target platform to a runtime role. They control feature selection, store choices, adapter visibility, validation scope, and capability snapshots.

## Profile Matrix

| Profile feature | Target | Role | Store posture | Adapter posture |
| --- | --- | --- | --- | --- |
| `profile-esp-standalone-memory` | ESP | standalone memory | embedded or in-memory | compact entry surface plus local/client transports allowed by capability policy |
| `profile-esp-embedded-sdk` | ESP | embedded SDK | embedded or in-memory | in-process SDK by default |
| `profile-linux-device-standalone-memory` | Linux device | standalone memory | file or sqlite | local/device entry surface |
| `profile-desktop-macos-standalone-memory` | macOS | standalone desktop app | file or sqlite | in-process Tauri command surface plus optional local transports |
| `profile-desktop-macos-embedded-sdk` | macOS | embedded SDK | file, sqlite, or in-memory | in-process SDK plus local entry surface |
| `profile-desktop-windows-embedded-sdk` | Windows | embedded SDK | file, sqlite, or in-memory | in-process SDK plus local entry surface |
| `profile-server-linux-memory-gateway` | Linux server | memory gateway | sqlite or file | HTTP, WebSocket, MCP, A2A, and LLM gateway server surfaces are profile-allowed; runtime visibility still depends on capability policy and transport config |
| `profile-server-linux-dev-full` | Linux server | development full profile | sqlite, file, or in-memory | full adapter, LLM gateway server, and replay validation surfaces are profile-allowed; runtime visibility still depends on capability policy and transport config |

## Naming

Developers select `profile-*` convenience features, for example `profile-server-linux-memory-gateway`. Each profile maps to one `target-*` feature and one `role-*` feature. Platform capability snapshot files also use the `profile-*` names.

```bash
cargo run -p bm-cli --bin bm -- \
  platform capability-snapshot \
  --profile profile-esp-standalone-memory
```

## Compile Rules

- Select at most one `target-*` feature per build.
- Select at most one `role-*` feature per build.
- ESP profiles must not enable `sqlite-index`.
- ESP profiles may use `embedded` or `in-memory` store backends.
- ESP profiles reject `file` and `sqlite` store backends.
- Linux device, desktop, and server profiles can use sqlite when the matching profile/store features are enabled.
- `profile-server-linux-dev-full` is a development profile, not an embedded default.
- `llm_gateway_server` belongs only to server Linux memory gateway / dev-full profiles; ESP, device, and desktop embedded SDK profiles do not expose this entry surface.
- The profile catalog answers whether a surface is allowed for a profile. Runtime `EntryCapabilityView.visible` is the intersection of profile allowance, enabled capability policy, and `EntryTransportConfig`.

## Snapshot Fixtures

Platform capability fixtures live in `fixtures/platform/capabilities/`. Refresh or check them with:

These fixtures are strict-policy snapshots. Strict policy keeps communication adapters disabled by default, so server profiles can report `entry.llm_gateway_server.server_allowed=true` while `visible=false`. That means the profile allows the server surface, but the snapshot policy has not enabled it.

```bash
bash scripts/emit_platform_capability_snapshots.sh --write
bash scripts/emit_platform_capability_snapshots.sh --check
```

Release environments with all target toolchains should run:

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```
