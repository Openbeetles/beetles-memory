<script lang="ts">
  import { Activity, Bot, Check, ChevronDown, ClipboardList, Copy, ExternalLink, LoaderCircle, Play, Power, Rocket } from "lucide-svelte";
  import ConfirmActionModal from "../components/ConfirmActionModal.svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import {
    disableOllamaTransparent,
    enableOllamaTransparent,
    openOllamaApp,
    runLlmGatewaySmokeCheck,
  } from "../lib/console-api";
  import type { ConsoleCopy } from "../lib/i18n";
  import type {
    ConsoleApiCapabilities,
    ConsoleApiLlmGateway,
    ConsoleApiLlmGatewaySmokeCheck,
    ConsoleApiLlmGatewaySmokeRunReport,
    ConsoleApiOllamaTransition,
    ConsoleApiOllamaTransparentState,
    ConsoleApiOllamaTransparentStatus,
    ConsoleApiPortBindingReport,
    ConsoleApiPortOwnerKind,
    ConsoleApiTransitionStep,
    StatusKind,
  } from "../lib/types";
  import { statusLabel } from "../lib/view-model";

  let {
    t,
    llmGateway,
    consoleCapabilities,
    ollamaTransparent,
    backendConnected,
    onRefresh,
    onBackendDisconnected,
  }: {
    t: ConsoleCopy;
    llmGateway: ConsoleApiLlmGateway | null;
    consoleCapabilities: ConsoleApiCapabilities | null;
    ollamaTransparent: ConsoleApiOllamaTransparentStatus | null;
    backendConnected: boolean;
    onRefresh: () => void | Promise<void>;
    onBackendDisconnected: () => void;
  } = $props();

  const gatewayStatus = $derived((backendConnected ? (llmGateway?.status ?? "draft") : "blocked") as StatusKind);
  const protocols = $derived(llmGateway?.protocols ?? []);
  const ruleExports = $derived(llmGateway?.ruleExports ?? []);
  const smokeChecks = $derived(llmGateway?.smokeChecks ?? []);
  let copiedCommand = $state<string | null>(null);
  let copyingCommand = $state<string | null>(null);
  let runningSmokeId = $state<string | null>(null);
  let smokeReports = $state<Record<string, ConsoleApiLlmGatewaySmokeRunReport>>({});
  let transparentBusy = $state<"enable" | "disable" | "open" | null>(null);
  let transparentTransition = $state<ConsoleApiOllamaTransition | null>(null);
  let transparentError = $state("");
  let transparentEnableConfirmOpen = $state(false);
  let transparentDisableConfirmOpen = $state(false);

  const transparentFeature = $derived(consoleCapabilities?.features.ollamaTransparentApp ?? null);
  const transparentAvailable = $derived(transparentFeature?.visible === true && ollamaTransparent != null);
  const transparentState = $derived(ollamaTransparent?.state ?? "Disabled");
  const transparentStatus = $derived(transparentStateToStatus(transparentState, backendConnected));
  const transparentShowsDisable = $derived(["Active", "Degraded", "Disabling", "RollingBack"].includes(transparentState));
  const transparentDisabled = $derived(
    !transparentAvailable || !backendConnected || transparentBusy !== null || ["Enabling", "Disabling", "RollingBack"].includes(transparentState),
  );
  const transparentReportLine = $derived(visibleTransparentReportLine());
  const transparentReportStatus = $derived(visibleTransparentReportStatus());
  const transparentBlockers = $derived(transitionBlockerLines());
  const memoryFlow = $derived(memoryFlowCard());
  const appUse = $derived(appUseCard());

  $effect(() => {
    if (!transparentAvailable) {
      transparentEnableConfirmOpen = false;
      transparentDisableConfirmOpen = false;
      transparentBusy = null;
      transparentTransition = null;
      transparentError = "";
    }
  });

  type TransparentUserCard = {
    label: string;
    value: string;
    detail: string;
    status: StatusKind;
  };

  function protocolDetail(id: string, fallback: string): string {
    return t.llmGatewayPanel.protocolDetails[id as keyof typeof t.llmGatewayPanel.protocolDetails] ?? fallback;
  }

  function protocolTitle(id: string, fallback: string): string {
    return t.llmGatewayPanel.protocolTitles[id as keyof typeof t.llmGatewayPanel.protocolTitles] ?? fallback;
  }

  async function copyCommand(id: string, command: string) {
    if (copyingCommand !== null || typeof navigator === "undefined" || !navigator.clipboard) return;
    copyingCommand = id;
    try {
      await navigator.clipboard.writeText(command);
      copiedCommand = id;
      window.setTimeout(() => {
        if (copiedCommand === id) copiedCommand = null;
      }, 1400);
    } finally {
      if (copyingCommand === id) copyingCommand = null;
    }
  }

  async function runSmokeCheck(check: ConsoleApiLlmGatewaySmokeCheck) {
    if (!backendConnected || runningSmokeId !== null) return;
    runningSmokeId = check.id;
    try {
      const report = await runLlmGatewaySmokeCheck(check.id);
      smokeReports = { ...smokeReports, [check.id]: report };
    } catch (error) {
      smokeReports = {
        ...smokeReports,
        [check.id]: {
          id: check.id,
          label: check.label,
          status: "blocked",
          command: check.command,
          exitCode: null,
          stdout: "",
          stderr: error instanceof Error ? error.message : String(error),
          durationMs: 0,
          timedOut: false,
          startedAtUnixSecs: Math.floor(Date.now() / 1000),
          cwd: "",
        },
      };
    } finally {
      if (runningSmokeId === check.id) runningSmokeId = null;
    }
  }

  function smokeOutput(report: ConsoleApiLlmGatewaySmokeRunReport): string {
    return [report.stdout.trim(), report.stderr.trim()].filter(Boolean).join("\n");
  }

  function transparentStateToStatus(state: ConsoleApiOllamaTransparentState, connected: boolean): StatusKind {
    if (!connected) return "blocked";
    if (state === "Active") return "active";
    if (state === "Disabled") return "disabled";
    if (state === "Degraded" || state === "PreflightFailed") return "limited";
    return "draft";
  }

  function ownerLabel(owner: ConsoleApiPortOwnerKind | undefined): string {
    if (!owner) return "—";
    return t.llmGatewayPanel.transparent.ownerLabels[owner] ?? owner;
  }

  function portDetail(report: ConsoleApiPortBindingReport | null | undefined): string {
    if (!report) return "—";
    if (report.process) return `${report.process.command} pid=${report.process.pid}`;
    return report.detail ?? ownerLabel(report.owner);
  }

  function memoryFlowCard(): TransparentUserCard {
    if (transparentState === "Active") {
      return {
        label: t.llmGatewayPanel.transparent.memoryFlow,
        value: t.llmGatewayPanel.transparent.memoryFlowActive,
        detail: t.llmGatewayPanel.transparent.memoryFlowActiveDetail,
        status: "active",
      };
    }
    if (transparentState === "Enabling" || transparentState === "Disabling" || transparentState === "RollingBack") {
      return {
        label: t.llmGatewayPanel.transparent.memoryFlow,
        value: t.llmGatewayPanel.transparent.memoryFlowChanging,
        detail: t.llmGatewayPanel.transparent.memoryFlowChangingDetail,
        status: "draft",
      };
    }
    return {
      label: t.llmGatewayPanel.transparent.memoryFlow,
      value: t.llmGatewayPanel.transparent.memoryFlowPending,
      detail: t.llmGatewayPanel.transparent.memoryFlowPendingDetail,
      status: "disabled",
    };
  }

  function appUseCard(): TransparentUserCard {
    if (transparentState === "Active") {
      return {
        label: t.llmGatewayPanel.transparent.appUse,
        value: t.llmGatewayPanel.transparent.appUseReady,
        detail: t.llmGatewayPanel.transparent.appUseReadyDetail,
        status: "active",
      };
    }
    if (transparentState === "Disabling" || transparentState === "RollingBack") {
      return {
        label: t.llmGatewayPanel.transparent.appUse,
        value: t.llmGatewayPanel.transparent.appUseRecovering,
        detail: t.llmGatewayPanel.transparent.appUseRecoveringDetail,
        status: "draft",
      };
    }
    return {
      label: t.llmGatewayPanel.transparent.appUse,
      value: t.llmGatewayPanel.transparent.appUseOriginal,
      detail: t.llmGatewayPanel.transparent.appUseOriginalDetail,
      status: "disabled",
    };
  }

  function transitionFailureLine(step: ConsoleApiTransitionStep): string {
    return t.llmGatewayPanel.transparent.stepFailureMessages[
      step.step as keyof typeof t.llmGatewayPanel.transparent.stepFailureMessages
    ] ?? t.llmGatewayPanel.transparent.genericFailure;
  }

  function transitionBlockerLines(): string[] {
    const transition = transparentTransition ?? ollamaTransparent?.lastTransition;
    const message = transition?.failingStep?.message?.trim();
    if (!message) return [];
    return message
      .split(";")
      .map((line) => userFacingTransparentBlocker(line.trim()))
      .filter(Boolean);
  }

  function userFacingTransparentBlocker(message: string): string {
    if (!message) return "";
    const transparent = t.llmGatewayPanel.transparent;
    if (message.includes("bm-llm-gateway transparent front binary is missing")) {
      return transparent.blockerMessages.gatewayMissing;
    }
    if (message.includes("official Ollama binary is missing")) {
      return transparent.blockerMessages.ollamaMissing;
    }
    if (message.includes("official Ollama owns 11434")) {
      return transparent.blockerMessages.officialOllamaRunning;
    }
    if (message.includes("public transparent port is owned by an unknown process")) {
      return transparent.blockerMessages.publicPortUnknown;
    }
    if (message.includes("managed upstream port is owned by a non-managed process")) {
      return transparent.blockerMessages.upstreamPortBusy;
    }
    if (message.includes("managed upstream runner must not own the public Ollama App port")) {
      return transparent.blockerMessages.managedRunnerOnPublicPort;
    }
    return message;
  }

  function runtimeErrorLine(message: string): string {
    if (message.includes("managed_upstream_api") || message.includes("11435")) {
      return t.llmGatewayPanel.transparent.stepFailureMessages.ProbeManagedUpstream;
    }
    if (message.includes("public_front_api") || message.includes("11434")) {
      return t.llmGatewayPanel.transparent.stepFailureMessages.ProbePublicFront;
    }
    return t.llmGatewayPanel.transparent.genericFailure;
  }

  function lastReportLine(): string {
    const transition = transparentTransition ?? ollamaTransparent?.lastTransition;
    const step = transition?.failingStep;
    if (step) return transitionFailureLine(step);
    if (transition && transition.toState === transparentState && transition.outcome !== "Completed") {
      return t.llmGatewayPanel.transparent.transitionOutcomes[transition.outcome] ?? transition.outcome;
    }
    if (transparentState === "PreflightFailed") return t.llmGatewayPanel.transparent.preflightRejected;
    if (transparentState === "Degraded") return t.llmGatewayPanel.transparent.degradedReport;
    return "";
  }

  function visibleTransparentError(): string {
    if (!transparentError) return "";
    return transparentState === "Active" ? "" : runtimeErrorLine(transparentError);
  }

  function visibleTransparentReportLine(): string {
    const error = visibleTransparentError();
    if (error) return error;
    if (transparentTransition && transparentTransition.toState !== transparentState) return "";
    return lastReportLine();
  }

  function visibleTransparentReportStatus(): StatusKind {
    const transition = transparentTransition ?? ollamaTransparent?.lastTransition;
    if (visibleTransparentError()) return "blocked";
    if (transition?.failingStep) return "blocked";
    if (transition && transition.toState === transparentState && transition.outcome !== "Completed") return "blocked";
    if (transparentState === "PreflightFailed" || transparentState === "Degraded") return "blocked";
    return transparentStatus;
  }

  function openTransparentEnableConfirm() {
    if (!transparentAvailable || transparentDisabled || transparentState === "Active") return;
    transparentError = "";
    transparentEnableConfirmOpen = true;
  }

  function closeTransparentEnableConfirm() {
    transparentEnableConfirmOpen = false;
  }

  function openTransparentDisableConfirm() {
    if (!transparentAvailable || transparentDisabled || !transparentShowsDisable) return;
    transparentError = "";
    transparentDisableConfirmOpen = true;
  }

  function closeTransparentDisableConfirm() {
    transparentDisableConfirmOpen = false;
  }

  async function runTransparentEnable() {
    if (!transparentAvailable || transparentBusy !== null) return;
    if (!backendConnected) {
      transparentError = t.labels.backendOffline;
      return;
    }
    transparentBusy = "enable";
    transparentError = "";
    try {
      transparentTransition = await enableOllamaTransparent();
      await onRefresh();
    } catch (error) {
      transparentError = error instanceof Error ? error.message : String(error);
      onBackendDisconnected();
    } finally {
      transparentBusy = null;
    }
  }

  async function runTransparentDisable() {
    if (!transparentAvailable || transparentBusy !== null) return;
    if (!backendConnected) {
      transparentError = t.labels.backendOffline;
      return;
    }
    await runTransparentAction("disable");
  }

  async function runTransparentAction(action: "disable" | "open"): Promise<boolean> {
    if (!transparentAvailable || !backendConnected || transparentBusy !== null) return false;
    transparentBusy = action;
    transparentError = "";
    try {
      if (action === "disable") {
        transparentTransition = await disableOllamaTransparent();
      } else {
        await openOllamaApp();
      }
      await onRefresh();
      return true;
    } catch (error) {
      transparentError = error instanceof Error ? error.message : String(error);
      onBackendDisconnected();
      return false;
    } finally {
      transparentBusy = null;
    }
  }
</script>

<div class="llm-gateway-page">

  <div class="gateway-top-grid">
    <!-- ① 外部工具接入：状态 + 监听地址 + 可复制协议地址 -->
    <section class="panel gateway-summary-panel">
      <PanelHeader label={t.llmGatewayPanel.label} title={t.llmGatewayPanel.title} icon={Bot} />
      <div class="gateway-status-row">
        <span class={`badge ${gatewayStatus}`}>{statusLabel(t, gatewayStatus)}</span>
        <span>{t.llmGatewayPanel.gatewayEndpoint}</span>
        <code class="gateway-main-url">{llmGateway?.endpoint ?? "—"}</code>
      </div>

      {#if protocols.length > 0}
        <div class="gateway-protocol-list">
          {#each protocols as protocol}
            <article class="gateway-protocol-row {protocol.status}">
              <div class="gateway-protocol-body">
                <div class="gateway-protocol-head">
                  <span class={`badge ${protocol.status}`}>{statusLabel(t, protocol.status)}</span>
                  <strong>{protocolTitle(protocol.id, protocol.title)}</strong>
                  <button
                    aria-label={`${t.llmGatewayPanel.copy} ${protocolTitle(protocol.id, protocol.title)}`}
                    class="input-action-btn"
                    type="button"
                    disabled={copyingCommand !== null}
                    onclick={() => void copyCommand(`protocol:${protocol.id}`, protocol.endpoint)}
                  >
                    {#if copyingCommand === `protocol:${protocol.id}`}<LoaderCircle class="spin-icon" size={14} />{:else if copiedCommand === `protocol:${protocol.id}`}<Check size={14} />{:else}<Copy size={14} />{/if}
                  </button>
                </div>
                <code class="gateway-proto-url">{protocol.endpoint}</code>
                <small>{protocolDetail(protocol.id, protocol.detail)}</small>
              </div>
            </article>
          {/each}
        </div>
      {:else}
        <div class="skill-empty">{t.llmGatewayPanel.empty}</div>
      {/if}
    </section>

    <!-- ② 验收命令 -->
    <section class="panel gateway-smoke-panel">
      <PanelHeader label={t.llmGatewayPanel.smokeChecks} title={t.llmGatewayPanel.smokeChecks} icon={Activity} />
      {#if smokeChecks.length > 0}
        <div class="gateway-smoke-list">
          {#each smokeChecks as check}
            {@const report = smokeReports[check.id]}
            <article class="gateway-smoke-row {check.status}">
              <div class="gateway-smoke-head">
                <span class={`badge ${check.status}`}>{statusLabel(t, check.status)}</span>
                <strong>{check.label}</strong>
                <button
                  aria-label={`${t.llmGatewayPanel.run} ${check.label}`}
                  class="gateway-run-btn"
                  type="button"
                  disabled={!backendConnected || runningSmokeId !== null}
                  onclick={() => void runSmokeCheck(check)}
                >
                  {#if runningSmokeId === check.id}<LoaderCircle size={13} class="spin-icon" />{t.llmGatewayPanel.running}{:else}<Play size={13} />{t.llmGatewayPanel.run}{/if}
                </button>
                <button
                  aria-label={`${t.llmGatewayPanel.copy} ${check.label}`}
                  class="input-action-btn"
                  type="button"
                  disabled={copyingCommand !== null}
                  onclick={() => void copyCommand(`smoke:${check.id}`, check.command)}
                >
                  {#if copyingCommand === `smoke:${check.id}`}<LoaderCircle class="spin-icon" size={14} />{:else if copiedCommand === `smoke:${check.id}`}<Check size={14} />{:else}<Copy size={14} />{/if}
                </button>
              </div>
              <input
                class="input-readonly"
                aria-label={`${check.label} ${t.llmGatewayPanel.command}`}
                readonly
                value={check.command}
              />
              {#if report}
                <div class={`gateway-smoke-result ${report.status}`}>
                  <div class="gateway-smoke-result-meta">
                    <span class={`badge ${report.status}`}>{statusLabel(t, report.status)}</span>
                    <span>{t.llmGatewayPanel.exitCode}: {report.exitCode ?? "—"}</span>
                    <span>{report.durationMs}ms</span>
                    {#if report.timedOut}<span>{t.llmGatewayPanel.timedOut}</span>{/if}
                  </div>
                  {#if smokeOutput(report)}
                    <pre>{smokeOutput(report)}</pre>
                  {:else}
                    <small>{t.llmGatewayPanel.noOutput}</small>
                  {/if}
                </div>
              {/if}
            </article>
          {/each}
        </div>
      {:else}
        <div class="skill-empty">{t.llmGatewayPanel.empty}</div>
      {/if}
    </section>
  </div>

  {#if transparentAvailable}
    <section class="panel gateway-transparent-panel">
      <PanelHeader
        label={t.llmGatewayPanel.transparent.label}
        title={t.llmGatewayPanel.transparent.title}
        icon={Rocket}
      />

      <div class="gateway-transparent-grid user-facing">
        <div class={`gateway-transparent-kv ${memoryFlow.status}`}>
          <span>{memoryFlow.label}</span>
          <strong>{memoryFlow.value}</strong>
          <small>{memoryFlow.detail}</small>
        </div>
        <div class={`gateway-transparent-kv ${appUse.status}`}>
          <span>{appUse.label}</span>
          <strong>{appUse.value}</strong>
          <small>{appUse.detail}</small>
        </div>
      </div>

      <details class="gateway-transparent-diagnostics">
        <summary>
          <span>{t.llmGatewayPanel.transparent.diagnostics}</span>
          <ChevronDown class="diagnostic-chevron" size={14} />
        </summary>
        <div class="gateway-transparent-grid diagnostic-grid">
          <div class="gateway-transparent-kv">
            <span>{t.llmGatewayPanel.transparent.publicPort}</span>
            <strong>{ollamaTransparent?.publicPort.bind ?? "127.0.0.1:11434"}</strong>
            <small>{ownerLabel(ollamaTransparent?.publicPort.owner)} · {portDetail(ollamaTransparent?.publicPort)}</small>
          </div>
          <div class="gateway-transparent-kv">
            <span>{t.llmGatewayPanel.transparent.upstreamPort}</span>
            <strong>{ollamaTransparent?.upstreamPort.bind ?? "127.0.0.1:11435"}</strong>
            <small>{ownerLabel(ollamaTransparent?.upstreamPort.owner)} · {portDetail(ollamaTransparent?.upstreamPort)}</small>
          </div>
          <div class="gateway-transparent-kv">
            <span>{t.llmGatewayPanel.transparent.managedRunner}</span>
            <strong>{ollamaTransparent?.managedRunner.installed ? t.llmGatewayPanel.transparent.installed : t.llmGatewayPanel.transparent.notInstalled}</strong>
            <code title={ollamaTransparent?.managedRunner.managedPath ?? ""}>{ollamaTransparent?.managedRunner.managedPath ?? "—"}</code>
          </div>
          <div class="gateway-transparent-kv">
            <span>{t.llmGatewayPanel.transparent.appBundle}</span>
            <strong>{ollamaTransparent?.app.openAppAfterEnable ? t.llmGatewayPanel.transparent.openAfterEnable : t.llmGatewayPanel.transparent.openManual}</strong>
            <code title={ollamaTransparent?.app.bundlePath ?? ""}>{ollamaTransparent?.app.bundlePath ?? "—"}</code>
          </div>
        </div>
      </details>

      <div class="gateway-transparent-actions">
        {#if transparentShowsDisable}
          <button
            class="primary-button danger-primary"
            type="button"
            disabled={transparentDisabled}
            onclick={openTransparentDisableConfirm}
          >
            {#if transparentBusy === "disable"}<LoaderCircle class="spin-icon" size={14} />{:else}<Power size={14} />{/if}
            {t.llmGatewayPanel.transparent.disable}
          </button>
        {:else}
          <button
            class="primary-button success-primary"
            type="button"
            disabled={transparentDisabled}
            onclick={openTransparentEnableConfirm}
          >
            {#if transparentBusy === "enable"}<LoaderCircle class="spin-icon" size={14} />{:else}<Power size={14} />{/if}
            {t.llmGatewayPanel.transparent.enable}
          </button>
        {/if}
        <button
          class="ghost-button"
          type="button"
          disabled={transparentDisabled}
          onclick={() => void runTransparentAction("open")}
        >
          {#if transparentBusy === "open"}<LoaderCircle class="spin-icon" size={14} />{:else}<ExternalLink size={14} />{/if}
          {t.llmGatewayPanel.transparent.openApp}
        </button>
      </div>

      {#if transparentReportLine}
        <div class={`gateway-transparent-report ${transparentReportStatus}`}>
          <span class={`badge ${transparentReportStatus}`}>
            {statusLabel(t, transparentReportStatus)}
          </span>
          <p>{transparentReportLine}</p>
          {#if transparentBlockers.length > 0}
            <ul class="gateway-transparent-blockers">
              {#each transparentBlockers as blocker}
                <li>{blocker}</li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
    </section>
  {/if}

  <!-- ④ 规则导出：2 列卡片网格，每张卡片充分展示命令 -->
  {#if ruleExports.length > 0}
    <section class="panel gateway-rules-panel">
      <PanelHeader label={t.llmGatewayPanel.ruleExports} title={t.llmGatewayPanel.ruleExports} icon={ClipboardList} />
      <div class="gateway-rule-grid">
        {#each ruleExports as rule}
          <div class="gateway-rule-card">
            <strong>{rule.label}</strong>
            <div class="gateway-command-input-row">
              <input
                class="input-readonly"
                aria-label={`${rule.label} ${t.llmGatewayPanel.command}`}
                readonly
                value={rule.command}
              />
              <button
                aria-label={`${t.llmGatewayPanel.copy} ${rule.label}`}
                class="input-action-btn"
                type="button"
                disabled={copyingCommand !== null}
                onclick={() => void copyCommand(`rule:${rule.target}`, rule.command)}
              >
                {#if copyingCommand === `rule:${rule.target}`}<LoaderCircle class="spin-icon" size={14} />{:else if copiedCommand === `rule:${rule.target}`}<Check size={14} />{:else}<Copy size={14} />{/if}
              </button>
            </div>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if transparentAvailable && transparentEnableConfirmOpen}
    <ConfirmActionModal
      title={t.llmGatewayPanel.transparent.confirmTitle}
      description={t.llmGatewayPanel.transparent.notice}
      subjectLabel={t.llmGatewayPanel.transparent.confirmSubjectLabel}
      subject={t.llmGatewayPanel.transparent.confirmSubject}
      meta={t.llmGatewayPanel.transparent.confirmMeta}
      confirmLabel={t.llmGatewayPanel.transparent.enable}
      cancelLabel={t.actions.cancel}
      closeLabel={t.addDevice.closeLabel}
      danger={false}
      success={true}
      onClose={closeTransparentEnableConfirm}
      onConfirm={() => void runTransparentEnable()}
    />
  {/if}

  {#if transparentAvailable && transparentDisableConfirmOpen}
    <ConfirmActionModal
      title={t.llmGatewayPanel.transparent.disableConfirmTitle}
      description={t.llmGatewayPanel.transparent.disableNotice}
      subjectLabel={t.llmGatewayPanel.transparent.disableConfirmSubjectLabel}
      subject={t.llmGatewayPanel.transparent.disableConfirmSubject}
      meta={t.llmGatewayPanel.transparent.disableConfirmMeta}
      confirmLabel={t.llmGatewayPanel.transparent.disable}
      cancelLabel={t.actions.cancel}
      closeLabel={t.addDevice.closeLabel}
      onClose={closeTransparentDisableConfirm}
      onConfirm={() => void runTransparentDisable()}
    />
  {/if}
</div>
