<script lang="ts">
  import { Globe2 } from "lucide-svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Transport, TransportId } from "../lib/types";

  let {
    t,
    transports,
    backendConnected,
    onToggleTransport,
    onSaveTransportEndpoint,
  }: {
    t: ConsoleCopy;
    transports: Transport[];
    backendConnected: boolean;
    onToggleTransport: (id: TransportId) => void;
    onSaveTransportEndpoint: (id: TransportId, endpoint: string) => void;
  } = $props();
</script>

<section class="panel">
  <PanelHeader label={t.transportsPanel.label} title={t.transportsPanel.title} icon={Globe2} />
  <div class="transport-grid">
    {#each transports as transport}
      {@const transportCopy = t.transports[transport.id]}
      <article class:disabled={!transport.enabled} class="transport-card">
        <div class="transport-head">
          <button
            aria-label={t.actions.toggleTransport(transportCopy.name)}
            class:enabled={transport.enabled}
            class="switch"
            type="button"
            disabled={!backendConnected || transport.editable === false}
            onclick={() => onToggleTransport(transport.id)}
          ><span></span></button>
          <div>
            <h4>{transportCopy.name}</h4>
            <p>{transportCopy.detail}</p>
          </div>
        </div>
        <label>
          <span>{t.labels.addressMode}</span>
          <input
            value={transport.endpoint}
            disabled={!backendConnected || !transport.enabled || transport.editable === false}
            onchange={(event) => onSaveTransportEndpoint(transport.id, (event.currentTarget as HTMLInputElement).value)}
          />
        </label>
        <div class="chips">{#each transportCopy.fields as field}<span>{field}</span>{/each}</div>
      </article>
    {/each}
  </div>
</section>
