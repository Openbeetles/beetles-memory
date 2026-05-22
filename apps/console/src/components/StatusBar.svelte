<script lang="ts">
  import { Moon, Power, Sun } from "lucide-svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { Theme } from "../lib/types";

  let {
    t,
    theme,
    skillCount,
    enabledTransportCount,
    transportCount,
    activeDeviceCount,
    deviceCount,
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
    onThemeChange: (theme: Theme) => void;
    onRefresh: () => void;
  } = $props();
</script>

<div class="statusbar">
  <span class="sb-brand">{t.statusbar.brand}</span>
  <span class="sb-item">v1.0.0-dev</span>
  <div class="sb-right">
    <div class="sb-theme-toggle">
      <button class:active={theme === "light"} type="button" onclick={() => onThemeChange("light")} aria-label="日间模式"><Sun size={12} /></button>
      <button class:active={theme === "dark"}  type="button" onclick={() => onThemeChange("dark")}  aria-label="夜间模式"><Moon size={12} /></button>
    </div>
    <span class="sb-sep">│</span>
    <span class="sb-item">{t.statusbar.skills}: {skillCount}</span>
    <span class="sb-sep">│</span>
    <span class="sb-item">{t.statusbar.transports}: {enabledTransportCount}/{transportCount}</span>
    <span class="sb-sep">│</span>
    <span class="sb-item">{t.statusbar.devices}: {activeDeviceCount}/{deviceCount}</span>
    <span class="sb-sep">│</span>
    <button class="sb-restart" type="button" onclick={onRefresh}><Power size={11} /> {t.actions.apply}</button>
  </div>
</div>
