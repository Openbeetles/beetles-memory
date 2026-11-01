# Beetle Memory 0.10.0 源码发布说明

0.10.0 完成生产方法学习、受治理人格成长与用户设备本地时间接线。发布身份由 `v0.10.0`
源码 tag 与对应 commit 确定；包验收不代表 crates.io 上传。接入方式见[集成文档](integration.md)
和 [API](api.md)。

## 生产方法学习

`PostTurnLearningInputV3` 接受有界、有来源的学习素材、真实方法采用与独立任务评价。
官方 `MemoryLearningService` / `MemoryLearningEngine` 从已获权素材提炼 `ModelInferred`
Candidate。模型不能授予 producer 权限、冒充实际执行或自行把任务评价成成功。

使用 `discover_method_trials` 发现获准试用的 Candidate；正常方法交付可通过
`MethodContentRefV1::from_selection` 获得精确引用。选择、采用、工具执行和任务成功是不同事实。
晋升要求实际采用与独立获权的任务证据；同一任务的重复观察不算多次独立成功。
撤销无关的声明能力不能抹去仍有效的 Failed 评价。

撤权、更正和删除原文使受影响的当前及历史读取失效。删除原文会清除依赖该来源的方法与技能正文；
不会仅因无关使用证据删除而删除独立创作的技能。原文删除后，保留的无正文回执仍绑定精确 job 输入
与不可变执行配置。缺失或矛盾的权威必须修复，不能重新计算摘要或默默补造证明。

## 受治理人格成长

带签名 Soul 准入的已交付 canonical turn 可以成为现有主体 generation 内带来源的观察。
模型只解释证据，不能指定主体、generation、revision 或确认状态。稳定、独立的观察进入既有
SelfModel/Core revision 治理。自然价值取向与 canonical 治理优先级分别表达；任务／工具习惯
不能升级成人格，没有证据不创造人格或关系。

来源复审期间，受影响 Soul／私域停止投影，独立合法共享事实仍可用。真实后续交互可以支持同代
受治理更正；恢复当前读取不会复活被拒绝的历史 revision。reset 后不能把旧签名回合或排队观察
重新授权给新 generation。关系边界、私域运行时与公开报告分离继续生效。

## 用户设备本地时间

`UserLocalTimeProvider` 向前台和后台模型上下文提供用户本地时间与时区。桌面 profile 默认读取
设备 OS 时区，针对每个 instant 计算偏移。远端／服务器／设备 profile 需要可信的用户级 provider，
不能把服务器时区当用户时区。返回 `None` 时整个字段省略，不猜 UTC、不沿用旧时间、不输出无标注
的 epoch。持久时间、lease/CAS 和排序仍使用 UTC instant。

## Breaking 公共合同与 Store 准入

- 要求 Store v15。Store v14 及更早格式只读拒绝，不改写、不自动另开空库；不提供
  automatic migration、兼容 reader 或双写。
- Input/evidence V3、semantic Job V4／scope index／receipt V4、method material V4 与
  application ledger V3 承载新的 provenance 和原子完成合同。Projection receipt V2、Soul
  schema 2、source-dependents schema 2、reconciliation checkpoint schema 2 保证精确
  generation／来源／任务闭包。长期记忆 material v5 与 Adapter V2 不变。
- 消费者需更新 producer claims、evaluator 定义与显式结构体字段。获权程序性提交仍需要
  `MemoryProceduralSubmissionCapability`；普通回合使用 `PostTurnLearningInputV3::empty()`
  和 `finalize_turn`，无需 producer grant。
- 人格 `Growth.value_orientation` 显式表达，不能用自由价值描述替代 canonical 优先级 token。
  调用方不能写私域证据 ledger 或自带治理权限。
- SubjectShell 不再单独展示无时区的当前 epoch。使用 SDK typed 时间 provider 和原有投影
  renderer，不新增宿主时间渲染层。

本代开发／合成数据使用新 Store；已有数据单独保留。本次源码发布不会删除或迁移真实数据，任何
真实数据替换都需要 operator 明确操作。回退使用旧源码 tag 与匹配的完整旧 Store；旧代码打开
Store v15 或 archive import 都不是 schema 回滚方式。

## 官方服务接入

将官方服务附着到同一个 Runtime、Store 与 SubjectRegistry，或直接使用 Entry 创建的 Runtime。
复用宿主唯一配置和凭证 resolver。网络模型执行须编译 `bm-entry/governance-model-client-std`；
只配置 Key 不能补齐被裁掉的执行能力。缺 binding／凭证时保留 durable job，等待合法恢复。
宿主提供事实和凭证，不另建 worker、policy store 或晋升引擎。
`procedural_reconciliation_status` 继续作为恢复任务发现 API。

## 验收与交付边界

自动合同覆盖正式 SDK／Entry、InMemory／File／SQLite 重开、精确 current/as-of、撤权／重置／
删除、隐私、任务证据、来源损坏与本地时间。合成模型和 loopback HTTP 证明接线，不证明真实
Provider 学习效果；严格平台编译不等于可信 Linux 动态执行或 GUI/UAT。
完整比较学习实验、真实数据迁移、外部 benchmark、托管 Release、安装包和部署不在本次源码交付内。
打包与 `cargo publish --dry-run` 不上传软件包。
