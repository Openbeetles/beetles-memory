# 发布清单

此清单面向 maintainer release candidate。

## 文档

- 根 README 链接英文和中文文档。
- `docs/README.md` 链接 `docs/en/README.md` 和 `docs/zh-CN/README.md`。
- 英文和中文文档覆盖同一组主题：架构、集成、部署、CLI、API、Profile、存储、Adapter、回放、运维、发布。
- 版本专属发布说明必须披露 breaking API/schema 变更、迁移可用性、回滚、发布集合和未验证证据平面。

## Metadata

- Workspace license 是 `Apache-2.0`。
- 根目录存在 `LICENSE`。
- 可发布 crates 有 package description。
- 可发布 crates 继承规范 repository URL 和根 README。
- Workspace crate dependencies 同时包含 `version` 和 `path`。

## 验证

```bash
cargo fmt --all -- --check
cargo test --locked --workspace --exclude bm-desktop
cargo clippy --locked --workspace --exclude bm-desktop --all-targets -- -D warnings
# 在 macOS 上使用必需的 production profile 执行 desktop 门禁。
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
# PL2 feature-gated 发布矩阵；workspace 默认得到的 0-test 结果不是证据。
cargo test --locked -p bm-core --test post_turn_memory_governance_contract
cargo test --locked -p bm-sdk --test post_turn_deferred_governance_contract \
  --no-default-features --features nonproduction-replay-harness,sqlite-store
cargo test --locked -p bm-entry --no-default-features \
  --features governance-model-client-std,nonproduction-replay-harness
cargo test --locked -p bm-entry --no-default-features \
  --test post_turn_learning_capability_contract
cargo test --locked -p bm-store-contract-tests --no-default-features \
  --features sqlite-store
cargo clippy --locked -p bm-core --all-targets -- -D warnings
cargo clippy --locked -p bm-sdk --all-targets --no-default-features \
  --features nonproduction-replay-harness,sqlite-store -- -D warnings
cargo clippy --locked -p bm-entry --all-targets --no-default-features \
  --features governance-model-client-std,nonproduction-replay-harness -- -D warnings
bash scripts/check_platform_compile_gates.sh
bash scripts/check_deployment_runtime_contract.sh
bash scripts/check_next_gen_memory_plan.sh
bash scripts/check_production_hardening_contract.sh
bash scripts/check_release_surface.sh
```

构建缓存需要放到其他卷时，将 `BM_RELEASE_SURFACE_WORK_ROOT` 指向现有绝对目录；macOS launchd fixture 的 `TMPDIR` 仍保留在宿主文件系统。

缺少必要目标工具链的开发机可以在工程交接中将对应行记录为 `deferred_not_passed`。
任何发布候选都必须配置全部目标工具链并取得 strict GREEN；工具链缺失会阻断发布，不能算通过：

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```

## Publish Order

`bm-sdk` 是唯一的公开 persistence 发布面。`bm-store-contract-tests` 是不发布的验收门禁，必须在发布 `bm-sdk` 前通过。

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

通过 `scripts/check_release_surface.sh` 运行 staged `cargo publish --dry-run -p <crate>`。release gate 会运行 production hardening 检查、使用临时 Cargo target，并在 ignored artifact baseline 发生变化时失败。

## 回滚

- 首次真实发布前，任一门禁失败都应停止，在 `dev` 修复并冻结新候选；不得通过移动 `main` 或 release tag 掩盖失败候选。
- crates.io 版本不可变。已发布包存在缺陷时，不覆盖版本、不删除 tag 伪装回滚；停止对外宣布该版本，修复后按相同依赖顺序发布新的 patch 版本。
- 任何可能打开既有 store 的新二进制部署前，都必须在数据路径之外备份该精确 store。部署回滚恢复上一版已签名二进制及其匹配的 store 备份；archive import/export 不是隐式 schema migration 或回滚路径。
- release tag 只能从已验收的 `main` commit 创建，已存在的 release tag 永不重指向。

## 范围检查

- README、examples 和 crates 描述的是宿主无关的 memory runtime。
- Adapter crates 保持 memory write、recall、projection 和 store 语义由 `MemoryRuntime` 承担。
- Standalone deployment 覆盖 memory runtime 入口。产品专属表面和部署基础设施由宿主部署提供。
