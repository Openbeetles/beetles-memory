import type { CapRow, Lang, Page, StatusKind, Theme } from "./types";

export const STORAGE_KEYS = {
  theme: "beetle-memory-console-theme",
  lang: "beetle-memory-console-lang",
} as const;

export const copy = {
  "zh-CN": {
    headTitle: "Beetle Memory",
    brand: { name: "Beetle Memory", sub: "MEMORY ENGINE" },
    labels: {
      console: "配置台",
      status: "状态",
      backendOffline: "后端未连接",
      systemInfo: "系统信息",
      addressMode: "地址 / 模式",
    },
    actions: {
      apply: "重启",
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
      { id: "skills", label: "Skills", eyebrow: "Skills", title: "Skills" },
      { id: "llm-gateway", label: "LLM 网关", eyebrow: "模型协议", title: "LLM 网关" },
      { id: "transports", label: "通信方式", count: "5", eyebrow: "通信入口", title: "通信方式配置" },
      { id: "devices", label: "开放设备", count: "4", eyebrow: "访问控制", title: "开放设备列表" },
      { id: "account", label: "系统设置", eyebrow: "系统设置", title: "设置" },
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
    overview: {
      storage: { title: "存储占用", value: "0 B / 0 B", desc: "当前系统占用 / 实际系统可用大小" },
      writes: { title: "今日写入", value: "0", desc: "等待后端运行数据" },
      recall: { title: "召回命中", value: "0.0%", desc: "等待后端运行数据" },
      projection: { title: "投影预算", value: "0", desc: "等待后端运行数据" },
      devices: { title: "开放设备", value: "0/0", desc: "等待后端运行数据" },
      observation: "运行观测",
      communicationAccess: "通信与访问",
      memoryContextLabel: "记忆作用域",
      memoryContextTitle: "当前记忆上下文",
      memoryContextEmpty: "后端未连接，等待记忆上下文",
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
    systemSettings: {
      panel: "系统偏好",
      title: "系统设置",
      langLabel: "界面语言",
      langOptions: { zh: "中文", en: "English" },
    },
    transportsPanel: { label: "通信入口", title: "通信方式与必要配置" },
    llmGatewayPanel: {
      label: "LLM 网关",
      title: "外部工具接入",
      endpoints: "外部工具接入地址",
      protocols: "填到 IDE / Agent / 本地模型工具里",
      ruleExports: "规则导出",
      smokeChecks: "验收命令",
      gatewayEndpoint: "主监听地址",
      openaiBaseUrl: "OpenAI Base URL",
      ollamaBaseUrl: "Ollama Base URL",
      providerCapabilitiesUrl: "Provider Capabilities",
      mcpStreamableHttpUrl: "MCP Streamable HTTP",
      command: "命令",
      target: "目标",
      copy: "复制",
      copied: "已复制",
      run: "运行",
      running: "运行中",
      exitCode: "退出码",
      timedOut: "超时",
      noOutput: "无输出",
      protocolDetails: {
        "openai-compatible": "用于 Continue、Cline、Aider、Open WebUI、Zed 等 OpenAI Base URL 配置",
        "ollama-native": "用于支持 Ollama API 的客户端；不是官方 Ollama 11434 地址",
        "mcp-streamable-http": "用于支持 MCP 的 IDE / Agent 工具调用",
      },
      protocolTitles: {
        "openai-compatible": "OpenAI 兼容地址",
        "ollama-native": "Ollama API 地址",
        "mcp-streamable-http": "MCP 工具地址",
      },
      empty: "后端未连接，等待 LLM Gateway 运行数据",
    },
    skillsPanel: {
      label: "Skills",
      title: "Skills",
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
      empty: "暂无 Skills",
      emptyDetail: "选择一个 Skill 查看详情",
      name: "名称",
      titleLabel: "标题",
      topic: "主题",
      summary: "摘要",
      procedure: "过程",
      citationsInput: "引用，一行一个",
      file: "读取本地文本",
      deleteTitle: "删除 Skill",
      deleteDesc: "删除后会从存储移除，不保留配置台墓碑。",
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
      "llm-gateway": {
        name: "LLM 网关",
        detail: "OpenAI/Ollama 模型协议入口",
        fields: ["OpenAI /v1", "Ollama /api", "端口：8787"],
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
    kernel: {
      label: "运行状态",
      title: "内核摘要",
      rows: [
        { label: "存储后端", value: "嵌入式 / 文件后端可用" },
        { label: "密钥策略", value: "只显示存在状态" },
        { label: "生命周期", value: "运行时已打开" },
      ],
    },
    statusbar: { brand: "Beetle Memory", skills: "Skills", transports: "传输", devices: "设备" },
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
    deviceConfirm: {
      deviceLabel: "设备",
      rotateTitle: "轮换 app_key",
      rotateDesc: "轮换后旧 app_key 会立即失效，设备需要使用新 app_key 重新接入。",
      rotateConfirm: "确认轮换",
      disableTitle: "停用设备",
      disableDesc: "停用后该设备将不能继续访问当前记忆服务，之后可重新启用。",
      disableConfirm: "确认停用",
    },
  },
  en: {
    headTitle: "Beetle Memory",
    brand: { name: "Beetle Memory", sub: "MEMORY ENGINE" },
    labels: {
      console: "Console",
      status: "Status",
      backendOffline: "Backend offline",
      systemInfo: "System Info",
      addressMode: "Address / Mode",
    },
    actions: {
      apply: "Restart",
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
      { id: "skills", label: "Skills", eyebrow: "Skills", title: "Skills" },
      { id: "llm-gateway", label: "LLM Gateway", eyebrow: "Model Protocols", title: "LLM Gateway" },
      { id: "transports", label: "Communication", count: "5", eyebrow: "Entry Points", title: "Communication Setup" },
      { id: "devices", label: "Devices", count: "4", eyebrow: "Access Control", title: "Allowed Devices" },
      { id: "account", label: "Settings", eyebrow: "System Settings", title: "Settings" },
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
    overview: {
      storage: { title: "Storage Usage", value: "0 B / 0 B", desc: "Current usage / available system storage" },
      writes: { title: "Writes Today", value: "0", desc: "Waiting for backend runtime data" },
      recall: { title: "Recall Hits", value: "0.0%", desc: "Waiting for backend runtime data" },
      projection: { title: "Projection Budget", value: "0", desc: "Waiting for backend runtime data" },
      devices: { title: "Allowed Devices", value: "0/0", desc: "Waiting for backend runtime data" },
      observation: "Runtime Observation",
      communicationAccess: "Communication & Access",
      memoryContextLabel: "Memory Scope",
      memoryContextTitle: "Current Memory Context",
      memoryContextEmpty: "Backend offline, waiting for memory context",
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
    systemSettings: {
      panel: "Preferences",
      title: "System Settings",
      langLabel: "Interface Language",
      langOptions: { zh: "中文", en: "English" },
    },
    transportsPanel: { label: "Entry Points", title: "Communication Methods & Required Settings" },
    llmGatewayPanel: {
      label: "LLM Gateway",
      title: "External Tool Access",
      endpoints: "External Tool Connection URLs",
      protocols: "Use these in IDE, agent, and local model tool settings",
      ruleExports: "Rule Exports",
      smokeChecks: "Smoke Checks",
      gatewayEndpoint: "Gateway Listen Address",
      openaiBaseUrl: "OpenAI Base URL",
      ollamaBaseUrl: "Ollama Base URL",
      providerCapabilitiesUrl: "Provider Capabilities",
      mcpStreamableHttpUrl: "MCP Streamable HTTP",
      command: "Command",
      target: "Target",
      copy: "Copy",
      copied: "Copied",
      run: "Run",
      running: "Running",
      exitCode: "Exit",
      timedOut: "Timed out",
      noOutput: "No output",
      protocolDetails: {
        "openai-compatible": "Use in Continue, Cline, Aider, Open WebUI, Zed, and other OpenAI Base URL settings",
        "ollama-native": "Use in clients that support the Ollama API; this is not the official Ollama 11434 address",
        "mcp-streamable-http": "Use in IDEs and agents that support MCP tool calls",
      },
      protocolTitles: {
        "openai-compatible": "OpenAI-compatible URL",
        "ollama-native": "Ollama API URL",
        "mcp-streamable-http": "MCP Tools URL",
      },
      empty: "Backend offline, waiting for LLM Gateway runtime data",
    },
    skillsPanel: {
      label: "Skills",
      title: "Skills",
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
      empty: "No Skills yet",
      emptyDetail: "Select a skill to inspect details",
      name: "Name",
      titleLabel: "Title",
      topic: "Topic",
      summary: "Summary",
      procedure: "Procedure",
      citationsInput: "Citations, one per line",
      file: "Read local text",
      deleteTitle: "Delete Skill",
      deleteDesc: "Deletion removes the record from storage without a console tombstone.",
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
      "llm-gateway": {
        name: "LLM Gateway",
        detail: "OpenAI/Ollama model protocol entry",
        fields: ["OpenAI /v1", "Ollama /api", "Port: 8787"],
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
    kernel: {
      label: "Runtime Status",
      title: "Kernel Summary",
      rows: [
        { label: "Storage Backend", value: "Embedded / file backend available" },
        { label: "Key Policy", value: "Only presence state is shown" },
        { label: "Lifecycle", value: "Runtime opened" },
      ],
    },
    statusbar: { brand: "Beetle Memory", skills: "Skills", transports: "Transports", devices: "Devices" },
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
    deviceConfirm: {
      deviceLabel: "Device",
      rotateTitle: "Rotate app_key",
      rotateDesc: "The old app_key is revoked immediately. The device must reconnect with the new app_key.",
      rotateConfirm: "Rotate",
      disableTitle: "Disable Device",
      disableDesc: "The device will no longer be allowed to access this memory service. It can be enabled again later.",
      disableConfirm: "Disable",
    },
  },
};

export type ConsoleCopy = typeof copy["zh-CN"];

export const isTheme = (value: string | null): value is Theme => value === "light" || value === "dark";
export const isLang = (value: string | null): value is Lang => value === "zh-CN" || value === "en";

export function readTheme(): Theme {
  if (typeof localStorage === "undefined") return "dark";
  try {
    const value = localStorage.getItem(STORAGE_KEYS.theme);
    return isTheme(value) ? value : "dark";
  } catch {
    return "dark";
  }
}

export function readLang(): Lang {
  if (typeof localStorage === "undefined") return "zh-CN";
  try {
    const value = localStorage.getItem(STORAGE_KEYS.lang);
    return isLang(value) ? value : "zh-CN";
  } catch {
    return "zh-CN";
  }
}

export function writeStorage(key: string, value: string) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(key, value);
  } catch {
    // 本地偏好写入失败时不阻断配置台渲染。
  }
}

export type CapabilityRows = CapRow[];
