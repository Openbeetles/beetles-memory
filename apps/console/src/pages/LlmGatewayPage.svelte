<script lang="ts">
  import { Activity, Bot, Check, ClipboardList, Copy, LoaderCircle, Play } from "lucide-svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import { runLlmGatewaySmokeCheck } from "../lib/console-api";
  import type { ConsoleCopy } from "../lib/i18n";
  import type {
    ConsoleApiLlmGateway,
    ConsoleApiLlmGatewaySmokeCheck,
    ConsoleApiLlmGatewaySmokeRunReport,
    StatusKind,
  } from "../lib/types";
  import { statusLabel } from "../lib/view-model";

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
  const protocols = $derived(llmGateway?.protocols ?? []);
  const ruleExports = $derived(llmGateway?.ruleExports ?? []);
  const smokeChecks = $derived(llmGateway?.smokeChecks ?? []);
  let copiedCommand = $state<string | null>(null);
  let runningSmokeId = $state<string | null>(null);
  let smokeReports = $state<Record<string, ConsoleApiLlmGatewaySmokeRunReport>>({});

  function protocolDetail(id: string, fallback: string): string {
    return t.llmGatewayPanel.protocolDetails[id as keyof typeof t.llmGatewayPanel.protocolDetails] ?? fallback;
  }

  function protocolTitle(id: string, fallback: string): string {
    return t.llmGatewayPanel.protocolTitles[id as keyof typeof t.llmGatewayPanel.protocolTitles] ?? fallback;
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
                    onclick={() => void copyCommand(`protocol:${protocol.id}`, protocol.endpoint)}
                  >
                    {#if copiedCommand === `protocol:${protocol.id}`}<Check size={14} />{:else}<Copy size={14} />{/if}
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
