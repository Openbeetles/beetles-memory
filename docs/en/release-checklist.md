# Release Checklist

Use this checklist for maintainer release candidates.

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
bash scripts/check_platform_compile_gates.sh
bash scripts/check_deployment_runtime_contract.sh
bash scripts/check_next_gen_memory_plan.sh
bash scripts/check_production_hardening_contract.sh
bash scripts/check_release_surface.sh
```

When build cache space must live on another volume, set `BM_RELEASE_SURFACE_WORK_ROOT` to an existing absolute directory while keeping `TMPDIR` on the host filesystem for macOS launchd fixtures.

An engineering handoff from a host that lacks a required target toolchain may record that row as
`deferred_not_passed`. Every release candidate must provision all required target toolchains and
obtain strict GREEN; a missing toolchain blocks release and is not a pass:

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

Run staged `cargo publish --dry-run -p <crate>` through `scripts/check_release_surface.sh`. The release gate runs production hardening checks, uses temporary Cargo target directories, and fails if repository ignored artifacts change.

## Rollback

- Before the first real publish, stop on any failed gate, fix on `dev`, and freeze a new candidate; do not move `main` or a release tag to hide a failed candidate.
- crates.io versions are immutable. If a published package is defective, do not overwrite the version or delete the tag as a rollback; stop announcing the release, fix the defect, and publish a new patch version in the same dependency order.
- Before deploying a binary that may open an existing store, back up that exact store outside the data path. Deployment rollback restores the previous signed binary and its matching store backup; archive import/export is not an implicit schema migration or rollback path.
- Create a release tag only from the verified `main` commit. Never retarget an existing release tag.

## Scope Checks

- README, examples, and crates describe a host-neutral memory runtime.
- Adapter crates keep memory write, recall, projection, and store semantics inside `MemoryRuntime`.
- Standalone deployment covers memory runtime entry points. Product-specific surfaces and deployment infrastructure are supplied by the host deployment.
