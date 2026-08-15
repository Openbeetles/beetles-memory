import {
  Activity,
  AlertTriangle,
  BarChart3,
  CheckCircle2,
  Circle,
  Cpu,
  Database,
  MemoryStick,
  Smartphone,
} from "lucide-svelte";
import type { ConsoleCopy } from "./i18n";
import type {
  ConsoleApiDevice,
  ConsoleApiMemoryArchiveScope,
  ConsoleApiMemorySpacePrivateMaterialPolicy,
  ConsoleApiMetric,
  ConsoleApiOverview,
  ConsoleApiSession,
  ConsoleApiSkillSummary,
  ConsoleApiSystemInfo,
  ConsoleApiTransport,
  Device,
  IconComponent,
  KVRow,
  Lang,
  OverviewCard,
  Page,
  SkillStatusFilter,
  StaticDeviceId,
  StatusKind,
  TimelineEvent,
  Transport,
} from "./types";

const STATUS_ICONS: Record<StatusKind, IconComponent> = {
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
  superseded: Circle,
  retired: AlertTriangle,
};

export type NavGroupId = "memoryCore" | "ingress" | "system";
export type NavGroup = { id: NavGroupId; label: string; pages: Page[] };

const NAV_GROUP_PAGE_IDS: Record<NavGroupId, Page["id"][]> = {
  memoryCore: ["overview", "workbench", "skills"],
  ingress: ["llm-gateway", "transports", "devices"],
  system: ["model-config", "account"],
};

export function navGroupsWithPages(t: ConsoleCopy, pages: Page[]): NavGroup[] {
  return (Object.keys(NAV_GROUP_PAGE_IDS) as NavGroupId[]).map((id) => ({
    id,
    label: t.navGroups[id],
    pages: NAV_GROUP_PAGE_IDS[id]
      .map((pageId) => pages.find((page) => page.id === pageId))
      .filter((page): page is Page => page !== undefined),
  }));
}

export const mapId = <T extends { id: string }>(list: T[], id: string, fn: (t: T) => T) =>
  list.map((t) => (t.id === id ? fn(t) : t));

export const mapDev = <T extends { deviceId: string }>(list: T[], id: string, fn: (t: T) => T) =>
  list.map((t) => (t.deviceId === id ? fn(t) : t));

export const statusLabel = (t: ConsoleCopy, status: StatusKind) => t.statusLabels[status] ?? status;
export const statusIcon = (status: StatusKind) => STATUS_ICONS[status] ?? Circle;

export function archiveScopeLabel(
  scope: ConsoleApiMemoryArchiveScope,
  currentLang: Lang,
): string {
  if (scope.kind === "subject") {
    const kind = currentLang === "zh-CN" ? "主体记忆" : "Subject";
    return `${kind}: ${scope.memory_space_id} / ${scope.mounted_subject_id}`;
  }
  const kind = currentLang === "zh-CN" ? "共享程序记忆" : "Shared program";
  return `${kind}: ${scope.memory_space_id}`;
}

export function archivePolicyLabel(
  policy: ConsoleApiMemorySpacePrivateMaterialPolicy,
  currentLang: Lang,
): string {
  if (policy === "exclude_private") {
    return currentLang === "zh-CN" ? "不包含私密内容" : "Private material excluded";
  }
  return currentLang === "zh-CN" ? "包含私密内容" : "Private material included";
}

export const fromApiTransport = (transport: ConsoleApiTransport): Transport => ({
  id: transport.id,
  enabled: transport.enabled,
  status: transport.status,
  endpoint: transport.endpoint,
  editable: transport.editable,
});

export const fromApiDevice = (device: ConsoleApiDevice): Device => ({
  deviceId: device.deviceId,
  label: device.label,
  appKey: device.appKeyFingerprint,
  status: device.status,
});

export function pagesWithCounts(t: ConsoleCopy, skillCount: number, transportsLength: number, devicesLength: number): Page[] {
  return t.pages.map((page) => {
    if (page.id === "skills") return { ...page, count: String(skillCount) };
    if (page.id === "transports") return { ...page, count: String(transportsLength) };
    if (page.id === "devices") return { ...page, count: String(devicesLength) };
    return page;
  });
}

export function sessionAccountLabel(account: string, currentLang: Lang): string {
  if (account === "operator") return currentLang === "zh-CN" ? "本地账户" : "Local Account";
  return account;
}

export function sessionStateLabel(state: string, currentLang: Lang): string {
  if (state === "paired") return currentLang === "zh-CN" ? "已配对" : "Paired";
  return state;
}

export function profileLabel(profile: string, currentLang: Lang): string {
  const zh: Record<string, string> = {
    "profile-esp-standalone-memory": "ESP 独立记忆设备",
    "profile-esp-embedded-sdk": "ESP 嵌入式集成",
    "target-linux-device+role-standalone-memory": "Linux 设备独立记忆",
    "target-desktop-macos+role-standalone-memory": "macOS 桌面独立记忆",
    "target-desktop-macos+role-embedded-sdk": "macOS 应用内集成",
    "target-desktop-windows+role-embedded-sdk": "Windows 应用内集成",
    "target-server-linux+role-memory-gateway": "Linux 服务器记忆网关",
    "target-server-linux+role-dev-full": "本机开发完整版",
  };
  const en: Record<string, string> = {
    "profile-esp-standalone-memory": "ESP standalone memory",
    "profile-esp-embedded-sdk": "ESP embedded SDK",
    "target-linux-device+role-standalone-memory": "Linux device standalone",
    "target-desktop-macos+role-standalone-memory": "macOS desktop standalone memory",
    "target-desktop-macos+role-embedded-sdk": "macOS embedded SDK",
    "target-desktop-windows+role-embedded-sdk": "Windows embedded SDK",
    "target-server-linux+role-memory-gateway": "Linux server memory gateway",
    "target-server-linux+role-dev-full": "Local development full runtime",
  };
  return (currentLang === "zh-CN" ? zh : en)[profile] ?? profile;
}

export function storeLabel(store: string, currentLang: Lang): string {
  const zh: Record<string, string> = {
    "in-memory": "内存存储",
    embedded: "嵌入式存储",
    file: "文件存储",
    sqlite: "SQLite 存储",
  };
  const en: Record<string, string> = {
    "in-memory": "in-memory backend",
    embedded: "embedded backend",
    file: "file backend",
    sqlite: "SQLite backend",
  };
  return (currentLang === "zh-CN" ? zh : en)[store] ?? store;
}

export function displaySystemInfo(api: ConsoleApiSystemInfo | undefined, currentLang: Lang): ConsoleApiSystemInfo {
  if (api) return api;
  return {
    name: currentLang === "zh-CN" ? "系统名称" : "System Name",
    cpu: "CPU",
    memory: currentLang === "zh-CN" ? "内存" : "Memory",
    timeUnixSecs: Math.floor(Date.now() / 1000),
  };
}

export function systemInfoDesc(info: ConsoleApiSystemInfo, currentLang: Lang): string {
  const time = formatSystemTime(info.timeUnixSecs, currentLang);
  return `${info.cpu} \\ ${info.memory} \\ ${time}`;
}

export function formatSystemTime(timeUnixSecs: number, currentLang: Lang): string {
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

export function createOverviewCards(
  t: ConsoleCopy,
  overviewData: ConsoleApiOverview | null,
  systemInfo: ConsoleApiSystemInfo,
  currentLang: Lang,
): OverviewCard[] {
  return [
    {
      title: t.labels.systemInfo,
      value: systemInfo.name,
      desc: systemInfoDesc(systemInfo, currentLang),
      icon: Cpu,
      tone: "ready",
      compact: true,
      progress: null,
    },
    metricCard("storage", t.overview.storage, overviewData?.storage, Database, "ready", 0, currentLang),
    metricCard("writes", t.overview.writes, overviewData?.writesToday, BarChart3, "ready", null, currentLang),
    metricCard("recall", t.overview.recall, overviewData?.recall, Activity, "ready", 0, currentLang),
    metricCard("projection", t.overview.projection, overviewData?.projection, MemoryStick, "limited", null, currentLang),
    metricCard("devices", t.overview.devices, overviewData?.devices, Smartphone, "limited", 0, currentLang),
  ];
}

function metricCard(
  kind: "storage" | "writes" | "recall" | "projection" | "devices",
  fallback: { title: string; value: string; desc: string },
  metric: ConsoleApiMetric | undefined,
  icon: IconComponent,
  tone: string,
  progressFallback: number | null,
  currentLang: Lang,
): OverviewCard {
  return {
    title: fallback.title,
    value: metric ? localizedMetricValue(kind, metric.value, fallback.value, currentLang) : fallback.value,
    desc: metric ? localizedMetricDesc(kind, metric.desc, fallback.desc, currentLang) : fallback.desc,
    icon,
    tone,
    progress: metric?.progress ?? progressFallback,
  };
}

function localizedMetricValue(
  kind: "storage" | "writes" | "recall" | "projection" | "devices",
  value: string,
  fallback: string,
  currentLang: Lang,
): string {
  if (kind !== "projection") return value || fallback;
  const match = /^(\d+)\s+(?:chars|characters)$/.exec(value.trim());
  if (!match) return value || fallback;
  const amount = Number(match[1]);
  if (!Number.isFinite(amount)) return value || fallback;
  return currentLang === "zh-CN"
    ? `${amount.toLocaleString("zh-CN")} 字`
    : `${amount.toLocaleString("en")} characters`;
}

function localizedMetricDesc(
  kind: "storage" | "writes" | "recall" | "projection" | "devices",
  desc: string,
  fallback: string,
  currentLang: Lang,
): string {
  if (currentLang === "en") {
    if (kind === "projection") {
      return (desc || fallback).replace("projection requests served / render budget", "conversations received memory context / current limit").replace(" chars", " characters");
    }
    return desc || fallback;
  }
  if (kind === "storage") return "记忆占用 / 本机总空间";
  if (kind === "writes") return "运行时已接受的记忆写入次数";
  if (kind === "recall") {
    const text = desc
      .replace(" recall requests / ", " 次检索请求 / ")
      .replace(" with hits", " 次命中");
    return text || "相关记忆检索命中情况";
  }
  if (kind === "projection") return localizedMemoryContextDesc(desc, fallback);
  if (kind === "devices") return "已授权 / 设备总数";
  return fallback;
}

function localizedMemoryContextDesc(desc: string, fallback: string): string {
  const text = desc || fallback;
  const match = /^(\d+)\s+(?:projection requests served|conversations received memory context)\s+\/\s+(?:render budget|max context|current limit)\s+(\d+)\s+(?:chars|characters)$/.exec(text.trim());
  if (!match) return "已为对话补充可用记忆";
  const requests = Number(match[1]);
  const limit = Number(match[2]);
  if (!Number.isFinite(requests) || !Number.isFinite(limit)) return "已为对话补充可用记忆";
  return `${requests.toLocaleString("zh-CN")} 次对话已补充记忆 / 单次上限 ${limit.toLocaleString("zh-CN")} 字`;
}

export function localizedEvents(events: TimelineEvent[] | undefined, fallback: TimelineEvent[], currentLang: Lang): TimelineEvent[] {
  if (!events || events.length === 0) return fallback;
  if (currentLang === "en") return events;
  return events.map((event) => ({ ...event, text: localizeEventText(event.text) }));
}

function localizeEventText(text: string): string {
  if (text.startsWith("Transport ")) return text.replace("Transport ", "通信方式 ").replace(" updated", " 已更新");
  if (text.startsWith("Device ") && text.endsWith(" key rotated")) {
    return text.replace("Device ", "设备 ").replace(" key rotated", " 已更换访问密钥");
  }
  if (text.startsWith("Device ") && text.endsWith(" added")) return text.replace("Device ", "设备 ").replace(" added", " 已添加");
  if (text.startsWith("Device ") && text.endsWith(" updated")) return text.replace("Device ", "设备 ").replace(" updated", " 已更新");
  if (text.startsWith("Skill ")) {
    return text
      .replace("Skill ", "经验记忆 ")
      .replace(" imported", " 已导入")
      .replace(" updated", " 已更新")
      .replace(" disabled", " 已停用")
      .replace(" enabled", " 已启用")
      .replace(" retired", " 已退役");
  }
  if (text.startsWith("Memory write accepted")) return text.replace("Memory write accepted, changed", "记忆写入已接受，变更");
  if (text.startsWith("Recall served for")) return text.replace("Recall served for", "记忆检索：").replace(" with ", "，命中 ").replace(" hits", " 条");
  if (text.startsWith("Projection served")) return text.replace("Projection served,", "已补充对话上下文，").replace(" chars", " 字");
  if (text.startsWith("Memory context added")) return text.replace("Memory context added,", "已补充对话上下文，").replace(" characters", " 字");
  if (text.endsWith("communication entries enabled")) return text.replace("communication entries enabled", "个通信入口已启用");
  if (text === "Console runtime opened") return "管理台运行时已打开";
  return text;
}

export function localizedKernelRows(rows: KVRow[] | undefined, fallback: KVRow[], currentLang: Lang): KVRow[] {
  const source = rows && rows.length > 0 ? rows : fallback;
  const visibleRows = source.filter((row) => row.label.toLowerCase() !== "console shell");
  if (currentLang === "en") return visibleRows;
  return visibleRows.map((row) => ({
    label: kernelLabel(row.label),
    value: kernelValue(row.label, row.value, currentLang),
  }));
}

export function localizedMemoryContextRows(rows: KVRow[] | undefined, currentLang: Lang): KVRow[] {
  const source = rows ?? [];
  if (currentLang === "en") {
    return source.map((row) => ({
      label: memoryContextEnglishLabel(row.label),
      value: row.value,
    }));
  }
  return source.map((row) => ({
    label: memoryContextChineseLabel(row.label),
    value: row.value,
  }));
}

function memoryContextChineseLabel(label: string): string {
  const labels: Record<string, string> = {
    Store: "存储位置",
    Owner: "记忆归属",
    Agent: "入口身份",
    Channel: "来源通道",
    Chat: "会话范围",
  };
  return labels[label] ?? label;
}

function memoryContextEnglishLabel(label: string): string {
  const labels: Record<string, string> = {
    Store: "Storage",
    Owner: "Owner",
    Agent: "Entry Agent",
    Channel: "Source Channel",
    Chat: "Chat Scope",
  };
  return labels[label] ?? label;
}

function kernelLabel(label: string): string {
  const labels: Record<string, string> = {
    Profile: "运行档位",
    "Store backend": "存储方式",
  };
  return labels[label] ?? label;
}

function kernelValue(label: string, value: string, currentLang: Lang): string {
  if (label === "Profile") return profileLabel(value, currentLang);
  if (label === "Store backend") return storeLabel(value, currentLang);
  return value;
}

export function accountRows(t: ConsoleCopy, sessionData: ConsoleApiSession | null, currentLang: Lang): KVRow[] {
  if (!sessionData) return t.account.fields;
  return [
    { label: t.account.fields[0].label, value: sessionAccountLabel(sessionData.account, currentLang) },
    { label: t.account.fields[1].label, value: sessionData.owner },
    { label: t.account.fields[2].label, value: sessionData.memoryScope },
    { label: t.account.fields[3].label, value: sessionStateLabel(sessionData.sessionState, currentLang) },
  ];
}

export function systemInfoSpecRows(systemInfo: ConsoleApiSystemInfo, currentLang: Lang) {
  const parts = formatSystemTime(systemInfo.timeUnixSecs, currentLang).split(" ");
  return {
    cpu: systemInfo.cpu,
    memory: systemInfo.memory,
    date: parts[0] ?? "-",
    time: parts[1] ?? "-",
  };
}

export function transportStatRows(t: ConsoleCopy, enabledTransportCount: number, transportsLength: number, activeDeviceCount: number, devicesLength: number): KVRow[] {
  return [
    { label: t.transportStats.enabled, value: `${enabledTransportCount} / ${transportsLength}` },
    { label: t.transportStats.devices, value: `${activeDeviceCount} / ${devicesLength}` },
  ];
}

export function filterSkills(
  source: ConsoleApiSkillSummary[],
  search: string,
  statusFilter: SkillStatusFilter,
): ConsoleApiSkillSummary[] {
  const needle = search.trim().toLowerCase();
  return source.filter((skill) => {
    if (statusFilter === "active" && (!skill.enabled || skill.status !== "active")) return false;
    if (statusFilter === "disabled" && skill.enabled) return false;
    if (statusFilter === "retired" && skill.status !== "retired") return false;
    if (!needle) return true;
    return [skill.ownerId, skill.title, skill.topic, skill.status]
      .some((value) => value.toLowerCase().includes(needle));
  });
}

export function skillQuality(skill: ConsoleApiSkillSummary): string {
  return skill.qualityScore === null ? "-" : `${skill.qualityScore}`;
}

export function parseCitations(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

export function citationsText(citations: string[]): string {
  return citations.join("\n");
}

export function deviceLabel(t: ConsoleCopy, device: Device): string {
  return device.label ?? (isStaticDevice(device.deviceId, t) ? t.devices[device.deviceId].label : device.deviceId);
}

function isStaticDevice(id: string, t: ConsoleCopy): id is StaticDeviceId {
  return id in (t.devices as Record<string, unknown>);
}
