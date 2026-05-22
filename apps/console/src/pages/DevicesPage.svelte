<script lang="ts">
  import { Plus, RefreshCw, Smartphone } from "lucide-svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Device } from "../lib/types";
  import { deviceLabel, statusLabel } from "../lib/view-model";

  let {
    t,
    devices,
    backendConnected,
    onOpenAddDevice,
    onRotateAppKey,
    onToggleDevice,
  }: {
    t: ConsoleCopy;
    devices: Device[];
    backendConnected: boolean;
    onOpenAddDevice: () => void;
    onRotateAppKey: (deviceId: string) => void;
    onToggleDevice: (deviceId: string) => void;
  } = $props();
</script>

<section class="panel">
  <div class="panel-title">
    <div>
      <p class="panel-label">{t.devicesPanel.label}</p>
      <h3>{t.devicesPanel.title}</h3>
    </div>
    <div class="panel-title-actions">
      <button class="primary-button" type="button" onclick={onOpenAddDevice}>
        <Plus size={13} /> {t.addDevice.btn}
      </button>
      <Smartphone size={18} />
    </div>
  </div>
  <div class="device-table">
    <div class="device-row header">
      {#each t.deviceHeaders as header}<span>{header}</span>{/each}
    </div>
    {#each devices as device}
      <div class="device-row">
        <span><strong>{deviceLabel(t, device)}</strong><small>{device.deviceId}</small></span>
        <span class="mono">{device.appKey}</span>
        <span class={`badge ${device.status}`}>{statusLabel(t, device.status)}</span>
        <span class="row-actions">
          <button type="button" disabled={!backendConnected} onclick={() => onRotateAppKey(device.deviceId)}><RefreshCw size={12} /> {t.actions.rotate}</button>
          <button type="button" disabled={!backendConnected} onclick={() => onToggleDevice(device.deviceId)}>
            {device.status === "disabled" ? t.actions.enable : t.actions.disable}
          </button>
        </span>
      </div>
    {/each}
  </div>
</section>
