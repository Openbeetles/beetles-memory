# API 表面

SDK API 是主要入口。宿主项目应通过 `bm-sdk` 进入，或通过 `bm-entry` 加协议 adapter 进入；不应自行实现记忆 schema、store envelope、replay 格式或 adapter dispatch 规则。

## Crates

| Crate | 责任 |
| --- | --- |
| `bm-core` | 记忆平面、召回、投影、生命周期、feature 合同和核心错误模型。 |
| `bm-sdk` | `MemoryRuntime` facade、不透明 `MemoryStoreHandle`、request/report 类型、capability catalog、profile snapshot 和私有 persistence kernel。 |
| `bm-store-contract-tests` | 不发布的开发合同测试，覆盖 `bm-sdk` persistence kernel。 |
| `bm-replay` | 开发用 fixture runner、cross-store replay、harness gate 和 benchmark gate；`nonproduction-replay-harness` 不是部署能力。 |
| `bm-evolve` | proposal-only evolution sandbox 和 SDK 写入 helper。 |
| `bm-adapter` | 协议无关 envelope、command、policy、dispatch 和 response 合同。 |
| `bm-entry` | 进程级 runtime opening、profile/auth/source/idempotency 归一化和 adapter response envelope。 |
| `bm-cli` | CLI 命令、capability rendering、platform snapshot 和 memory command execution。 |
| `bm-http`, `bm-wss`, `bm-mcp`, `bm-a2a` | 消费 `bm-entry` 或 `bm-adapter` 的轻量 transport shell，不拥有记忆语义。 |
| `bm-ollama-transparent` | 已发布的 macOS 本地 Ollama App 透明模式 controller，负责跨进程 OS transition lease、精确 PID/start/executable receipt、经验证的 executable 启动与可恢复 `launchd` job authority，以及有界 process/probe report。调用方必须显式提供绝对 gateway path 与 typed memory authority；模型与记忆语义仍由 `bm-llm-gateway` 持有。 |

## Runtime 操作

| 操作 | SDK method | 用途 |
| --- | --- | --- |
| Write | `MemoryRuntime::write` | 存储 procedural memory 或 long-term extraction 结果。 |
| Recall | `MemoryRuntime::recall` | 按 query 取回 memory hits。 |
| Project | `MemoryRuntime::project` | 生成受长度限制的模型上下文 memory block。 |
| Maintain | `MemoryRuntime::maintain` | 在显式配置 LLM client 后执行 post-reply memory maintenance。 |
| Inspect | `MemoryRuntime::inspect` | 返回 recall/operator/lifecycle inspection 数据。 |
| Runtime Skill List / Detail | `MemoryRuntime::list_runtime_skills` / `MemoryRuntime::get_runtime_skill` | 列出和查看运行时沉淀的 procedural memory record，不执行 skill。 |
| Runtime Skill Mutation | `MemoryRuntime::edit_runtime_skill` / `MemoryRuntime::set_runtime_skill_enabled` / `MemoryRuntime::retire_runtime_skill` | 只允许编辑、启停或退役已存在的运行时 Skill；不提供新建、导入或托管标准 Agent Skill。 |
| Long-Term Memory List / Detail | `MemoryRuntime::list_long_term_memory` / `MemoryRuntime::get_long_term_memory` | 列出、搜索、查看已接受的长期记忆，返回脱敏 view、evidence summary、revision/tombstone 信息。 |
| Long-Term Memory Mutation | `MemoryRuntime::mutate_long_term_memory` | 对已接受长期记忆执行 correct、supersede、delete、forget_by_query、mark_stale、change_scope，并返回 affected records、tombstone、projection impact 和 lifecycle report。 |
| Long-Term Governance Policy | `MemoryRuntime::mutate_memory_governance_policy` | 暂停、恢复或 suppress 后续长期记忆更新；影响未来写入治理，不静默删除已接受记忆。 |
| Agent Skill Directory | `MemoryRuntimeBuilder::agent_skill_dirs` / `add_agent_skill_dir` | 宿主把标准 Agent Skill 目录交给 SDK 只读扫描；SDK 只召回和投影摘要，不添加、不编辑、不执行目录资源。 |
| Agent Tool Registry | `MemoryRuntimeBuilder::agent_tool_registry` / `MemoryRuntime::upsert_agent_tool_registry` | 宿主注册工具索引和 fingerprint；SDK 只基于已治理工具经验返回 `agent_tool_hints`，无经验返回空，不做工具路由。 |
| Replay | `MemoryRuntime::replay` | 检查某个 chat 的 turn ledger 历史。 |
| Transcript Attr Write | `MemoryRuntime::record_transcript_attrs` | 给 transcript evidence 追加受治理的 turn/message metadata，用于 replay、export、redaction、repair 和 profile budget。 |
| Memory-Space Export / Import | `MemoryRuntime::export_memory_space` / `MemoryRuntime::import_memory_space` | 在显式 private-material policy 下导出 opaque archive，并原子替换完全相同的 `MemoryArchiveScope`。 |
| Recover / Close | `MemoryRuntime::recover` / `MemoryRuntime::close` | 控制 runtime lifecycle 并产生 lifecycle report。 |

## Universal Long-Term Learning

`finalize_turn` 会提交 delivered canonical turn 和唯一 durable governance intent。之后由 `bm-sdk::MemoryLearningEngine` 统一拥有完整 due-job cycle：exact scope discovery、lease/CAS、当前 transcript/subject/privacy 准入、最小 Provider 外发、候选严格验证、accepted long-term mutation、receipt/audit、retry、blocking、cancellation 与 terminal completion。生产宿主不得再把旧 low-level worker transition 拼成第二套 Worker。

`bm-entry::MemoryLearningService` 是官方异步执行的 process owner。它从已有 `Arc<MemoryRuntime>`、唯一 `GovernanceBindingSource` 和唯一 `GovernanceCredentialResolver` 启动；只有 Store authority、Subject Registry 与 MemorySpace 全部一致的其他 Runtime 才能 attach。`EntryRuntime` 内部也消费同一 service，因此 server 与 embedded consumer 不存在两套 governance 状态机。

```rust,ignore
let control_authorities = governor_runtime.learning_service_control_authorities()?;
let (service, attachment) = MemoryLearningService::builder(runtime.clone())
    .control_authorities(control_authorities)
    .binding_source(binding_source)
    .credential_resolver(credential_resolver)
    .start()?;

let another_control = another_governor_runtime.learning_service_control_authorities()?;
let another = service.attach_runtime(another_runtime, another_control)?;
service.wake();
service.credential_changed("product.primary-key", 2, "credential-op-2")?;
```

产品 Provider 配置继续由宿主拥有。Beetle 只把不含 secret 的 immutable execution binding snapshot 持久化为 job 历史 authority，绝不保存 raw credential。`provider_config_changed`、`credential_changed` 和 `provider_permission_changed` 是重新读取同一 source、带 operation-aware Store receipt 的 typed notification，不是第二份配置 payload。

Status read 与 recovery control 必须携带 SDK 铸造的 opaque capability。`MemoryRuntime::learning_service_status_authority` 和 `MemoryRuntime::learning_service_control_authorities` 要求 Runtime actor 本身就是 exact active governing `SystemGovernor`；其中每个 recovery authority 绑定 exact Store、registry、MemorySpace、mounted subject、scope 与 recovery kind。`MemoryRuntime::learning_attachment_status_authority` 要求 exact active mounted-subject actor。跨主体、跨 operation 或 foreign Store/registry authority 必须在返回 job identity、reason detail 或执行 mutation 前失败。

## Subject Soul Provisioning

`bm-sdk` 0.6.0 提供宿主无关的 Subject Soul 建档与生命周期公共合同。宿主只能提交 typed intent；Soul revision、generation、material、manifest、ledger、审计、事件和 durable operation receipt 由 Core、SDK 与 Store 在同一事务中拥有。Adapter、HTTP、MCP、Console 或宿主数据库不得维护第二套 Soul 状态，也不得先写默认人格再覆盖。

| 操作 | SDK surface | 合同 |
| --- | --- | --- |
| 可选首次建档 | `MemoryRuntime::provision_subject_soul` + `SubjectSoulProvisionIntentV1` | `Unseeded` 是零 mutation 的合法状态；`Founding` 只接受同一 MemorySpace 中 active `HumanUser` 的 canonical partial charter，并原子创建 generation 1 / revision 1。 |
| 安全读取 | `MemoryRuntime::read_subject_soul` + `SubjectSoulReadRequestV1` | 公共读取只允许 `OperatorSafe` metadata；`Current` 与 `Exact` selector 由 immutable closure 验证，terminated generation 只返回 tombstone metadata。 |
| 安全导出 | `MemoryRuntime::export_subject_soul_operator_safe` | 仅返回 state、generation、revision、digest、origin 和安全 tombstone；不返回 founding charter、SelfAuthoredCore、Private Garden、Inner Life、private docs 或关系私密正文。 |
| 受治理披露 | `MemoryRuntime::disclose_subject_soul_governed` | 只消费 Store-verified Soul/relationship closure，并按 MentalPrivacy 与 Relationship Source 的有效 disclosure ceiling 返回受治理摘要、改写或拒绝；宿主不能传入自称安全的摘要。 |
| 生命周期 | `MemoryRuntime::archive_subject_soul_self_governed` / `restore_subject_soul_self_governed` / `mutate_subject_soul` | self-governed archive/restore 的 capability 由 SDK 内部注入，不向 caller 暴露；maintenance archive/restore 使用 typed `SystemGovernor`；reset/reseed/delete 必须同时绑定 `SystemGovernor` 与同空间 active `HumanUser` confirmation、exact generation/head/manifest。 |
| 关系来源 | `MemoryRuntime::control_relationship_source` / `read_relationship_source` | 公共 contribution 只接受 exact relationship member `HumanUser`；Agent self-boundary 与 SystemGovernor floor 由 SDK 内部 capability owner 执行。Relationship Source Constitution 的 source root 与 manifest root 使用独立双 root / 四 CAS，Soul lifecycle 不能代替关系治理。 |
| 受治理投影 | `MemoryRuntime::project_with_subject_soul_selector` | current projection 读取 verified current Soul；historical projection 必须显式提供 exact Soul selector，不能把当前 Soul 错套到历史 memory projection。 |

Founding charter 是可选、可部分提供的主体宪章，不是 raw 人物画像。它只承载 identity anchor、character tendencies、priority/non-negotiable constitution、默认回应/主动性/关系姿态，以及边界、求真、自保、修复和变更原则。Display name、用户称呼、外貌背景、任务角色、工具习惯和宿主 presentation 仍由宿主相应 owner 管理，不能借建档晋升为 Soul。

建档成功后，任何人格变化都必须进入既有 self-authored revision proposal/governance；宿主不能每轮重复 provision。reset/reseed/delete 是独立破坏性生命周期：旧 generation 的 raw material 与派生私域数据必须在同一事务中清除，旧 exact selector 只能得到安全 tombstone。SPV1 不定义 raw Soul import、Portable Vault、加密 wire 或密钥生命周期；这些继续由 EAP2 拥有。

所有失败都通过 `SubjectSoulSdkError { operation, key, disposition }` 返回 typed 结果。调用方应对 `ExpectedStateConflict` 重读 verified state 后重试；`RepairRequired`、`AuthorityRejected`、`CapacityRejected` 或 `StoreCommitRejected` 不得降级为直接写 Store。

Generation-owned Soul layer envelope、自治 capability、Core revision plan 和 Store post-image 都不是公共宿主写入面。SDK 自己从持久 governance job、verified Soul snapshot 与 typed evidence 运行自治裁决，并在一个 operation-aware Store batch 内提交；宿主不能提交 `origin`、`revision`、`next_core`、ledger 或 raw private-layer JSON 来声称“自主演化”。

## Memory Evidence System

Conversation Transcript Substrate 发布面是当前已落地的基础证据合同，用于 governed transcript commit、redacted replay、lifecycle review 和 archive-ready evidence handling。它不是宿主任务系统，也不替代 Soul Governance、Subject Projection、Program Memory、procedural memory 或已接受的长期记忆平面。

owner 仍然是 `MemoryRuntime`：宿主和 adapter 只提供 delivered turn delta、actor attribution 和 opaque host refs；Beetle Memory 负责提交 evidence、执行治理并返回 report。外部代码不能另写一套 transcript store，也不能从 raw conversation history 自行推断 memory facts。

`MemoryScope::new(channel, chat_id)` 仍是单 agent 默认接入形态。若宿主有区别于 legacy chat id 的稳定 conversation id，可使用 `MemoryScope::with_conversation_id(...)`；`finalize_turn` 和 `commit_transcript` 也会记住最近一次提交的 transcript conversation，供后续 recall、projection、maintenance 和 inspection 使用。

SDK transcript 操作：

| 操作 | SDK surface | 用途 |
| --- | --- | --- |
| Transcript Commit | `MemoryRuntime::finalize_turn` + `CanonicalTurnDelta`；手动提交使用 `MemoryTranscriptCommitRequest` / `MemoryTranscriptCommitReport`，通过 `MemoryRuntime::commit_transcript` 调用 | 将 delivered turn 作为 governed evidence 提交到 `memory_space_id + channel_id + conversation_id`。 |
| Redacted Transcript Replay | `MemoryTranscriptReplayRequest` / `MemoryTranscriptReplayReport`，通过 `MemoryRuntime::replay_transcript` 调用 | 通过 model context、host UI、operator audit 或 export 等分层视图读取 transcript evidence。 |
| Conversation Catalog | `MemoryConversationListRequest` / `MemoryConversationListReport`，通过 `MemoryRuntime::list_conversations` 调用 | 列出当前 mounted subject 可见且已有受治理 evidence 的 conversations，可按 channel 与 lifecycle 过滤。 |
| Transcript Timeline | `MemoryTranscriptTimelineRequest` / `MemoryTranscriptTimelineReport`，通过 `MemoryRuntime::query_transcript_timeline` 调用 | 围绕 `Latest`、`Before`、`After`、durable anchor、sequence、UTC time 或 UTC range 内第一条可见消息读取同一 conversation。 |
| Transcript Search | `MemoryTranscriptSearchRequest` / `MemoryTranscriptSearchReport`，通过 `MemoryRuntime::search_transcripts` 调用 | 在 exact conversation 或当前 mounted subject 范围搜索可见 transcript text，返回受治理 excerpt 与可进入 timeline 的 anchor。 |
| Transcript Activity | `MemoryTranscriptActivityRequest` / `MemoryTranscriptActivityReport`，通过 `MemoryRuntime::query_transcript_activity` 调用 | 计算有界 UTC 半开区间内的可见消息数及首末 anchor，供日期导航使用。 |
| Transcript Lifecycle | `MemoryTranscriptLifecycleRequest` / `MemoryTranscriptLifecycleReport`，通过 `MemoryRuntime::request_transcript_lifecycle` 调用 | 执行 archive、mask、delete raw content 或 lifecycle review，并产出 audit。 |
| Transcript Repair | `MemoryTranscriptRepairRequest` / `MemoryTranscriptRepairReport`，通过 `MemoryRuntime::repair_transcript` 调用 | 检查 Memory-owned evidence link 断裂，不扫描宿主业务数据库。 |
| Transcript Attr Write | `MemoryTranscriptAttrWriteRequest` / `MemoryTranscriptAttrWriteReport`，通过 `MemoryRuntime::record_transcript_attrs` 调用 | 在 transcript target 已存在后写入 turn/message `TranscriptAttrEnvelope`。适用于每条消息的模型用量、运行延迟/状态、附件摘要、provenance 标签等轻量 metadata。 |
| Transcript Export | `MemoryTranscriptExportRequest` / `MemoryTranscriptExportReport`，通过 `MemoryRuntime::export_transcript` 调用；`MemorySpaceExportRequest { private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate, .. }` 会把 private transcript material 及其依赖的 export-visible index 作为一个受治理 closure 排除 | 导出 redacted transcript slice；除非调用方明确选择 `IncludePrivate`，否则 private transcript material 不进入公开 memory-space archive。 |

`MemoryTranscriptReplayRequest` 和 `MemoryTranscriptExportRequest` 接收 `limit` 与可选 `cursor`；对应 report 返回 `next_cursor` 和 `has_more`。SDK 调用方应通过 `MemoryRuntime` 分页 replay/export transcript，不应下沉到 core/store trait。Runtime profile budget 可以裁剪 page size、每 turn 可见 host refs、每 turn/message 可见 attrs、redaction items、lifecycle derived refs 和 repair issues 数量，但不能放宽 redaction、lifecycle 或 privacy policy。Lifecycle 和 repair report 的列表被裁剪时会设置 `profile_budget_applied=true`。

CTQ1 query surface 统一使用 Store-owned `TranscriptQueryCursor`。调用方必须把它当 opaque value：不得解码、铸造、签名、持久化 claims 或注入 host cursor authority。Cursor validation 绑定 operation、exact MemorySpace、mounted subject、filters、view、query digest、方向/anchor、Store incarnation 与 owner/index generation；每一页都重新执行 capability、subject、lifecycle、privacy 和 disclosure。Catalog/timeline 继续受 `transcript_replay` 控制；indexed search/activity 还分别要求 `transcript_search` 和 `transcript_activity`。Platform capability snapshot 通过 `beetle-memory.platform.capability.v4` 暴露这些开关。

`HostUi` 只是 **host-presentable redacted disclosure view**。它不是聊天窗口 API、分页方向、宿主产品名、transcript index owner 或 authorization token。Catalog、timeline、search、activity 只返回 Runtime hydration 后仍能通过请求 view 脱敏规则的结果。Search hit 携带受治理的 Unicode-safe excerpt 和 durable `TranscriptAnchor`；宿主应把 anchor 交给 `TranscriptTimelineAnchor::Around`，不得在 UI 内扫描或二次匹配 transcript 正文。

0.6.0 只接受 Store v12，不提供 public Store migration API、compatibility reader、双写或 automatic migration。Store v11、governance V2、partial v12 closure 与 foreign schema payload 都会 fail closed。旧代开发数据只能由其 owner 明确删除并重建；archive export/import 不是 schema migration。

Timeline 支持 latest、before、after、around-anchor、around-sequence、around-time 与 first-visible-in-range；页内 turn 始终按 sequence 正序，report 可返回 opaque older/newer cursor。日历换算归宿主：按用户 IANA timezone 把本地日期转换为 canonical UTC `[start_inclusive, end_exclusive)` 后再调用 Memory。不得假设每天都是 86400 秒，DST 当天可以是 23 或 25 小时；Beetle Memory 不保存也不猜宿主时区。

0.6.0 source candidate 保留 CTQ1 public query shape 与 capability snapshot v4，并把 Store 升到 v12 来承载 PL2 Job/Index/Binding closure。InMemory/File/SQLite 合同覆盖 query/learning persistence、reopen、repair/archive closure 与 privacy exact-zero。该工程结论不等于真实数据、Provider、GUI/UAT、crates.io 或托管 Release 回执。

Transcript attrs 是 Memory-owned transcript metadata，不是宿主业务对象库。每条 attr 都必须有 `TranscriptAttrTarget`、命名空间化 key、`TranscriptAttrValueKind`、JSON value、`HostRefVisibility`、`TranscriptAttrSource`、`TranscriptAttrGovernance` 和可选 `TranscriptAttrLink`。`HostUi` replay 只返回 HostUi-visible attrs，`ModelContext` 只返回 model-context attrs，`OperatorAudit` 返回审计可见 attrs，`Export` 只返回 export-visible 且 `export_allowed=true` 的 attrs；`RawOwnerOnly` 仍是内部视图。Repair report 会把 target turn/message 缺失、attr source key 不匹配、非法 key、超限 value、corrupt attr record 作为 fail-closed issue。`DeleteRaw` 默认隐藏 attrs；`OperatorAuditOnlyAfterMask` 最多保留脱敏后的审计 metadata，raw deletion 后绝不返回原始 attr value。

`MemoryTranscriptAttrWriteReport` 除 lifecycle report 外，还会返回 `accepted_attrs`、`rejected_attrs`、`redactions_preview`、`profile_budget_applied` 和 `audit_event_id`。Transport adapter 必须返回这些 SDK 字段，不能把 report 压成计数。

不要把宿主 owner record 或 raw payload 塞进 attrs。`Task`、`TaskDelegation`、`PolicyDecision`、`HumanGate`、`CapabilityCall`、`ArtifactRecord`、`FileWorkspace`、file revision、Memory governance command/report 本体都必须留在各自 owner record；attrs 最多链接它们或提供轻量展示标签。Attr value 不能包含 raw prompt、provider secret、raw memory value、本地真实文件路径、完整附件内容或宿主私有数据库 payload。

`HostUi` transcript replay 是宿主 UI 读回聊天记录的安全视图，由 `capabilities.transcript_replay` 控制，不依赖 `MemoryRuntime::replay` 使用的 debug/inspection `capabilities.replay`。

核心发布面概念：

| 概念 | 合同 |
| --- | --- |
| `ConversationKey` | 由 `memory_space_id`、`channel_id` 和 `conversation_id` 组成；`chat_id` 继续作为 `MemoryReplayRequest` 的 turn-ledger inspection key。 |
| `ActorAttribution` | 保留 speaker、subject、actor subject、mounted subject、agent id 和 trigger source，不把它们压成一个身份。 |
| `HostOpaqueRef` | 携带 task、project、ticket、document、order 等宿主对象引用，但 Memory 不解析宿主业务状态机；`HostRefVisibility` 会按 replay/export view 执行，`label` 会在非 owner 允许视图外做字段级脱敏，并在 redaction report 中记录 `HostRefLabel`。 |
| `TranscriptAttrEnvelope` | 承载受治理的 turn/message metadata。`TranscriptAttrScope` 是 `turn` 或 `message`；key 必须在 `host.*` 或 `memory.*` 命名空间下；value 由 `TranscriptAttrValueKind` 声明类型；visibility、export policy、redaction policy、source、links 和 value-size budget 由 Memory 执行。 |
| `RedactedTranscriptSlice` | 分离 raw owner-only、model-context、host-UI、operator-audit 和 export 视图，并返回结构化 `TranscriptRedactionReportItem` 以及 message/host ref 脱敏计数。 |
| `TranscriptLifecycleRequest` | 必须产出 report 和 audit event；删除 raw transcript content 不会静默删除已接受的长期记忆。`TranscriptLifecycleReport` 会返回 affected turn ids、message ids、按视图脱敏后的 host refs、host-ref redaction items，以及已知的 Memory-owned derived refs。lifecycle request completed 不等于 transcript changed；没有命中 turn 时 `affected_turns=0`，SDK lifecycle report 必须是 `changed=false`。 |
| `TranscriptEvidenceRef` / `DerivedMemoryRef` | Memory 自己的结构化 evidence reference，用于把 transcript evidence 连接到已接受的长期记忆、共享事实、程序性记忆、私域材料或 soul handoff。展示 citation 可以继续是字符串，但治理逻辑不应只靠字符串解析。 |
| `TranscriptTurnPage` / `TranscriptRepairReport` | 提供有界 transcript 分页和 repair 诊断，用于发现 source turn 缺失、`MissingSourceMessage`、orphan derived ref、corrupt transcript record、mismatched source key、duplicate sequence/cursor evidence；repair report 必须 fail closed，不能掩盖断裂的 Memory-owned evidence link。 |
| `TranscriptGovernanceBudget` | 由 runtime budget/profile owner 提供的 transcript page size、可见 host refs、turn/message attrs、redaction report items、lifecycle derived refs、repair issues 数量上限。Store backend 只负责持久化和分页，不拥有 profile budget policy。 |

隐私和投影边界：

- Transcript evidence 不会自动变成 canonical fact、soul mutation、procedural skill 或 task experience。
- 通过 governed candidate write、手动 extraction 或自动 post-turn extraction 接受的长期记忆、共享事实、程序性 Skill、private garden 和 soul candidate handoff 会写入结构化 transcript-derived refs，供 lifecycle impact review 使用。
- Runtime recall、projection、maintenance、long-term refresh 和 operator inspection 会优先消费 transcript-backed evidence，再退到 legacy `SessionStore(chat_id)` shadow；如果 transcript 已 mask、delete raw，或 legacy `chat_id` alias 不可信，这些路径会 fail closed，不会回退读取 session shadow 原文。
- Assistant self-claim 在被对应记忆平面治理前，只是 low-authority transcript evidence。
- `HostUi` replay 不得泄漏 private garden、inner-life、soul-private raw material、backend trace 或 operator-only audit 内容。
- `ModelContext` replay 必须经过 privacy gate、profile budget 和模型可见 projection policy。
- Host refs 默认保持 opaque；replay 可以展示 metadata 和 relation，不返回宿主对象 payload。`Export` 只返回 export-visible refs，`ModelContext` 只返回 model-context refs。
- `MemoryRuntime::finalize_turn` 同时报告 session commit 和 transcript commit 状态；当 legacy session shadow 已有该 turn 但 transcript backfill 成功时，不会被误报成 no-op。

## Request Shapes

最常用的 SDK request types：

| Request type | 必填字段 | 说明 |
| --- | --- | --- |
| `MemoryWriteRequest::Procedural` | `writes`, `owning_scope`, `source` | 每项写入必须同时携带 `RuntimeSkillWrite`、typed creation ref 和 privacy class；`name` 仅是展示输入，不参与 owner identity。 |
| `MemoryWriteRequest::AgentToolUsageFeedback` | `feedback` | 宿主执行工具后回传 `registry_ref` 和 observation 摘要；SDK 治理后才可能沉淀工具经验。 |
| `MemoryWriteRequest::LongTermExtraction` | `extraction` | 用于 extraction pipeline 已经产出 validated long-term memory extraction 的场景。 |
| `MemoryWriteRequest::GovernedEvidenceDocuments` | `mutations` | 在同一事务中创建、修订或删除 governed evidence owner、source claim 和派生索引。`Upsert` 携带有界 `GovernedEvidenceDocumentDraft`；`Delete` 必须携带 expected owner revision。 |
| `MemoryRecallRequest` | `temporal_operation`, `query`, `limit`, `structured_query_facets`, `tool_registry_refs` | 返回运行时 Skill hits、标准 Agent Skill hits、working recall inspection 和经验型 `agent_tool_hints`；structured facets 是 typed query constraint，无治理经验时 `agent_tool_hints=[]`。 |
| `MemoryProjectionRequest` | `temporal_operation`, `user_query`, `system_max_len`, `recent_messages_limit`, `pressure`, `mode_input`, `structured_query_facets`, `tool_registry_refs` | 返回受 `system_max_len` 限制的 `system_memory_block`；structured facets 与 recall 共用受治理 query 合同，标准 Agent Skill 只以只读提示摘要进入上下文，Agent Tool 只以经验 hint 进入，不包含完整 schema。 |
| `MemoryEvidenceDocumentReadRequest` | `memory_space_id`, `document_ids` | 通过 `MemoryRuntime::read_governed_evidence_documents(request)` 精确、有界地读取 governed evidence documents。runtime 会拒绝 memory-space 不一致、空/重复 document id 和超过当前 profile read budget 的请求；结果经过 privacy filter，并携带 typed owner identity、revision、canonical evidence binding、安全 source metadata 与有界 body/chunks。 |
| `MemoryInspectionRequest` | `query`, `system_max_len`, `pressure`, `mode_input` | 返回 capability、lifecycle、operator inspection 数据、Agent Skill 目录扫描报告和 Agent Tool registry 报告。 |
| `RuntimeSkillListRequest` | `owning_scope`, `query`, `include_disabled`, `include_retired`, `limit` | 只列出显式 Subject 或 SharedProgram scope manifest 中的 exact typed owners。 |
| `RuntimeSkillDetailRequest` | `locator` | locator 同时绑定 owning scope、owner ref 和 expected revision；名称不能翻译成 identity。 |
| `RuntimeSkillEditRequest` | `locator`, `title`, `topic`, `summary`, `procedure`, `edit_reason`, `observed_at` | 以 locator revision 为并发前提，成功后追加 immutable owner revision，并返回 `current_locator`。 |
| `RuntimeSkillSetEnabledRequest` | `locator`, `enabled`, `observed_at` | 追加 lifecycle revision，不写 `skill_meta`。 |
| `RuntimeSkillRetireRequest` | `locator`, `observed_at` | 追加 disabled + retired revision并保留 lineage；不物理删除 owner。 |
| `MemoryLongTermListRequest` | `query`, `limit`, `view` | 通过 `MemoryRuntime::list_long_term_memory` 读取 accepted long-term memory 列表；支持 `cursor` 分页，默认面向 `HostUi` 脱敏 embedded record 的 source metadata。 |
| `MemoryLongTermDetailRequest` | `target`, `view` | 通过 record id、slot 或 transcript derived ref 查看一条长期记忆及 revision/tombstone/evidence refs。 |
| `MemoryLongTermMutationRequest` | `operation`, `reason`, `dry_run`, `mode_input` | 执行 correct、supersede、delete、forget_by_query、mark_stale 或 change_scope；批量 forget 必须先 dry-run preview 并带 confirmation token。 |
| `MemoryLongTermPolicyRequest` | `operation`, `reason`, `dry_run`, `mode_input` | 执行 pause、resume、suppress 或 remove_suppression；被 policy 拦截的后续写入会进入 SDK governance report。 |
| `MemoryConversationListRequest` | `channel_id`, `lifecycle`, `limit`, `cursor`, `view` | 列出 runtime mounted subject 可见的 evidenceful conversations；`TranscriptCatalogLifecycle` 控制仅 active 或 active + archived。 |
| `MemoryTranscriptTimelineRequest` | `channel_id`, `conversation_id`, `anchor`, `limit`, `cursor`, `view` | 使用 `TranscriptTimelineAnchor` 查询 exact conversation；search/activity 返回的 anchor 可直接回到该 timeline。 |
| `MemoryTranscriptSearchRequest` | `scope`, `query_text`, `sort`, `lifecycle`, `limit`, `cursor`, `view` | 在 mounted subject 或 exact conversation 内执行 governed text search；空、仅标点、非法或超预算查询在 Store search 前拒绝。 |
| `MemoryTranscriptActivityRequest` | `channel_id`, `conversation_id`, `ranges`, `lifecycle`, `view` | 计算 sorted、non-overlapping UTC 半开区间并返回 visible count 与首末 `TranscriptAnchor`。 |
| `MemoryTranscriptAttrWriteRequest` | `memory_space_id`, `channel_id`, `conversation_id`, `attrs`, `dry_run` | 把受治理的 `TranscriptAttrEnvelope` metadata 写到已存在的 transcript turn/message。`idempotency_key` 用于宿主/adapter 关联；dry-run 只校验 target 存在和 attr envelope 规则，不落库，并返回 rejected attrs 与 `redactions_preview`。 |
| `MemoryReplayRequest` | `chat_id`, `limit` | 只做 inspection 的 replay surface。 |
| `MemorySpaceExportRequest` | `scope`, `private_material_policy` | 使用 `MemoryArchiveScope::subject(...)` 或 `MemoryArchiveScope::shared_program(...)`，返回带 canonical governed root 的 opaque archive。 |
| `MemorySpaceImportRequest` | `scope`, `expected_private_material_policy`, `archive` | 在 store mutation 前重算 archive root；仅当 runtime、request 与 archive 的精确 scope 和 private-material policy 全部一致时原子替换。 |
| `MemoryRecoverRequest` | `trigger`, `mode_input` | 执行可恢复 lifecycle recovery。 |
| `MemoryCloseRequest` | `reason` | 发出 close lifecycle report。 |

通用 adapter dispatch 支持 write、recall、project、inspect、recover、replay、long-term list/detail/mutate/policy、transcript attr write、capabilities、close。受治理的 memory-space export/import 绑定 runtime，不再通过旧的自由 snapshot 命令暴露。Maintain 只在调用方通过 `AdapterRuntimeServices` 显式提供 LLM/HTTP services 时执行；未注入 services 的 dispatch 会返回结构化拒绝。

Transport helper crates 会对其声明的 memory operations 使用共享 JSON adapter decoder；subscribe 这类 stream-only operation 仍属于 transport-specific 行为。每种协议的 route/frame/tool/message 表面见 [部署文档](deployment.md)。

## Accepted Long-Term Memory Control

Beetle Memory 已接受的长期记忆真源在 `MemoryRuntime`。宿主可以把用户自然语言命令映射成 SDK request，但不能在自己的 SQLite、本地 JSON 或 UI state 中维护一套“看起来已删除/已修改”的 shadow memory。

控制面和自动写入面是两条不同路径：

- `MemoryWriteRequest::Candidates` / `LongTermExtraction` 负责把候选内容交给 Memory 治理、合并和写入。
- `MemoryLongTermMutationRequest` 负责用户或 operator 对已接受长期记忆做 correction、supersede、delete、forget、scope change。
- `MemoryLongTermPolicyRequest` 负责“以后不要记这类事情”或“暂停这个范围的长期记忆更新”。
- Transcript lifecycle 的 `DeleteRaw` / `Mask` 只处理 conversation evidence；它会报告 `DerivedMemoryRef` impact，但不会自动删除 accepted long-term memory。要撤销派生长期记忆，必须再调用长期记忆控制面。
- Runtime Skill 管理面只管理 procedural memory 中的 runtime skill，不等同普通长期记忆的 edit/retire。

所有 mutation report 都必须可用于审计：affected records、tombstones、transcript refs、projection impact、deferred governance impact、policy decision 和 lifecycle report 由 SDK 返回。Profile 不允许某项操作时，SDK 返回结构化拒绝；宿主不得 fallback 到本地 DB 直接改 store。

长期记忆控制能力由 `MemoryCapabilityCatalog` 暴露：

```rust
let capabilities = runtime.capabilities();
assert!(capabilities.long_term_control_inspect.visible);
assert!(capabilities.long_term_control_mutation.visible);
assert!(capabilities.long_term_control_policy.visible);
```

`long_term_control_bulk_forget` 是高风险能力。低配或 embedded profile 可以只开放 targeted inspect/mutation/policy，并隐藏 destructive bulk forget。

## Agent Tool API

Agent Tool 是宿主真实可执行工具。Beetle Memory 不管理、不安装、不执行工具，也不保存完整工具 schema。宿主只把 compact registry snapshot 和 fingerprint 注册给 Memory，用于把历史工具经验绑定到当前工具合同。

SDK / HTTP 的共同语义：

- 有已治理经验时，recall/project 返回 `agent_tool_hints`，其中包含 `tool_id`、`registry_id`、`schema_fingerprint`、经验原因、权限/风险标签和 `host_execution_required=true`。
- 没有已治理经验时，返回 `agent_tool_hints=[]` 和 `tool_experience_status.reason="no_governed_tool_experience"`；宿主自行决定冷启动时暴露哪些工具。
- 宿主按 hint 里的 `tool_id` / `registry_ref` 取自己的完整 schema，再拼接到真正的 LLM tools 参数。
- Memory 返回的 hint 不是授权；权限、用户确认、执行、错误处理和 provider payload 都由宿主负责。

独立 HTTP 部署支持以下 registry 路由：

| Route | Method | 用途 |
| --- | --- | --- |
| `/agent-tool-registries/{id}` | `PUT` | 注册或更新 compact registry snapshot，payload 的 `registry_id` 必须和 path 一致。 |
| `/agent-tool-registries` | `GET` | 返回当前 registry snapshots 和 registry 报告。 |
| `/agent-tool-registries/{id}` | `GET` | 返回单个 registry snapshot。 |
| `/agent-tool-registries/{id}` | `DELETE` | 删除 registry snapshot；不会删除已沉淀的历史经验，但后续 projection 会因 registry 缺失或 fingerprint mismatch 拒绝旧经验。 |

`/memory/write` 可提交：

```json
{
  "tool_usage_feedback": {
    "registry_ref": {
      "registry_id": "host-tools",
      "fingerprint": "current-fingerprint",
      "scope": "global"
    },
    "observations": [],
    "user_visible_result_summary": "工具执行摘要",
    "reuse_outcome": "succeeded",
    "operator_note": null
  }
}
```

`observations` 必须是工具执行后的结构化摘要，不要放完整原始结果、secret 或完整 schema。

## Console API

Console API 只服务独立部署形态的配置台，不属于 SDK 集成方必须暴露的接口。SDK 宿主仍只消费 `bm-sdk` 或 memory adapter surface；宿主自己的配置页、账户页和 UI 由宿主负责。

`bm-entry` 持有 console 状态，`bm-http` 只负责把 `/console/*` 请求路由到 entry console 操作。Console API 不写 memory plane，不实现第二套记忆语义，也不替代 `/memory/*`。

`/console/overview` 的指标来自同一进程内的真实 runtime 状态：系统信息读取当前运行系统、CPU、内存和系统时间；存储占用前半段读取当前 Beetle Memory store 路径实际占用，后半段读取当前系统总存储；写入、召回、投影指标由 `/memory/*` 操作结果回写。配置台前端不得硬编码这些观测值，只能在后端不可达时使用本地占位数据。

| Route | Method | 用途 |
| --- | --- | --- |
| `/console/overview` | `GET` | 返回系统信息、运行形态、观测指标、内核摘要、session 概览和当前记忆上下文。 |
| `/console/skills` | `GET` | 返回运行时 Skill 列表和统计。 |
| `/console/skills/detail` | `POST` | 按 typed owner locator 返回单条运行时 Skill 详情。 |
| `/console/skills` | `PATCH` | 按 typed owner locator 创建不可变的新修订。 |
| `/console/skills/enabled` | `PATCH` | 按 typed owner locator 启用或停用运行时 Skill。 |
| `/console/skills/retire` | `POST` | 按 typed owner locator 将运行时 Skill 追加为 retired 修订。 |
| `/console/llm-gateway` | `GET` | 返回 LLM Gateway 操作面：OpenAI/Ollama/MCP 端点、规则导出命令和 smoke checks。 |
| `/console/llm-gateway/smoke-checks/{id}/run` | `POST` | 运行后端白名单中的 LLM Gateway 验收项，并返回退出码、耗时和受限 stdout/stderr。 |
| `/console/transports` | `GET` | 返回可配置通信入口。 |
| `/console/transports/{id}` | `PATCH` | 更新通信入口开关或 endpoint。 |
| `/console/devices` | `GET` | 返回允许访问设备列表，只包含 app_key 指纹。 |
| `/console/devices` | `POST` | 添加设备，由 runtime 生成一次性 app_key。 |
| `/console/devices/{id}` | `PATCH` | 更新设备状态或名称。 |
| `/console/devices/{id}/rotate-key` | `POST` | 轮换设备 app_key，并仅在响应中返回一次明文。 |
| `/console/session` | `GET` | 返回已配对 session 的账户和主体摘要。 |

安全边界：

- 列表接口永远不返回 app_key 明文，只返回 `appKeyFingerprint`。
- 新增和轮换设备时，`appKeyOnce` 只在该次响应中返回。
- 通信页的 HTTP 开关表示外部 memory HTTP API，不表示配置台自身的 HTTP console 入口。
- Skill 管理页只管理运行时 procedural memory，不新增、不导入标准 Agent Skill，不提供 marketplace、executor 或 workflow runner。
- 标准 Agent Skill 通过 SDK builder 或独立部署的 `BM_AGENT_SKILL_DIRS` 配置只读挂载；运行时只扫描 `SKILL.md` 摘要、资源计数和指纹，召回时不读取或执行 scripts/assets。
- LLM Gateway 验收运行接口只接受后端已知的 smoke check `id`，不执行前端传入的任意命令。

## Capability Catalog

每个 runtime 都暴露 `MemoryCapabilityCatalog`。能力可见性由所选 profile、compiled features、runtime policy 和 privacy policy 共同决定。

```rust
let capabilities = runtime.capabilities();
assert!(capabilities.write.visible);
assert!(capabilities.recall.visible);
assert!(capabilities.transcript_replay.visible);
```

通过 CLI 输出稳定 platform snapshot：

```bash
cargo run --locked -p bm-cli --bin bm --no-default-features \
  --features profile-server-linux-memory-gateway -- \
  platform capability-snapshot \
  --profile profile-server-linux-memory-gateway
```

## 边界

外部代码可以选择 profile、打开受支持的 store backend、调用 SDK 操作并消费 report。外部代码不能绕过 `MemoryRuntime` 写记忆状态，也不能实现一条语义不同的 adapter/store 并行路径。
