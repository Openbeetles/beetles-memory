<script lang="ts">
  import { Activity, Bot, Check, ClipboardList, Copy, LoaderCircle, Network, Play } from "lucide-svelte";
  import KvStack from "../components/KvStack.svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import { runLlmGatewaySmokeCheck } from "../lib/console-api";
  import type { ConsoleCopy } from "../lib/i18n";
  import type {
    ConsoleApiLlmGateway,
    ConsoleApiLlmGatewaySmokeCheck,
    ConsoleApiLlmGatewaySmokeRunReport,
    KVRow,
    StatusKind,
  } from "../lib/types";
  import { statusIcon, statusLabel } from "../lib/view-model";

  let {
    t,
    llmGateway,
    backendConnected,
  }: {
    t: ConsoleCopy;
    llmGateway: ConsoleApiLlmGateway | null;
    backendConnected: boolean;
  } = $props();

  const gatewayStatus = $derived((backendConnected ? (llmGateway?.status ?? "draft") : "blocked") as StatusKind);
  const endpointRows = $derived<KVRow[]>(
    llmGateway
      ? [
          { label: t.llmGatewayPanel.openaiBaseUrl, value: llmGateway.openaiBaseUrl },
          { label: t.llmGatewayPanel.ollamaBaseUrl, value: llmGateway.ollamaBaseUrl },
          { label: t.llmGatewayPanel.providerCapabilitiesUrl, value: llmGateway.providerCapabilitiesUrl },
          { label: t.llmGatewayPanel.mcpStreamableHttpUrl, value: llmGateway.mcpStreamableHttpUrl },
        ]
      : [],
  );
  const sharedRuntime = $derived(llmGateway?.sharedRuntime ?? []);
  const protocols = $derived(llmGateway?.protocols ?? []);
  const ruleExports = $derived(llmGateway?.ruleExports ?? []);
  const smokeChecks = $derived(llmGateway?.smokeChecks ?? []);
  let copiedCommand = $state<string | null>(null);
  let runningSmokeId = $state<string | null>(null);
  let smokeReports = $state<Record<string, ConsoleApiLlmGatewaySmokeRunReport>>({});

  function protocolDetail(id: string, fallback: string): string {
    return t.llmGatewayPanel.protocolDetails[id as keyof typeof t.llmGatewayPanel.protocolDetails] ?? fallback;
  }

  async function copyCommand(id: string, command: string) {
    if (typeof navigator === "undefined" || !navigator.clipboard) return;
    await navigator.clipboard.writeText(command);
    copiedCommand = id;
    window.setTimeout(() => {
      if (copiedCommand === id) copiedCommand = null;
    }, 1400);
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
</script>

<div class="llm-gateway-page">

  <!-- ① 摘要：状态 + 网关 URL + 所有子端点（全宽）-->
  <section class="panel gateway-summary-panel">
    <PanelHeader label={t.llmGatewayPanel.label} title={t.llmGatewayPanel.title} icon={Bot} />
    <div class="gateway-status-row">
      <span class={`badge ${gatewayStatus}`}>{statusLabel(t, gatewayStatus)}</span>
      <code class="gateway-main-url">{llmGateway?.endpoint ?? "—"}</code>
    </div>
    {#if endpointRows.length > 0}
      <div class="gateway-endpoint-list">
        {#each endpointRows as row}
          <div class="gateway-endpoint-row">
            <span>{row.label}</span>
            <code>{row.value}</code>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <!-- ② 协议端点：全宽，每行充分展示 URL -->
  <section class="panel gateway-protocols-panel">
    <PanelHeader label={t.llmGatewayPanel.protocols} title={t.llmGatewayPanel.endpoints} icon={Network} />
    {#if protocols.length > 0}
      <div class="gateway-protocol-list">
        {#each protocols as protocol}
          {@const Icon = statusIcon(protocol.status)}
          <article class="gateway-protocol-row {protocol.status}">
            <Icon size={14} />
            <div class="gateway-protocol-body">
              <div class="gateway-protocol-head">
                <strong>{protocol.title}</strong>
                <span class={`badge ${protocol.status}`}>{statusLabel(t, protocol.status)}</span>
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

  <!-- ③ 共享运行时 + Smoke 检测：左右横排 -->
  <div class="gateway-checks-grid">
    <section class="panel">
      <PanelHeader label={t.llmGatewayPanel.sharedRuntime} title={t.llmGatewayPanel.sharedRuntime} icon={Activity} />
      {#if sharedRuntime.length > 0}
        <KvStack items={sharedRuntime} />
      {:else}
        <div class="skill-empty">{t.llmGatewayPanel.empty}</div>
      {/if}
    </section>

    <section class="panel">
      <PanelHeader label={t.llmGatewayPanel.smokeChecks} title={t.llmGatewayPanel.smokeChecks} icon={Activity} />
      {#if smokeChecks.length > 0}
        <div class="gateway-smoke-list">
          {#each smokeChecks as check}
            {@const Icon = statusIcon(check.status)}
            {@const report = smokeReports[check.id]}
            <article class="gateway-smoke-row {check.status}">
              <div class="gateway-smoke-head">
                <Icon size={13} />
                <strong>{check.label}</strong>
                <span class={`badge ${check.status}`}>{statusLabel(t, check.status)}</span>
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
                  onclick={() => void copyCommand(`smoke:${check.id}`, check.command)}
                >
                  {#if copiedCommand === `smoke:${check.id}`}<Check size={14} />{:else}<Copy size={14} />{/if}
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
                onclick={() => void copyCommand(`rule:${rule.target}`, rule.command)}
              >
                {#if copiedCommand === `rule:${rule.target}`}<Check size={14} />{:else}<Copy size={14} />{/if}
              </button>
            </div>
          </div>
        {/each}
      </div>
    </section>
  {/if}

</div>
