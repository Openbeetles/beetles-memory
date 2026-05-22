<script lang="ts">
  import { Activity, Globe2 } from "lucide-svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import KvStack from "../components/KvStack.svelte";
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
  }: {
    t: ConsoleCopy;
    overviewCards: OverviewCard[];
    overviewData: ConsoleApiOverview | null;
    systemInfoSpecs: { cpu: string; memory: string; date: string; time: string };
    recentEvents: TimelineEvent[];
    transportStats: KVRow[];
    kernelRows: KVRow[];
  } = $props();
</script>

{#if overviewCards[0]}
  {@const heroCard = overviewCards[0]}
  {@const HeroIcon = heroCard.icon}
  <div class="hud-hero">
    <div class="hud-hero-ident">
      <em class="dot ready"></em>
      <div class="hud-hero-text">
        <strong class="hud-sysname">{heroCard.value}</strong>
        <span class="hud-profile">{overviewData?.runtimeShape?.profile ?? "—"}</span>
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

<div class="hud-stats">
  {#each overviewCards.slice(1) as card}
    {@const Icon = card.icon}
    <div class="stat-block {card.tone}">
      <div class="stat-head"><Icon size={12} /><span>{card.title}</span></div>
      <strong>{card.value}</strong>
      {#if card.progress !== null}
        <div class="hud-bar"><div class="hud-bar-fill" style="width:{card.progress}%"></div></div>
      {/if}
      <small>{card.desc}</small>
    </div>
  {/each}
</div>

<div class="hud-lower">
  <article class="panel">
    <PanelHeader label={t.overview.recentEvents} title={t.overview.timeline} icon={Activity} />
    <div class="event-list">
      {#each recentEvents as ev}
        <div class="event-row">
          <span>{ev.time}</span>
          <strong>{ev.text}</strong>
          <em class={`dot ${ev.tone}`}></em>
        </div>
      {/each}
    </div>
  </article>
  <article class="panel">
    <PanelHeader label={t.overview.observation} title={t.overview.communicationAccess} icon={Globe2} />
    <KvStack items={transportStats} />
    <div class="panel-divider"></div>
    <KvStack items={kernelRows} />
  </article>
</div>
