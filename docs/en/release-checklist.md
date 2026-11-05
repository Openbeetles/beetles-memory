# Release Checklist

Use this checklist for maintainer release candidates.

## 0.12.0 Source Candidate

- Confirm the `assistant_messages` SDK break and `InterruptedDeliveredPrefix` status rules against Core, SDK, Store, and the bilingual release notes. Verify exact retry, 16-message/4 KiB rejection, and `Cancelled` settled-prefix replay.
- Confirm Store v17 opens only v17; an attempted Store v16 open fails without writes. Do not use archive import as migration or run destructive operations on the only real Store copy.
- Verify `DeleteRaw` closes over Session shadow, transcript attrs/auxiliary material, host-reference labels, indexes, and dependent summary text. Record the targeted Core/SDK/Store evidence and explicitly list unrun Provider, GUI, real-data, and platform checks.
- Only after source checks and documentation agree, record the source commit, immutable `v0.12.0` tag, and both remote readbacks. Do not count a local tag, crates.io package, hosted Release, binary, or deployment as evidence for another surface.

## Documentation

- Root README links to the English and Chinese documentation.
- `docs/README.md` links to `docs/en/README.md` and `docs/zh-CN/README.md`.
- English and Chinese docs cover the same developer topics.
- Architecture, integration, deployment, CLI, API, profile, store, adapter, replay, operator, and release guides are all present in both languages.
- Version-specific release notes disclose breaking API/schema changes, migration availability, rollback, publish scope, and unverified evidence planes.

## Metadata

- Workspace license is `Apache-2.0`.
- Root `LICENSE` exists.
- Publishable crates have package descriptions.
- Publishable crates inherit the canonical repository URL and root README.
- Workspace crate dependencies include both `version` and `path`.

## Verification

For a source tag, select checks by the changed contract and reuse applicable engineering evidence. A version bump alone does not require the full workspace or every platform profile. Record skipped checks as unverified. Run the broader matrix below when the affected surface or a real platform integration requires it.

```bash
cargo fmt --all -- --check
cargo test --locked --workspace --exclude bm-desktop
cargo clippy --locked --workspace --exclude bm-desktop --all-targets -- -D warnings
# Run these desktop gates on macOS with the required production profile.
cargo test --locked -p bm-desktop --no-default-features \
  --features profile-desktop-macos-standalone-memory
cargo clippy --locked -p bm-desktop --all-targets --no-default-features \
  --features profile-desktop-macos-standalone-memory -- -D warnings
cargo doc --locked --no-deps --no-default-features \
  -p bm-core \
  -p bm-sdk \
  -p bm-replay \
  -p bm-evolve \
  -p bm-adapter \
  -p bm-entry \
  -p bm-cli \
  -p bm-llm-gateway \
  -p bm-ollama-transparent \
  -p bm-http \
  -p bm-wss \
  -p bm-mcp \
  -p bm-a2a
cargo test --locked -p bm-store-contract-tests
# PL2 feature-gated release matrix; a default-workspace 0-test result is not evidence.
cargo test --locked -p bm-core --test post_turn_memory_governance_contract
cargo test --locked -p bm-core --test post_turn_learning_contract \
  --test agent_tool_experience_owner_contract
cargo test --locked -p bm-sdk --lib --tests --no-default-features \
  --features nonproduction-replay-harness,sqlite-store
cargo test --locked -p bm-entry --no-default-features \
  --features governance-model-client-std,nonproduction-replay-harness,bm-sdk/sqlite-store
cargo test --locked -p bm-entry --no-default-features \
  --test post_turn_learning_capability_contract
cargo test --locked -p bm-store-contract-tests --no-default-features \
  --features sqlite-store
cargo clippy --locked -p bm-core --all-targets -- -D warnings
cargo clippy --locked -p bm-sdk --all-targets --no-default-features \
  --features nonproduction-replay-harness,sqlite-store -- -D warnings
cargo clippy --locked -p bm-entry --all-targets --no-default-features \
  --features governance-model-client-std,nonproduction-replay-harness,bm-sdk/sqlite-store -- -D warnings
bash scripts/check_platform_compile_gates.sh
bash scripts/check_deployment_runtime_contract.sh
bash scripts/check_next_gen_memory_plan.sh
bash scripts/check_production_hardening_contract.sh
bash scripts/check_release_surface.sh
```

When build cache space must live on another volume, set `BM_RELEASE_SURFACE_WORK_ROOT` to an existing absolute directory while keeping `TMPDIR` on the host filesystem for macOS launchd fixtures.

When a target platform is actually being integrated or changed, provision its toolchain and obtain its target gate result. A missing toolchain is `deferred_not_passed`, never a pass. Run the full strict matrix when all target platforms are in scope:

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```

## Publish Order

`bm-sdk` is the only public persistence release surface. `bm-store-contract-tests` is a non-published acceptance gate and must pass before releasing `bm-sdk`.

```text
bm-core
bm-sdk
bm-replay
bm-evolve
bm-adapter
bm-entry
bm-ollama-transparent
bm-cli
bm-llm-gateway
bm-http
bm-wss
bm-mcp
bm-a2a
```

Before uploading crates to crates.io, run staged `cargo publish --dry-run -p <crate>` through `scripts/check_release_surface.sh`. The release gate runs production hardening checks, uses temporary Cargo target directories, and fails if repository ignored artifacts change. A source tag without crate upload does not require those 13 package dry-runs.

## Rollback

- Before the first crates.io upload, stop on any failed crate publish gate, fix on `dev`, and freeze a new candidate; do not move `main` or a release tag to hide a failed crate candidate.
- crates.io versions are immutable. If a published package is defective, do not overwrite the version or delete the tag as a rollback; stop announcing the release, fix the defect, and publish a new patch version in the same dependency order.
- Before deploying a binary that may open an existing store, back up that exact store outside the data path. Deployment rollback restores the previous signed binary and its matching store backup; archive import/export is not an implicit schema migration or rollback path.
- Create a release tag only from the verified `main` commit. Never retarget an existing release tag.

## Scope Checks

- README, examples, and crates describe a host-neutral memory runtime.
- Adapter crates keep memory write, recall, projection, and store semantics inside `MemoryRuntime`.
- Standalone deployment covers memory runtime entry points. Product-specific surfaces and deployment infrastructure are supplied by the host deployment.

## 0.11.2 candidate scope

Validate exact replay after producer revocation and method revision changes, Store-private raw input commitment, zero-write replay, and missing-intent recovery under current producer/source authority with exact preconditions. Store v16, material v6, control revision v4, Adapter V2, and the public API remain unchanged. Run the affected macOS SDK contracts, formatting, static checks, and documentation/version checks. Windows/Linux/ESP integration and crates.io publication are separate gates.

## 0.11.1 candidate scope (historical)

0.11.1 only fixes long-term catalog/detail read capacity and pagination at larger record counts. Store v16, material v6, control revision v4, Adapter V2, and the public API are unchanged. Validate production macOS pagination with 43 records, related targeted contracts on the three Stores, formatting, static checks, and documentation/version consistency. The source tag does not require the Windows/Linux/ESP full matrix or the 13 crate publish dry-runs.

## 0.11.0 candidate scope (historical)

Verify subject-filtered long-term pagination, durable policy list/detail/exact removal and replay, atomic Transcript evidence withdrawal, and factual admission barriers against stale queued/in-flight learning. Require Store v16 / material v6 / control revision v4. Use the LCI1 InMemory/File/SQLite reopen, current/as-of/delivery privacy, CAS/budget/closure evidence and affected contract reruns; check the macOS desktop contract, versions, public docs and source inventory. Windows/Linux/ESP integration and 13 crate dry-runs are deferred until those surfaces or crates.io publication are in scope. Keep all third-party lock records and old tags unchanged. There is no compatibility reader or automatic migration. Full comparative-learning experiments, real Provider quality and GUI/UAT are separate evidence planes.
