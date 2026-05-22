<script lang="ts">
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Page, PageId } from "../lib/types";
  import { windowDragRegion } from "../lib/window-drag";

  let {
    pages,
    activePage,
    isTauri,
    isMacOS = false,
    brand,
    onSelectPage,
  }: {
    pages: Page[];
    activePage: PageId;
    isTauri: boolean;
    isMacOS?: boolean;
    brand: ConsoleCopy["brand"];
    onSelectPage: (page: PageId) => void;
  } = $props();
</script>

<aside class="sidebar">
  <div class="brand" data-tauri-drag-region use:windowDragRegion>
    {#if !isTauri}
      <div class="brand-icon"><img src="/logo.png" alt="Beetle Memory" /></div>
      <div class="brand-text">
        <span class="brand-name">{brand.name}</span>
        <span class="brand-sub">{brand.sub}</span>
      </div>
    {/if}
  </div>

  <nav class="nav">
    {#each pages as page}
      <button
        class:active={activePage === page.id}
        class="nav-item"
        type="button"
        onclick={() => onSelectPage(page.id)}
      >
        <span class="nav-chevron">{activePage === page.id ? "▶" : "›"}</span>
        <span class="nav-label">{page.label}</span>
        {#if page.count}<code class="nav-count">[{page.count}]</code>{/if}
      </button>
    {/each}
  </nav>
</aside>
