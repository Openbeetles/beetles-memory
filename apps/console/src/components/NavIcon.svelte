<script lang="ts">
  import { BrainCircuit } from "lucide-svelte";
  import type { PageId } from "../lib/types";

  let { pageId }: { pageId: PageId } = $props();

  /** Typographic HUD glyphs — not Material / Lucide */
  const glyphs: Record<Exclude<PageId, "model-config">, string> = {
    overview: "◉",
    workbench: "⌖",
    skills: "⌬",
    "llm-gateway": "⧉",
    transports: "⋙",
    devices: "▣",
    account: "⬡",
  };
</script>

{#if pageId === "model-config"}
  <BrainCircuit class="nav-glyph" size={15} aria-hidden="true" />
{:else}
  <span class="nav-glyph" data-page={pageId} aria-hidden="true">{glyphs[pageId]}</span>
{/if}
