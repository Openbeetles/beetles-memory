# SDK Host Readiness Fixtures

These fixtures define host-integration parity inputs for Beetle Memory as an SDK.
Every fixture must use the public SDK contract only: identity, scope, host
platform injection, canonical turn delta, write candidates, projection,
deferred governance jobs, and export/import/replay.

- `generic-rust-host/`: a neutral SDK host with no Beetle-specific behavior.
- `beetle-derived/`: Beetle-shaped host evidence exercised through the current
  public contract. It must not receive a special kernel or compatibility branch.
