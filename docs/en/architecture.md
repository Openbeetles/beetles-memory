# Architecture

Beetle Memory has one memory runtime and multiple entry surfaces. The SDK, CLI, HTTP, WebSocket, MCP, and A2A paths all converge on `MemoryRuntime`; protocol crates must not implement their own memory semantics.

The workspace crates are peers. The core dependency direction is:

```text
bm-core <- bm-store <- bm-sdk
bm-sdk <- bm-adapter <- bm-entry
bm-entry <- bm-cli / bm-http / bm-wss / bm-mcp / bm-a2a
```

`bm-entry` depends on both `bm-sdk` and `bm-adapter`: it opens the SDK runtime and then dispatches adapter envelopes into that runtime.

## Layer Map

```text
Host application or deployment process
  -> bm-sdk or bm-entry
    -> bm-adapter, when a protocol entry is used
      -> bm-sdk::MemoryRuntime
        -> bm-core memory, skill, lifecycle, profile, and recall contracts
        -> bm-store persistence backends
```

| Layer | Crates | Responsibility |
| --- | --- | --- |
| Memory kernel | `bm-core` | Memory planes, recall, projection, lifecycle, feature/profile contracts, skills as procedural memory, task and continuity primitives. |
| Persistence | `bm-store` | In-memory, file, sqlite, and embedded stores; event log; schema manifest; snapshot envelopes; repair reports. |
| SDK facade | `bm-sdk` | Public runtime builder, operation request/report types, capability catalog, store opening re-exports. |
| Replay/evolution | `bm-replay`, `bm-evolve` | Fixture replay, cross-store validation, harness gates, benchmark gates, proposal-only evolution sandbox. |
| Entry runtime | `bm-entry` | Process-level opening of store/runtime plus identity, scope, auth, transport, and idempotency normalization. |
| Adapter contract | `bm-adapter` | Transport-independent envelope, command, operation, dispatch, and response model. |
| Transport shells | `bm-cli`, `bm-http`, `bm-wss`, `bm-mcp`, `bm-a2a` | Decode transport input, build adapter commands, call `EntryRuntime`, and render protocol output. |

## Main Call Chains

Embedded SDK path:

```text
host code
  -> StorePlatform::open(StoreBackendConfig)
  -> MemoryRuntime::builder()
  -> runtime.write / recall / project / maintain / inspect / replay / export / import / recover / close
```

Standalone entry path:

```text
transport request
  -> transport crate decoder
  -> EntryTransportContext + AdapterCommand
  -> EntryRuntime::handle()
  -> AdapterEnvelope<AdapterCommand>
  -> dispatch_adapter_command()
  -> MemoryRuntime operation
  -> AdapterResponse
```

## Memory Operations

| Operation | Runtime method | Typical caller |
| --- | --- | --- |
| Write | `MemoryRuntime::write` | SDK host, CLI, HTTP write candidate |
| Recall | `MemoryRuntime::recall` | SDK host, CLI, HTTP, WebSocket, MCP, A2A |
| Project | `MemoryRuntime::project` | SDK host or CLI when assembling model context |
| Maintain | `MemoryRuntime::maintain` | SDK host with explicit LLM client injection |
| Inspect | `MemoryRuntime::inspect` | Operator tooling and health checks |
| Replay | `MemoryRuntime::replay` | Migration validation and debugging |
| Export / Import | `MemoryRuntime::export` / `MemoryRuntime::import` | Snapshot migration |
| Recover / Close | `MemoryRuntime::recover` / `MemoryRuntime::close` | Runtime lifecycle control |

`Maintain` is deliberately not executed by generic adapter dispatch because it needs explicit LLM/HTTP service injection. Protocol integrations should expose maintain only after they own that dependency injection boundary.

## Data Flow

1. A host chooses a `ProfileId` and opens a supported store backend.
2. `MemoryRuntime` resolves the capability catalog from the profile, compiled features, runtime policy, and privacy policy.
3. Write operations update policy-checked memory state through `bm-core` and persist through `bm-store`.
4. Recall and projection read recent/session/procedural/long-term/continuity data through the runtime facade.
5. Lifecycle events and operator reports are emitted as structured reports instead of hidden side effects.
6. Export/import and replay use snapshot and event-lineage contracts so migrations remain explainable.

## Profile And Store Boundaries

Profiles are not labels; they are compile/runtime contracts:

- ESP profiles can use `embedded` or `in-memory` stores and reject `file`/`sqlite`.
- Linux device, desktop, and server profiles can use file or sqlite stores when matching features are enabled.
- Server gateway profiles can expose protocol adapters; embedded SDK profiles default to in-process use.
- `profile-server-linux-dev-full` is a development profile with broad replay/adapter visibility.

## Deployment Boundary

Beetle Memory provides the memory runtime, SDK, store, entry runtime, and adapter shells. Product-specific surfaces and deployment infrastructure are supplied by the host system; memory state still flows through `MemoryRuntime`.
