<script lang="ts">
  import {
    Activity,
    BarChart3,
    DatabaseZap,
    GitBranch,
    LoaderCircle,
    RefreshCw,
    ShieldCheck,
    Tags,
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
  import {
    archivePolicyLabel,
    archiveScopeLabel,
    statusLabel,
  } from "../lib/view-model";

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
  const facetStatus = $derived(report?.facetInspector.status ?? offlineStatus());
  const projectionStatus = $derived(report?.projectionInspector.status ?? offlineStatus());
  const proceduralStatus = $derived(report?.proceduralEvolution.status ?? offlineStatus());
  const archiveStatus = $derived(report?.archiveRestore.status ?? offlineStatus());
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
      agent_tool_experience: "工具经验",
    };
    return labels[value] ?? value;
  }

  function plainToken(value: string): string {
    if (lang === "en") return value.replace(/_/g, " ");
    const labels: Record<string, string> = {
      memory_graph_nodes_empty: "还没有可展示的记忆关系",
      memory_facet_index_no_query_match: "当前查询没有命中 facet index",
      runtime_recall_graph_preview_not_persistent: "当前是预览结果，未写入长期记忆图",
      inspection_unavailable: "健康检查暂时不可用",
      no_governed_tool_experience: "还没有可复用工具经验",
      governed_tool_experience_available: "已有可复用工具经验",
      recall_unavailable: "找记忆暂时不可用",
    };
    return labels[value] ?? value.replace(/_/g, " ");
  }

  function localLabel(zh: string, en: string): string {
    return lang === "zh-CN" ? zh : en;
  }

  function surfaceTitle(surfaceId: string): string {
    const zh: Record<string, string> = {
      home: "总览",
      recall_inspector: "找记忆",
      facet_inspector: "Facet 索引",
      projection_inspector: "整理上下文",
      soul_health: "健康状态",
      procedural_evolution: "习惯与技能",
      replay_diff: "回放验收",
      archive_restore: "归档恢复",
    };
    const en: Record<string, string> = {
      home: "Overview",
      recall_inspector: "Find memory",
      facet_inspector: "Facet index",
      projection_inspector: "Prepare context",
      soul_health: "Health status",
      procedural_evolution: "Habits and skills",
      replay_diff: "Replay check",
      archive_restore: "Archive restore",
    };
    return (lang === "zh-CN" ? zh : en)[surfaceId] ?? plainToken(surfaceId);
  }

  function surfaceDetail(surfaceId: string): string {
    const zh: Record<string, string> = {
      home: "运行状态和关键数量",
      recall_inspector: "检查系统能不能找回相关记忆",
      facet_inspector: "只读检查 facet index 和诊断",
      projection_inspector: "检查放进对话前的记忆上下文",
      soul_health: "检查治理队列和安全动作",
      procedural_evolution: "检查可复用的习惯与技能",
      replay_diff: "检查回放结果是否退化",
      archive_restore: "检查类型化范围、隐私策略与归档闭包",
    };
    const en: Record<string, string> = {
      home: "Runtime status and key counts",
      recall_inspector: "Checks whether relevant memories can be found",
      facet_inspector: "Read-only facet index diagnostics",
      projection_inspector: "Checks the memory context before it enters a reply",
      soul_health: "Checks governance queues and safe actions",
      procedural_evolution: "Checks reusable habits and skills",
      replay_diff: "Checks whether replay results regressed",
      archive_restore: "Checks typed scope, privacy policy, and archive closure",
    };
    return (lang === "zh-CN" ? zh : en)[surfaceId] ?? plainToken(surfaceId);
  }

  function shortSha256(value: string): string {
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
      { label: localLabel("候选工具", "Tool hints"), value: numberText(recall.agentToolHints) },
      { label: localLabel("工具经验", "Tool experience"), value: plainToken(recall.toolExperienceReason) },
      { label: localLabel("首次用工具", "First-use tools"), value: recall.hostFallbackRequired ? localLabel("由宿主决定", "Host decides") : wb.yes },
    ];
  }

  function facetRows(): KVRow[] {
    if (!report) return [];
    const facet = report.facetInspector;
    return [
      { label: localLabel("Owner", "Owner"), value: facet.owner },
      { label: localLabel("只读报告", "Report only"), value: boolText(facet.reportOnly) },
      { label: localLabel("允许直接修改", "Direct mutation"), value: boolText(facet.directMutationAllowed) },
      { label: localLabel("全量扫描回退", "Full-scan fallback"), value: boolText(facet.fallbackFullScan) },
      { label: localLabel("命中来源", "Matched sources"), value: `${numberText(facet.matchedSourceCandidateCount)}/${numberText(facet.sourceCandidateCount)}` },
      { label: localLabel("审计格式", "Audit format"), value: facet.auditMarkdownFormat },
      { label: localLabel("索引版本", "Index revision"), value: facet.indexRevision ?? "none" },
    ];
  }

  function projectionRows(): KVRow[] {
    if (!report) return [];
    const projection = report.projectionInspector;
    return [
      { label: wb.systemChars, value: numberText(projection.systemMemoryChars) },
      { label: wb.sourceBudget, value: numberText(projection.sourceBudgetChars) },
      { label: wb.renderBudget, value: numberText(projection.renderBudgetChars) },
      { label: wb.privateRuntime, value: boolText(projection.runtimePrivateContextAllowed) },
      { label: wb.publicDisclosure, value: boolText(projection.foregroundDisclosureAllowed) },
      { label: wb.faithfulness, value: boolText(projection.faithfulnessPassed) },
      { label: wb.privateEcho, value: String(projection.rawPrivateViolationCount) },
      { label: localLabel("工具提示", "Tool hints"), value: numberText(projection.agentToolHints) },
      { label: localLabel("旧工具经验", "Old tool experience"), value: numberText(projection.agentToolRejections) },
    ];
  }

  function archiveRows(): KVRow[] {
    if (!report) return [];
    const archive = report.archiveRestore;
    const rows: KVRow[] = [
      { label: wb.archiveScope, value: archiveScopeLabel(archive.scope, lang) },
      {
        label: wb.archivePolicy,
        value: archivePolicyLabel(archive.privateMaterialPolicy, lang),
      },
    ];
    if (!archive.archiveRoot) return rows;
    rows.push(
      { label: wb.jsonDocs, value: numberText(archive.archiveRoot.json_doc_count) },
      { label: wb.events, value: numberText(archive.archiveRoot.event_count) },
      { label: wb.jsonBytes, value: numberText(archive.archiveRoot.json_bytes) },
      { label: wb.eventBytes, value: numberText(archive.archiveRoot.event_bytes) },
    );
    return rows;
  }

  function soulRows(): KVRow[] {
    if (!report) return [];
    const soul = report.soulHealth;
    return [
      { label: wb.currentDevice, value: profileText(soul.profile) },
      { label: wb.skills, value: numberText(soul.runtimeSkillRecords) },
      { label: wb.deferred, value: `${soul.deferredPending}/${soul.deferredTotal}` },
      { label: wb.failed, value: numberText(soul.deferredFailed) },
      { label: localLabel("工具索引", "Tool registries"), value: `${numberText(soul.agentToolRegistries)} / ${numberText(soul.agentToolRegistryTools)}` },
      { label: localLabel("工具经验", "Tool experience"), value: numberText(soul.agentToolExperiences) },
      { label: localLabel("过期工具经验", "Stale tool experience"), value: numberText(soul.agentToolStaleExperiences) },
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
      <div><span>{localLabel("Facet", "Facet")}</span><strong>{statusText(facetStatus)}</strong></div>
      <div><span>{wb.projection}</span><strong>{statusText(projectionStatus)}</strong></div>
      <div><span>{wb.archive}</span><strong>{statusText(archiveStatus)}</strong></div>
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
            <div><span>{wb.soulStable}</span><strong>{boolText(benchmark.soulKernelJudge.releaseGatePassed)}</strong></div>
            <div><span>{wb.contextStable}</span><strong>{boolText(benchmark.subjectProjectionJudge.releaseGatePassed)}</strong></div>
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
          <div><span>{wb.skillHits}</span><strong>{report.recallInspector.proceduralDeliveryReports}</strong></div>
        </div>
        <KvStack items={recallRows()} />
        {#if report.recallInspector.graphFailures.length > 0}
          <div class="chips">{#each report.recallInspector.graphFailures as failure}<span>{plainToken(failure)}</span>{/each}</div>
        {/if}
      </article>

      <article class="panel">
        <PanelHeader label={localLabel("Facet", "Facet")} title={localLabel("索引诊断", "Index diagnostics")} icon={Tags} />
        <div class="workbench-metric-grid">
          <div><span>{localLabel("Exact docs", "Exact docs")}</span><strong>{report.facetInspector.exactFacetDocCount}</strong></div>
          <div><span>{localLabel("Expanded docs", "Expanded docs")}</span><strong>{report.facetInspector.expandedFacetDocCount}</strong></div>
          <div><span>{localLabel("Render growth", "Render growth")}</span><strong>{report.facetInspector.renderGrowth}</strong></div>
        </div>
        <KvStack items={facetRows()} />
        {#if report.facetInspector.failures.length > 0}
          <div class="chips">{#each report.facetInspector.failures as failure}<span>{plainToken(failure)}</span>{/each}</div>
        {:else}
          <div class="skill-empty">{wb.emptyFailures}</div>
        {/if}
        {#if report.facetInspector.auditMarkdownPreview}
          <pre class="workbench-audit-preview">{report.facetInspector.auditMarkdownPreview}</pre>
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
        <PanelHeader label={wb.archive} title={wb.archiveTitle} icon={ShieldCheck} />
        <KvStack items={archiveRows()} />
        {#if report.archiveRestore.archiveRoot}
          <div class="workbench-fingerprint">
            <span>{wb.closureSha256}</span>
            <code>{shortSha256(report.archiveRestore.archiveRoot.closure_sha256)}</code>
          </div>
        {:else}
          <div class="skill-empty">{plainToken(report.archiveRestore.status.reason)}</div>
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
