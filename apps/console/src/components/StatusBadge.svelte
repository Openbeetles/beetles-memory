<script lang="ts">
  import type { ConsoleCopy } from "../lib/i18n";
  import type { StatusKind } from "../lib/types";
  import { statusLabel } from "../lib/view-model";

  let {
    t,
    status,
    label,
  }: {
    t: ConsoleCopy;
    status: StatusKind;
    label?: string;
  } = $props();

  const text = $derived(label ?? statusLabel(t, status));
</script>

<span class={`badge ${status}`}>{text}</span>
