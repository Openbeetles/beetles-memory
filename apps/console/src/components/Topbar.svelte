<script lang="ts">
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Page } from "../lib/types";

  let {
    consoleLabel,
    currentPage,
  }: {
    consoleLabel: ConsoleCopy["labels"]["console"];
    currentPage: Page;
  } = $props();
</script>

<header class="topbar" data-tauri-drag-region>
  <div class="topbar-left">
    <div class="breadcrumb">
      <span>{consoleLabel}</span>
      <span class="bc-sep">›</span>
      <span>{currentPage.eyebrow}</span>
      <span class="bc-sep">›</span>
      <span class="bc-title">{currentPage.title}</span>
    </div>
  </div>
  <div class="top-actions"></div>
</header>
