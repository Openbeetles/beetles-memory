export type PageId = "overview" | "skills" | "transports" | "devices" | "account";
export type IconComponent = typeof import("lucide-svelte")["Activity"];
export type StatusKind =
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
export type Lang = "zh-CN" | "en";
export type Theme = "light" | "dark";
export type TransportId = "http" | "wss" | "mcp" | "a2a";
export type SkillOrigin = "user_provided" | "runtime_learned";
export type SkillKind = "runtime_skill" | "manual_document";
export type SkillModal = "create" | "import" | "edit" | "delete" | null;
export type SkillStatusFilter = "all" | "active" | "disabled" | "retired";
export type SkillOriginFilter = "all" | SkillOrigin;

export type Page = { id: PageId; label: string; count?: string; eyebrow: string; title: string };
export type Transport = {
  id: TransportId;
  enabled: boolean;
  status: StatusKind;
  endpoint: string;
  editable?: boolean;
};
export type Device = { deviceId: string; appKey: string; status: StatusKind; label?: string };
export type KVRow = { label: string; value: string };
export type CapRow = { title: string; status: StatusKind; desc: string };
export type TimelineEvent = { time: string; text: string; tone: string };
export type OverviewCard = {
  title: string;
  value: string;
  desc: string;
  icon: IconComponent;
  tone: string;
  compact?: boolean;
  progress: number | null;
};

export type ConsoleApiTransport = {
  id: TransportId;
  enabled: boolean;
  status: StatusKind;
  endpoint: string;
  editable: boolean;
};
export type ConsoleApiDevice = {
  deviceId: string;
  label: string;
  appKeyFingerprint: string;
  status: StatusKind;
};
export type ConsoleApiSession = {
  account: string;
  owner: string;
  memoryScope: string;
  sessionState: string;
};
export type ConsoleApiRuntimeShape = {
  profile: string;
  name: string;
  store: string;
  shell: string;
};
export type ConsoleApiSystemInfo = {
  name: string;
  cpu: string;
  memory: string;
  timeUnixSecs: number;
};
export type ConsoleApiMetric = {
  value: string;
  desc: string;
  progress: number | null;
};
export type ConsoleApiSkillSummary = {
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
export type ConsoleApiSkillList = {
  total: number;
  active: number;
  disabled: number;
  runtimeLearned: number;
  userProvided: number;
  skills: ConsoleApiSkillSummary[];
};
export type ConsoleApiSkillDetail = {
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
export type ConsoleApiSkillMutation = {
  accepted: boolean;
  changed: boolean;
  name: string;
  operation: string;
  reason: string;
};
export type SkillForm = {
  title: string;
  topic: string;
  summary: string;
  procedure: string;
  citations: string;
};
export type ConsoleApiOverview = {
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

export type StaticDeviceId = "bm-linux-core-01" | "bm-desktop-mac-studio" | "bm-esp-memory-7f2" | "legacy-lab-device";
