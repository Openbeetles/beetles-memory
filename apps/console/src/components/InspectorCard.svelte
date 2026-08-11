<script lang="ts">
  import type { IconComponent, KVRow, StatusKind } from "../lib/types";
  import type { ConsoleCopy } from "../lib/i18n";
  import KvStack from "./KvStack.svelte";
  import PanelHeader from "./PanelHeader.svelte";
  import StatusBadge from "./StatusBadge.svelte";

  let {
    t,
    title,
    icon,
    status,
    statusLabelText,
    metrics = [],
    rows = [],
    children,
  }: {
    t: ConsoleCopy;
    title: string;
    icon: IconComponent;
    status?: StatusKind;
    statusLabelText?: string;
    metrics?: { label: string; value: string }[];
    rows?: KVRow[];
    children?: import("svelte").Snippet;
  } = $props();
</script>

<article class="panel inspector-card">
  <PanelHeader {title} {icon} />
  {#if status}
    <div class="inspector-status-row">
      <span>{t.labels.status}</span>
      <StatusBadge {t} {status} label={statusLabelText} />
    </div>
  {/if}
  {#if metrics.length > 0}
    <div class="workbench-metric-grid">
      {#each metrics as metric}
        <div><span>{metric.label}</span><strong>{metric.value}</strong></div>
      {/each}
    </div>
  {/if}
  {#if rows.length > 0}
    <KvStack items={rows} />
  {/if}
  {#if children}
    {@render children()}
  {/if}
</article>
