# Release Checklist

Use this checklist for maintainer release candidates.

## Documentation

- Root README links to the English and Chinese documentation.
- `docs/README.md` links to `docs/en/README.md` and `docs/zh-CN/README.md`.
- English and Chinese docs cover the same developer topics.
- Architecture, integration, deployment, CLI, API, profile, store, adapter, replay, operator, and release guides are all present in both languages.

## Metadata

- Workspace license is `Apache-2.0`.
- Root `LICENSE` exists.
- Publishable crates have package descriptions.
- Workspace crate dependencies include both `version` and `path`.

## Verification

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --no-deps --no-default-features \
  -p bm-core \
  -p bm-sdk \
  -p bm-replay \
  -p bm-evolve \
  -p bm-adapter \
  -p bm-entry \
  -p bm-cli \
  -p bm-http \
  -p bm-wss \
  -p bm-mcp \
  -p bm-a2a
cargo test -p bm-store-contract-tests
bash scripts/check_platform_compile_gates.sh
bash scripts/check_deployment_runtime_contract.sh
bash scripts/check_next_gen_memory_plan.sh
bash scripts/check_production_hardening_contract.sh
bash scripts/check_release_surface.sh
```

Release environments with target toolchains should also run:

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```

## Publish Order

`bm-sdk` is the only public persistence release surface. `bm-store-contract-tests` is a non-published acceptance gate and must pass before releasing `bm-sdk`.

```text
bm-core
bm-sdk
bm-replay / bm-evolve / bm-adapter
bm-entry
bm-cli / bm-http / bm-wss / bm-mcp / bm-a2a
```

Run staged `cargo publish --dry-run -p <crate>` through `scripts/check_release_surface.sh`. The release gate runs production hardening checks, uses temporary Cargo target directories, and fails if repository ignored artifacts change.

## Scope Checks

- README, examples, and crates describe a host-neutral memory runtime.
- Adapter crates keep memory write, recall, projection, and store semantics inside `MemoryRuntime`.
- Standalone deployment covers memory runtime entry points. Product-specific surfaces and deployment infrastructure are supplied by the host deployment.
