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
  import InspectorCard from "../components/InspectorCard.svelte";
  import KvStack from "../components/KvStack.svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import StatusBadge from "../components/StatusBadge.svelte";
  import VerdictStrip from "../components/VerdictStrip.svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import { statusTone } from "../lib/status";
  import type {
    ConsoleApiBenchmarkBaseline,
    ConsoleApiWorkbenchReport,
    ConsoleApiWorkbenchStatus,
    KVRow,
    Lang,
    StatusKind,
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
  const inspectorStatuses = $derived(
    [benchmarkStatus, recallStatus, facetStatus, projectionStatus, proceduralStatus, archiveStatus, soulStatus],
  );
  const verdictCounts = $derived((() => {
    let ready = 0;
    let limited = 0;
    let blocked = 0;
    let unavailable = 0;
    for (const status of inspectorStatuses) {
      if (!status.available) {
        unavailable += 1;
        continue;
      }
      const tone = statusTone(status.status as StatusKind);
      if (tone === "blocked") blocked += 1;
      else if (tone === "limited" || tone === "locked") limited += 1;
      else ready += 1;
    }
    return { ready, limited, blocked, unavailable };
  })());

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
    return (wb.classLabels as Record<string, string>)[value] ?? value.replace(/_/g, " ");
  }

  function plainToken(value: string): string {
    return (wb.tokenLabels as Record<string, string>)[value] ?? value.replace(/_/g, " ");
  }

  function surfaceTitle(surfaceId: string): string {
    const labels = wb.surfaceLabels as Record<string, { title: string; detail: string }>;
    return labels[surfaceId]?.title ?? plainToken(surfaceId);
  }

  function surfaceDetail(surfaceId: string): string {
    const labels = wb.surfaceLabels as Record<string, { title: string; detail: string }>;
    return labels[surfaceId]?.detail ?? plainToken(surfaceId);
  }

  function shortSha256(value: string): string {
    if (!value) return "—";
    return value.length > 18 ? `${value.slice(0, 8)}...${value.slice(-6)}` : value;
  }

  function profileText(value: string): string {
    if (value === "local-desktop") return wb.localDesktop;
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
      { label: wb.candidateTools, value: numberText(recall.agentToolHints) },
      { label: wb.toolExperience, value: plainToken(recall.toolExperienceReason) },
      { label: wb.firstUseTools, value: recall.hostFallbackRequired ? wb.hostDecides : wb.yes },
    ];
  }

  function facetRows(): KVRow[] {
    if (!report) return [];
    const facet = report.facetInspector;
    return [
      { label: wb.owner, value: facet.owner },
      { label: wb.reportOnly, value: boolText(facet.reportOnly) },
      { label: wb.directMutation, value: boolText(facet.directMutationAllowed) },
      { label: wb.fullScanFallback, value: boolText(facet.fallbackFullScan) },
      { label: wb.matchedSources, value: `${numberText(facet.matchedSourceCandidateCount)}/${numberText(facet.sourceCandidateCount)}` },
      { label: wb.auditFormat, value: facet.auditMarkdownFormat },
      { label: wb.indexRevision, value: facet.indexRevision ?? wb.none },
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
      { label: wb.candidateTools, value: numberText(projection.agentToolHints) },
      { label: wb.oldToolExperience, value: numberText(projection.agentToolRejections) },
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
      { label: wb.toolRegistries, value: `${numberText(soul.agentToolRegistries)} / ${numberText(soul.agentToolRegistryTools)}` },
      { label: wb.toolExperience, value: numberText(soul.agentToolExperiences) },
      { label: wb.staleToolExperience, value: numberText(soul.agentToolStaleExperiences) },
    ];
  }
</script>

<div class="workbench-page">
  <section class="panel workbench-top">
    <div class="panel-title">
      <h3>{wb.title}</h3>
      <div class="panel-title-actions">
        <button class="ghost-button" type="button" disabled={!backendConnected || loading} onclick={() => void onRefresh()}>
          {#if loading}<LoaderCircle class="spin-icon" size={13} />{:else}<RefreshCw size={13} />{/if}
          {wb.refresh}
        </button>
      </div>
    </div>
    <VerdictStrip
      {t}
      ready={verdictCounts.ready}
      limited={verdictCounts.limited}
      blocked={verdictCounts.blocked}
      unavailable={verdictCounts.unavailable}
    />
    <div class="skill-stats workbench-stat-strip">
      <div><span>{wb.benchmark}</span><strong>{statusText(benchmarkStatus)}</strong></div>
      <div><span>{wb.recall}</span><strong>{statusText(recallStatus)}</strong></div>
      <div><span>{wb.facet}</span><strong>{statusText(facetStatus)}</strong></div>
      <div><span>{wb.projection}</span><strong>{statusText(projectionStatus)}</strong></div>
      <div><span>{wb.archive}</span><strong>{statusText(archiveStatus)}</strong></div>
      <div><span>{wb.privateRawClosed}</span><strong>{boolText(allPrivateRawClosed)}</strong></div>
    </div>
  </section>

  {#if !report}
    <div class="skill-empty detail-empty">{loading ? wb.loading : t.labels.backendOffline}</div>
  {:else}
    <div class="workbench-grid lower-grid">
      <InspectorCard
        {t}
        title={wb.recallTitle}
        icon={GitBranch}
        status={(recallStatus.available ? recallStatus.status : "blocked") as StatusKind}
        statusLabelText={statusText(recallStatus)}
        metrics={[
          { label: wb.memoryPoints, value: numberText(report.recallInspector.graphNodes) },
          { label: wb.memoryLinks, value: numberText(report.recallInspector.graphEdges) },
          { label: wb.skillHits, value: numberText(report.recallInspector.proceduralDeliveryReports) },
        ]}
        rows={recallRows()}
      >
        {#if report.recallInspector.graphFailures.length > 0}
          <div class="chips">{#each report.recallInspector.graphFailures as failure}<span>{plainToken(failure)}</span>{/each}</div>
        {/if}
      </InspectorCard>

      <InspectorCard
        {t}
        title={wb.indexDiagnostics}
        icon={Tags}
        status={(facetStatus.available ? facetStatus.status : "blocked") as StatusKind}
        statusLabelText={statusText(facetStatus)}
        metrics={[
          { label: wb.exactDocs, value: numberText(report.facetInspector.exactFacetDocCount) },
          { label: wb.expandedDocs, value: numberText(report.facetInspector.expandedFacetDocCount) },
          { label: wb.renderGrowth, value: numberText(report.facetInspector.renderGrowth) },
        ]}
        rows={facetRows()}
      >
        {#if report.facetInspector.failures.length > 0}
          <div class="chips">{#each report.facetInspector.failures as failure}<span>{plainToken(failure)}</span>{/each}</div>
        {:else}
          <div class="skill-empty">{wb.emptyFailures}</div>
        {/if}
        {#if report.facetInspector.auditMarkdownPreview}
          <pre class="workbench-audit-preview">{report.facetInspector.auditMarkdownPreview}</pre>
        {/if}
      </InspectorCard>

      <InspectorCard
        {t}
        title={wb.projectionTitle}
        icon={Activity}
        status={(projectionStatus.available ? projectionStatus.status : "blocked") as StatusKind}
        statusLabelText={statusText(projectionStatus)}
        metrics={[
          { label: wb.citations, value: numberText(report.projectionInspector.evidenceRefs) },
          { label: wb.lengthControl, value: numberText(report.projectionInspector.budgetDecisions) },
          { label: wb.filtered, value: numberText(report.projectionInspector.droppedCandidates) },
        ]}
        rows={projectionRows()}
      >
        {#if report.projectionInspector.unsupportedClaims.length > 0}
          <div class="chips">{#each report.projectionInspector.unsupportedClaims as claim}<span>{plainToken(claim)}</span>{/each}</div>
        {:else}
          <div class="skill-empty">{wb.emptyFailures}</div>
        {/if}
      </InspectorCard>

      <InspectorCard
        {t}
        title={wb.proceduralTitle}
        icon={DatabaseZap}
        status={(proceduralStatus.available ? proceduralStatus.status : "blocked") as StatusKind}
        statusLabelText={statusText(proceduralStatus)}
        metrics={[
          { label: wb.skills, value: numberText(report.proceduralEvolution.totalSkills) },
          { label: wb.activeSkills, value: numberText(report.proceduralEvolution.activeSkills) },
          { label: wb.runtimeLearned, value: numberText(report.proceduralEvolution.runtimeLearned) },
        ]}
      >
        {#if report.proceduralEvolution.topSkills.length > 0}
          <div class="workbench-surface-list">
            {#each report.proceduralEvolution.topSkills as skill}
              <div class="workbench-surface-row">
                <div>
                  <strong>{skill.title}</strong>
                  <small>{skill.topic || wb.noTopic}</small>
                </div>
                <StatusBadge {t} status={skill.status} />
              </div>
            {/each}
          </div>
        {:else}
          <div class="skill-empty">{wb.noSkills}</div>
        {/if}
      </InspectorCard>

      <InspectorCard
        {t}
        title={wb.archiveTitle}
        icon={ShieldCheck}
        status={(archiveStatus.available ? archiveStatus.status : "blocked") as StatusKind}
        statusLabelText={statusText(archiveStatus)}
        rows={archiveRows()}
      >
        {#if report.archiveRestore.archiveRoot}
          <div class="workbench-fingerprint">
            <span>{wb.closureSha256}</span>
            <code>{shortSha256(report.archiveRestore.archiveRoot.closure_sha256)}</code>
          </div>
        {:else}
          <div class="skill-empty">{plainToken(report.archiveRestore.status.reason)}</div>
        {/if}
      </InspectorCard>

      <InspectorCard
        {t}
        title={wb.soulTitle}
        icon={ShieldCheck}
        status={(soulStatus.available ? soulStatus.status : "blocked") as StatusKind}
        statusLabelText={statusText(soulStatus)}
        rows={soulRows()}
      >
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
      </InspectorCard>
    </div>

    <div class="workbench-grid">
      <article class="panel workbench-api-panel">
        <PanelHeader title={`${numberText(report.apiMap.surfaces.length)} ${wb.surfaceUnit}`} icon={Workflow} />
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
        <PanelHeader title={benchmark ? wb.benchmarkTitle : benchmarkStatus.reason} icon={BarChart3} />
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
  {/if}
</div>
