# 发布清单

此清单面向 maintainer release candidate。

## 文档

- 根 README 链接英文和中文文档。
- `docs/README.md` 链接 `docs/en/README.md` 和 `docs/zh-CN/README.md`。
- 英文和中文文档覆盖同一组主题：架构、集成、部署、CLI、API、Profile、存储、Adapter、回放、运维、发布。

## Metadata

- Workspace license 是 `Apache-2.0`。
- 根目录存在 `LICENSE`。
- 可发布 crates 有 package description。
- Workspace crate dependencies 同时包含 `version` 和 `path`。

## 验证

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

具备目标工具链的 release 环境还应运行：

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```

## Publish Order

`bm-sdk` 是唯一的公开 persistence 发布面。`bm-store-contract-tests` 是不发布的验收门禁，必须在发布 `bm-sdk` 前通过。

```text
bm-core
bm-sdk
bm-replay / bm-evolve / bm-adapter
bm-entry
bm-cli / bm-http / bm-wss / bm-mcp / bm-a2a
```

通过 `scripts/check_release_surface.sh` 运行 staged `cargo publish --dry-run -p <crate>`。release gate 会运行 production hardening 检查、使用临时 Cargo target，并在 ignored artifact baseline 发生变化时失败。

## 范围检查

- README、examples 和 crates 描述的是宿主无关的 memory runtime。
- Adapter crates 保持 memory write、recall、projection 和 store 语义由 `MemoryRuntime` 承担。
- Standalone deployment 覆盖 memory runtime 入口。产品专属表面和部署基础设施由宿主部署提供。
