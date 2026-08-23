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
bm-sdk = { version = "0.4.0", features = ["profile-desktop-macos-embedded-sdk"] }
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

## 5. 写入记忆

Procedural memory 是当前可直接写入的 reusable runtime knowledge 路径：

```rust
use bm_sdk::{MemoryWriteRequest, RuntimeSkillWrite, RuntimeSkillWriteSource};

let report = runtime.write(MemoryWriteRequest::Procedural {
    writes: vec![RuntimeSkillWrite {
        name: "release_guard".to_string(),
        topic: "release".to_string(),
        title: "Release guard".to_string(),
        summary: "Verify release artifacts before publishing.".to_string(),
        content: "Run examples, platform gates, and publish dry-run.".to_string(),
        citations: vec!["integration-guide".to_string()],
        source_chat_id: Some("chat-1".to_string()),
        observed_at: 1_800_000_000,
    }],
    source: RuntimeSkillWriteSource::Manual,
})?;

assert!(report.accepted);
```

Long-term extraction 写入应由 extraction pipeline 产生，然后通过 `MemoryWriteRequest::LongTermExtraction` 进入 runtime。

## 6. 召回与投影

```rust
use bm_sdk::{
    MemoryProjectionRequest, MemoryRecallRequest, MemoryRecallTemporalOperation, PressureLevel,
    RuntimeLifecycleModeInput,
};

let recall = runtime.recall(MemoryRecallRequest {
    temporal_operation: MemoryRecallTemporalOperation::Current,
    query: "release artifacts".to_string(),
    limit: 4,
    structured_query_facets: Vec::new(),
    tool_registry_refs: Vec::new(),
})?;

let projection = runtime.project(MemoryProjectionRequest {
    temporal_operation: MemoryRecallTemporalOperation::Current,
    user_query: "How should this host release?".to_string(),
    system_max_len: 4096,
    recent_messages_limit: 8,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
    structured_query_facets: Vec::new(),
    tool_registry_refs: Vec::new(),
})?;

let memory_block = projection.system_memory_block;
```

把 projected memory block 放进你的模型上下文组装流程。最终 system、developer、user、tool message 的排序仍由宿主 prompt assembly 负责。

## 7. 显式注入 LLM 后维护

`MemoryRuntime::maintain` 面向已经配置 LLM client 的宿主。通用 adapter 会拒绝 maintain，因为它不能替应用擅自决定 LLM/HTTP 边界。

```rust
let capabilities = runtime.capabilities();
if capabilities.lifecycle.maintain_lightweight.visible {
    // 在拥有 LLM injection 的宿主路径里调用 runtime.maintain(...)。
}
```

## 8. 提交记忆候选，不直接改存储面

宿主应该提交候选事实或流程，由 Beetle Memory 判断能不能写、写到哪个记忆面。
这样 SDK、HTTP、gateway、后续任意宿主都会走同一套记忆治理合同。

```rust
use bm_sdk::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPrivacyClass, MemorySemanticJudgmentSource, MemorySubjectVisibilityPolicy,
    MemoryWriteCandidate, MemoryWriteRequest,
};

runtime.write(MemoryWriteRequest::Candidates {
    runtime_skill_owning_scope: None,
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

如果 post-turn LLM 服务暂时不可用，`finalize_turn` 仍会先提交会话，
并在 StorePlatform 的 `post_turn_governance_jobs` 与
`post_turn_governance_scope_indexes` typed namespace 中原子建立 V2 intent。服务恢复后，SDK
先通过 `MemoryRuntime::claim_governance_job` 取得 exact lease，再调用
`MemoryRuntime::run_claimed_governance`；后者在首个网络字节前复核 transcript/privacy
authority，并把 memory post-image、job=`succeeded`、scope index 与 receipt 放入同一 backend
transaction。旧 `memory/governance_jobs/pending.json` 不再读取或迁移；检测到非空旧文件时会
返回 `legacy_governance_queue_reset_required`，必须由 operator 显式处理。宿主不能自己重做队列、
拼装 memory mutation 或用宿主语义重试。
若 canonical transcript 已提交但 intent admission 曾中断，调用
`MemoryRuntime::reconcile_governance_intents` 以 1 到 32 条的有界页修复当前 exact conversation；
cursor 与新建 intent 在同一 CAS 前进，不会扫描其他主体或会话。
运维面使用 `MemoryRuntime::deferred_governance_report()` 或 `inspect.deferred_governance`
查看当前 runtime scope 下的 pending / retrying / failed / terminal 计数、recent jobs、scope、subject、turn、reason 和 last error。

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
3. 用 `MemoryWriteRequest::Candidates` 提交事实、偏好、流程、诊断、subject hint 和 soul candidate。
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
