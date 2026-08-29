# Beetle Memory 0.6.0 发布说明

发布身份：`v0.6.0`。精确源码 commit 由不可变 annotated tag 标识；Git 托管、crates.io、二进制和托管 Release 页面各自需要独立平台回执，本文不预声明这些动作已经完成。

## Breaking Release

Beetle Memory 0.6.0 是 SemVer breaking 的 SDK 与持久化版本。PL2 将回合结束后的 durable governance 与长期学习收口成 Memory-owned 服务，同时支持 Entry 自建 Runtime 和已经构造好的 embedded 多主体 `MemoryRuntime`。

本版本精确合同如下：

- 只接受 Store v12、Post-Turn Governance Job V3、Scope Index V3、Job Ref V2、immutable binding snapshot 与 binding revision index 这一代合同。
- long-term material v5 继续是唯一 immutable material 合同，platform capability snapshot 继续使用 v4，Adapter V2 继续是唯一可写 adapter 协议。
- Store v11 与 governance V2 直接拒绝；不提供 v11→v12 migration API、兼容 reader、双写或 automatic migration。现有开发数据只能由其 owner 明确删除并重建。
- Binding 历史最多保留 256 个 revision；Store 只裁剪未被引用的 revision，全部被 durable job 引用时必须 backpressure。
- credential 与 provider permission recovery 是 actor、operation kind、job、authority 和 intent 全绑定的 operation-aware durable mutation，并且每次操作只有一对权威 receipt/audit。

## Universal Long-Term Learning

- `bm-sdk::MemoryLearningEngine` 统一拥有 bounded due discovery、lease/CAS fencing、当前 transcript/subject/privacy 准入、最小受治理外发、候选严格验证、长期记忆 mutation、decision receipt、retry/block/cancel 与 terminal completion。
- `bm-entry::MemoryLearningService` 拥有 bounded process worker、wake/poll lifecycle、immutable binding ingestion、host-neutral credential resolution、官方 OpenAI-compatible/Ollama 执行与 bounded shutdown。
- `MemoryLearningService::attach_runtime` 只在 Store authority、Subject Registry 和 MemorySpace authority 完全一致，且调用方提供由 exact active governing `SystemGovernor` Runtime 铸造的 operation-scoped control authority 时接入已有 Runtime；它不会重建或替换宿主的多主体 Runtime。
- `finalize_turn` 原子提交 canonical transcript 和唯一 durable governance intent。wake 只是提示；File/SQLite reopen 与多实例 claim 的恢复真相仍然是 Store。
- 宿主只提供 delivered-turn 事实、唯一当前 Provider 配置源、opaque credential resolver 与进程生命周期；不得另建 queue、worker、accepted-memory policy、Store schema 或写后过滤层。

## Provider、Credential 与 Inspection Authority

产品配置继续由宿主拥有。Beetle 只持久化不含 secret 的 immutable execution binding snapshot，用来证明 job 当时获准使用什么配置。raw credential 每次 attempt 临时解析，绝不序列化或进入日志，并在 retry/shutdown 边界前销毁。

Key missing/locked 会进入 durable configuration block，network exact-zero；401 归类为 credential rejection，403 归类为 provider permission block；429、临时 I/O 与可重试 5xx 使用有界 durable backoff。恢复通知不携带 secret，并且必须让 exact credential/permission generation 单调前进。

Learning status 不再是无权限的 report getter。每次读取都必须携带 typed inspection authority：service 全局 status 要求 Runtime actor 本身就是 exact active governing `SystemGovernor`，attachment status 要求 exact active mounted-subject authority。Credential 与 permission recovery 使用两份不同的 opaque control capability，并绑定 Store、registry、MemorySpace、mounted subject、scope 与 operation kind。跨主体或跨 operation 请求必须在返回 job identity、reason detail 或执行 mutation 前 fail closed。

## Store Closure、Recovery 与隐私

每个 governed transaction 都把变更的 Job/Scope Index/Binding authority 作为一个 post-image 校验：新增或变更的 index ref 不能指向缺失 job；删除 index ref 必须同批删除 exact job；binding snapshot/index 删除必须成对；新绑定 job 必须携带 exact immutable binding snapshot 与 referenced revision；已引用 binding revision 不得降级或删除。同一 canonical binding 的首次并发安装只允许一次 exact reread 后幂等成功，同一 immutable identity 下的不同 payload 继续返回 typed conflict。Store open 与 snapshot import 会验证全量 closure，孤儿或混代状态一律 fail closed。

Provider 首个网络字节发出前必须重新验证 current lease、transcript lifecycle、subject、privacy 与 binding authority。无权、已撤销、malformed 或 stale work 的 accepted memory mutation 必须为零。safe service report 只包含有界 aggregate；credential、transcript body、private evidence 与 denied-subject job detail 均不得出现。

## 证据边界

发布门禁覆盖 formatting、check、clippy、公共文档、SDK/Entry/Store 合同、InMemory/File/SQLite 持久化与 reopen、crash/CAS、profile/cross-target gate 和 package dry-run。真实 Provider、真实用户数据、GUI/宿主 UAT、trusted Linux 外部质量、安装包、签名/公证、crates.io 与托管 Release 都需要独立证据，不由源码 tag 自动声明。
