# 甲壳虫记忆配置台 — UI 设计规范

> 版本：1.0.0  
> 最后更新：2026-05-21  
> 风格基调：**IDE HUD**（集成开发环境 × 抬头显示仪表盘）

---

## 1. 设计原则

| 原则 | 说明 |
|------|------|
| **信息密度优先** | 每像素传达实际状态，不做纯装饰性留白 |
| **扁平精确** | 无毛玻璃、无阴影堆叠，边框宽度精确到 1px |
| **语义即颜色** | 颜色不作品牌装饰，仅用于传达运行状态 |
| **等宽一致** | 全局使用单一等宽字体族，数字对齐可扫读 |
| **安全可见** | 危险操作区域始终有语义色警示，不依赖文字描述 |

---

## 2. 色彩体系

### 2.1 背景层级（暗色主题）

```
--bg   #09090e   最底层背景，整个 shell 容器
--s0   #0d0d16   一级表面：sidebar、topbar、statusbar
--s1   #111120   二级表面：panel 面板主体
--s2   #171726   三级表面：卡片、行、输入框容器
--s3   #1e1e30   四级表面：chip、badge 背景
--s4   #252538   五级表面：滚动条、hover 深色层
```

层级越大，颜色越浅。组件嵌套时严格按层级取色，不跨级混用。

### 2.2 边框层级

```
--bd     rgba(90, 90, 160, 0.18)   默认边框（静止状态）
--bd-md  rgba(110, 110, 180, 0.30)  中等边框（hover / 激活）
--bd-hi  rgba(140, 140, 210, 0.46)  高亮边框（聚焦 / 选中）
```

边框颜色基于蓝紫调，与珊瑚红主色形成冷暖对比。

### 2.3 文字层级

```
--tx   #c4c6e2   主文本：数据值、标题
--mu   #6464a0   次要文本：标签、描述、占位
--fa   #363658   弱化文本：表头、hint、禁用状态
```

### 2.4 主色 — 珊瑚红

```
--p      #ff6f61   主色，用于：激活态、主按钮、高亮线
--p-hi   #ff9a8d   主色亮版，用于：hover 态
--p-lo   rgba(255, 111, 97, 0.09)   主色淡背景
--p-gw   rgba(255, 111, 97, 0.22)   主色光晕（过渡动画）
--p-bd   rgba(255, 111, 97, 0.36)   主色边框
```

珊瑚红仅用于**交互激活态与关键引导**，不作为通用装饰色。

### 2.5 语义色

| 变量 | 色值 | 含义 | 使用场景 |
|------|------|------|---------|
| `--gn` | `#2dd4a4` | 翠绿 · 就绪 | badge.ready / badge.allowed / 状态点 |
| `--am` | `#f0a418` | 琥珀 · 受限 | badge.limited / badge.draft / HUD 进度条 |
| `--rd` | `#ff4466` | 红色 · 危险 | badge.disabled / badge.blocked |
| `--cy` | `#52c8f0` | 青色 · 信息 | mono 字体标识（app_key 指纹等） |
| `--pu` | `#a87cf8` | 紫色 · 草稿 | draft 状态扩展色（备用） |

**禁止**将语义色用于非对应语义的场景（如将红色用于普通装饰）。

### 2.6 亮色主题覆盖

亮色主题通过 `.shell.light` 覆盖所有 CSS 变量，不额外新增 class。
背景层级对应浅色版：`#f0f0f6` → `#e0e0ec` 递减。主色调整为稍暗版 `#e05a50` 以保证对比度。

---

## 3. 字体规范

### 3.1 字体栈

```css
"JetBrains Mono", "Fira Code", "Cascadia Code",
"SF Mono", "Consolas", ui-monospace, monospace
```

整个应用**只使用一套字体栈**，无衬线/等宽混排。

### 3.2 字号层级

| 用途 | 字号 | 字重 | 字距 |
|------|------|------|------|
| 数据大值（overview card） | clamp(24px, 3.5vw, 42px) | 800 | -0.02em |
| compact 大值 | clamp(15px, 2vw, 20px) | 800 | 默认 |
| 面板标题 h3 | 14px | 700 | 0.02em |
| 正文 / 行数据 | 12px | 400 | 默认 |
| 标签 / 上级分类 | 10px | 700 | 0.10em（全大写） |
| 状态栏 / chip / badge | 10–11px | 600–700 | 0.04–0.08em |
| hint / 弱化信息 | 10–11px | 400 | 默认 |

### 3.3 数字对齐

启用 `font-feature-settings: "tnum" 1`，确保数字表格对齐，数据列可视扫读。

---

## 4. 间距与圆角

### 4.1 圆角

```
--r: 2px   全局统一圆角
```

所有面板、按钮、输入框、badge 均使用 2px 圆角。  
**例外**：`.dot` 状态点使用 `border-radius: 50%`。

### 4.2 间距节奏

| 层级 | 数值 | 说明 |
|------|------|------|
| shell padding | 无（0） | 全边无留白，组件自带边框 |
| page-shell 内边距 | 14px / 16px | 页面内容区顶、侧边距 |
| panel 内边距 | 14px | 面板内统一内边距 |
| 卡片 / 行 padding | 7–12px | 根据行高密度调整 |
| grid gap | 10–14px | 组件之间的网格间距 |
| 元素间 gap | 3–8px | 行内元素间距 |

---

## 5. 组件规范

### 5.1 Shell 布局

```
grid-template-areas:
  "sidebar   workspace"
  "statusbar statusbar"

grid-template-columns: 224px 1fr
grid-template-rows:    1fr   24px
```

三区域不重叠：sidebar（左栏）、workspace（主内容区）、statusbar（底部全宽）。

### 5.2 Sidebar

- 宽度：224px（≤1180px 时收窄至 200px）
- 背景：`--s0`，右侧 1px `--bd` 分隔线
- **Brand 区**：高度固定 48px，含 28×28 图标框（珊瑚红边框 + 淡背景）
- **Nav 导航项**：高度 34px，左侧 2px 激活线（`--p`），hover 背景 `--s2`
  - 激活指示符：`▶`，非激活：`›`
  - 计数格式：`[7]`（code 元素，等宽字体）
- **Sidebar 状态区**（底部）：2–3 行 key/value 信息，字号 10–11px

### 5.3 Topbar

- 高度：固定 48px
- 左：面包屑 `CONSOLE › 二级 › 页面标题`，字号 12px
- 右：操作按钮组（ghost + primary）
- 底部：1px `--bd` 分隔线

### 5.4 Status Bar

- 高度：24px，横跨全宽（grid-column: 1 / -1）
- 最左：品牌标签（珊瑚红背景 `--p`，白色文字）
- 中间：`│` 分隔的状态条目
- 最右（`sb-right`）：自动推至右侧，显示实时计数与连接状态

### 5.5 Panel

```css
border: 1px solid var(--bd)
background: var(--s1)
border-radius: var(--r)   /* 2px */

::before {
  /* 顶部珊瑚红渐变高亮线 */
  height: 1px
  background: linear-gradient(90deg, var(--p) 0%, transparent 55%)
}
```

- panel-title：内部分隔线 `border-bottom: 1px solid --bd`
- panel-label：10px 全大写，珊瑚红，字距 0.10em

### 5.6 Overview Card

```
顶部 2px 语义色线（::before）
  ready   → --gn（翠绿）
  limited → --am（琥珀）
  blocked → --rd（红色）
```

卡片内从上到下：`head（标签 + 图标）`→ `strong（大数值）`→ `[hud-bar]`→ `small（描述）`

**HUD 进度条**（hud-bar）：
- 高度 3px，背景 `--s4`，填充色跟随 `--card-accent`（即顶部语义色）
- 仅在 `progress !== null` 时渲染

### 5.7 按钮

| 类型 | 样式 |
|------|------|
| `.primary-button` | 珊瑚红实色背景，白色文字，28px 高 |
| `.ghost-button` | `--s1` 背景，`--bd` 边框，`--mu` 文字，hover 加深 |
| `.row-actions button` | `--s3` 背景，22px 高，微型操作按钮 |
| `.operator-grid button` | 68px 高竖排图标+文字，hover 切换珊瑚红边框 |

所有按钮高度固定、无文字换行（`white-space: nowrap`），过渡 100ms。

### 5.8 Badge / Chip

- **Badge**：20px 高，全大写，1px 语义色边框，对应语义色背景（低透明度）
- **Chip**：20px 高，`--s3` 背景，`--mu` 文字，用于字段标注

### 5.9 Switch 开关

- 34×18px，矩形（非胶囊）
- 关：`--s4` 背景，`--bd-md` 边框，指示块颜色 `--mu`
- 开：珊瑚红淡背景 `--p-lo`，珊瑚红边框 `--p-bd`，指示块颜色 `--p`
- 指示块位移：`translateX(14px)`

### 5.10 Input / Label

- 标签：10px 全大写，`--fa` 颜色，字距 0.08em
- 输入框：32px 高，背景 `--s0`，`--bd` 边框，禁用时 opacity 0.45
- 聚焦：`outline: 1px solid var(--p)`

---

## 6. 视觉特效

### 6.1 扫描线叠层

```css
body::after {
  position: fixed; inset: 0; z-index: 9999; pointer-events: none;
  background: repeating-linear-gradient(
    0deg, transparent, transparent 2px,
    rgba(0,0,0,0.045) 2px, rgba(0,0,0,0.045) 3px
  );
}
```

极细微的水平扫描线，仅在暗色环境下可感知，不影响可读性。亮色模式下同样存在但更不明显。

### 6.2 面板顶部高亮线

`panel::before` 使用从珊瑚红渐变到透明的 1px 横线，强化面板左侧视觉锚点。

---

## 7. 状态语义映射

| 状态值 | 中文 | 颜色变量 | 使用场景 |
|--------|------|----------|---------|
| `ready` / `allowed` | 可用 | `--gn` 翠绿 | badge、card 顶部线、dot |
| `limited` / `draft` | 受限 | `--am` 琥珀 | badge、card 顶部线、dot |
| `locked` | 未启用 | `--mu` 灰紫 | badge |
| `blocked` / `disabled` | 禁止 / 停用 | `--rd` 红色 | badge、dot |

状态颜色必须同时作用于：顶部语义线、badge 边框、badge 背景、图标色（三者保持一致）。

---

## 8. 响应式断点

| 断点 | 行为 |
|------|------|
| `> 1180px` | 标准双栏布局，sidebar 224px |
| `≤ 1180px` | sidebar 收窄至 200px；section-grid 单栏；transport/operator grid 2 列 |
| `≤ 820px` | 完全单栏；sidebar 改为顶部横向导航；device-table 隐藏表头改为卡片式 |

---

## 9. 可访问性

- 所有交互元素有 `:focus-visible` 轮廓：`1px solid var(--p)`，`outline-offset: 2px`
- Switch 开关有 `aria-label` 描述
- 颜色不是唯一区分手段：badge 同时包含文字标签
- 支持 `prefers-reduced-motion`：所有过渡动画关闭

---

## 10. 文件结构

```
src/
  app.css       全局样式 + 组件样式（单文件，按注释块分区）
  App.svelte    唯一组件（演示阶段），包含所有页面逻辑
  main.ts       挂载入口
```

样式组织顺序（app.css 内）：

1. CSS 变量定义（`:root`）
2. 亮色主题覆盖（`.shell.light`）
3. Reset
4. Shell 网格
5. Sidebar
6. Workspace / Topbar
7. 按钮系统
8. Status Bar
9. Panel
10. 各页面组件（overview / event / transport / device / capability / operator / account）
11. 响应式媒体查询
12. `prefers-reduced-motion`
