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
    backendConnected = false,
    profile = null,
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
    backendConnected?: boolean;
    profile?: string | null;
    loading?: boolean;
    onThemeChange: (theme: Theme) => void;
    onRefresh: () => void | Promise<void>;
  } = $props();
</script>

<div class="statusbar" data-tauri-drag-region use:windowDragRegion>
  <span class="sb-brand" data-tauri-drag-region>{t.statusbar.brand}</span>
  <div class="sb-telemetry" data-tauri-drag-region>
    <span class={`sb-item ${backendConnected ? "ok" : "bad"}`} data-tauri-drag-region>
      {t.statusbar.link}: {backendConnected ? t.labels.connected : t.labels.disconnected}
    </span>
    {#if profile}
      <span class="sb-sep" data-tauri-drag-region>│</span>
      <span class="sb-item mono" data-tauri-drag-region title={profile}>
        {t.statusbar.profile}: {profile}
      </span>
    {/if}
  </div>
  <div class="sb-right" data-tauri-drag-region>
    <div class="sb-theme-toggle" role="group" aria-label={t.actions.theme}>
      <button class:active={theme === "light"} type="button" onclick={() => onThemeChange("light")} aria-label={t.actions.light} aria-pressed={theme === "light"}><Sun size={12} /></button>
      <button class:active={theme === "dark"}  type="button" onclick={() => onThemeChange("dark")}  aria-label={t.actions.dark} aria-pressed={theme === "dark"}><Moon size={12} /></button>
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
