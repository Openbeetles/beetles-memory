<script lang="ts">
  import type { IconComponent } from "../lib/types";

  let {
    title,
    value,
    desc,
    icon,
    tone = "ready",
  }: {
    title: string;
    value: string;
    desc: string;
    icon: IconComponent;
    tone?: string;
  } = $props();

  const Icon = $derived(icon);
</script>

<div class={`stat-block ${tone}`}>
  <div class="stat-head"><Icon size={12} /><span>{title}</span></div>
  <strong>{value}</strong>
  <small>{desc}</small>
</div>
