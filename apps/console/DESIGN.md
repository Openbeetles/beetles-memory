# Beetle Memory Console — UI 设计规范

> 版本：4.6.0<br>
> 最后更新：2026-08-05<br>
> 风格基调：**克制仪器感 — 干净纸面 + 点缀信号色**<br>
> 运行时真源：[`src/app.css`](src/app.css)

---

## 1. 设计原则

| 原则 | 说明 |
|------|------|
| **功能叙事优先** | 服务七个真实功能面 |
| **真实数据优先** | 只消费 `/console/*`；离线空态，禁止假健康 |
| **纸面优先** | 面板白底；列表 flush；禁止嵌套灰盒 |
| **边线克制** | `--bd` / `--rule` 尽量淡；结构面保留，装饰描边与短 KV 分割线可去掉 |
| **信号色克制** | `--sig` 只用于激活条、底栏品牌、少量状态点缀；不渗进表面 wash |
| **选中态统一** | 中性 `--s3` + inset/`border` 信号条；禁止 `--sig-lo` 铺底 |
| **单行标题** | `PanelHeader` 只有 `title`（+ 可选 icon），禁止 eyebrow/label 双标题 |
| **壳层无边框** | sidebar / topbar / statusbar 不加分隔边框 |
| **间距统一** | 页面主网格 gap `12px` |
| **安全可见** | 密钥发放、停用、退役必须有危险色与明确文案 |

---

## 2. 色彩

- 表面：`--bg` / `--s0`…`--s5`；亮色 `--s1`/`--s2` 同为白，hover 用 `--s3`
- 边框：中性 `--bd`（很淡）；行内分隔：`--rule`（更淡，只用于长列表/区块）
- 语义：`--gn` / `--am` / `--rd` → `statusClass()` → `StatusBadge`
- 暗色可选 corner ticks（`--tick`）；亮色 ticks 关闭

---

## 3. 字体与图标

- 数据：Share Tech Mono
- 分组 / 关键数字：Orbitron（克制使用）
- 侧栏：Unicode 字体符号（`NavIcon`），非 Material / Lucide

---

## 4. Shell

```
200px | 1fr
1fr | 28px
sidebar | workspace
statusbar
```

导航三组：记忆核心 / 接入边界 / 系统。Topbar 面包屑：`分组 › 页面`。

---

## 5. 约束

1. 文案走 `lib/i18n.ts`
2. API 只走 `console-api.ts` / `api.ts`
3. 保留 `.tauri` / `.macos` / `.windows`
4. 禁止第二套 memory 读写路径
5. 禁止 Atmosphere / 左侧状态色条 / 嵌套灰卡堆叠
