# 集成文档

本文描述如何通过 `bm-sdk` 把 Beetle Memory 内嵌到 Rust 项目里。

## 1. 选择 Profile

按部署目标和运行角色选择 profile：

| 场景 | Profile feature | `ProfileId` |
| --- | --- | --- |
| Beetle Memory macOS 独立桌面 App | `profile-desktop-macos-standalone-memory` | `ProfileId::DesktopMacosStandaloneMemory` |
| macOS Rust desktop host | `profile-desktop-macos-embedded-sdk` | `ProfileId::DesktopMacosEmbeddedSdk` |
| Windows Rust desktop host | `profile-desktop-windows-embedded-sdk` | `ProfileId::DesktopWindowsEmbeddedSdk` |
| Linux Rust desktop host | `profile-desktop-linux-embedded-sdk` | `ProfileId::DesktopLinuxEmbeddedSdk` |
| Linux 硬件设备 runtime | `profile-linux-device-standalone-memory` | `ProfileId::LinuxDeviceStandaloneMemory` |
| Linux server memory gateway | `profile-server-linux-memory-gateway` | `ProfileId::ServerLinuxMemoryGateway` |
| ESP embedded SDK host | `profile-esp-embedded-sdk` | `ProfileId::EspEmbeddedSdk` |
| ESP standalone memory runtime | `profile-esp-standalone-memory` | `ProfileId::EspStandaloneMemory` |

## 2. 添加依赖

在本仓库内开发：

```toml
[dependencies]
bm-sdk = { path = "crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

crates 发布后：

```toml
[dependencies]
bm-sdk = { version = "0.8.0", features = ["profile-desktop-macos-embedded-sdk"] }
```

每次构建只使用一个 profile feature。Linux desktop、Linux device 与 Linux server 是三个不同部署目标，禁止相互替代。

## 3. 打开 Store

测试和短生命周期 session：

```rust
use bm_sdk::{MemoryStoreHandle, ProfileId, StoreBackendConfig};

let profile = ProfileId::DesktopMacosEmbeddedSdk;
let store = MemoryStoreHandle::open(StoreBackendConfig::in_memory(profile)?)?;
```

持久化 desktop 或 server storage：

```rust
let store = MemoryStoreHandle::open(StoreBackendConfig::file(
    "/var/lib/beetle-memory",
    ProfileId::ServerLinuxMemoryGateway,
)?)?;
```

SQLite storage：

```rust
let store = MemoryStoreHandle::open(StoreBackendConfig::sqlite(
    "/var/lib/beetle-memory/memory.sqlite3",
    ProfileId::ServerLinuxMemoryGateway,
)?)?;
```

ESP profile 应使用 `StoreBackendConfig::embedded(profile)` 或 `in_memory(profile)`。

## 4. 构建 Runtime

```rust
use bm_sdk::{AgentSkillDirConfig, MemoryIdentity, MemoryRuntime, MemoryScope};

let runtime = MemoryRuntime::builder()
    .identity(MemoryIdentity::new("agent-main", "owner-default")?)
    .scope(MemoryScope::new("local", "chat-1")?)
    .store(store)
    .add_agent_skill_dir(AgentSkillDirConfig::read_only("./skills", "host-project"))
    .build()?;
```

`agent_id` 标识 agent 实例。`owner_id` 标识 owner 或 tenant。普通 single-agent 宿主不需要传 `subject_id`：SDK 会自动生成 `space:<owner_id>` 和默认 `agent:<agent_id>` 主体，并隐藏 `system_governor` / `human_user` / relationship graph 细节。只有高级多主体宿主才显式配置 subject registry、relationship graph 或 mounted subject。`channel` 和 `chat_id` 定义 runtime 操作的默认 memory scope。

`add_agent_skill_dir` 是可选只读挂载。标准 Agent Skill 的添加、编辑、导入、删除和执行仍归宿主；Beetle Memory 只扫描 `SKILL.md` 摘要参与召回和投影。

## 5. 提交受治理的记忆输入

宿主可以提交 typed long-term candidate 或 canonical extraction 结果。Runtime Skill
和 Agent Tool 经验不是 caller-authored record：它们只能在 `finalize_turn` 之后由受治理的
post-turn learning worker 创建。第 8 节给出了公共 candidate write 合同。

## 6. 召回与投影

```rust
use bm_sdk::{
    MemoryProjectionRequest, MemoryRecallRequest, MemoryRecallTemporalOperation, PressureLevel,
    ProceduralProjectionBindingV1, RuntimeLifecycleModeInput,
};

let recall = runtime.recall(MemoryRecallRequest {
    temporal_operation: MemoryRecallTemporalOperation::Current,
    query: "release artifacts".to_string(),
    limit: 4,
    structured_query_facets: Vec::new(),
    tool_registry_refs: Vec::new(),
})?;

let projection = runtime.project(MemoryProjectionRequest {
    binding: ProceduralProjectionBindingV1::Preview,
    temporal_operation: MemoryRecallTemporalOperation::Current,
    user_query: "How should this host release?".to_string(),
    system_max_len: 4096,
    recent_messages_limit: 8,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
    structured_query_facets: Vec::new(),
    tool_registry_refs: Vec::new(),
})?;

let preview_block = projection.provider_payload().system_memory_block();
```

`Preview` 只用于检查，不返回 selection receipt。真实 model/tool turn 必须由宿主在请求入口
一次性选择稳定 turn id，再用 `ProceduralProjectionBindingV1::Turn { turn_id }` 投影并把
该 memory block 放入模型上下文。完成同一个 canonical turn 时，要在
`PostTurnLearningInputV2` 中原样提交完整 `selection_receipt`。同一 turn 的重试复用 id 与
receipt；即使输入正文相同，另一个 turn 也必须使用新 id。禁止从 selected ids 或 digest
重建 receipt。

0.8.0 中，有反馈的提交还必须使用显式签发的
`MemoryProceduralSubmissionCapability` 和 `finalize_turn_with_procedural_evidence`。
无反馈普通回合仍直接使用 `finalize_turn`，不要求 producer grant。
受信初始化、部分接受、撤权与容量恢复见 [程序性证据声明权](api.md#程序性证据声明权)。
这不是与已发布 0.7.0 feedback input 兼容的合同。

重开后通过 `MemoryRuntime::procedural_reconciliation_status` 或官方服务 attachment 的
`status().procedural_reconciliation` 发现当前重算状态。容量阻断返回绑定主体范围的
job/revision 引用；解决容量问题后，仍须使用原有 SystemGovernor 控制能力显式恢复。
不要另存恢复任务目录、把旧报告当作当前授权，或期待 wake/重开自动解除阻断。

### 可编译的官方学习服务示例

现有 Rust embedded 工程的 [learning-service 入口](../../examples/rust-sdk-embedded/src/bin/learning-service.rs)
展示同一 Store/SubjectRegistry 下两个主体的 Runtime、按 mounted scope 的 SystemGovernor 控制能力、
官方服务附着、一次显式 producer 注册、typed 执行事实/方法提交、无反馈普通回合和有期限的关闭。
两个主体分别使用自己的 `chat_id` 和 `conversation_id`；共享 Store/MemorySpace 不等于共享一条主体拥有的会话。
它不使用测试专用 harness，也不创建宿主自己的 worker、grant 表或学习规则。

在仓库根目录静态检查（不启动服务）：

```sh
cargo check --manifest-path examples/rust-sdk-embedded/Cargo.toml --features learning-service --all-targets
cargo clippy --manifest-path examples/rust-sdk-embedded/Cargo.toml --features learning-service --all-targets -- -D warnings
```

Linux/Windows 配置分别增加 `--no-default-features --features desktop-linux,learning-service`
或 `--no-default-features --features desktop-windows,learning-service`，替代原 `--features`。
在 macOS 上检查这些 feature 只证明配置编译，不证明 Windows/Linux 目标运行。
编译缓存可通过 `CARGO_TARGET_DIR` 指向外置工作目录。

如需亲自运行合成演示，使用 `cargo run --manifest-path examples/rust-sdk-embedded/Cargo.toml --features learning-service --bin learning-service`。
运行时仅创建进程内 InMemory Store 和官方后台线程，执行固定合成文本的词数统计；不打开真实数据，
不监听网络，不读取 Key，也不调用 Provider。未配置模型时，提交成功不等于语义学习完成；一次方法见证也不等于方法已激活或 Skill 已晋升。
该示例没有验证持久重开或等待全部后台任务结束，不能当作这些能力的验收证明。

示例的 `expected_revision: None` 只用于本次新建的易失 Store。持久宿主应保存/查询既有 binding 引用并显式管理其 revision，
不能在每次启动或每个回合自动重授权限。真实模型配置与凭证分别通过既有 `GovernanceBindingSource`、
`GovernanceCredentialResolver` 接入产品的同一配置真源；本示例刻意不配置它们。
行为变更与发布边界见 [0.8.0 源码发布说明](release-notes-0.8.0.md)。

system、developer、user、tool message 的最终排序仍归宿主 prompt assembly。

## 7. 显式注入 LLM 后维护

`MemoryRuntime::maintain` 面向已经配置 LLM client 的宿主。通用 adapter 会拒绝 maintain，因为它不能替应用擅自决定 LLM/HTTP 边界。

```rust
let capabilities = runtime.capabilities();
if capabilities.lifecycle.maintain_lightweight.visible {
    // 在拥有 LLM injection 的宿主路径里调用 runtime.maintain(...)。
}
```

## 8. 提交记忆候选，不直接改存储面

宿主应该提交候选事实或受治理证据，由 Beetle Memory 判断能不能写、写到哪个记忆面。
这样 SDK、HTTP、gateway、后续任意宿主都会走同一套记忆治理合同。

```rust
use bm_sdk::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPrivacyClass, MemorySemanticJudgmentSource, MemorySubjectVisibilityPolicy,
    MemoryWriteCandidate, MemoryWriteRequest,
};

runtime.write(MemoryWriteRequest::Candidates {
    candidates: vec![MemoryWriteCandidate {
        candidate_id: "turn-1:preferred-name".to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Profile,
            topic: "preferred_name".to_string(),
        },
        long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "preferred_name".to_string(),
            body: "The user prefers to be called Qingchuan.".to_string(),
            keywords: vec!["name".to_string()],
        },
        evidence_refs: vec!["chat-1:turn-1".to_string()],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::RuntimeGate,
            decision: MemoryCandidateSemanticDecision::Accept,
            governed_target: None,
            reason: "运行时门禁接受了这条显式用户陈述".to_string(),
        }),
    }],
})?;
```

如果 post-turn LLM 服务暂时不可用，`finalize_turn` 仍会先提交会话，并原子建立 exact Job V3
governance intent。durable intent 是恢复真源，wake signal 只是一条可丢失提示。

生产宿主应把官方 `bm-entry::MemoryLearningService` attach 到已有
`Arc<MemoryRuntime>`。service 只消费 SDK-owned `MemoryLearningEngine`；bounded discovery、
reconciliation、lease/CAS fencing、当前 transcript/subject/privacy 准入、最小 Provider 外发、
候选严格验证、retry classification、memory mutation 与 terminal receipt/audit closure 全部只由
Engine 拥有。只有 Store、Subject Registry 与 MemorySpace authority 完全一致的其他 Runtime 才能
attach。Credential 或 Provider 变化必须使用 typed notification 重新读取同一份宿主配置真源；
raw credential 永不持久化。

宿主不得自行 claim job、运行 governance transition、拼装 memory mutation，也不得维护第二套
queue/worker/retry policy。Operator 与 attachment status read 必须携带 SDK 铸造的 typed inspection authority；
无权或跨主体请求必须在返回 job identity 或 reason detail 前失败。Store v13 及更早 schema
直接拒绝；可丢弃的开发 Store 由 owner 明确重建。v0.8.0 不自动删除真实数据，也不提供自动迁移或兼容 reader。

`project()` 返回的 `MemoryProjectionReport.audit` 是投影诊断真源，包含 source plane、selected ids、
section chars、source/render budget、scope 和 private gate decision。宿主可以展示这些字段，
但不能读取 store internals 后自行解释 projection。

需要主动执行保守压缩时，调用 `MemoryRuntime::run_retention_compaction()`。该入口只运行 SDK-owned
hygiene / factual evidence metadata compaction / runtime skill governance，并在 report 中声明
`host_direct_deletion_allowed=false`；宿主不能因配额压力删除已接受记忆。

## 9. 管理已接受长期记忆

用户后续要求查看、纠正、删除、遗忘或限制长期记忆时，宿主应调用长期记忆控制面。宿主可以负责自然语言理解和 UI 展示，但不能在自己的本地 DB 里维护一套 shadow memory。

```rust
use bm_sdk::{
    MemoryLongTermControlView, MemoryLongTermListRequest, MemoryLongTermMutation,
    MemoryLongTermMutationRequest, MemoryLongTermTarget, LongTermMemoryQuery,
    RuntimeLifecycleModeInput,
};

let page = runtime.list_long_term_memory(MemoryLongTermListRequest {
    query: LongTermMemoryQuery {
        topic: Some("preferred_editor".to_string()),
        limit: 8,
        ..LongTermMemoryQuery::default()
    },
    cursor: None,
    limit: 8,
    view: MemoryLongTermControlView::HostUi,
})?;

if let Some(record) = page.records.first() {
    let report = runtime.mutate_long_term_memory(MemoryLongTermMutationRequest {
        operation: MemoryLongTermMutation::Delete {
            target: MemoryLongTermTarget::RecordId(record.record.id.clone()),
        },
        reason: "user requested deletion".to_string(),
        dry_run: false,
        mode_input: RuntimeLifecycleModeInput::default(),
    })?;
    assert!(report.accepted);
}
```

宿主不能通过 `LongTermMemoryDraft` 声明确认。普通 create、upsert、extraction、
Adapter metadata、citation 和 observer log 都保持未确认。每次显式
`MemoryLongTermMutation::Correct` 都会生成不可变 typed correction evidence，即使
replacement 正文没有变化；confirmation 是另一份证据，只有 exact actor 在当前
`SubjectRegistry` 中是 active `HumanUser` 时，SDK 才会增加 typed human-confirmation
evidence。`AgentPersona`、`SystemGovernor` 或 suspended human 发起的纠正仍未确认。
两类 evidence 都精确绑定 MemorySpace、actor、predecessor/successor、时间和同事务
control revision；`Supersede` 会清除 confirmation，不把它转移给另一个 owner。

### Adapter V2 持久 mutation 结果

Adapter V2 为 `Write` 与 `LongTermMutate` 提供 Store-owned durable operation receipt。
首次成功执行返回带 receipt 的 `Accepted`；相同 operation identity 与相同 canonical
intent 的重试返回带同一 receipt 的终态成功 `Replayed`，不会追加第二次 effect、revision、
audit 或 event；同 identity 对应不同 intent 则 typed conflict。

durable `Write` 与 `LongTermMutate` 必须由 transport 调用方提供稳定、非敏感的 idempotency
key；Adapter 在持久化前对它做哈希，缺 key 时直接拒绝，不会代填一个响应丢失后无法复现的
一次性 identity。Adapter V1 只接受读取。其它 V2 mutation 被明确标为 non-durable，或由独立
domain receipt 拥有；调用方应同时读取 Adapter capability report 与 SDK mutation inventory，
不能假设全局 exactly-once。receipt 会一直 pinned，直至 Store 容量耗尽；容量不足时整批 fail
closed，绝不静默驱逐旧 receipt。

`forget_by_query` 这类批量遗忘必须先 dry-run preview，再带 confirmation token 执行。`MemoryLongTermPolicyRequest` 用于“以后不要记这类事情”或暂停某个 scope 的未来长期记忆更新；policy 不 retroactively 删除已接受记录。

Transcript lifecycle 的 raw delete/mask 只处理 conversation evidence。它会报告受影响的 `DerivedMemoryRef`，但撤销对应长期记忆仍然要走 `mutate_long_term_memory`。运行时 Skill 的 edit/retire 只管理 procedural memory 中的 runtime skill，不是普通长期记忆管理面；retire 追加 terminal revision 并保留 lineage，不物理删除 owner。

## 10. 宿主回合生命周期

完整 SDK 宿主回合只走一条 public path：

1. 打开 `MemoryStoreHandle`，并通过 `MemoryRuntime::builder().store(...)` 注入；persistence engine、raw transaction 和 writable store trait 不是公开 runtime path。
2. 用稳定的 owner、agent、channel、conversation id 构建 `MemoryIdentity` 和 `MemoryScope`。
3. 用 `MemoryWriteRequest::Candidates` 提交事实候选。调用方自写的流程及改投程序性目标会被拒绝；真实执行证据通过 `finalize_turn` 进入受治理的程序性学习。
4. 需要 transcript governance 时，通过 canonical turn 语义 finalize 当前回合。
5. 用 `recall` 和 `project` 生成模型上下文；宿主不自己拼 memory plane。
6. 用 `inspect` 提供运维可见性和安全恢复上下文。
7. 替换或发布闸口走 typed memory-space export、直接同 scope import 和 replay。

`fixtures/sdk-host-readiness/` 里的 generic host fixture 与 Beetle-derived fixture 走同一条路径。Beetle-derived 数据只是当前合同的 host evidence，不是 SDK 特殊分支或兼容分支。

## 11. Archive Import 与 Replay

```rust
use bm_sdk::{
    MemoryArchiveScope, MemoryReplayRequest, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy,
};

let replay = runtime.replay(MemoryReplayRequest {
    chat_id: "chat-1".to_string(),
    limit: 32,
})?;

let scope = MemoryArchiveScope::subject(
    runtime.memory_space_id(),
    runtime.subject_id(),
)?;
let private_material_policy = MemorySpacePrivateMaterialPolicy::IncludePrivate;
let exported = runtime.export_memory_space(MemorySpaceExportRequest {
    scope: scope.clone(),
    private_material_policy,
})?;

assert_eq!(&exported.archive.root().scope, &scope);
target_runtime.import_memory_space(MemorySpaceImportRequest {
    scope,
    expected_private_material_policy: private_material_policy,
    archive: exported.archive,
})?;
```

公开恢复不接受自由拼装的 continuity snapshot。source 与 target runtime、request 和 opaque archive root 必须在 replacement 前声明完全相同的 `MemoryArchiveScope` 与 private-material policy。

Archive root 是可携带的 integrity report。调用方应检查 schema id/version、精确 scope、private-material policy、JSON/event count 与 byte count，以及 canonical `closure_sha256`。Import 会在任何 backend mutation 之前重算该 root，并只原子替换声明的 scope。bootstrap/full continuity mode 只属于内部 Soul-recovery bundle。

## 11. Operator Inspect

```rust
use bm_sdk::{MemoryInspectionRequest, PressureLevel, RuntimeLifecycleModeInput};

let inspect = runtime.inspect(MemoryInspectionRequest {
    query: "archive readiness".to_string(),
    system_max_len: 4096,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
})?;

assert!(inspect.capabilities.inspection.visible);
```

Operator inspect 是 selected id、plane evidence、capability visibility、deferred governance queue、
lifecycle diagnosis 和 safe action 的支持路径。宿主 UI 可以展示这个 report，但不能从私有 store 文件推断写入决策、replay 状态或 projection 内容。

## 12. 宿主禁区

宿主禁止：

- 直接写 memory plane 文件；
- 在 `MemoryRuntime` 外决定 plane routing；
- 维护第二套 long-term extraction、subject、soul、private garden 或 procedural write policy；
- 读取 store internals 后自己拼 memory projection；
- 把 Beetle、IDE、Ollama 或设备通道当成内核 source kind；
- 吞掉 deferred governance job，或用宿主自有语义重试；
- 为兼容旧字段污染当前 SDK 合同。

## 13. 暴露 UI 或工具前检查能力

```rust
let catalog = runtime.capabilities();
if catalog.adapter.http.visible {
    // 当前 profile/policy/privacy 组合可以暴露 HTTP。
}
```

不要因为 crate 能编译就暴露某个协议或操作。Capability catalog 才是运行时真相。

## 14. 建议宿主测试

集成项目至少增加一个 smoke test：

1. 通过 `MemoryStoreHandle` 打开选定 backend。
2. 通过 `MemoryRuntime::builder().store(handle)` 构建 `MemoryRuntime`。
3. 写入一条 `MemoryWriteCandidate`，检查 governance report。
4. 在维护不可用时 finalize 一轮 turn，验证 deferred job。
5. 检查 `deferred_governance_report()` 和 `inspect.deferred_governance`。
6. 从另一个 chat 召回或投影 candidate 写入的记忆，并检查 `MemoryProjectionReport.audit`。
7. 调用 `run_retention_compaction()`，确认不授权宿主删除已接受记忆。
8. 导出 opaque archive，将其导入具有完全相同 typed scope 与 policy 的 runtime，并检查 governed archive root。
9. 对替换后的 scope 运行 operator inspect 和 replay。
