<script lang="ts">
  import type { KVRow } from "../lib/types";

  let { items }: { items: KVRow[] } = $props();
</script>

<div class="status-stack">
  {#each items as row}
    <div><span>{row.label}</span><strong>{row.value}</strong></div>
  {/each}
</div>
