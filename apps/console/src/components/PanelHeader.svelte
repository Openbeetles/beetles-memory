<script lang="ts">
  import type { IconComponent } from "../lib/types";

  let {
    title,
    icon: Icon = undefined,
  }: {
    title: string;
    icon?: IconComponent;
  } = $props();
</script>

<div class="panel-title">
  <h3>{title}</h3>
  {#if Icon}
    <Icon size={14} />
  {/if}
</div>
