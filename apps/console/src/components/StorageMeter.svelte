<script lang="ts">
  let {
    progress = null,
    ariaLabel,
  }: {
    progress?: number | null;
    ariaLabel: string;
  } = $props();

  const pct = $derived(
    progress === null || !Number.isFinite(progress)
      ? null
      : Math.max(0, Math.min(100, progress)),
  );

  // Semicircle r=38 → arc length = π*r
  const ARC = Math.PI * 38;
  const offset = $derived(pct === null ? ARC : ARC * (1 - pct / 100));
  const label = $derived(pct === null ? "—" : `${pct < 1 && pct > 0 ? pct.toFixed(2) : pct.toFixed(pct < 10 ? 1 : 0)}%`);
</script>

<div class="storage-meter" role="img" aria-label={`${ariaLabel}: ${label}`}>
  <svg class="storage-meter-svg" viewBox="0 0 100 58" aria-hidden="true">
    <path
      class="storage-meter-track"
      d="M 12 50 A 38 38 0 0 1 88 50"
      fill="none"
      stroke-width="7"
      stroke-linecap="butt"
    />
    <path
      class="storage-meter-fill"
      class:empty={pct === null || pct <= 0}
      d="M 12 50 A 38 38 0 0 1 88 50"
      fill="none"
      stroke-width="7"
      stroke-linecap="butt"
      stroke-dasharray={ARC}
      stroke-dashoffset={offset}
    />
  </svg>
  <strong class="storage-meter-pct">{label}</strong>
</div>
