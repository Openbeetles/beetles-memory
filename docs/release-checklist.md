# Release Checklist

Release Surface 通过后，Beetle Memory 才进入可发布候选状态。

## Metadata

- `Cargo.toml` workspace internal dependencies 带 `version + path`。
- 每个可发布 crate 有 `description`。
- workspace license 固定为 `Apache-2.0`。
- 根目录存在 `LICENSE`。
- README 索引当前 docs 和 dev-docs 真源。

## Feature Matrix

- 七个 first-class profile 都有 capability snapshot fixture。
- ESP standalone / ESP embedded SDK 不拉入 sqlite。
- Linux device、desktop、Linux server profile 的 sqlite 能力只由 profile feature / store feature 打开。
- adapter crates 只依赖 `bm-adapter` / `bm-sdk`，不能直接依赖 `bm-core` 或 `bm-store`。

## Verification

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --no-deps --no-default-features \
  -p bm-core \
  -p bm-store \
  -p bm-sdk \
  -p bm-replay \
  -p bm-evolve \
  -p bm-adapter \
  -p bm-entry \
  -p bm-cli \
  -p bm-http \
  -p bm-wss \
  -p bm-mqtt \
  -p bm-mcp \
  -p bm-a2a
bash scripts/check_platform_compile_gates.sh
bash scripts/check_release_surface.sh
```

具备目标工具链的 release 环境追加：

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```

## Package Audit

可发布 crates：

- `bm-core`
- `bm-store`
- `bm-sdk`
- `bm-replay`
- `bm-evolve`
- `bm-adapter`
- `bm-entry`
- `bm-cli`
- `bm-http`
- `bm-wss`
- `bm-mqtt`
- `bm-mcp`
- `bm-a2a`

发布前必须执行 `scripts/check_release_surface.sh` 中的 staged `cargo publish --dry-run --allow-dirty -p <crate>`。内部 crate 尚未进入 registry 时，本地 dry-run 使用 `patch.crates-io` 指向同版本 workspace crate 来模拟发布顺序；正式发布前必须使用干净工作区并按 `bm-core -> bm-store -> bm-sdk -> replay/evolve/adapter -> bm-entry -> thin adapter crates` 顺序发布。

## Drift Red Lines

- public docs、examples、crate API 不能出现来源项目专属 adapter、source kind 或默认宿主绑定。
- Release Surface 不能扩展成 UI、管理控制台、workflow runner、skill marketplace 或真实网络 listener。
- 独立部署和 SDK 集成必须使用同一套 `MemoryRuntime` / store / adapter contract。
