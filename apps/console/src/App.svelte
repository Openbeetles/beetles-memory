<script lang="ts">
  import {
    Activity,
    AlertTriangle,
    BarChart3,
    BookOpen,
    CheckCircle2,
    Circle,
    Cpu,
    Database,
    FileText,
    Globe2,
    KeyRound,
    LockKeyhole,
    MemoryStick,
    Moon,
    Pencil,
    Plus,
    Power,
    RefreshCw,
    Search,
    Server,
    ShieldCheck,
    Smartphone,
    Sun,
    Trash2,
    Upload,
  } from "lucide-svelte";
  import { onMount } from "svelte";
  import type { Component } from "svelte";

  type PageId = "overview" | "skills" | "transports" | "devices" | "account";
  type StatusKind =
    | "ready"
    | "allowed"
    | "limited"
    | "draft"
    | "locked"
    | "blocked"
    | "disabled"
    | "active"
    | "stale"
    | "low_value"
    | "retired";
  type Lang = "zh-CN" | "en";
  type Theme = "light" | "dark";
  type TransportId = "http" | "wss" | "mcp" | "a2a";
  type SkillOrigin = "user_provided" | "runtime_learned";
  type SkillKind = "runtime_skill" | "manual_document";
  type SkillModal = "create" | "import" | "edit" | "delete" | null;

  type Page = { id: PageId; label: string; count?: string; eyebrow: string; title: string };
  type Transport = {
    id: TransportId;
    enabled: boolean;
    status: StatusKind;
    endpoint: string;
    editable?: boolean;
  };
  type Device = { deviceId: string; appKey: string; status: StatusKind; label?: string };
  type KVRow = { label: string; value: string };
  type CapRow = { title: string; status: StatusKind; desc: string };
  type TimelineEvent = { time: string; text: string; tone: string };
  type OverviewCard = {
    title: string;
    value: string;
    desc: string;
    icon: Component;
    tone: string;
    compact?: boolean;
    progress: number | null;
  };
  type ConsoleApiTransport = {
    id: TransportId;
    enabled: boolean;
    status: StatusKind;
    endpoint: string;
    editable: boolean;
  };
  type ConsoleApiDevice = {
    deviceId: string;
    label: string;
    appKeyFingerprint: string;
    status: StatusKind;
  };
  type ConsoleApiSession = {
    account: string;
    owner: string;
    memoryScope: string;
    sessionState: string;
  };
  type ConsoleApiRuntimeShape = {
    profile: string;
    name: string;
    store: string;
    shell: string;
  };
  type ConsoleApiSystemInfo = {
    name: string;
    cpu: string;
    memory: string;
    timeUnixSecs: number;
  };
  type ConsoleApiMetric = {
    value: string;
    desc: string;
    progress: number | null;
  };
  type ConsoleApiSkillSummary = {
    name: string;
    kind: SkillKind;
    origin: SkillOrigin;
    title: string;
    topic: string;
    status: StatusKind;
    enabled: boolean;
    qualityScore: number | null;
    useCount: number;
    validatedSuccessCount: number;
    mismatchCount: number;
    revisionPending: boolean;
    updatedAt: number;
    lastUsedAt: number | null;
  };
  type ConsoleApiSkillList = {
    total: number;
    active: number;
    disabled: number;
    runtimeLearned: number;
    userProvided: number;
    skills: ConsoleApiSkillSummary[];
  };
  type ConsoleApiSkillDetail = {
    summary: ConsoleApiSkillSummary;
    summaryText: string;
    procedureText: string;
    rawContent: string;
    citations: string[];
    sourceChatId: string | null;
    lineage: string[];
    strategyDiffs: string[];
    lastOutcomeNote: string;
  };
  type ConsoleApiSkillMutation = {
    accepted: boolean;
    changed: boolean;
    name: string;
    operation: string;
    reason: string;
  };
  type SkillForm = {
    title: string;
    topic: string;
    summary: string;
    procedure: string;
    citations: string;
  };
  type ConsoleApiOverview = {
    runtimeShape: ConsoleApiRuntimeShape;
    systemInfo: ConsoleApiSystemInfo;
    storage: ConsoleApiMetric;
    writesToday: ConsoleApiMetric;
    recall: ConsoleApiMetric;
    projection: ConsoleApiMetric;
    devices: ConsoleApiMetric;
    recentEvents: TimelineEvent[];
    capabilities: CapRow[];
    kernel: KVRow[];
    session: KVRow[];
  };

  const STORAGE_KEYS = {
    theme: "beetle-memory-console-theme",
    lang: "beetle-memory-console-lang",
  } as const;

  const STATUS_ICONS: Record<StatusKind, Component> = {
    ready: CheckCircle2,
    allowed: CheckCircle2,
    limited: Circle,
    draft: Circle,
    locked: AlertTriangle,
    blocked: AlertTriangle,
    disabled: AlertTriangle,
    active: CheckCircle2,
    stale: Circle,
    low_value: AlertTriangle,
    retired: AlertTriangle,
  };

  const isTheme = (value: string | null): value is Theme => value === "light" || value === "dark";
  const isLang = (value: string | null): value is Lang => value === "zh-CN" || value === "en";

  function readTheme(): Theme {
    if (typeof localStorage === "undefined") return "dark";
    try {
      const value = localStorage.getItem(STORAGE_KEYS.theme);
      return isTheme(value) ? value : "dark";
    } catch {
      return "dark";
    }
  }

  function readLang(): Lang {
    if (typeof localStorage === "undefined") return "zh-CN";
    try {
      const value = localStorage.getItem(STORAGE_KEYS.lang);
      return isLang(value) ? value : "zh-CN";
    } catch {
      return "zh-CN";
    }
  }

  function writeStorage(key: string, value: string) {
    if (typeof localStorage === "undefined") return;
    try {
      localStorage.setItem(key, value);
    } catch {
      // 本地偏好写入失败时不阻断配置台渲染。
    }
  }

  const copy = {
    "zh-CN": {
      headTitle: "Beetles Memory",
      brand: { name: "Beetles Memory", sub: "" },
      labels: {
        console: "配置台",
        status: "状态",
        shell: "配置壳",
        backendOffline: "后端未连接",
        backendConnected: "已连接后端",
        loggedIn: "已登录",
        online: "在线",
        offline: "离线",
        systemInfo: "系统信息",
        addressMode: "地址 / 模式",
      },
      actions: {
        apply: "应用",
        dark: "夜间",
        light: "日间",
        language: "切换语言",
        rotate: "轮换",
        enable: "启用",
        disable: "停用",
        edit: "编辑",
        delete: "删除",
        save: "保存",
        cancel: "取消",
        createSkill: "新建 Skill",
        importSkill: "导入 Skill",
        toggleTransport: (name: string) => `切换 ${name}`,
      },
      language: { zh: "中文", en: "English" },
      pages: [
        { id: "overview", label: "总览", eyebrow: "观测总览", title: "运行状态" },
        { id: "skills", label: "Skill 记忆", eyebrow: "程序性记忆", title: "Skill 管理" },
        { id: "transports", label: "通信方式", count: "5", eyebrow: "通信入口", title: "通信方式配置" },
        { id: "devices", label: "开放设备", count: "4", eyebrow: "访问控制", title: "开放设备列表" },
        { id: "account", label: "账户安全", eyebrow: "账户安全", title: "账户" },
      ] satisfies Page[],
      statusLabels: {
        ready: "可用",
        allowed: "可用",
        limited: "受限",
        draft: "受限",
        locked: "未启用",
        blocked: "禁止",
        disabled: "停用",
        active: "启用",
        stale: "待复核",
        low_value: "低价值",
        retired: "已退役",
      } satisfies Record<StatusKind, string>,
      runtimeShape: {
        name: "Linux 设备独立部署",
        store: "文件后端可用，嵌入式后端可裁剪",
        shell: "HTTP 配置壳",
      },
      overview: {
        storage: { title: "存储占用", value: "0 B / 0 B", desc: "当前系统占用 / 实际系统可用大小" },
        writes: { title: "今日写入", value: "0", desc: "等待后端运行数据" },
        recall: { title: "召回命中", value: "0.0%", desc: "等待后端运行数据" },
        projection: { title: "投影预算", value: "0", desc: "等待后端运行数据" },
        devices: { title: "开放设备", value: "0/0", desc: "等待后端运行数据" },
        observation: "运行观测",
        communicationAccess: "通信与访问",
        recentEvents: "最近事件",
        timeline: "事件时间线",
      },
      transportStats: {
        enabled: "已开启通信",
        devices: "开放设备",
      },
      recentEvents: [
        { time: "--", text: "后端未连接，等待运行数据", tone: "limited" },
      ],
      account: {
        panel: "账户安全",
        title: "当前账户",
        notice: "配对门禁发生在进入配置台之前；这里仅展示当前已配对会话和主体信息。",
        fields: [
          { label: "账户", value: "本地账户" },
          { label: "所属主体", value: "本地所有者" },
          { label: "记忆范围", value: "个人记忆" },
          { label: "会话状态", value: "已通过配对门禁" },
        ],
      },
      transportsPanel: { label: "通信入口", title: "通信方式与必要配置" },
      skillsPanel: {
        label: "程序性记忆",
        title: "Skill 记忆管理",
        search: "搜索名称、主题、摘要、过程",
        all: "全部",
        active: "启用",
        disabled: "停用",
        retired: "已退役",
        userProvided: "用户提供",
        runtimeLearned: "运行时沉淀",
        total: "总数",
        quality: "质量分",
        uses: "使用次数",
        successes: "成功次数",
        mismatches: "不匹配次数",
        revisionPending: "修订中",
        citations: "引用",
        lineage: "演化谱系",
        strategyDiffs: "策略变更",
        empty: "暂无 Skill 记忆",
        emptyDetail: "选择一个 Skill 查看详情",
        name: "名称",
        titleLabel: "标题",
        topic: "主题",
        summary: "摘要",
        procedure: "过程",
        citationsInput: "引用，一行一个",
        file: "读取本地文本",
        deleteTitle: "删除 Skill",
        deleteDesc: "删除后会从记忆存储移除，不保留配置台墓碑。",
        modalTitle: {
          create: "新建 Skill",
          import: "导入 Skill",
          edit: "编辑 Skill",
          delete: "删除 Skill",
        },
      },
      transports: {
        http: {
          name: "HTTP 接口",
          detail: "配置页、写入、召回和检查报告入口",
          fields: ["鉴权：必须", "限流：120/分钟", "模式：本地"],
        },
        wss: {
          name: "WebSocket 订阅",
          detail: "长连接订阅运行事件与召回流",
          fields: ["鉴权：必须", "订阅上限：8"],
        },
        mcp: {
          name: "MCP 标准输入输出",
          detail: "本地工具进程入口，消费同一套运行时",
          fields: ["工具：召回、投影、检查"],
        },
        a2a: {
          name: "A2A HTTP",
          detail: "智能体到智能体的记忆桥接",
          fields: ["鉴权：必须", "桥接：薄适配"],
        },
      },
      devicesPanel: { label: "访问控制", title: "开放设备列表" },
      deviceHeaders: ["设备", "app_key 指纹", "状态", "操作"],
      devices: {
        "bm-linux-core-01": {
          label: "Linux 设备核心",
          lastSeen: "18 秒前",
        },
        "bm-desktop-mac-studio": {
          label: "桌面应用",
          lastSeen: "2 分钟前",
        },
        "bm-esp-memory-7f2": {
          label: "ESP 独立记忆设备",
          lastSeen: "9 分钟前",
        },
        "legacy-lab-device": {
          label: "已停用实验设备",
          lastSeen: "昨天",
        },
      },
      capabilityPanel: { label: "能力报告", title: "可见能力预览" },
      capabilityRows: [
        { title: "写入治理", status: "ready", desc: "所有写入进入统一运行时" },
        { title: "灵魂与主体记忆", status: "ready", desc: "投影与主体记忆已启用" },
        { title: "归档回放", status: "limited", desc: "当前运行形态只允许轻量回放" },
        { title: "设备白名单", status: "ready", desc: "后端连接后显示设备数量" },
      ] satisfies CapRow[],
      kernel: {
        label: "运行状态",
        title: "内核摘要",
        rows: [
          { label: "存储后端", value: "嵌入式 / 文件后端可用" },
          { label: "密钥策略", value: "只显示存在状态" },
          { label: "生命周期", value: "运行时已打开" },
        ],
      },
      statusbar: { brand: "Beetles Memory", skills: "Skill", transports: "传输", devices: "设备" },
      addDevice: {
        btn: "添加设备",
        title: "添加新设备",
        nameLabel: "设备名称",
        namePlaceholder: "例：ESP32 边缘节点",
        save: "保存",
        cancel: "取消",
        closeLabel: "关闭",
        issuedKeyNotice: "新 app_key 只展示一次",
        keyDialogTitle: "一次性 app_key",
        keyDialogDesc: "关闭后将不再展示，请立即保存。",
        copyKey: "复制",
        copied: "已复制",
      },
    },
    en: {
      headTitle: "Beetles Memory",
      brand: { name: "Beetles Memory", sub: "" },
      labels: {
        console: "Console",
        status: "Status",
        shell: "Shell",
        backendOffline: "Backend offline",
        backendConnected: "Backend connected",
        loggedIn: "Signed in",
        online: "Online",
        offline: "Offline",
        systemInfo: "System Info",
        addressMode: "Address / Mode",
      },
      actions: {
        apply: "Apply",
        dark: "Dark",
        light: "Light",
        language: "Switch language",
        rotate: "Rotate",
        enable: "Enable",
        disable: "Disable",
        edit: "Edit",
        delete: "Delete",
        save: "Save",
        cancel: "Cancel",
        createSkill: "New Skill",
        importSkill: "Import Skill",
        toggleTransport: (name: string) => `Toggle ${name}`,
      },
      language: { zh: "中文", en: "English" },
      pages: [
        { id: "overview", label: "Overview", eyebrow: "Observability", title: "Runtime Status" },
        { id: "skills", label: "Skill Memory", eyebrow: "Procedural Memory", title: "Skill Management" },
        { id: "transports", label: "Communication", count: "5", eyebrow: "Entry Points", title: "Communication Setup" },
        { id: "devices", label: "Devices", count: "4", eyebrow: "Access Control", title: "Allowed Devices" },
        { id: "account", label: "Account", eyebrow: "Account Security", title: "Account" },
      ] satisfies Page[],
      statusLabels: {
        ready: "Ready",
        allowed: "Ready",
        limited: "Limited",
        draft: "Limited",
        locked: "Locked",
        blocked: "Blocked",
        disabled: "Disabled",
        active: "Active",
        stale: "Stale",
        low_value: "Low Value",
        retired: "Retired",
      } satisfies Record<StatusKind, string>,
      runtimeShape: {
        name: "Linux Device Standalone",
        store: "File backend available, embedded backend can be trimmed",
        shell: "HTTP Configuration Shell",
      },
      overview: {
        storage: { title: "Storage Usage", value: "0 B / 0 B", desc: "Current usage / available system storage" },
        writes: { title: "Writes Today", value: "0", desc: "Waiting for backend runtime data" },
        recall: { title: "Recall Hits", value: "0.0%", desc: "Waiting for backend runtime data" },
        projection: { title: "Projection Budget", value: "0", desc: "Waiting for backend runtime data" },
        devices: { title: "Allowed Devices", value: "0/0", desc: "Waiting for backend runtime data" },
        observation: "Runtime Observation",
        communicationAccess: "Communication & Access",
        recentEvents: "Recent Events",
        timeline: "Event Timeline",
      },
      transportStats: {
        enabled: "Enabled Transports",
        devices: "Allowed Devices",
      },
      recentEvents: [
        { time: "--", text: "Backend offline, waiting for runtime data", tone: "limited" },
      ],
      account: {
        panel: "Account Security",
        title: "Current Account",
        notice:
          "Pairing is enforced before entering this console. This page only shows the current paired session and owner summary.",
        fields: [
          { label: "Account", value: "Local Account" },
          { label: "Owner", value: "Local owner" },
          { label: "Memory Scope", value: "Personal memory" },
          { label: "Session State", value: "Passed pairing gate" },
        ],
      },
      transportsPanel: { label: "Entry Points", title: "Communication Methods & Required Settings" },
      skillsPanel: {
        label: "Procedural Memory",
        title: "Skill Memory Management",
        search: "Search name, topic, summary, procedure",
        all: "All",
        active: "Active",
        disabled: "Disabled",
        retired: "Retired",
        userProvided: "User Provided",
        runtimeLearned: "Runtime Learned",
        total: "Total",
        quality: "Quality",
        uses: "Uses",
        successes: "Successes",
        mismatches: "Mismatches",
        revisionPending: "Revision Pending",
        citations: "Citations",
        lineage: "Lineage",
        strategyDiffs: "Strategy Diffs",
        empty: "No Skill Memory yet",
        emptyDetail: "Select a skill to inspect details",
        name: "Name",
        titleLabel: "Title",
        topic: "Topic",
        summary: "Summary",
        procedure: "Procedure",
        citationsInput: "Citations, one per line",
        file: "Read local text",
        deleteTitle: "Delete Skill",
        deleteDesc: "Deletion removes the memory record from storage without a console tombstone.",
        modalTitle: {
          create: "New Skill",
          import: "Import Skill",
          edit: "Edit Skill",
          delete: "Delete Skill",
        },
      },
      transports: {
        http: {
          name: "HTTP API",
          detail: "Configuration, write, recall, and inspection report entry",
          fields: ["Auth: required", "Rate limit: 120/min", "Mode: local"],
        },
        wss: {
          name: "WebSocket Subscription",
          detail: "Long-lived subscription for runtime events and recall streams",
          fields: ["Auth: required", "Subscription limit: 8"],
        },
        mcp: {
          name: "MCP stdio",
          detail: "Local tool process entry backed by the same runtime",
          fields: ["Tools: recall, projection, check"],
        },
        a2a: {
          name: "A2A HTTP",
          detail: "Memory bridge from agent to agent",
          fields: ["Auth: required", "Bridge: thin adapter"],
        },
      },
      devicesPanel: { label: "Access Control", title: "Allowed Devices" },
      deviceHeaders: ["Device", "app_key Fingerprint", "Status", "Actions"],
      devices: {
        "bm-linux-core-01": {
          label: "Linux Device Core",
          lastSeen: "18 seconds ago",
        },
        "bm-desktop-mac-studio": {
          label: "Desktop App",
          lastSeen: "2 minutes ago",
        },
        "bm-esp-memory-7f2": {
          label: "ESP Standalone Memory Device",
          lastSeen: "9 minutes ago",
        },
        "legacy-lab-device": {
          label: "Disabled Lab Device",
          lastSeen: "Yesterday",
        },
      },
      capabilityPanel: { label: "Capability Report", title: "Visible Capability Preview" },
      capabilityRows: [
        { title: "Write Governance", status: "ready", desc: "Every write enters the unified runtime" },
        { title: "Soul & Subject Memory", status: "ready", desc: "Projection and subject memory are enabled" },
        { title: "Archive Replay", status: "limited", desc: "Current runtime shape only allows lightweight replay" },
        { title: "Device Allowlist", status: "ready", desc: "Device count loads from backend" },
      ] satisfies CapRow[],
      kernel: {
        label: "Runtime Status",
        title: "Kernel Summary",
        rows: [
          { label: "Storage Backend", value: "Embedded / file backend available" },
          { label: "Key Policy", value: "Only presence state is shown" },
          { label: "Lifecycle", value: "Runtime opened" },
        ],
      },
      statusbar: { brand: "Beetles Memory", skills: "Skills", transports: "Transports", devices: "Devices" },
      addDevice: {
        btn: "Add Device",
        title: "Add New Device",
        nameLabel: "Device Name",
        namePlaceholder: "e.g. ESP32 Edge Node",
        save: "Save",
        cancel: "Cancel",
        closeLabel: "Close",
        issuedKeyNotice: "New app_key is shown once only",
        keyDialogTitle: "One-time app_key",
        keyDialogDesc: "It will not be shown again after this dialog is closed.",
        copyKey: "Copy",
        copied: "Copied",
      },
    },
  };

  const mapId = <T extends { id: string }>(list: T[], id: string, fn: (t: T) => T) =>
    list.map((t) => (t.id === id ? fn(t) : t));
  const mapDev = <T extends { deviceId: string }>(list: T[], id: string, fn: (t: T) => T) =>
    list.map((t) => (t.deviceId === id ? fn(t) : t));

  let activePage: PageId = $state("overview");
  let theme: Theme = $state(readTheme());
  let lang: Lang = $state(readLang());
  let backendConnected = $state(false);
  let overviewData: ConsoleApiOverview | null = $state(null);
  let sessionData: ConsoleApiSession | null = $state(null);

  let transports: Transport[] = $state([]);

  let devices: Device[] = $state([]);
  let skillReport: ConsoleApiSkillList | null = $state(null);
  let skills: ConsoleApiSkillSummary[] = $state([]);
  let selectedSkillName: string | null = $state(null);
  let selectedSkill: ConsoleApiSkillDetail | null = $state(null);
  let skillModal: SkillModal = $state(null);
  let skillForm: SkillForm = $state({ title: "", topic: "", summary: "", procedure: "", citations: "" });
  let skillError = $state("");
  let skillSearch = $state("");
  let skillStatusFilter: "all" | "active" | "disabled" | "retired" = $state("all");
  let skillOriginFilter: "all" | SkillOrigin = $state("all");

  const t = $derived(copy[lang]);
  const runtimeShape = $derived(displayRuntimeShape(overviewData?.runtimeShape, t.runtimeShape, lang));
  const systemInfo = $derived(displaySystemInfo(overviewData?.systemInfo, lang));
  const enabledTransportCount = $derived(transports.filter((transport) => transport.enabled).length);
  const activeDeviceCount = $derived(devices.filter((device) => device.status !== "disabled").length);
  const skillCount = $derived(skillReport?.total ?? skills.length);
  const selectedSkillSummary = $derived(skills.find((skill) => skill.name === selectedSkillName) ?? selectedSkill?.summary ?? null);
  const filteredSkills = $derived(filterSkills(skills, skillSearch, skillStatusFilter, skillOriginFilter));
  const pages = $derived(t.pages.map((page) => {
    if (page.id === "skills") return { ...page, count: String(skillCount) };
    if (page.id === "transports") return { ...page, count: String(transports.length) };
    if (page.id === "devices") return { ...page, count: String(devices.length) };
    return page;
  }));
  const currentPage = $derived(pages.find((page) => page.id === activePage) ?? pages[0]);
  const backendStatusLabel = $derived(backendConnected ? t.labels.backendConnected : t.labels.backendOffline);

  const overviewCards = $derived([
    {
      title: t.labels.systemInfo,
      value: systemInfo.name,
      desc: systemInfoDesc(systemInfo, lang),
      icon: Cpu,
      tone: "ready",
      compact: true,
      progress: null,
    },
    metricCard("storage", t.overview.storage, overviewData?.storage, Database, "ready", 0),
    metricCard("writes", t.overview.writes, overviewData?.writesToday, BarChart3, "ready", null),
    metricCard("recall", t.overview.recall, overviewData?.recall, Activity, "ready", 0),
    metricCard("projection", t.overview.projection, overviewData?.projection, MemoryStick, "limited", null),
    metricCard("devices", t.overview.devices, overviewData?.devices, Smartphone, "limited", 0),
  ] satisfies OverviewCard[]);

  const transportStats = $derived<KVRow[]>([
    { label: t.transportStats.enabled, value: `${enabledTransportCount} / ${transports.length}` },
    { label: t.transportStats.devices, value: `${activeDeviceCount} / ${devices.length}` },
  ]);
  const accountFields = $derived<KVRow[]>(
    sessionData
      ? [
          { label: t.account.fields[0].label, value: sessionAccountLabel(sessionData.account, lang) },
          { label: t.account.fields[1].label, value: sessionData.owner },
          { label: t.account.fields[2].label, value: sessionData.memoryScope },
          { label: t.account.fields[3].label, value: sessionStateLabel(sessionData.sessionState, lang) },
        ]
      : t.account.fields,
  );
  const recentEvents = $derived(localizedEvents(overviewData?.recentEvents, t.recentEvents, lang));
  const capabilityRows = $derived(localizedCapabilityRows(overviewData?.capabilities, t.capabilityRows, lang));
  const kernelRows = $derived(localizedKernelRows(overviewData?.kernel, t.kernel.rows, lang));

  $effect(() => writeStorage(STORAGE_KEYS.theme, theme));
  $effect(() => writeStorage(STORAGE_KEYS.lang, lang));

  const statusLabel = (status: StatusKind) => t.statusLabels[status] ?? status;
  const statusIcon = (status: StatusKind) => STATUS_ICONS[status] ?? Circle;

  function sessionAccountLabel(account: string, currentLang: Lang): string {
    if (account === "operator") return currentLang === "zh-CN" ? "本地账户" : "Local Account";
    return account;
  }

  function sessionStateLabel(state: string, currentLang: Lang): string {
    if (state === "paired") return currentLang === "zh-CN" ? "已通过配对门禁" : "Paired";
    return state;
  }

  function displayRuntimeShape(api: ConsoleApiRuntimeShape | undefined, fallback: typeof copy["zh-CN"]["runtimeShape"], currentLang: Lang) {
    if (!api) return fallback;
    if (currentLang === "zh-CN") {
      return {
        name: profileLabel(api.profile, currentLang),
        store: storeLabel(api.store, currentLang),
        shell: "HTTP 配置壳",
      };
    }
    return {
      name: profileLabel(api.profile, currentLang) || api.name,
      store: storeLabel(api.store, currentLang),
      shell: api.shell,
    };
  }

  function profileLabel(profile: string, currentLang: Lang): string {
    const zh: Record<string, string> = {
      "profile-esp-standalone-memory": "ESP 独立记忆部署",
      "profile-esp-embedded-sdk": "ESP SDK 集成",
      "target-linux-device+role-standalone-memory": "Linux 设备独立部署",
      "target-desktop-macos+role-embedded-sdk": "macOS SDK 集成",
      "target-desktop-windows+role-embedded-sdk": "Windows SDK 集成",
      "target-server-linux+role-memory-gateway": "Linux 服务器记忆网关",
      "target-server-linux+role-dev-full": "本地开发完整运行时",
    };
    const en: Record<string, string> = {
      "profile-esp-standalone-memory": "ESP standalone memory",
      "profile-esp-embedded-sdk": "ESP embedded SDK",
      "target-linux-device+role-standalone-memory": "Linux device standalone",
      "target-desktop-macos+role-embedded-sdk": "macOS embedded SDK",
      "target-desktop-windows+role-embedded-sdk": "Windows embedded SDK",
      "target-server-linux+role-memory-gateway": "Linux server memory gateway",
      "target-server-linux+role-dev-full": "Local development full runtime",
    };
    return (currentLang === "zh-CN" ? zh : en)[profile] ?? profile;
  }

  function storeLabel(store: string, currentLang: Lang): string {
    const zh: Record<string, string> = {
      "in-memory": "内存后端",
      embedded: "嵌入式后端",
      file: "文件后端",
      sqlite: "SQLite 后端",
    };
    const en: Record<string, string> = {
      "in-memory": "in-memory backend",
      embedded: "embedded backend",
      file: "file backend",
      sqlite: "SQLite backend",
    };
    return (currentLang === "zh-CN" ? zh : en)[store] ?? store;
  }

  function displaySystemInfo(api: ConsoleApiSystemInfo | undefined, currentLang: Lang): ConsoleApiSystemInfo {
    if (api) return api;
    return {
      name: currentLang === "zh-CN" ? "系统名称" : "System Name",
      cpu: "CPU",
      memory: currentLang === "zh-CN" ? "内存" : "Memory",
      timeUnixSecs: Math.floor(Date.now() / 1000),
    };
  }

  function systemInfoDesc(info: ConsoleApiSystemInfo, currentLang: Lang): string {
    const time = formatSystemTime(info.timeUnixSecs, currentLang);
    return `${info.cpu} \\ ${info.memory} \\ ${time}`;
  }

  function formatSystemTime(timeUnixSecs: number, currentLang: Lang): string {
    const date = new Date(timeUnixSecs * 1000);
    if (Number.isNaN(date.getTime())) return "-";
    return new Intl.DateTimeFormat(currentLang, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    }).format(date);
  }

  function metricCard(
    kind: "storage" | "writes" | "recall" | "projection" | "devices",
    fallback: { title: string; value: string; desc: string },
    metric: ConsoleApiMetric | undefined,
    icon: Component,
    tone: string,
    progressFallback: number | null,
  ): OverviewCard {
    return {
      title: fallback.title,
      value: metric?.value ?? fallback.value,
      desc: metric ? localizedMetricDesc(kind, metric.desc, fallback.desc, lang) : fallback.desc,
      icon,
      tone,
      progress: metric?.progress ?? progressFallback,
    };
  }

  function localizedMetricDesc(
    kind: "storage" | "writes" | "recall" | "projection" | "devices",
    desc: string,
    fallback: string,
    currentLang: Lang,
  ): string {
    if (currentLang === "en") return desc || fallback;
    if (kind === "storage") return "当前系统占用 / 实际系统可用大小";
    if (kind === "writes") return "当前运行时已接受的记忆写入";
    if (kind === "recall") return desc.replace(" recall requests / ", " 次召回请求 / ").replace(" with hits", " 次命中");
    if (kind === "projection") return desc.replace(" projection requests served", " 次投影请求已服务");
    if (kind === "devices") return "开放设备访问状态";
    return fallback;
  }

  function localizedEvents(events: TimelineEvent[] | undefined, fallback: TimelineEvent[], currentLang: Lang): TimelineEvent[] {
    if (!events || events.length === 0) return fallback;
    if (currentLang === "en") return events;
    return events.map((event) => ({ ...event, text: localizeEventText(event.text) }));
  }

  function localizeEventText(text: string): string {
    if (text.startsWith("Transport ")) return text.replace("Transport ", "通信 ").replace(" updated", " 已更新");
    if (text.startsWith("Device ") && text.endsWith(" key rotated")) {
      return text.replace("Device ", "设备 ").replace(" key rotated", " 密钥已轮换");
    }
    if (text.startsWith("Device ") && text.endsWith(" added")) return text.replace("Device ", "设备 ").replace(" added", " 已添加");
    if (text.startsWith("Device ") && text.endsWith(" updated")) return text.replace("Device ", "设备 ").replace(" updated", " 已更新");
    if (text.startsWith("Skill ")) {
      return text
        .replace("Skill ", "Skill ")
        .replace(" imported", " 已导入")
        .replace(" updated", " 已更新")
        .replace(" disabled", " 已停用")
        .replace(" enabled", " 已启用")
        .replace(" deleted", " 已删除");
    }
    if (text.startsWith("Memory write accepted")) return text.replace("Memory write accepted, changed", "记忆写入已接受，变更");
    if (text.startsWith("Recall served for")) return text.replace("Recall served for", "召回已执行：").replace(" with ", "，命中 ").replace(" hits", " 条");
    if (text.startsWith("Projection served")) return text.replace("Projection served,", "投影已生成，");
    if (text.endsWith("communication entries enabled")) return text.replace("communication entries enabled", "个通信入口已启用");
    if (text === "Console runtime opened") return "配置台运行时已打开";
    return text;
  }

  function localizedCapabilityRows(rows: CapRow[] | undefined, fallback: CapRow[], currentLang: Lang): CapRow[] {
    if (!rows || rows.length === 0) return fallback;
    if (currentLang === "en") return rows;
    return rows.map((row) => ({
      ...row,
      title: capabilityTitle(row.title),
      desc: capabilityDesc(row.title, row.desc),
    }));
  }

  function localizedKernelRows(rows: KVRow[] | undefined, fallback: KVRow[], currentLang: Lang): KVRow[] {
    if (!rows || rows.length === 0) return fallback;
    if (currentLang === "en") return rows;
    return rows.map((row) => ({
      label: kernelLabel(row.label),
      value: kernelValue(row.label, row.value, currentLang),
    }));
  }

  function capabilityTitle(title: string): string {
    const titles: Record<string, string> = {
      "Write governance": "写入治理",
      "Soul and subject memory": "灵魂与主体记忆",
      "Device allowlist": "设备白名单",
    };
    return titles[title] ?? title;
  }

  function capabilityDesc(title: string, desc: string): string {
    if (title === "Write governance") return "所有写入进入统一记忆运行时";
    if (title === "Soul and subject memory") return "投影与主体记忆已启用";
    if (title === "Device allowlist") return desc.replace(" devices configured", " 个设备已配置");
    return desc;
  }

  function kernelLabel(label: string): string {
    const labels: Record<string, string> = {
      Profile: "运行档位",
      "Store backend": "存储后端",
      "Console shell": "配置壳",
    };
    return labels[label] ?? label;
  }

  function kernelValue(label: string, value: string, currentLang: Lang): string {
    if (label === "Profile") return profileLabel(value, currentLang);
    if (label === "Store backend") return storeLabel(value, currentLang);
    if (label === "Console shell") return "HTTP 配置壳";
    return value;
  }

  type StaticDeviceId = "bm-linux-core-01" | "bm-desktop-mac-studio" | "bm-esp-memory-7f2" | "legacy-lab-device";
  const isStaticDevice = (id: string): id is StaticDeviceId =>
    id in (t.devices as Record<string, unknown>);

  const deviceLabel = (device: Device): string =>
    device.label ?? (isStaticDevice(device.deviceId) ? t.devices[device.deviceId].label : device.deviceId);

  let addDeviceOpen = $state(false);
  let newDevice = $state({ label: "" });
  let formError = $state("");
  let issuedKeyDialog: { deviceId: string; label: string; appKey: string } | null = $state(null);
  let issuedKeyCopied = $state(false);

  function openAddDevice() {
    newDevice = { label: "" };
    formError = "";
    addDeviceOpen = true;
  }
  function closeModal() { addDeviceOpen = false; }

  function showIssuedKey(device: ConsoleApiDevice, appKey: string) {
    issuedKeyCopied = false;
    issuedKeyDialog = {
      deviceId: device.deviceId,
      label: device.label,
      appKey,
    };
  }

  function closeIssuedKeyDialog() {
    issuedKeyDialog = null;
    issuedKeyCopied = false;
  }

  async function copyIssuedKey() {
    if (!issuedKeyDialog || typeof navigator === "undefined" || !navigator.clipboard) return;
    await navigator.clipboard.writeText(issuedKeyDialog.appKey);
    issuedKeyCopied = true;
  }

  onMount(() => {
    void loadConsoleData();
  });

  async function apiJson<T>(path: string, init: RequestInit = {}): Promise<T> {
    const headers = new Headers(init.headers);
    headers.set("content-type", "application/json");
    headers.set("x-loopback", "true");
    const response = await fetch(path, {
      ...init,
      headers,
    });
    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`);
    }
    return await response.json() as T;
  }

  const fromApiTransport = (transport: ConsoleApiTransport): Transport => ({
    id: transport.id,
    enabled: transport.enabled,
    status: transport.status,
    endpoint: transport.endpoint,
    editable: transport.editable,
  });
  const fromApiDevice = (device: ConsoleApiDevice): Device => ({
    deviceId: device.deviceId,
    label: device.label,
    appKey: device.appKeyFingerprint,
    status: device.status,
  });

  function filterSkills(
    source: ConsoleApiSkillSummary[],
    search: string,
    statusFilter: "all" | "active" | "disabled" | "retired",
    originFilter: "all" | SkillOrigin,
  ): ConsoleApiSkillSummary[] {
    const needle = search.trim().toLowerCase();
    return source.filter((skill) => {
      if (originFilter !== "all" && skill.origin !== originFilter) return false;
      if (statusFilter === "active" && (!skill.enabled || skill.status === "retired")) return false;
      if (statusFilter === "disabled" && skill.enabled) return false;
      if (statusFilter === "retired" && skill.status !== "retired") return false;
      if (!needle) return true;
      return [skill.name, skill.title, skill.topic, skill.status, skill.origin]
        .some((value) => value.toLowerCase().includes(needle));
    });
  }

  function skillOriginLabel(origin: SkillOrigin): string {
    if (origin === "user_provided") return t.skillsPanel.userProvided;
    return t.skillsPanel.runtimeLearned;
  }

  function skillKindLabel(kind: SkillKind): string {
    if (kind === "runtime_skill") return "Runtime Skill";
    return lang === "zh-CN" ? "手工文档" : "Manual Document";
  }

  function skillQuality(skill: ConsoleApiSkillSummary): string {
    return skill.qualityScore === null ? "-" : `${skill.qualityScore}`;
  }

  function parseCitations(value: string): string[] {
    return value
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);
  }

  function citationsText(citations: string[]): string {
    return citations.join("\n");
  }

  function resetSkillForm() {
    skillForm = { title: "", topic: "", summary: "", procedure: "", citations: "" };
    skillError = "";
  }

  function openSkillCreate() {
    resetSkillForm();
    skillModal = "create";
  }

  function openSkillImport() {
    resetSkillForm();
    skillModal = "import";
  }

  function openSkillEdit() {
    if (!selectedSkill) return;
    skillForm = {
      title: selectedSkill.summary.title,
      topic: selectedSkill.summary.topic,
      summary: selectedSkill.summaryText,
      procedure: selectedSkill.procedureText,
      citations: citationsText(selectedSkill.citations),
    };
    skillError = "";
    skillModal = "edit";
  }

  function openSkillDelete() {
    if (!selectedSkillSummary) return;
    skillError = "";
    skillModal = "delete";
  }

  function closeSkillModal() {
    skillModal = null;
    skillError = "";
  }

  async function selectSkill(name: string) {
    selectedSkillName = name;
    if (!backendConnected) return;
    try {
      const response = await apiJson<{ skill: ConsoleApiSkillDetail }>(`/console/skills/${encodeURIComponent(name)}`);
      selectedSkill = response.skill;
    } catch {
      selectedSkill = null;
      backendConnected = false;
    }
  }

  async function submitSkillForm(e: SubmitEvent) {
    e.preventDefault();
    if (!backendConnected) {
      skillError = t.labels.backendOffline;
      return;
    }
    const title = skillForm.title.trim();
    const topic = skillForm.topic.trim();
    const summary = skillForm.summary.trim();
    const procedure = skillForm.procedure.trim();
    if (!title || !topic || !summary || !procedure) {
      skillError = lang === "zh-CN" ? "标题、主题、摘要和过程都不能为空" : "Title, topic, summary, and procedure are required";
      return;
    }
    const name = skillModal === "edit" ? selectedSkill?.summary.name : undefined;
    try {
      const response = await apiJson<{ mutation: ConsoleApiSkillMutation }>(
        name ? `/console/skills/${encodeURIComponent(name)}` : "/console/skills",
        {
          method: name ? "PATCH" : "POST",
          body: JSON.stringify({
            title,
            topic,
            summary,
            procedure,
            citations: parseCitations(skillForm.citations),
          }),
        },
      );
      closeSkillModal();
      await loadConsoleData();
      await selectSkill(response.mutation.name);
    } catch (error) {
      skillError = error instanceof Error ? error.message : String(error);
      backendConnected = false;
    }
  }

  async function toggleSkillEnabled(skill: ConsoleApiSkillSummary) {
    if (!backendConnected) return;
    try {
      await apiJson<{ mutation: ConsoleApiSkillMutation }>(`/console/skills/${encodeURIComponent(skill.name)}/enabled`, {
        method: "PATCH",
        body: JSON.stringify({ enabled: !skill.enabled }),
      });
      await loadConsoleData();
      if (selectedSkillName === skill.name) await selectSkill(skill.name);
    } catch {
      backendConnected = false;
    }
  }

  async function deleteSelectedSkill() {
    if (!selectedSkillSummary || !backendConnected) return;
    try {
      await apiJson<{ mutation: ConsoleApiSkillMutation }>(`/console/skills/${encodeURIComponent(selectedSkillSummary.name)}`, {
        method: "DELETE",
      });
      closeSkillModal();
      selectedSkillName = null;
      selectedSkill = null;
      await loadConsoleData();
    } catch (error) {
      skillError = error instanceof Error ? error.message : String(error);
      backendConnected = false;
    }
  }

  async function readSkillFile(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    const text = await file.text();
    skillForm = {
      ...skillForm,
      title: skillForm.title || file.name.replace(/\.[^.]+$/, ""),
      procedure: text,
      summary: skillForm.summary || text.trim().split(/\r?\n/).find(Boolean)?.slice(0, 180) || "",
    };
    input.value = "";
  }

  async function loadConsoleData() {
    try {
      const [overviewResponse, skillResponse, transportResponse, deviceResponse, sessionResponse] = await Promise.all([
        apiJson<{ overview: ConsoleApiOverview }>("/console/overview"),
        apiJson<{ skills: ConsoleApiSkillList }>("/console/skills"),
        apiJson<{ transports: ConsoleApiTransport[] }>("/console/transports"),
        apiJson<{ devices: ConsoleApiDevice[] }>("/console/devices"),
        apiJson<{ session: ConsoleApiSession }>("/console/session"),
      ]);
      overviewData = overviewResponse.overview;
      skillReport = skillResponse.skills;
      skills = skillResponse.skills.skills;
      if (selectedSkillName && !skills.some((skill) => skill.name === selectedSkillName)) {
        selectedSkillName = null;
        selectedSkill = null;
      }
      transports = transportResponse.transports.map(fromApiTransport);
      devices = deviceResponse.devices.map(fromApiDevice);
      sessionData = sessionResponse.session;
      backendConnected = true;
    } catch {
      overviewData = null;
      skillReport = null;
      skills = [];
      selectedSkillName = null;
      selectedSkill = null;
      transports = [];
      devices = [];
      sessionData = null;
      backendConnected = false;
    }
  }

  async function saveDevice(e: SubmitEvent) {
    e.preventDefault();
    if (!newDevice.label.trim()) {
      formError = lang === "zh-CN" ? "设备名称不能为空" : "Device name is required";
      return;
    }
    if (!backendConnected) {
      formError = t.labels.backendOffline;
      return;
    }
    try {
      const response = await apiJson<{ device: ConsoleApiDevice; appKeyOnce: string }>("/console/devices", {
        method: "POST",
        body: JSON.stringify({
          label: newDevice.label.trim(),
        }),
      });
      devices = [...devices, fromApiDevice(response.device)];
      closeModal();
      showIssuedKey(response.device, response.appKeyOnce);
      void loadConsoleData();
    } catch (error) {
      formError = error instanceof Error ? error.message : String(error);
      backendConnected = false;
    }
  }

  async function updateTransport(id: TransportId, update: Partial<Transport>) {
    if (!backendConnected) return;
    try {
      const response = await apiJson<{ transport: ConsoleApiTransport }>(`/console/transports/${id}`, {
        method: "PATCH",
        body: JSON.stringify({
          enabled: update.enabled,
          endpoint: update.endpoint,
        }),
      });
      transports = mapId(transports, id, () => fromApiTransport(response.transport));
      void loadConsoleData();
    } catch {
      backendConnected = false;
    }
  }

  function toggleTransport(id: TransportId) {
    if (!backendConnected) return;
    const current = transports.find((transport) => transport.id === id);
    if (!current || current.editable === false) return;
    const enabled = !current.enabled;
    void updateTransport(id, { enabled });
  }

  function saveTransportEndpoint(id: TransportId, endpoint: string) {
    void updateTransport(id, { endpoint });
  }

  async function rotateAppKey(deviceId: string) {
    if (!backendConnected) return;
    try {
      const response = await apiJson<{ device: ConsoleApiDevice; appKeyOnce: string }>(`/console/devices/${deviceId}/rotate-key`, {
        method: "POST",
        body: "{}",
      });
      devices = mapDev(devices, deviceId, () => fromApiDevice(response.device));
      showIssuedKey(response.device, response.appKeyOnce);
      void loadConsoleData();
    } catch {
      backendConnected = false;
    }
  }

  function toggleDevice(deviceId: string) {
    if (!backendConnected) return;
    const current = devices.find((device) => device.deviceId === deviceId);
    if (!current) return;
    const status: StatusKind = current.status === "disabled" ? "allowed" : "disabled";
    void apiJson<{ device: ConsoleApiDevice }>(`/console/devices/${deviceId}`, {
      method: "PATCH",
      body: JSON.stringify({ status }),
    })
      .then((response) => {
        devices = mapDev(devices, deviceId, () => fromApiDevice(response.device));
        void loadConsoleData();
      })
      .catch(() => {
        backendConnected = false;
      });
  }

</script>

<svelte:head>
  <title>{t.headTitle}</title>
</svelte:head>

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
  <aside class="sidebar">
    <div class="brand">
      <div class="brand-icon"><img src="/logo.png" alt="BM" /></div>
      <div class="brand-text">
        <span class="brand-name">{t.brand.name}</span>
        <span class="brand-sub">{t.brand.sub}</span>
      </div>
    </div>

    <nav class="nav">
      {#each pages as page}
        <button
          class:active={activePage === page.id}
          class="nav-item"
          type="button"
          onclick={() => (activePage = page.id)}
        >
          <span class="nav-chevron">{activePage === page.id ? "▶" : "›"}</span>
          <span class="nav-label">{page.label}</span>
          {#if page.count}<code class="nav-count">[{page.count}]</code>{/if}
        </button>
      {/each}
    </nav>

    <div class="sidebar-status">
      <div class="ss-row"><span class="ss-label">{t.labels.status}</span><span class="ss-value ok">{t.labels.loggedIn}</span></div>
      <div class="ss-row"><span class="ss-label">{t.labels.shell}</span><span class="ss-value">{runtimeShape.shell}</span></div>
      <small class="ss-note">{backendStatusLabel}</small>
    </div>
  </aside>

  <section class="workspace">
    <header class="topbar">
      <div class="topbar-left">
        <div class="breadcrumb">
          <span>{t.labels.console}</span>
          <span class="bc-sep">›</span>
          <span>{currentPage.eyebrow}</span>
          <span class="bc-sep">›</span>
          <span class="bc-title">{currentPage.title}</span>
        </div>
      </div>
      <div class="top-actions">
        <button class="ghost-button" type="button" onclick={() => (theme = theme === "light" ? "dark" : "light")}>
          {#if theme === "light"}<Moon size={13} /> {t.actions.dark}{:else}<Sun size={13} /> {t.actions.light}{/if}
        </button>
        <select class="lang-select" bind:value={lang} aria-label={t.actions.language}>
          <option value="zh-CN">{t.language.zh}</option>
          <option value="en">{t.language.en}</option>
        </select>
        <button class="primary-button" type="button" onclick={loadConsoleData}><Power size={13} /> {t.actions.apply}</button>
      </div>
    </header>

    <div class="page-shell">
      {#if activePage === "overview"}
        <section class="overview-grid">
          {#each overviewCards as card, i}
            {@const Icon = card.icon}
            {#if i === 0}
              <article class={`overview-card ${card.tone} featured`}>
                <div class="oc-main">
                  <div class="overview-card-head"><span>{card.title}</span></div>
                  <strong>{card.value}</strong>
                  <small>{card.desc}</small>
                </div>
                <div class="oc-deco">
                  <Icon size={48} />
                  <span class={`badge ${card.tone}`}>{card.tone === "ready" ? "READY" : "WARN"}</span>
                </div>
              </article>
            {:else}
              <article class={`overview-card ${card.tone}${i === overviewCards.length - 1 ? " wide" : ""}`}>
                <div class="overview-card-head"><span>{card.title}</span><Icon size={18} /></div>
                <strong>{card.value}</strong>
                {#if card.progress !== null}
                  <div class="hud-bar"><div class="hud-bar-fill" style="width:{card.progress}%"></div></div>
                {/if}
                <small>{card.desc}</small>
              </article>
            {/if}
          {/each}
        </section>

        <section class="section-grid overview-lower">
          <article class="panel">
            {@render panelHeader(t.overview.observation, t.overview.communicationAccess, Globe2)}
            {@render kvStack(transportStats)}
          </article>
          <article class="panel">
            {@render panelHeader(t.overview.recentEvents, t.overview.timeline, Activity)}
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
        <section class="section-grid overview-lower">
          <article class="panel">
            {@render panelHeader(t.capabilityPanel.label, t.capabilityPanel.title, ShieldCheck)}
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
            {@render panelHeader(t.kernel.label, t.kernel.title, Server)}
            {@render kvStack(kernelRows)}
          </article>
        </section>
      {:else if activePage === "skills"}
        <section class="skill-layout">
          <article class="panel skill-list-panel">
            <div class="panel-title">
              <div>
                <p class="panel-label">{t.skillsPanel.label}</p>
                <h3>{t.skillsPanel.title}</h3>
              </div>
              <div class="panel-title-actions">
                <button class="ghost-button" type="button" onclick={openSkillImport}>
                  <Upload size={13} /> {t.actions.importSkill}
                </button>
                <button class="primary-button" type="button" onclick={openSkillCreate}>
                  <Plus size={13} /> {t.actions.createSkill}
                </button>
              </div>
            </div>

            <div class="skill-stats">
              <div><span>{t.skillsPanel.total}</span><strong>{skillReport?.total ?? 0}</strong></div>
              <div><span>{t.skillsPanel.active}</span><strong>{skillReport?.active ?? 0}</strong></div>
              <div><span>{t.skillsPanel.disabled}</span><strong>{skillReport?.disabled ?? 0}</strong></div>
              <div><span>{t.skillsPanel.runtimeLearned}</span><strong>{skillReport?.runtimeLearned ?? 0}</strong></div>
              <div><span>{t.skillsPanel.userProvided}</span><strong>{skillReport?.userProvided ?? 0}</strong></div>
            </div>

            <div class="skill-toolbar">
              <label class="skill-search">
                <span><Search size={13} /> {t.skillsPanel.search}</span>
                <input bind:value={skillSearch} placeholder={t.skillsPanel.search} />
              </label>
              <div class="skill-filters">
                <select bind:value={skillStatusFilter}>
                  <option value="all">{t.skillsPanel.all}</option>
                  <option value="active">{t.skillsPanel.active}</option>
                  <option value="disabled">{t.skillsPanel.disabled}</option>
                  <option value="retired">{t.skillsPanel.retired}</option>
                </select>
                <select bind:value={skillOriginFilter}>
                  <option value="all">{t.skillsPanel.all}</option>
                  <option value="user_provided">{t.skillsPanel.userProvided}</option>
                  <option value="runtime_learned">{t.skillsPanel.runtimeLearned}</option>
                </select>
              </div>
            </div>

            <div class="skill-list">
              {#if filteredSkills.length === 0}
                <div class="skill-empty">{t.skillsPanel.empty}</div>
              {:else}
                {#each filteredSkills as skill}
                  <button
                    class:active={selectedSkillName === skill.name}
                    class="skill-row"
                    type="button"
                    onclick={() => selectSkill(skill.name)}
                  >
                    <span class="skill-row-main">
                      <strong>{skill.title}</strong>
                      <small>{skill.topic} · {skill.name}</small>
                    </span>
                    <span class="skill-row-meta">
                      <span class={`badge ${skill.enabled ? skill.status : "disabled"}`}>{skill.enabled ? statusLabel(skill.status) : statusLabel("disabled")}</span>
                      <span>{skillOriginLabel(skill.origin)}</span>
                      <span>{t.skillsPanel.quality}: {skillQuality(skill)}</span>
                      <span>{t.skillsPanel.uses}: {skill.useCount}</span>
                    </span>
                  </button>
                {/each}
              {/if}
            </div>
          </article>

          <article class="panel skill-detail-panel">
            {#if selectedSkill && selectedSkillSummary}
              <div class="panel-title">
                <div>
                  <p class="panel-label">{skillOriginLabel(selectedSkillSummary.origin)} · {skillKindLabel(selectedSkillSummary.kind)}</p>
                  <h3>{selectedSkillSummary.title}</h3>
                </div>
                <div class="panel-title-actions">
                  <button class="ghost-button" type="button" onclick={() => toggleSkillEnabled(selectedSkillSummary)} disabled={!backendConnected}>
                    {selectedSkillSummary.enabled ? t.actions.disable : t.actions.enable}
                  </button>
                  <button class="ghost-button" type="button" onclick={openSkillEdit}><Pencil size={13} /> {t.actions.edit}</button>
                  <button class="ghost-button danger-button" type="button" onclick={openSkillDelete}><Trash2 size={13} /> {t.actions.delete}</button>
                </div>
              </div>

              <div class="skill-meta-grid">
                <div><span>{t.skillsPanel.name}</span><strong>{selectedSkillSummary.name}</strong></div>
                <div><span>{t.skillsPanel.topic}</span><strong>{selectedSkillSummary.topic}</strong></div>
                <div><span>{t.skillsPanel.quality}</span><strong>{skillQuality(selectedSkillSummary)}</strong></div>
                <div><span>{t.skillsPanel.uses}</span><strong>{selectedSkillSummary.useCount}</strong></div>
                <div><span>{t.skillsPanel.successes}</span><strong>{selectedSkillSummary.validatedSuccessCount}</strong></div>
                <div><span>{t.skillsPanel.mismatches}</span><strong>{selectedSkillSummary.mismatchCount}</strong></div>
                <div><span>{t.skillsPanel.revisionPending}</span><strong>{selectedSkillSummary.revisionPending ? "YES" : "NO"}</strong></div>
                <div><span>{t.labels.status}</span><strong>{selectedSkillSummary.enabled ? statusLabel(selectedSkillSummary.status) : statusLabel("disabled")}</strong></div>
              </div>

              <div class="skill-detail">
                <section>
                  <h4>{t.skillsPanel.summary}</h4>
                  <p>{selectedSkill.summaryText}</p>
                </section>
                <section>
                  <h4>{t.skillsPanel.procedure}</h4>
                  <pre>{selectedSkill.procedureText}</pre>
                </section>
                <section>
                  <h4>{t.skillsPanel.citations}</h4>
                  {#if selectedSkill.citations.length === 0}
                    <p>-</p>
                  {:else}
                    <div class="chips">{#each selectedSkill.citations as citation}<span>{citation}</span>{/each}</div>
                  {/if}
                </section>
                <section>
                  <h4>{t.skillsPanel.lineage}</h4>
                  {#if selectedSkill.lineage.length === 0}
                    <p>-</p>
                  {:else}
                    <ul>{#each selectedSkill.lineage as item}<li>{item}</li>{/each}</ul>
                  {/if}
                </section>
                <section>
                  <h4>{t.skillsPanel.strategyDiffs}</h4>
                  {#if selectedSkill.strategyDiffs.length === 0}
                    <p>-</p>
                  {:else}
                    <ul>{#each selectedSkill.strategyDiffs as item}<li>{item}</li>{/each}</ul>
                  {/if}
                </section>
              </div>
            {:else}
              <div class="skill-empty detail-empty">
                <BookOpen size={28} />
                <span>{t.skillsPanel.emptyDetail}</span>
              </div>
            {/if}
          </article>
        </section>
      {:else if activePage === "account"}
        <section class="panel account-panel">
          {@render panelHeader(t.account.panel, t.account.title, KeyRound)}
          <div class="runtime-summary">
            {#each accountFields as row}
              <div><span>{row.label}</span><strong>{row.value}</strong></div>
            {/each}
          </div>
          <div class="notice">
            <LockKeyhole size={16} />
            <span>{t.account.notice}</span>
          </div>
        </section>
      {:else if activePage === "transports"}
        <section class="panel">
          {@render panelHeader(t.transportsPanel.label, t.transportsPanel.title, Globe2)}
          <div class="transport-grid">
            {#each transports as transport}
              {@const transportCopy = t.transports[transport.id]}
              <article class:disabled={!transport.enabled} class="transport-card">
                <div class="transport-head">
                  <button
                    aria-label={t.actions.toggleTransport(transportCopy.name)}
                    class:enabled={transport.enabled}
                    class="switch"
                    type="button"
                    disabled={!backendConnected || transport.editable === false}
                    onclick={() => toggleTransport(transport.id)}
                  ><span></span></button>
                  <div>
                    <h4>{transportCopy.name}</h4>
                    <p>{transportCopy.detail}</p>
                  </div>
                </div>
                <label>
                  <span>{t.labels.addressMode}</span>
                  <input
                    bind:value={transport.endpoint}
                    disabled={!backendConnected || !transport.enabled || transport.editable === false}
                    onchange={() => saveTransportEndpoint(transport.id, transport.endpoint)}
                  />
                </label>
                <div class="chips">{#each transportCopy.fields as field}<span>{field}</span>{/each}</div>
              </article>
            {/each}
          </div>
        </section>
      {:else if activePage === "devices"}
        <section class="panel">
          <div class="panel-title">
            <div>
              <p class="panel-label">{t.devicesPanel.label}</p>
              <h3>{t.devicesPanel.title}</h3>
            </div>
            <div class="panel-title-actions">
              <button class="ghost-button" type="button" onclick={openAddDevice}>
                <Plus size={13} /> {t.addDevice.btn}
              </button>
              <Smartphone size={18} />
            </div>
          </div>
          <div class="device-table">
            <div class="device-row header">
              {#each t.deviceHeaders as header}<span>{header}</span>{/each}
            </div>
            {#each devices as device}
              <div class="device-row">
                <span><strong>{deviceLabel(device)}</strong><small>{device.deviceId}</small></span>
                <span class="mono">{device.appKey}</span>
                <span class={`badge ${device.status}`}>{statusLabel(device.status)}</span>
                <span class="row-actions">
                  <button type="button" disabled={!backendConnected} onclick={() => rotateAppKey(device.deviceId)}><RefreshCw size={12} /> {t.actions.rotate}</button>
                  <button type="button" disabled={!backendConnected} onclick={() => toggleDevice(device.deviceId)}>
                    {device.status === "disabled" ? t.actions.enable : t.actions.disable}
                  </button>
                </span>
              </div>
            {/each}
          </div>
        </section>
      {/if}
    </div>
  </section>

  <div class="statusbar">
    <span class="sb-brand">{t.statusbar.brand}</span>
    <span class="sb-item">v1.0.0-dev</span>
    <span class="sb-sep">│</span>
    <span class="sb-item">{backendStatusLabel}</span>
    <div class="sb-right">
      <span class="sb-item">{t.statusbar.skills}: {skillCount}</span>
      <span class="sb-sep">│</span>
      <span class="sb-item">{t.statusbar.transports}: {enabledTransportCount}/{transports.length}</span>
      <span class="sb-sep">│</span>
      <span class="sb-item">{t.statusbar.devices}: {activeDeviceCount}/{devices.length}</span>
      <span class="sb-sep">│</span>
      <span class:ok={backendConnected} class="sb-item">● {backendConnected ? t.labels.online : t.labels.offline}</span>
    </div>
  </div>

  {#if skillModal && skillModal !== "delete"}
    <button class="modal-backdrop" type="button" onclick={closeSkillModal} aria-label={t.addDevice.closeLabel}></button>
    <div class="modal skill-editor-modal" role="dialog" aria-modal="true" aria-labelledby="skill-editor-title">
      <div class="modal-header">
        <h3 id="skill-editor-title">
          {#if skillModal === "import"}<Upload size={14} />{:else if skillModal === "edit"}<Pencil size={14} />{:else}<Plus size={14} />{/if}
          {t.skillsPanel.modalTitle[skillModal]}
        </h3>
        <button class="modal-close" type="button" onclick={closeSkillModal} aria-label={t.addDevice.closeLabel}>✕</button>
      </div>
      <form class="modal-body" onsubmit={submitSkillForm}>
        <div class="skill-form-grid">
          <label>
            <span>{t.skillsPanel.titleLabel}</span>
            <input bind:value={skillForm.title} required autocomplete="off" />
          </label>
          <label>
            <span>{t.skillsPanel.topic}</span>
            <input bind:value={skillForm.topic} required autocomplete="off" />
          </label>
        </div>
        <label>
          <span>{t.skillsPanel.summary}</span>
          <textarea bind:value={skillForm.summary} rows="3" required></textarea>
        </label>
        <label>
          <span>{t.skillsPanel.procedure}</span>
          <textarea bind:value={skillForm.procedure} rows="8" required></textarea>
        </label>
        {#if skillModal === "import"}
          <label class="file-reader">
            <span>{t.skillsPanel.file}</span>
            <input type="file" accept=".md,.txt,text/plain,text/markdown" onchange={readSkillFile} />
          </label>
        {/if}
        <label>
          <span>{t.skillsPanel.citationsInput}</span>
          <textarea bind:value={skillForm.citations} rows="3"></textarea>
        </label>
        {#if skillError}<p class="modal-error">{skillError}</p>{/if}
        <div class="modal-footer">
          <button class="ghost-button" type="button" onclick={closeSkillModal}>{t.actions.cancel}</button>
          <button class="primary-button" type="submit"><FileText size={13} /> {t.actions.save}</button>
        </div>
      </form>
    </div>
  {/if}

  {#if skillModal === "delete" && selectedSkillSummary}
    <button class="modal-backdrop" type="button" onclick={closeSkillModal} aria-label={t.addDevice.closeLabel}></button>
    <div class="modal" role="dialog" aria-modal="true" aria-labelledby="skill-delete-title">
      <div class="modal-header">
        <h3 id="skill-delete-title"><Trash2 size={14} /> {t.skillsPanel.deleteTitle}</h3>
        <button class="modal-close" type="button" onclick={closeSkillModal} aria-label={t.addDevice.closeLabel}>✕</button>
      </div>
      <div class="modal-body">
        <div class="issued-key-meta">
          <span>{t.skillsPanel.deleteDesc}</span>
          <strong>{selectedSkillSummary.title}</strong>
          <small>{selectedSkillSummary.name}</small>
        </div>
        {#if skillError}<p class="modal-error">{skillError}</p>{/if}
        <div class="modal-footer">
          <button class="ghost-button" type="button" onclick={closeSkillModal}>{t.actions.cancel}</button>
          <button class="primary-button danger-primary" type="button" onclick={deleteSelectedSkill}>{t.actions.delete}</button>
        </div>
      </div>
    </div>
  {/if}

  {#if addDeviceOpen}
    <button class="modal-backdrop" type="button" onclick={closeModal} aria-label={t.addDevice.closeLabel}></button>
    <div class="modal" role="dialog" aria-modal="true" aria-labelledby="add-device-title">
      <div class="modal-header">
        <h3 id="add-device-title"><Plus size={14} /> {t.addDevice.title}</h3>
        <button class="modal-close" type="button" onclick={closeModal} aria-label={t.addDevice.closeLabel}>✕</button>
      </div>
      <form class="modal-body" onsubmit={saveDevice}>
        <label>
          <span>{t.addDevice.nameLabel}</span>
          <input bind:value={newDevice.label} placeholder={t.addDevice.namePlaceholder} required autocomplete="off" />
        </label>
        {#if formError}<p class="modal-error">{formError}</p>{/if}
        <div class="modal-footer">
          <button class="ghost-button" type="button" onclick={closeModal}>{t.addDevice.cancel}</button>
          <button class="primary-button" type="submit">{t.addDevice.save}</button>
        </div>
      </form>
    </div>
  {/if}

  {#if issuedKeyDialog}
    <button class="modal-backdrop" type="button" onclick={closeIssuedKeyDialog} aria-label={t.addDevice.closeLabel}></button>
    <div class="modal key-modal" role="dialog" aria-modal="true" aria-labelledby="issued-key-title">
      <div class="modal-header">
        <h3 id="issued-key-title"><KeyRound size={14} /> {t.addDevice.keyDialogTitle}</h3>
        <button class="modal-close" type="button" onclick={closeIssuedKeyDialog} aria-label={t.addDevice.closeLabel}>✕</button>
      </div>
      <div class="modal-body">
        <div class="issued-key-meta">
          <span>{t.addDevice.issuedKeyNotice}</span>
          <strong>{issuedKeyDialog.label}</strong>
          <small>{issuedKeyDialog.deviceId}</small>
        </div>
        <code class="issued-key-code">{issuedKeyDialog.appKey}</code>
        <p class="modal-hint">{t.addDevice.keyDialogDesc}</p>
        <div class="modal-footer">
          <button class="ghost-button" type="button" onclick={copyIssuedKey}>{issuedKeyCopied ? t.addDevice.copied : t.addDevice.copyKey}</button>
          <button class="primary-button" type="button" onclick={closeIssuedKeyDialog}>{t.addDevice.closeLabel}</button>
        </div>
      </div>
    </div>
  {/if}
</main>
