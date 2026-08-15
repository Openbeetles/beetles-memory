<script lang="ts">
  import type { ConsoleCopy } from "../lib/i18n";
  import type { NavGroupId } from "../lib/view-model";
  import type { Page, PageId } from "../lib/types";
  import { windowDragRegion } from "../lib/window-drag";

  const PAGE_GROUP: Record<PageId, NavGroupId> = {
    overview: "memoryCore",
    workbench: "memoryCore",
    skills: "memoryCore",
    "llm-gateway": "ingress",
    transports: "ingress",
    devices: "ingress",
    "model-config": "system",
    account: "system",
  };

  let {
    t,
    currentPage,
  }: {
    t: ConsoleCopy;
    currentPage: Page;
  } = $props();

  const groupLabel = $derived(t.navGroups[PAGE_GROUP[currentPage.id]]);
</script>

<header class="topbar" data-tauri-drag-region use:windowDragRegion>
  <div class="topbar-left" data-tauri-drag-region>
    <div class="breadcrumb" data-tauri-drag-region>
      <span data-tauri-drag-region>{groupLabel}</span>
      <span class="bc-sep" data-tauri-drag-region>›</span>
      <span class="bc-title" data-tauri-drag-region>{currentPage.label}</span>
    </div>
  </div>
</header>
