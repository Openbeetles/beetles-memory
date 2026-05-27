<script lang="ts">
  import {
    Activity,
    BarChart3,
    DatabaseZap,
    GitBranch,
    LoaderCircle,
    RefreshCw,
    ShieldCheck,
    Workflow,
  } from "lucide-svelte";
  import KvStack from "../components/KvStack.svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type {
    ConsoleApiBenchmarkBaseline,
    ConsoleApiWorkbenchReport,
    ConsoleApiWorkbenchStatus,
    KVRow,
    Lang,
  } from "../lib/types";
  import { statusLabel } from "../lib/view-model";

  let {
    t,
    lang,
    report,
    backendConnected,
    loading,
    onRefresh,
  }: {
    t: ConsoleCopy;
    lang: Lang;
    report: ConsoleApiWorkbenchReport | null;
    backendConnected: boolean;
    loading: boolean;
    onRefresh: () => void | Promise<void>;
  } = $props();

  const wb = $derived(t.workbenchPanel);
  const benchmark = $derived(report?.benchmarkWall.report ?? null);
  const benchmarkStatus = $derived(report?.benchmarkWall.status ?? offlineStatus());
  const recallStatus = $derived(report?.recallInspector.status ?? offlineStatus());
  const projectionStatus = $derived(report?.projectionInspector.status ?? offlineStatus());
  const proceduralStatus = $derived(report?.proceduralEvolution.status ?? offlineStatus());
  const vaultStatus = $derived(report?.vaultMigration.status ?? offlineStatus());
  const soulStatus = $derived(report?.soulHealth.status ?? offlineStatus());
  const allPrivateRawClosed = $derived(report?.apiMap.surfaces.every((surface) => !surface.privateRawAllowed) ?? false);
  const missingApiCount = $derived(report?.apiMap.missingReportApis.length ?? 0);

  function offlineStatus(): ConsoleApiWorkbenchStatus {
    return {
      available: false,
      status: "blocked",
      reason: t.labels.backendOffline,
    };
  }

  function boolText(value: boolean): string {
    return value ? wb.yes : wb.no;
  }

  function statusText(status: ConsoleApiWorkbenchStatus): string {
    return status.available ? statusLabel(t, status.status) : wb.unavailable;
  }

  function bps(value: number): string {
    return `${(value / 100).toFixed(1)}%`;
  }

  function numberText(value: number): string {
    return value.toLocaleString(lang === "zh-CN" ? "zh-CN" : "en");
  }

  function classLabel(value: string): string {
    if (lang === "en") return value.replace(/_/g, " ");
    const labels: Record<string, string> = {
      recall_multisession: "跨会话找回",
      temporal_update: "新旧事实",
      subject_projection: "上下文准备",
      soul_regression: "个性稳定",
      procedural_reuse: "经验复用",
      privacy_refusal: "隐私保护",
    };
    return labels[value] ?? value;
  }

  function plainToken(value: string): string {
    if (lang === "en") return value.replace(/_/g, " ");
    const labels: Record<string, string> = {
      memory_graph_nodes_empty: "还没有可展示的记忆关系",
      runtime_recall_graph_preview_not_persistent: "当前是预览结果，未写入长期记忆图",
      inspection_unavailable: "健康检查暂时不可用",
    };
    return labels[value] ?? value.replace(/_/g, " ");
  }

  function surfaceTitle(surfaceId: string): string {
    const zh: Record<string, string> = {
      home: "总览",
      recall_inspector: "找记忆",
      projection_inspector: "整理上下文",
      soul_health: "健康状态",
      procedural_evolution: "习惯与技能",
      replay_diff: "回放验收",
      vault_migration: "搬家预检",
    };
    const en: Record<string, string> = {
      home: "Overview",
      recall_inspector: "Find memory",
      projection_inspector: "Prepare context",
      soul_health: "Health status",
      procedural_evolution: "Habits and skills",
      replay_diff: "Replay check",
      vault_migration: "Move precheck",
    };
    return (lang === "zh-CN" ? zh : en)[surfaceId] ?? plainToken(surfaceId);
  }

  function surfaceDetail(surfaceId: string): string {
    const zh: Record<string, string> = {
      home: "运行状态和关键数量",
      recall_inspector: "检查系统能不能找回相关记忆",
      projection_inspector: "检查放进对话前的记忆上下文",
      soul_health: "检查治理队列和安全动作",
      procedural_evolution: "检查可复用的习惯与技能",
      replay_diff: "检查回放结果是否退化",
      vault_migration: "检查导出、脱敏和迁移风险",
    };
    const en: Record<string, string> = {
      home: "Runtime status and key counts",
      recall_inspector: "Checks whether relevant memories can be found",
      projection_inspector: "Checks the memory context before it enters a reply",
      soul_health: "Checks governance queues and safe actions",
      procedural_evolution: "Checks reusable habits and skills",
      replay_diff: "Checks whether replay results regressed",
      vault_migration: "Checks export, redaction, and migration risk",
    };
    return (lang === "zh-CN" ? zh : en)[surfaceId] ?? plainToken(surfaceId);
  }

  function shortFingerprint(value: string): string {
    if (!value) return "—";
    return value.length > 18 ? `${value.slice(0, 8)}...${value.slice(-6)}` : value;
  }

  function profileText(value: string): string {
    if (lang === "zh-CN" && value === "local-desktop") return "本机桌面";
    return plainToken(value);
  }

  function baselineRows(baseline: ConsoleApiBenchmarkBaseline): KVRow[] {
    return [
      { label: wb.baselineAccuracy, value: bps(baseline.accuracyBps) },
      { label: wb.baselineEvidence, value: bps(baseline.evidencePrecisionBps) },
      { label: wb.baselineFaithfulness, value: bps(baseline.projectionFaithfulnessBps) },
      { label: wb.baselinePrivacy, value: String(baseline.privacyViolationCount) },
      { label: wb.baselineProcedural, value: bps(baseline.proceduralReuseSuccessBps) },
      { label: wb.baselineLatency, value: `${baseline.latencyMs}ms` },
    ];
  }

  function recallRows(): KVRow[] {
    if (!report) return [];
    const recall = report.recallInspector;
    return [
      { label: wb.checkScenario, value: wb.recallScenario },
      { label: wb.selected, value: numberText(recall.workingSelectedSurfaces) },
      { label: wb.skills, value: numberText(recall.runtimeSkillSelected) },
      { label: wb.evidence, value: numberText(recall.evidenceBacklinks) },
      { label: wb.highConfidence, value: recall.highConfidenceProjectionAllowed ? wb.yes : wb.previewOnly },
    ];
  }

  function projectionRows(): KVRow[] {
    if (!report) return [];
    const projection = report.projectionInspector;
    return [
      { label: wb.systemChars, value: numberText(projection.systemMemoryChars) },
      { label: wb.sourceBudget, value: numberText(projection.sourceBudgetChars) },
      { label: wb.renderBudget, value: numberText(projection.renderBudgetChars) },
      { label: wb.privateGate, value: boolText(projection.privateGateAllowed) },
      { label: wb.faithfulness, value: boolText(projection.faithfulnessPassed) },
      { label: wb.privateEcho, value: String(projection.privateEchoCount) },
    ];
  }

  function vaultRows(): KVRow[] {
    if (!report) return [];
    const vault = report.vaultMigration;
    return [
      { label: wb.dataItems, value: numberText(vault.jsonDocs) },
      { label: wb.fileItems, value: numberText(vault.blobs) },
      { label: wb.eventItems, value: numberText(vault.events) },
      { label: wb.redactions, value: numberText(vault.privacyRedactions) },
      { label: wb.lossRisk, value: boolText(vault.lossRisk) },
      { label: wb.preflight, value: boolText(vault.preflightPassed) },
    ];
  }

  function soulRows(): KVRow[] {
    if (!report) return [];
    const soul = report.soulHealth;
    return [
      { label: wb.currentDevice, value: profileText(soul.profile) },
      { label: wb.skills, value: numberText(soul.runtimeSkillRecords) },
      { label: wb.deferred, value: `${soul.deferredPending}/${soul.deferredTotal}` },
      { label: wb.failed, value: numberText(soul.deferredFailed) },
    ];
  }
</script>

<div class="workbench-page">
  <section class="panel workbench-top">
    <div class="panel-title">
      <div>
        <p class="panel-label">{wb.label}</p>
        <h3>{wb.title}</h3>
      </div>
      <div class="panel-title-actions">
        <button class="ghost-button" type="button" disabled={!backendConnected || loading} onclick={() => void onRefresh()}>
          {#if loading}<LoaderCircle class="spin-icon" size={13} />{:else}<RefreshCw size={13} />{/if}
          {wb.refresh}
        </button>
      </div>
    </div>

    <div class="skill-stats workbench-stat-strip">
      <div><span>{wb.benchmark}</span><strong>{statusText(benchmarkStatus)}</strong></div>
      <div><span>{wb.recall}</span><strong>{statusText(recallStatus)}</strong></div>
      <div><span>{wb.projection}</span><strong>{statusText(projectionStatus)}</strong></div>
      <div><span>{wb.vault}</span><strong>{statusText(vaultStatus)}</strong></div>
      <div><span>{wb.privateRawClosed}</span><strong>{boolText(allPrivateRawClosed)}</strong></div>
    </div>
  </section>

  {#if !report}
    <div class="skill-empty detail-empty">{loading ? wb.loading : t.labels.backendOffline}</div>
  {:else}
    <div class="workbench-grid">
      <article class="panel workbench-api-panel">
        <PanelHeader label={wb.apiMap} title={`${numberText(report.apiMap.surfaces.length)} ${wb.surfaceUnit}`} icon={Workflow} />
        <div class="workbench-surface-list">
          {#each report.apiMap.surfaces as surface}
            <div class="workbench-surface-row">
              <div>
                <strong>{surfaceTitle(surface.surfaceId)}</strong>
                <small>{surfaceDetail(surface.surfaceId)}</small>
              </div>
              <span class={`badge ${surface.privateRawAllowed ? "blocked" : "ready"}`}>
                {surface.privateRawAllowed ? wb.needsAttention : wb.secure}
              </span>
            </div>
          {/each}
        </div>
        <div class="workbench-mini-kv">
          <span>{wb.missingApis}</span>
          <strong>{missingApiCount}</strong>
        </div>
      </article>

      <article class="panel">
        <PanelHeader label={wb.benchmark} title={benchmark ? wb.benchmarkTitle : benchmarkStatus.reason} icon={BarChart3} />
        {#if benchmark}
          <div class="workbench-metric-grid">
            <div><span>{wb.fixtures}</span><strong>{benchmark.passedFixtures}/{benchmark.totalFixtures}</strong></div>
            <div><span>{wb.passed}</span><strong>{boolText(benchmark.passed)}</strong></div>
            <div><span>{wb.failures}</span><strong>{benchmark.failures.length}</strong></div>
          </div>
          <KvStack items={baselineRows(benchmark.baseline)} />
          <div class="workbench-coverage-list">
            {#each benchmark.classCoverage as row}
              <div class="workbench-coverage-row">
                <strong>{classLabel(row.class)}</strong>
                <span>{wb.compactSet} {row.compactFixtures}</span>
                <span>{wb.fullSet} {row.fullFixtures}</span>
              </div>
            {/each}
          </div>
          {#if benchmark.failures.length > 0}
            <div class="workbench-failure-list">
              {#each benchmark.failures as failure}
                <div class="gateway-smoke-result blocked">
                  <small>{classLabel(failure.class)} · {plainToken(failure.stage)}</small>
                  <pre>{plainToken(failure.reason)}</pre>
                </div>
              {/each}
            </div>
          {:else}
            <div class="skill-empty">{wb.emptyFailures}</div>
          {/if}
        {:else}
          <div class="panel-action-error">{benchmarkStatus.reason}</div>
        {/if}
      </article>
    </div>

    <div class="workbench-grid lower-grid">
      <article class="panel">
        <PanelHeader label={wb.recall} title={wb.recallTitle} icon={GitBranch} />
        <div class="workbench-metric-grid">
          <div><span>{wb.memoryPoints}</span><strong>{report.recallInspector.graphNodes}</strong></div>
          <div><span>{wb.memoryLinks}</span><strong>{report.recallInspector.graphEdges}</strong></div>
          <div><span>{wb.skillHits}</span><strong>{report.recallInspector.proceduralHits}</strong></div>
        </div>
        <KvStack items={recallRows()} />
        {#if report.recallInspector.graphFailures.length > 0}
          <div class="chips">{#each report.recallInspector.graphFailures as failure}<span>{plainToken(failure)}</span>{/each}</div>
        {/if}
      </article>

      <article class="panel">
        <PanelHeader label={wb.projection} title={wb.projectionTitle} icon={Activity} />
        <KvStack items={projectionRows()} />
        <div class="workbench-metric-grid">
          <div><span>{wb.citations}</span><strong>{report.projectionInspector.evidenceRefs}</strong></div>
          <div><span>{wb.lengthControl}</span><strong>{report.projectionInspector.budgetDecisions}</strong></div>
          <div><span>{wb.filtered}</span><strong>{report.projectionInspector.droppedCandidates}</strong></div>
        </div>
        {#if report.projectionInspector.unsupportedClaims.length > 0}
          <div class="chips">{#each report.projectionInspector.unsupportedClaims as claim}<span>{plainToken(claim)}</span>{/each}</div>
        {:else}
          <div class="skill-empty">{wb.emptyFailures}</div>
        {/if}
      </article>

      <article class="panel">
        <PanelHeader label={wb.procedural} title={wb.proceduralTitle} icon={DatabaseZap} />
        <div class="workbench-metric-grid">
          <div><span>{wb.skills}</span><strong>{report.proceduralEvolution.totalSkills}</strong></div>
          <div><span>{wb.activeSkills}</span><strong>{report.proceduralEvolution.activeSkills}</strong></div>
          <div><span>{wb.runtimeLearned}</span><strong>{report.proceduralEvolution.runtimeLearned}</strong></div>
        </div>
        {#if report.proceduralEvolution.topSkills.length > 0}
          <div class="workbench-surface-list">
            {#each report.proceduralEvolution.topSkills as skill}
              <div class="workbench-surface-row">
                <div>
                  <strong>{skill.title}</strong>
                  <small>{skill.topic || wb.noTopic}</small>
                </div>
                <span class={`badge ${skill.status}`}>{statusLabel(t, skill.status)}</span>
              </div>
            {/each}
          </div>
        {:else}
          <div class="skill-empty">{wb.noSkills}</div>
        {/if}
      </article>

      <article class="panel">
        <PanelHeader label={wb.vault} title={wb.vaultTitle} icon={ShieldCheck} />
        <KvStack items={vaultRows()} />
        <div class="workbench-fingerprint">
          <span>{wb.fingerprints}</span>
          <code>{shortFingerprint(report.vaultMigration.snapshotFingerprint)}</code>
          <code>{shortFingerprint(report.vaultMigration.eventFingerprint)}</code>
        </div>
        {#if report.vaultMigration.preflightFailures.length > 0}
          <div class="chips">{#each report.vaultMigration.preflightFailures as failure}<span>{plainToken(failure)}</span>{/each}</div>
        {/if}
      </article>

      <article class="panel workbench-soul-panel">
        <PanelHeader label={wb.soul} title={wb.soulTitle} icon={ShieldCheck} />
        <KvStack items={soulRows()} />
        <section class="workbench-detail-section">
          <h4>{wb.hygiene}</h4>
          <p>{plainToken(report.soulHealth.hygieneSummary)}</p>
        </section>
        <section class="workbench-detail-section">
          <h4>{wb.safeActions}</h4>
          {#if report.soulHealth.safeActions.length > 0}
            <div class="chips">{#each report.soulHealth.safeActions as action}<span>{plainToken(action)}</span>{/each}</div>
          {:else}
            <p>{wb.none}</p>
          {/if}
        </section>
      </article>
    </div>
  {/if}
</div>
