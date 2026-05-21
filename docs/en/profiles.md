# Profiles

Profiles bind a target platform to a runtime role. They control feature selection, store choices, adapter visibility, validation scope, and capability snapshots.

## Profile Matrix

| Profile feature | Target | Role | Store posture | Adapter posture |
| --- | --- | --- | --- | --- |
| `profile-esp-standalone-memory` | ESP | standalone memory | embedded or in-memory | compact entry surface plus local/client transports allowed by capability policy |
| `profile-esp-embedded-sdk` | ESP | embedded SDK | embedded or in-memory | in-process SDK by default |
| `profile-linux-device-standalone-memory` | Linux device | standalone memory | file or sqlite | local/device entry surface |
| `profile-desktop-macos-embedded-sdk` | macOS | embedded SDK | file, sqlite, or in-memory | in-process SDK plus local entry surface |
| `profile-desktop-windows-embedded-sdk` | Windows | embedded SDK | file, sqlite, or in-memory | in-process SDK plus local entry surface |
| `profile-server-linux-memory-gateway` | Linux server | memory gateway | sqlite or file | HTTP/Webhook, WebSocket, MQTT, MCP, and A2A gateway surfaces |
| `profile-server-linux-dev-full` | Linux server | development full profile | sqlite, file, or in-memory | full adapter and replay validation surface |

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

## Snapshot Fixtures

Platform capability fixtures live in `fixtures/platform/capabilities/`. Refresh or check them with:

```bash
bash scripts/emit_platform_capability_snapshots.sh --write
bash scripts/emit_platform_capability_snapshots.sh --check
```

Release environments with all target toolchains should run:

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```
