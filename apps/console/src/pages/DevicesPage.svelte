<script lang="ts">
  import { LoaderCircle, Plus, RefreshCw, Smartphone } from "lucide-svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Device } from "../lib/types";
  import { deviceLabel, statusLabel } from "../lib/view-model";

  type DeviceBusyAction = "rotate_key" | "disable" | "enable";

  let {
    t,
    devices,
    backendConnected,
    actionError,
    busyDeviceId,
    busyDeviceAction,
    onOpenAddDevice,
    onRotateAppKey,
    onToggleDevice,
  }: {
    t: ConsoleCopy;
    devices: Device[];
    backendConnected: boolean;
    actionError: string;
    busyDeviceId: string | null;
    busyDeviceAction: DeviceBusyAction | null;
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
      <button class="primary-button" type="button" onclick={onOpenAddDevice} disabled={!backendConnected || busyDeviceId !== null}>
        <Plus size={13} /> {t.addDevice.btn}
      </button>
      <Smartphone size={18} />
    </div>
  </div>
  {#if actionError}
    <p class="panel-action-error">{actionError}</p>
  {/if}
  <div class="device-table">
    <div class="device-row header">
      {#each t.deviceHeaders as header}<span>{header}</span>{/each}
    </div>
    {#each devices as device}
      {@const rotateBusy = busyDeviceId === device.deviceId && busyDeviceAction === "rotate_key"}
      {@const toggleBusy = busyDeviceId === device.deviceId && (busyDeviceAction === "enable" || busyDeviceAction === "disable")}
      <div class="device-row">
        <span><strong>{deviceLabel(t, device)}</strong><small>{device.deviceId}</small></span>
        <span class="mono">{device.appKey}</span>
        <span class={`badge ${device.status}`}>{statusLabel(t, device.status)}</span>
        <span class="row-actions">
          <button type="button" disabled={!backendConnected || busyDeviceId !== null} onclick={() => onRotateAppKey(device.deviceId)}>
            {#if rotateBusy}<LoaderCircle class="spin-icon" size={12} />{:else}<RefreshCw size={12} />{/if}
            {t.actions.rotate}
          </button>
          <button type="button" disabled={!backendConnected || busyDeviceId !== null} onclick={() => onToggleDevice(device.deviceId)}>
            {#if toggleBusy}<LoaderCircle class="spin-icon" size={12} />{/if}
            {device.status === "disabled" ? t.actions.enable : t.actions.disable}
          </button>
        </span>
      </div>
    {/each}
  </div>
</section>
