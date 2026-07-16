# Adapters

Adapters convert external protocol input into the same SDK runtime operations. They are transport shells, not separate memory runtimes.

For protocol-specific routes, frames, topics, tools, and bridge messages, see [Deployment Guide](deployment.md). This document defines the shared adapter contract.

## Shared Chain

All protocol deployments should follow this chain:

```text
transport bytes -> adapter command -> bm-entry -> bm-adapter -> MemoryRuntime -> adapter response
```

`bm-entry` opens the store and runtime, normalizes identity/scope/auth/source/idempotency data, then dispatches an `AdapterCommand` through `bm-adapter`.

## Entry Runtime Shape

```rust
use bm_entry::{
    EntryAuthConfig, EntryIdentity, EntryIdempotencyConfig, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendConfig,
};

let mut capability = MemoryCapabilityPolicy::strict_profile();
capability.communication_adapter_enabled = true;

let entry = EntryRuntime::open(EntryRuntimeConfig {
    identity: EntryIdentity {
        agent_id: "gateway-agent".to_string(),
        owner_id: "owner-default".to_string(),
    },
    scope: EntryScope {
        channel: "gateway".to_string(),
        chat_id: "chat-1".to_string(),
    },
    store: StoreBackendConfig::in_memory(ProfileId::ServerLinuxMemoryGateway)?
        .with_fsync(false),
    transports: EntryTransportConfig::all_enabled(),
    auth: EntryAuthConfig::disabled_for_local(),
    idempotency: EntryIdempotencyConfig { max_keys: 128 },
    privacy: MemoryPrivacyPolicy::standard_private_boundary(),
    capability,
})?;
```

## Transport Crates

| Transport | Crate | Current interface |
| --- | --- | --- |
| CLI | `bm-cli` | Local memory commands and platform capability snapshots. |
| HTTP | `bm-http` | Request decoding, runtime shell, and standard-library listener backend. |
| WebSocket | `bm-wss` | Command frames, subscriptions, budgets, and WebSocket backend. |
| MCP | `bm-mcp` | Stdio JSON-RPC tool-call bridge. |
| A2A | `bm-a2a` | HTTP bridge for peer memory messages. |

Transport helpers use the shared JSON adapter decoder for declared memory operations. Treat `AdapterCommand` as the shared semantic contract, and treat each transport crate's route/frame/tool/message catalog as the executable protocol surface for that crate.

## Security Boundary

`AdapterAuthContext` and `EntryAuthConfig` represent authentication decisions at the entry layer. They do not replace SDK privacy policy or profile capability checks. Adapter visibility must still come from the runtime capability catalog.

Authentication failure uses the protocol-neutral `AdapterErrorKey::Unauthorized` and maps to HTTP `401`. An authenticated principal that lacks the required operation capability is rejected by `bm-entry` with `AdapterErrorKey::Forbidden`; HTTP transports map that rejection to `403` without redefining authorization semantics.
