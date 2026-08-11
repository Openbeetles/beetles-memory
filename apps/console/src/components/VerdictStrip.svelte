<script lang="ts">
  import type { ConsoleCopy } from "../lib/i18n";

  let {
    t,
    ready,
    limited,
    blocked,
    unavailable,
  }: {
    t: ConsoleCopy;
    ready: number;
    limited: number;
    blocked: number;
    unavailable: number;
  } = $props();

  const verdict = $derived(
    blocked > 0 ? "blocked" : limited > 0 || unavailable > 0 ? "limited" : "ready",
  );
  const summary = $derived(
    verdict === "blocked"
      ? t.workbenchPanel.verdictBlocked
      : verdict === "limited"
        ? t.workbenchPanel.verdictLimited
        : t.workbenchPanel.verdictReady,
  );
</script>

<section class={`verdict-strip verdict-${verdict}`} aria-live="polite">
  <div class="verdict-ident">
    <em class={`dot ${verdict === "ready" ? "" : verdict}`}></em>
    <strong>{summary}</strong>
  </div>
  <div class="verdict-counts">
    <div class="verdict-count ready"><span>{t.workbenchPanel.verdictReadyCount}</span><strong>{ready}</strong></div>
    <div class="verdict-count limited"><span>{t.workbenchPanel.verdictLimitedCount}</span><strong>{limited}</strong></div>
    <div class="verdict-count blocked"><span>{t.workbenchPanel.verdictBlockedCount}</span><strong>{blocked}</strong></div>
    <div class="verdict-count locked"><span>{t.workbenchPanel.verdictUnavailableCount}</span><strong>{unavailable}</strong></div>
  </div>
</section>
