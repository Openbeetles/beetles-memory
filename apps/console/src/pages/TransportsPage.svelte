<script lang="ts">
  import { CircleQuestionMark, Globe2, LoaderCircle } from "lucide-svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import StatusBadge from "../components/StatusBadge.svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Transport, TransportId } from "../lib/types";

  type TransportBusyAction = "toggle" | "save";

  let {
    t,
    transports,
    backendConnected,
    busyTransportId,
    busyTransportAction,
    onToggleTransport,
    onSaveTransportEndpoint,
  }: {
    t: ConsoleCopy;
    transports: Transport[];
    backendConnected: boolean;
    busyTransportId: TransportId | null;
    busyTransportAction: TransportBusyAction | null;
    onToggleTransport: (id: TransportId) => void;
    onSaveTransportEndpoint: (id: TransportId, endpoint: string) => void;
  } = $props();
</script>

<section class="panel">
  <PanelHeader title={t.transportsPanel.title} icon={Globe2} />
  <div class="transport-grid">
    {#each transports as transport}
      {@const transportCopy = t.transports[transport.id] ?? { name: transport.id, detail: transport.endpoint }}
      {@const toggleBusy = busyTransportId === transport.id && busyTransportAction === "toggle"}
      <article class:disabled={!transport.enabled} class="transport-card">
        <div class="transport-head">
          <button
            aria-label={t.actions.toggleTransport(transportCopy.name)}
            class:enabled={transport.enabled}
            class:loading={toggleBusy}
            class="switch"
            type="button"
            disabled={!backendConnected || busyTransportId !== null || transport.editable === false}
            onclick={() => onToggleTransport(transport.id)}
          >
            {#if toggleBusy}<LoaderCircle class="spin-icon" size={12} />{:else}<span></span>{/if}
          </button>
          <h4>{transportCopy.name}</h4>
          <StatusBadge {t} status={transport.status} />
        </div>
        <div class="transport-endpoint">
          <input
            aria-label={t.labels.addressMode}
            value={transport.endpoint}
            disabled={!backendConnected || busyTransportId !== null || !transport.enabled || transport.editable === false}
            onchange={(event) => onSaveTransportEndpoint(transport.id, (event.currentTarget as HTMLInputElement).value)}
          />
          <button type="button" class="transport-tip" data-tip={transportCopy.detail} aria-label={transportCopy.detail}>
            <CircleQuestionMark size={14} />
          </button>
        </div>
      </article>
    {/each}
  </div>
</section>
