<script lang="ts">
  import { LoaderCircle, Moon, Power, Sun } from "lucide-svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Theme } from "../lib/types";
  import { windowDragRegion } from "../lib/window-drag";

  let {
    t,
    theme,
    skillCount,
    enabledTransportCount,
    transportCount,
    activeDeviceCount,
    deviceCount,
    loading = false,
    onThemeChange,
    onRefresh,
  }: {
    t: ConsoleCopy;
    theme: Theme;
    skillCount: number;
    enabledTransportCount: number;
    transportCount: number;
    activeDeviceCount: number;
    deviceCount: number;
    loading?: boolean;
    onThemeChange: (theme: Theme) => void;
    onRefresh: () => void | Promise<void>;
  } = $props();
</script>

<div class="statusbar" data-tauri-drag-region use:windowDragRegion>
  <span class="sb-brand" data-tauri-drag-region>{t.statusbar.brand}</span>
  <span class="sb-live" aria-label="live" title="LIVE">●</span>
  <span class="sb-item" data-tauri-drag-region>v1.0.0-dev</span>
  <div class="sb-right" data-tauri-drag-region>
    <div class="sb-theme-toggle">
      <button class:active={theme === "light"} type="button" onclick={() => onThemeChange("light")} aria-label="日间模式"><Sun size={12} /></button>
      <button class:active={theme === "dark"}  type="button" onclick={() => onThemeChange("dark")}  aria-label="夜间模式"><Moon size={12} /></button>
    </div>
    <span class="sb-sep" data-tauri-drag-region>│</span>
    <span class="sb-item" data-tauri-drag-region>{t.statusbar.skills}: {skillCount}</span>
    <span class="sb-sep" data-tauri-drag-region>│</span>
    <span class="sb-item" data-tauri-drag-region>{t.statusbar.transports}: {enabledTransportCount}/{transportCount}</span>
    <span class="sb-sep" data-tauri-drag-region>│</span>
    <span class="sb-item" data-tauri-drag-region>{t.statusbar.devices}: {activeDeviceCount}/{deviceCount}</span>
    <span class="sb-sep" data-tauri-drag-region>│</span>
    <button class="sb-restart" type="button" onclick={onRefresh} disabled={loading}>
      {#if loading}<LoaderCircle class="spin-icon" size={11} />{:else}<Power size={11} />{/if}
      {t.actions.apply}
    </button>
  </div>
</div>
