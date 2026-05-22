<script lang="ts">
  import type { Component } from "svelte";

  let { label, title, icon: Icon }: { label: string; title: string; icon: Component } = $props();
</script>

<div class="panel-title">
  <div>
    <p class="panel-label">{label}</p>
    <h3>{title}</h3>
  </div>
  <Icon size={18} />
</div>
