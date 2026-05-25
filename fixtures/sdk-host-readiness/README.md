# SDK Host Readiness Fixtures

These fixtures define host-integration parity inputs for Beetle Memory as an SDK.
Every fixture must use the public SDK contract only: identity, scope, host
platform injection, canonical turn delta, write candidates, projection,
deferred governance jobs, and export/import/replay.

- `generic-rust-host/`: a neutral SDK host with no Beetle-specific behavior.
- `beetle-derived/`: legacy-shaped inputs extracted from Beetle-style
  memory behavior. It is evidence only and must not receive a special kernel
  branch.
- `beetle-derived-legacy/`: historical alias retained for older notes; new
  readiness tests use `beetle-derived/`.
