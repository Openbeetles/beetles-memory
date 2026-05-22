<script lang="ts">
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Page } from "../lib/types";
  import { windowDragRegion } from "../lib/window-drag";

  let {
    consoleLabel,
    currentPage,
  }: {
    consoleLabel: ConsoleCopy["labels"]["console"];
    currentPage: Page;
  } = $props();
</script>

<header class="topbar" data-tauri-drag-region use:windowDragRegion>
  <div class="topbar-left" data-tauri-drag-region>
    <div class="breadcrumb" data-tauri-drag-region>
      <span data-tauri-drag-region>{consoleLabel}</span>
      <span class="bc-sep" data-tauri-drag-region>›</span>
      <span data-tauri-drag-region>{currentPage.eyebrow}</span>
      <span class="bc-sep" data-tauri-drag-region>›</span>
      <span class="bc-title" data-tauri-drag-region>{currentPage.title}</span>
    </div>
  </div>
  <div class="top-actions" data-tauri-drag-region></div>
</header>
