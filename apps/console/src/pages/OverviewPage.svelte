<script lang="ts">
  import { Activity, Crosshair, Globe2 } from "lucide-svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import KvStack from "../components/KvStack.svelte";
  import StorageMeter from "../components/StorageMeter.svelte";
  import TelemetryStat from "../components/TelemetryStat.svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { ConsoleApiOverview, KVRow, OverviewCard, TimelineEvent } from "../lib/types";

  let {
    t,
    overviewCards,
    overviewData,
    systemInfoSpecs,
    recentEvents,
    transportStats,
    kernelRows,
    memoryContextRows,
    backendConnected = false,
  }: {
    t: ConsoleCopy;
    overviewCards: OverviewCard[];
    overviewData: ConsoleApiOverview | null;
    systemInfoSpecs: { cpu: string; memory: string; date: string; time: string };
    recentEvents: TimelineEvent[];
    transportStats: KVRow[];
    kernelRows: KVRow[];
    memoryContextRows: KVRow[];
    backendConnected?: boolean;
  } = $props();

  const storageCard = $derived(overviewCards[1]);
  const metricCards = $derived(overviewCards.slice(2));
</script>

{#if overviewCards[0]}
  {@const heroCard = overviewCards[0]}
  {@const HeroIcon = heroCard.icon}
  <div class="hud-hero">
    <div class="hud-hero-ident">
      <em class={`dot ${backendConnected ? "" : "blocked"}`}></em>
      <div class="hud-hero-text">
        <strong class="hud-sysname">{heroCard.value}</strong>
        <span class="hud-profile">{overviewData?.runtimeShape?.profile ?? "—"}</span>
        <span class={`hud-link-state ${backendConnected ? "ok" : "bad"}`}>
          {backendConnected ? t.labels.connected : t.labels.disconnected}
        </span>
      </div>
    </div>
    <div class="hud-hero-specs">
      <span class="hud-chip"><em>CPU</em><code>{systemInfoSpecs.cpu}</code></span>
      <span class="hud-chip"><em>MEM</em><code>{systemInfoSpecs.memory}</code></span>
      <span class="hud-chip hud-chip-time"><em>{systemInfoSpecs.date}</em><code>{systemInfoSpecs.time}</code></span>
      <div class="hud-hero-icon"><HeroIcon size={20} /></div>
    </div>
  </div>
{/if}

<div class="overview-align-grid">
  {#each metricCards as card}
    <TelemetryStat
      title={card.title}
      value={card.value}
      desc={card.desc}
      icon={card.icon}
      tone={card.tone}
    />
  {/each}
  {#if storageCard}
    {@const StorageIcon = storageCard.icon}
    <article class="panel overview-storage-panel">
      <div class="stat-head"><StorageIcon size={12} /><span>{storageCard.title}</span></div>
      <strong class="overview-storage-value">{storageCard.value}</strong>
      <StorageMeter progress={storageCard.progress} ariaLabel={storageCard.title} />
      <small class="overview-storage-desc">{storageCard.desc}</small>
    </article>
  {/if}
  <article class="panel memory-context-panel">
    <PanelHeader title={t.overview.memoryContextTitle} icon={Crosshair} />
    {#if memoryContextRows.length > 0}
      <KvStack items={memoryContextRows} />
    {:else}
      <div class="skill-empty">{t.overview.memoryContextEmpty}</div>
    {/if}
  </article>
</div>

<div class="hud-lower">
  <article class="panel">
    <PanelHeader title={t.overview.timeline} icon={Activity} />
    <div class="event-list">
      {#each recentEvents as ev}
        <div class="event-row">
          <span>{ev.time}</span>
          <strong>{ev.text}</strong>
          <em class={`dot ${ev.tone}`}></em>
        </div>
      {:else}
        <div class="skill-empty">{t.labels.backendOffline}</div>
      {/each}
    </div>
  </article>
  <div class="hud-lower-side">
    <article class="panel">
      <PanelHeader title={t.overview.observation} icon={Globe2} />
      <KvStack items={transportStats} />
      {#if kernelRows.length > 0}
        <div class="overview-kernel-block">
          <KvStack items={kernelRows} />
        </div>
      {/if}
    </article>
  </div>
</div>
