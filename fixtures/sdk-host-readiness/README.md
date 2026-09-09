# SDK Host Readiness Fixtures

These fixtures define host-integration parity inputs for Beetle Memory as an SDK.
Factual candidates use the public SDK contract: identity, scope, host
platform injection, canonical turn delta, write candidates, projection,
deferred governance jobs, and export/import/replay.

`runtime_skill_fixtures` contains typed, synthetic replay seeds, not public
write candidates or host-authorized production creation. Only the nonproduction
replay harness may consume those seeds. Public archives must exclude their
protected RuntimeSkill owners; production initial creation uses governed learning.

- `generic-rust-host/`: a neutral SDK host with no Beetle-specific behavior.
- `beetle-derived/`: Beetle-shaped host evidence exercised through the current
  public contract. It must not receive a special kernel or compatibility branch.
