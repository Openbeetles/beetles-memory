<script lang="ts">
  import type { PageId } from "../lib/types";

  let { pageId }: { pageId: PageId } = $props();

  /** Typographic HUD glyphs — not Material / Lucide */
  const glyphs: Record<PageId, string> = {
    overview: "◉",
    workbench: "⌖",
    skills: "⌬",
    "llm-gateway": "⧉",
    transports: "⋙",
    devices: "▣",
    account: "⬡",
  };
</script>

<span class="nav-glyph" data-page={pageId} aria-hidden="true">{glyphs[pageId]}</span>
