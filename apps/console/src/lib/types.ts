export type PageId = "overview" | "workbench" | "skills" | "llm-gateway" | "transports" | "devices" | "account";
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
export type TransportId = "http" | "wss" | "mcp" | "a2a" | "llm-gateway";
export type DeviceConfirmAction = "rotate_key" | "disable";
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
export type ConsoleApiLlmGatewayProtocol = {
  id: string;
  title: string;
  status: StatusKind;
  endpoint: string;
  detail: string;
};
export type ConsoleApiLlmGatewayRuleExport = {
  target: string;
  label: string;
  command: string;
};
export type ConsoleApiLlmGatewaySmokeCheck = {
  id: string;
  label: string;
  status: StatusKind;
  command: string;
};
export type ConsoleApiLlmGatewaySmokeRunReport = {
  id: string;
  label: string;
  status: StatusKind;
  command: string;
  exitCode: number | null;
  stdout: string;
  stderr: string;
  durationMs: number;
  timedOut: boolean;
  startedAtUnixSecs: number;
  cwd: string;
};
export type ConsoleApiLlmGateway = {
  enabled: boolean;
  status: StatusKind;
  endpoint: string;
  openaiBaseUrl: string;
  ollamaBaseUrl: string;
  providerCapabilitiesUrl: string;
  mcpStreamableHttpUrl: string;
  protocols: ConsoleApiLlmGatewayProtocol[];
  ruleExports: ConsoleApiLlmGatewayRuleExport[];
  smokeChecks: ConsoleApiLlmGatewaySmokeCheck[];
};
export type ConsoleApiCapabilityFeatureId = "ollamaTransparentApp" | string;
export type ConsoleApiCapabilityFeature = {
  id: ConsoleApiCapabilityFeatureId;
  visible: boolean;
  available: boolean;
  owner: string;
  reason: string | null;
  routes: Record<string, string>;
};
export type ConsoleApiCapabilities = {
  schema: string;
  features: Record<ConsoleApiCapabilityFeatureId, ConsoleApiCapabilityFeature | undefined>;
};
export type ConsoleApiOllamaTransparentState =
  | "Disabled"
  | "PreflightFailed"
  | "Enabling"
  | "Active"
  | "Degraded"
  | "Disabling"
  | "RollingBack";
export type ConsoleApiPortOwnerKind =
  | "NoListener"
  | "OfficialOllama"
  | "BeetleMemoryTransparentFront"
  | "ManagedOllamaRunner"
  | "Unknown";
export type ConsoleApiObservedProcess = {
  pid: number;
  command: string;
  executable: string;
};
export type ConsoleApiPortBindingReport = {
  bind: string;
  owner: ConsoleApiPortOwnerKind;
  process: ConsoleApiObservedProcess | null;
  detail: string | null;
};
export type ConsoleApiManagedRunnerReport = {
  sourcePath: string;
  managedPath: string;
  sourceExists: boolean;
  managedExists: boolean;
  installed: boolean;
  sourceDigest: string | null;
  managedDigest: string | null;
  copyDigest: string | null;
  message: string | null;
};
export type ConsoleApiGatewayFrontReport = {
  expectedOwner: ConsoleApiPortOwnerKind;
  bind: string;
  active: boolean;
  message: string | null;
};
export type ConsoleApiOllamaAppReport = {
  bundlePath: string;
  allowStopOfficialOllama: boolean;
  openAppAfterEnable: boolean;
  restoreOfficialAfterDisable: boolean;
  lastAction: unknown | null;
};
export type ConsoleApiTransitionStep = {
  step: string;
  ok: boolean;
  message: string | null;
};
export type ConsoleApiTransitionOutcome = "Completed" | "Rejected" | "Failed" | "RolledBack";
export type ConsoleApiRollbackReport = {
  attempted: boolean;
  completed: boolean;
  steps: ConsoleApiTransitionStep[];
};
export type ConsoleApiOllamaTransition = {
  fromState: ConsoleApiOllamaTransparentState;
  toState: ConsoleApiOllamaTransparentState;
  outcome: ConsoleApiTransitionOutcome;
  steps: ConsoleApiTransitionStep[];
  failingStep: ConsoleApiTransitionStep | null;
  rollback: ConsoleApiRollbackReport | null;
};
export type ConsoleApiPreflightBlocker = {
  code: string;
  message: string;
};
export type ConsoleApiOllamaPreflight = {
  accepted: boolean;
  resultingState: ConsoleApiOllamaTransparentState;
  publicPort: ConsoleApiPortBindingReport;
  upstreamPort: ConsoleApiPortBindingReport;
  managedRunner: ConsoleApiManagedRunnerReport;
  stopPlan: unknown | null;
  blockers: ConsoleApiPreflightBlocker[];
};
export type ConsoleApiOllamaTransparentStatus = {
  state: ConsoleApiOllamaTransparentState;
  publicPort: ConsoleApiPortBindingReport;
  upstreamPort: ConsoleApiPortBindingReport;
  app: ConsoleApiOllamaAppReport;
  managedRunner: ConsoleApiManagedRunnerReport;
  gatewayFront: ConsoleApiGatewayFrontReport;
  lastTransition: ConsoleApiOllamaTransition | null;
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

export type ConsoleApiWorkbenchSurface = {
  surfaceId: string;
  reportApi: string;
  privateRawAllowed: boolean;
};
export type ConsoleApiWorkbenchApiMap = {
  surfaces: ConsoleApiWorkbenchSurface[];
  missingReportApis: string[];
};
export type ConsoleApiWorkbenchStatus = {
  available: boolean;
  status: StatusKind;
  reason: string;
};
export type ConsoleApiBenchmarkClassCoverage = {
  class: string;
  compactFixtures: number;
  fullFixtures: number;
};
export type ConsoleApiBenchmarkFailure = {
  fixtureId: string;
  class: string;
  mode: string;
  profile: string;
  stage: string;
  reason: string;
};
export type ConsoleApiBenchmarkBaseline = {
  accuracyBps: number;
  evidencePrecisionBps: number;
  projectionFaithfulnessBps: number;
  privacyViolationCount: number;
  staleMemoryFalsePositiveCount: number;
  proceduralReuseSuccessBps: number;
  soulRegressionCount: number;
  latencyMs: number;
  tokenBudget: number;
  memoryBytes: number;
};
export type ConsoleApiMemoryBenchmarkReport = {
  suite: string;
  totalFixtures: number;
  passedFixtures: number;
  baseline: ConsoleApiBenchmarkBaseline;
  classCoverage: ConsoleApiBenchmarkClassCoverage[];
  missingClasses: Array<{ class: string; mode: string }>;
  failures: ConsoleApiBenchmarkFailure[];
  passed: boolean;
};
export type ConsoleApiWorkbenchBenchmarkWall = {
  status: ConsoleApiWorkbenchStatus;
  fixtureRoot: string;
  report: ConsoleApiMemoryBenchmarkReport | null;
};
export type ConsoleApiWorkbenchRecallInspector = {
  status: ConsoleApiWorkbenchStatus;
  query: string;
  proceduralHits: number;
  runtimeSkillSelected: number;
  workingSelectedSurfaces: number;
  graphNodes: number;
  graphEdges: number;
  evidenceBacklinks: number;
  highConfidenceProjectionAllowed: boolean;
  graphFailures: string[];
  graphSelectedIds: string[];
  staleFalsePositiveCount: number;
};
export type ConsoleApiWorkbenchProjectionInspector = {
  status: ConsoleApiWorkbenchStatus;
  query: string;
  systemMemoryChars: number;
  sourceBudgetChars: number;
  renderBudgetChars: number;
  injected: boolean;
  truncated: boolean;
  privateGateAllowed: boolean;
  privateGateReason: string;
  evidenceRefs: number;
  budgetDecisions: number;
  privacyDecisions: number;
  droppedCandidates: number;
  faithfulnessPassed: boolean;
  unsupportedClaims: string[];
  privateEchoGuardPassed: boolean;
  privateEchoCount: number;
};
export type ConsoleApiWorkbenchSkillRef = {
  name: string;
  title: string;
  topic: string;
  status: StatusKind;
  qualityScore: number | null;
};
export type ConsoleApiWorkbenchProceduralEvolution = {
  status: ConsoleApiWorkbenchStatus;
  totalSkills: number;
  activeSkills: number;
  runtimeLearned: number;
  userProvided: number;
  disabled: number;
  topSkills: ConsoleApiWorkbenchSkillRef[];
};
export type ConsoleApiWorkbenchVaultMigration = {
  status: ConsoleApiWorkbenchStatus;
  sourceMemorySpaceId: string;
  targetMemorySpaceId: string;
  jsonDocs: number;
  blobs: number;
  events: number;
  privacyRedactions: number;
  lossRisk: boolean;
  preflightPassed: boolean;
  preflightFailures: string[];
  snapshotFingerprint: string;
  eventFingerprint: string;
};
export type ConsoleApiWorkbenchSoulHealth = {
  status: ConsoleApiWorkbenchStatus;
  profile: string;
  hygieneSummary: string;
  runtimeSkillRecords: number;
  deferredTotal: number;
  deferredPending: number;
  deferredFailed: number;
  safeActions: string[];
};
export type ConsoleApiWorkbenchReport = {
  apiMap: ConsoleApiWorkbenchApiMap;
  benchmarkWall: ConsoleApiWorkbenchBenchmarkWall;
  recallInspector: ConsoleApiWorkbenchRecallInspector;
  projectionInspector: ConsoleApiWorkbenchProjectionInspector;
  proceduralEvolution: ConsoleApiWorkbenchProceduralEvolution;
  vaultMigration: ConsoleApiWorkbenchVaultMigration;
  soulHealth: ConsoleApiWorkbenchSoulHealth;
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
  runtimeBudget: ConsoleApiRuntimeBudget;
  storage: ConsoleApiMetric;
  writesToday: ConsoleApiMetric;
  recall: ConsoleApiMetric;
  projection: ConsoleApiMetric;
  devices: ConsoleApiMetric;
  recentEvents: TimelineEvent[];
  capabilities: CapRow[];
  kernel: KVRow[];
  session: KVRow[];
  memoryContext: KVRow[];
};

export type ConsoleApiRuntimeBudget = {
  reportId: string;
  profile: string;
  resourceSource: string;
  stale: boolean;
  limitedBy: string[];
  unavailableReasons: string[];
  storeSnapshotMaxBytes: number;
  httpBodyMaxBytes: number;
  wssFrameMaxBytes: number;
  projectionSourceMaxChars: number;
  projectionRenderMaxChars: number;
  maintenanceUserMaxChars: number;
  maintenanceReplyMaxChars: number;
};

export type StaticDeviceId = "bm-linux-core-01" | "bm-desktop-mac-studio" | "bm-esp-memory-7f2" | "legacy-lab-device";
