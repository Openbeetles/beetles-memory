<script lang="ts">
  import { LoaderCircle, Plus } from "lucide-svelte";
  import StatusBadge from "../components/StatusBadge.svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Device } from "../lib/types";
  import { deviceLabel } from "../lib/view-model";

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
    <h3>{t.devicesPanel.title}</h3>
    <div class="panel-title-actions">
      <button class="primary-button" type="button" onclick={onOpenAddDevice} disabled={!backendConnected || busyDeviceId !== null}>
        <Plus size={13} /> {t.addDevice.btn}
      </button>
    </div>
  </div>
  {#if actionError}
    <p class="panel-action-error" role="alert">{actionError}</p>
  {/if}
  <div class="device-ledger">
    <div class="device-ledger-row header">
      {#each t.deviceHeaders as header}<span>{header}</span>{/each}
    </div>
    {#each devices as device}
      {@const rotateBusy = busyDeviceId === device.deviceId && busyDeviceAction === "rotate_key"}
      {@const toggleBusy = busyDeviceId === device.deviceId && (busyDeviceAction === "disable" || busyDeviceAction === "enable")}
      <div class="device-ledger-row">
        <span>
          <strong>{deviceLabel(t, device)}</strong>
          <small>{device.deviceId}</small>
        </span>
        <span class="mono">{device.appKey}</span>
        <span><StatusBadge {t} status={device.status} /></span>
        <span class="row-actions">
          <button type="button" disabled={!backendConnected || busyDeviceId !== null} onclick={() => onRotateAppKey(device.deviceId)}>
            {#if rotateBusy}<LoaderCircle class="spin-icon" size={12} />{/if}
            {t.actions.rotate}
          </button>
          <button type="button" disabled={!backendConnected || busyDeviceId !== null} onclick={() => onToggleDevice(device.deviceId)}>
            {#if toggleBusy}<LoaderCircle class="spin-icon" size={12} />{/if}
            {device.status === "allowed" ? t.actions.disable : t.actions.enable}
          </button>
        </span>
      </div>
    {:else}
      <div class="skill-empty">{backendConnected ? t.devicesPanel.empty : t.labels.backendOffline}</div>
    {/each}
  </div>
</section>
