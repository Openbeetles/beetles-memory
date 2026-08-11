<script lang="ts">
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Page, PageId } from "../lib/types";
  import { navGroupsWithPages } from "../lib/view-model";
  import { windowDragRegion } from "../lib/window-drag";
  import NavIcon from "./NavIcon.svelte";

  let {
    t,
    pages,
    activePage,
    isTauri,
    brand,
    onSelectPage,
  }: {
    t: ConsoleCopy;
    pages: Page[];
    activePage: PageId;
    isTauri: boolean;
    brand: ConsoleCopy["brand"];
    onSelectPage: (pageId: PageId) => void;
  } = $props();

  const groups = $derived(navGroupsWithPages(t, pages));
</script>

<aside class="sidebar">
  {#if isTauri}
    <div class="titlebar-spacer" data-tauri-drag-region use:windowDragRegion aria-hidden="true"></div>
  {/if}

  <nav class="nav" aria-label={brand.name}>
    {#each groups as group}
      <div class="nav-group">
        <div class="nav-group-label">{group.label}</div>
        {#each group.pages as page}
          <button
            class:active={activePage === page.id}
            class="nav-item"
            type="button"
            onclick={() => onSelectPage(page.id)}
          >
            <NavIcon pageId={page.id} />
            <span class="nav-label">{page.label}</span>
            {#if page.count}<code class="nav-count">{page.count}</code>{/if}
          </button>
        {/each}
      </div>
    {/each}
  </nav>
</aside>
