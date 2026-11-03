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

源码 tag 按本轮改动涉及的合同选择检查，并复用仍适用的工程验收证据。仅改版本号不触发全仓和全部平台 profile 重跑；未执行的检查记为未验收。下列广矩阵仅在受影响范围或实际平台对接需要时运行。

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

构建缓存需要放到其他卷时，将 `BM_RELEASE_SURFACE_WORK_ROOT` 指向现有绝对目录；macOS launchd fixture 的 `TMPDIR` 仍保留在宿主文件系统。

实际对接或修改某个目标平台时，须配置其工具链并取得对应目标门禁结果。工具链缺失记为 `deferred_not_passed`，不能算通过；全部目标平台都在范围内时再运行 strict 全矩阵：

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

真正向 crates.io 上传前，通过 `scripts/check_release_surface.sh` 运行 staged `cargo publish --dry-run -p <crate>`。release gate 会运行 production hardening 检查、使用临时 Cargo target，并在 ignored artifact baseline 发生变化时失败。只打源码 tag、不上传 crate 时，不以 13 包 dry-run 为前置条件。

## 回滚

- 首次向 crates.io 上传前，任何 crate 发布门禁失败都应停止，在 `dev` 修复并冻结新候选；不得通过移动 `main` 或 release tag 掩盖失败的 crate 候选。
- crates.io 版本不可变。已发布包存在缺陷时，不覆盖版本、不删除 tag 伪装回滚；停止对外宣布该版本，修复后按相同依赖顺序发布新的 patch 版本。
- 任何可能打开既有 store 的新二进制部署前，都必须在数据路径之外备份该精确 store。部署回滚恢复上一版已签名二进制及其匹配的 store 备份；archive import/export 不是隐式 schema migration 或回滚路径。
- release tag 只能从已验收的 `main` commit 创建，已存在的 release tag 永不重指向。

## 范围检查

- README、examples 和 crates 描述的是宿主无关的 memory runtime。
- Adapter crates 保持 memory write、recall、projection 和 store 语义由 `MemoryRuntime` 承担。
- Standalone deployment 覆盖 memory runtime 入口。产品专属表面和部署基础设施由宿主部署提供。

## 0.11.0 候选范围

验收主体过滤后的长期记忆分页、持久规则查询与精确解除/幂等、Transcript 事实证据原子撤回，以及对旧排队/执行中学习任务的来源入场屏障。要求 Store v16 / material v6 / control revision v4。复用 LCI1 的 InMemory/File/SQLite 重开、current/as-of/delivery 隐私、CAS/budget/closure 证据，并重跑本轮受影响合同；核对 macOS 桌面合同、版本、公开文档与源码清单。Windows/Linux/ESP 对接与 13 包 dry-run 留到对应平台或 crates.io 发布进入范围时。第三方 lock records 与旧 tag 不变，不提供兼容 reader 或 automatic migration。完整多方法对照实验、真实 Provider 质量和 GUI/UAT 属于独立证据平面。
