# Beetle Memory 0.11.1 源码发布说明

0.11.1 修复 0.11.0 长期记忆目录和详情在记录较多时的容量缺口。发布身份以 `v0.11.1` 源码 tag 和对应 commit 为准。

目录现在在同一个不可变 Store 读取会话内按批扫描，先执行主体可见性、来源撤回和终态过滤，再用紧凑目录行计算精确总数和分页。详情只读取目标 owner 的闭包，不因同一空间其他记录超过历史召回候选预算而失败。分页 cursor 仍绑定查询、主体和匹配目录；匹配记录变化会使旧 cursor 失效，无关记录变化不会误使其失效。

本版没有改变公共 API、Store v16、长期记忆 material v6、control revision v4 或 Adapter V2。0.11.0 的[控制完整性合同](release-notes-0.11.0.md)仍适用，不提供旧 Store 的 automatic migration。所有 Beetle crate 应使用同一源码 tag。

生产 macOS profile 下以 43 条事实验证了 20/20/3 分页、精确总数、详情、主体隔离、终态过滤和 cursor 行为；File 与 SQLite 还验证了关闭重开。相关 Core、SDK、Store 定向回归、格式和静态检查通过。Windows/Linux/ESP 尚未实际对接，本版未运行其全量矩阵；未验证真实数据、Provider 或 GUI。此交付仅为 Git 源码和 tag，未上传 crates.io，也未创建托管 Release、安装包或部署。
