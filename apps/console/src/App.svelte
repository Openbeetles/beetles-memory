<script lang="ts">
  import {
    Activity, AlertTriangle, BarChart3, CheckCircle2, Circle,
    Cpu, Database, Globe2, KeyRound, LockKeyhole, MemoryStick,
    Moon, Power, RefreshCw, Server, Settings2, ShieldCheck,
    Smartphone, Sun,
  } from "lucide-svelte";
  import type { Component } from "svelte";

  type PageId = "overview" | "transports" | "devices" | "capability" | "operator" | "account";
  type StatusKind = "ready" | "allowed" | "limited" | "draft" | "locked" | "blocked" | "disabled";

  type Page = { id: PageId; label: string; count?: string; eyebrow: string; title: string };
  type Transport   = { id: string; name: string; detail: string; enabled: boolean; status: string; endpoint: string; fields: string[] };
  type Device      = { deviceId: string; label: string; appKey: string; scopes: string; status: StatusKind; lastSeen: string };
  type KVRow       = { label: string; value: string };
  type CapRow      = { title: string; status: StatusKind; desc: string };
  type OverviewCard = { title: string; value: string; desc: string; icon: Component; tone: string; compact?: boolean; progress: number | null };

  /* ── pages (navItems + pageTitles 合并) ── */
  const pages: Page[] = [
    { id: "overview",   label: "总览",     eyebrow: "观测总览", title: "运行状态"     },
    { id: "transports", label: "通信方式", count: "5", eyebrow: "通信入口", title: "通信方式配置" },
    { id: "devices",    label: "开放设备", count: "4", eyebrow: "访问控制", title: "开放设备列表" },
    { id: "capability", label: "能力预览", eyebrow: "能力报告", title: "能力预览"     },
    { id: "operator",   label: "运维动作", eyebrow: "安全运维", title: "运维动作"     },
    { id: "account",    label: "账户安全", eyebrow: "账户安全", title: "运维账户"     },
  ];

  /* ── 状态元数据表（statusIcon + statusLabel 合并） ── */
  const STATUS_META: Record<string, { label: string; icon: Component }> = {
    ready:    { label: "可用",   icon: CheckCircle2 },
    allowed:  { label: "可用",   icon: CheckCircle2 },
    limited:  { label: "受限",   icon: Circle },
    draft:    { label: "受限",   icon: Circle },
    locked:   { label: "未启用", icon: AlertTriangle },
    blocked:  { label: "禁止",   icon: AlertTriangle },
    disabled: { label: "停用",   icon: AlertTriangle },
  };
  const statusLabel = (s: string) => STATUS_META[s]?.label ?? s;
  const statusIcon  = (s: string) => STATUS_META[s]?.icon ?? Circle;

  /* ── 通用列表更新辅助 ── */
  const mapId  = <T extends { id: string }>(list: T[], id: string, fn: (t: T) => T) =>
    list.map(t => t.id === id ? fn(t) : t);
  const mapDev = <T extends { deviceId: string }>(list: T[], id: string, fn: (t: T) => T) =>
    list.map(t => t.deviceId === id ? fn(t) : t);

  /* ── 运行时形态 ── */
  const runtimeShape = { name: "Linux 设备独立部署", store: "文件后端可用，嵌入式后端可裁剪", shell: "HTTP 配置壳" };

  /* ── 静态 overview 卡片 ── */
  const overviewCards: OverviewCard[] = [
    { title: "当前形态", value: runtimeShape.name,  desc: `${runtimeShape.shell} / ${runtimeShape.store}`, icon: Cpu,        tone: "ready",   compact: true, progress: null },
    { title: "存储占用", value: "1.84 GB",           desc: "文件后端 / 72% 可用空间",                         icon: Database,   tone: "ready",   progress: 28   },
    { title: "今日写入", value: "428",                desc: "12 条进入人工复核队列",                            icon: BarChart3,  tone: "ready",   progress: null },
    { title: "召回命中", value: "91.6%",              desc: "最近 24 小时平均命中率",                           icon: Activity,   tone: "ready",   progress: 91.6 },
    { title: "投影预算", value: "63%",                desc: "当前上下文预算消耗",                               icon: MemoryStick, tone: "limited", progress: 63   },
    { title: "开放设备", value: "3/4",                desc: "1 个设备已停用",                                   icon: Smartphone, tone: "limited", progress: 75   },
  ];

  const capabilityRows: CapRow[] = [
    { title: "写入治理",     status: "ready",   desc: "所有写入进入统一运行时" },
    { title: "灵魂与主体记忆", status: "ready",  desc: "投影与隐私门禁已启用" },
    { title: "归档回放",     status: "limited", desc: "当前运行形态只允许轻量回放" },
    { title: "设备白名单",   status: "ready",   desc: "4 个设备，1 个受限，1 个禁用" },
    { title: "SDK 宿主界面", status: "blocked", desc: "SDK 使用方永远不暴露本配置台" },
  ];

  const recentEvents = [
    { time: "14:18", text: "HTTP 接口通过运维鉴权",     tone: "ready" },
    { time: "14:11", text: "ESP 独立记忆设备轻量检查完成", tone: "ready" },
    { time: "13:54", text: "归档回放被当前运行形态限制",   tone: "limited" },
    { time: "13:42", text: "旧实验设备已停用访问",         tone: "blocked" },
  ];

  /* ── 可变状态 ── */
  let activePage: PageId = $state("overview");
  let theme: "light" | "dark" = $state("dark");
  let lang: "zh-CN" | "en" | "ja" = $state("zh-CN");

  let transports: Transport[] = $state([
    { id: "http",    name: "HTTP 接口",      detail: "配置页、写入、召回和运维报告入口",   enabled: true,  status: "ready",  endpoint: "0.0.0.0:8718",            fields: ["鉴权：必须", "限流：120/分钟", "模式：本地"] },
    { id: "wss",     name: "WebSocket 订阅", detail: "长连接订阅运行事件与召回流",         enabled: true,  status: "draft",  endpoint: "/memory/events",           fields: ["鉴权：必须", "订阅上限：8"] },
    { id: "mcp",     name: "MCP 标准输入输出", detail: "本地工具进程入口，消费同一套运行时", enabled: false, status: "draft",  endpoint: "stdio",                    fields: ["工具：召回、投影、检查"] },
    { id: "a2a",     name: "A2A HTTP",       detail: "智能体到智能体的记忆桥接",           enabled: false, status: "draft",  endpoint: "http://127.0.0.1:8720/a2a", fields: ["鉴权：必须", "桥接：薄适配"] },
    { id: "cli",     name: "本地命令行",     detail: "本地检查、恢复、导入导出预检",       enabled: true,  status: "ready",  endpoint: "local-only",               fields: ["私域原文：禁止", "运维鉴权：必须"] },
  ]);

  let devices: Device[] = $state([
    { deviceId: "bm-linux-core-01",      label: "Linux 设备核心",    appKey: "fp:8c41:2f90", scopes: "写入、召回、检查",      status: "allowed",  lastSeen: "18 秒前" },
    { deviceId: "bm-desktop-mac-studio", label: "桌面应用",          appKey: "fp:d101:a33e", scopes: "运维、导出、回放",      status: "allowed",  lastSeen: "2 分钟前" },
    { deviceId: "bm-esp-memory-7f2",     label: "ESP 独立记忆设备",  appKey: "fp:62bc:910a", scopes: "写入、召回、轻量检查",  status: "limited",  lastSeen: "9 分钟前" },
    { deviceId: "legacy-lab-device",     label: "已停用实验设备",    appKey: "fp:hidden",    scopes: "无",                    status: "disabled", lastSeen: "昨天" },
  ]);

  /* ── 派生 ── */
  const enabledTransportCount = $derived(transports.filter(t => t.enabled).length);
  const activeDeviceCount      = $derived(devices.filter(d => d.status !== "disabled").length);
  const currentPage            = $derived(pages.find(p => p.id === activePage)!);

  /* KV 数组（响应式，用于 kvStack snippet） */
  const transportStats = $derived<KVRow[]>([
    { label: "已开启通信",   value: `${enabledTransportCount} / ${transports.length}` },
    { label: "开放设备",     value: `${activeDeviceCount} / ${devices.length}` },
    { label: "当前配置入口", value: runtimeShape.shell },
    { label: "隐私门禁",     value: "已启用，私域原文不可见" },
    { label: "SDK 宿主界面", value: "0，禁止暴露" },
  ]);

  const accountFields: KVRow[] = [
    { label: "账户",   value: "运维员" },
    { label: "所属主体", value: "本地所有者" },
    { label: "记忆范围", value: "个人记忆" },
    { label: "会话状态", value: "已通过配对门禁" },
  ];

  const kernelSummary: KVRow[] = [
    { label: "存储后端",     value: "嵌入式 / 文件后端可用" },
    { label: "密钥策略",     value: "只显示存在状态" },
    { label: "生命周期",     value: "演示打开状态" },
    { label: "SDK 宿主界面", value: "禁止" },
  ];

  /* ── 动作 ── */
  function toggleTransport(id: string) { transports = mapId(transports, id, t => ({ ...t, enabled: !t.enabled })); }
  function rotateAppKey(deviceId: string) {
    devices = mapDev(devices, deviceId, d => ({ ...d, appKey: "fp:new:" + Math.random().toString(16).slice(2, 6) }));
  }
  function toggleDevice(deviceId: string) {
    devices = mapDev(devices, deviceId, d => ({
      ...d,
      status: d.status === "disabled" ? "limited" : "disabled",
      scopes: d.status === "disabled" ? "写入、召回" : "无",
    }));
  }
</script>

<svelte:head><title>甲壳虫记忆配置台</title></svelte:head>

<!-- ── 共用 snippets ── -->
{#snippet panelHeader(label: string, title: string, Icon: Component)}
  <div class="panel-title">
    <div>
      <p class="panel-label">{label}</p>
      <h3>{title}</h3>
    </div>
    <Icon size={18} />
  </div>
{/snippet}

{#snippet kvStack(items: KVRow[])}
  <div class="status-stack">
    {#each items as row}
      <div><span>{row.label}</span><strong>{row.value}</strong></div>
    {/each}
  </div>
{/snippet}

<main class:light={theme === "light"} class="shell">

  <!-- ══ SIDEBAR ══ -->
  <aside class="sidebar">
    <div class="brand">
      <div class="brand-icon"><img src="/logo.png" alt="BM" /></div>
      <div class="brand-text">
        <span class="brand-name">甲壳虫记忆</span>
        <span class="brand-sub">配置台</span>
      </div>
    </div>

    <nav class="nav">
      {#each pages as page}
        <button
          class:active={activePage === page.id}
          class="nav-item" type="button"
          onclick={() => (activePage = page.id)}
        >
          <span class="nav-chevron">{activePage === page.id ? "▶" : "›"}</span>
          <span class="nav-label">{page.label}</span>
          {#if page.count}<code class="nav-count">[{page.count}]</code>{/if}
        </button>
      {/each}
    </nav>

    <div class="sidebar-status">
      <div class="ss-row"><span class="ss-label">STATUS</span><span class="ss-value ok">已登录</span></div>
      <div class="ss-row"><span class="ss-label">SHELL</span><span class="ss-value">{runtimeShape.shell}</span></div>
      <small class="ss-note">DEMO · 未连接后端</small>
    </div>
  </aside>

  <!-- ══ WORKSPACE ══ -->
  <section class="workspace">
    <header class="topbar">
      <div class="topbar-left">
        <div class="breadcrumb">
          <span>CONSOLE</span>
          <span class="bc-sep">›</span>
          <span>{currentPage.eyebrow}</span>
          <span class="bc-sep">›</span>
          <span class="bc-title">{currentPage.title}</span>
        </div>
      </div>
      <div class="top-actions">
        <button class="ghost-button" type="button" onclick={() => (theme = theme === "light" ? "dark" : "light")}>
          {#if theme === "light"}<Moon size={13} /> DARK{:else}<Sun size={13} /> LIGHT{/if}
        </button>
        <select class="lang-select" bind:value={lang} aria-label="切换语言">
          <option value="zh-CN">中文</option>
          <option value="en">EN</option>
          <option value="ja">日本語</option>
        </select>
        <button class="primary-button" type="button"><Power size={13} /> APPLY</button>
      </div>
    </header>

    <div class="page-shell">
      <!-- ── 总览 ── -->
      {#if activePage === "overview"}
        <section class="overview-grid">
          {#each overviewCards as card}
            {@const Icon = card.icon}
            <article class={`overview-card ${card.tone}${card.compact ? " compact" : ""}`}>
              <div class="overview-card-head"><span>{card.title}</span><Icon size={18} /></div>
              <strong>{card.value}</strong>
              {#if card.progress !== null}
                <div class="hud-bar"><div class="hud-bar-fill" style="width:{card.progress}%"></div></div>
              {/if}
              <small>{card.desc}</small>
            </article>
          {/each}
        </section>

        <section class="section-grid overview-lower">
          <article class="panel">
            {@render panelHeader("运行观测", "通信与访问", Globe2)}
            {@render kvStack(transportStats)}
          </article>
          <article class="panel">
            {@render panelHeader("最近事件", "运维时间线", Activity)}
            <div class="event-list">
              {#each recentEvents as ev}
                <div class="event-row">
                  <span>{ev.time}</span>
                  <strong>{ev.text}</strong>
                  <em class={`dot ${ev.tone}`}></em>
                </div>
              {/each}
            </div>
          </article>
        </section>

      <!-- ── 账户安全 ── -->
      {:else if activePage === "account"}
        <section class="panel account-panel">
          {@render panelHeader("账户安全", "当前运维账户", KeyRound)}
          <div class="runtime-summary">
            {#each accountFields as row}
              <div><span>{row.label}</span><strong>{row.value}</strong></div>
            {/each}
          </div>
          <div class="notice">
            <LockKeyhole size={16} />
            <span>首次配对发生在进入配置台之前；这里不提供重新配对流程，只提供密码轮换和会话管理。</span>
          </div>
        </section>

      <!-- ── 通信方式 ── -->
      {:else if activePage === "transports"}
        <section class="panel">
          {@render panelHeader("通信入口", "通信方式与必要配置", Globe2)}
          <div class="transport-grid">
            {#each transports as transport}
              <article class:disabled={!transport.enabled} class="transport-card">
                <div class="transport-head">
                  <button
                    aria-label={`切换 ${transport.name}`}
                    class:enabled={transport.enabled}
                    class="switch" type="button"
                    onclick={() => toggleTransport(transport.id)}
                  ><span></span></button>
                  <div>
                    <h4>{transport.name}</h4>
                    <p>{transport.detail}</p>
                  </div>
                </div>
                <label>
                  <span>地址 / 模式</span>
                  <input bind:value={transport.endpoint} disabled={!transport.enabled} />
                </label>
                <div class="chips">{#each transport.fields as f}<span>{f}</span>{/each}</div>
              </article>
            {/each}
          </div>
        </section>

      <!-- ── 开放设备 ── -->
      {:else if activePage === "devices"}
        <section class="panel">
          {@render panelHeader("访问控制", "开放设备列表", Smartphone)}
          <div class="device-table">
            <div class="device-row header">
              <span>设备</span><span>app_key 指纹</span><span>访问范围</span><span>状态</span><span>操作</span>
            </div>
            {#each devices as device}
              <div class="device-row">
                <span><strong>{device.label}</strong><small>{device.deviceId}</small></span>
                <span class="mono">{device.appKey}</span>
                <span>{device.scopes}</span>
                <span class={`badge ${device.status}`}>{statusLabel(device.status)}</span>
                <span class="row-actions">
                  <button type="button" onclick={() => rotateAppKey(device.deviceId)}><RefreshCw size={12} /> 轮换</button>
                  <button type="button" onclick={() => toggleDevice(device.deviceId)}>
                    {device.status === "disabled" ? "启用" : "停用"}
                  </button>
                </span>
              </div>
            {/each}
          </div>
        </section>

      <!-- ── 能力预览 ── -->
      {:else if activePage === "capability"}
        <section class="section-grid">
          <article class="panel">
            {@render panelHeader("能力报告", "可见能力预览", ShieldCheck)}
            <div class="capability-list">
              {#each capabilityRows as row}
                {@const Icon = statusIcon(row.status)}
                <div class="capability-row">
                  <Icon size={16} />
                  <div><strong>{row.title}</strong><small>{row.desc}</small></div>
                  <span class={`badge ${row.status}`}>{statusLabel(row.status)}</span>
                </div>
              {/each}
            </div>
          </article>
          <article class="panel">
            {@render panelHeader("运行状态", "内核摘要", Server)}
            {@render kvStack(kernelSummary)}
          </article>
        </section>

      <!-- ── 运维动作 ── -->
      {:else if activePage === "operator"}
        <section class="panel operator-panel">
          {@render panelHeader("安全运维", "运维动作", Settings2)}
          <div class="operator-grid">
            <button type="button"><Activity size={18} /> 检查运行时</button>
            <button type="button"><RefreshCw size={18} /> 安全恢复</button>
            <button type="button"><Database size={18} /> 导出快照</button>
            <button type="button"><ShieldCheck size={18} /> 回放冒烟</button>
          </div>
        </section>
      {/if}
    </div>
  </section>

  <!-- ══ STATUS BAR ══ -->
  <div class="statusbar">
    <span class="sb-brand">BM-CONSOLE</span>
    <span class="sb-item">DEMO</span>
    <span class="sb-sep">│</span>
    <span class="sb-item">v1.0.0-dev</span>
    <span class="sb-sep">│</span>
    <span class="sb-item">演示数据 · 未连接后端</span>
    <div class="sb-right">
      <span class="sb-item">传输: {enabledTransportCount}/{transports.length}</span>
      <span class="sb-sep">│</span>
      <span class="sb-item">设备: {activeDeviceCount}/{devices.length}</span>
      <span class="sb-sep">│</span>
      <span class="sb-item ok">● ONLINE</span>
    </div>
  </div>
</main>
