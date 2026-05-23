<script lang="ts">
  import type { Component } from "svelte";

  let { label, title, icon: Icon }: { label: string; title: string; icon: Component } = $props();
</script>

<div class="panel-title">
  <div>
    <span class="panel-label">{label}</span>
    <h3>{title}</h3>
  </div>
  <Icon size={13} />
</div>
