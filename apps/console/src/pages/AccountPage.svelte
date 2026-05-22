<script lang="ts">
  import { KeyRound, Languages, LockKeyhole, Settings } from "lucide-svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { KVRow, Lang } from "../lib/types";

  let {
    t,
    lang,
    accountFields,
    onLangChange,
  }: {
    t: ConsoleCopy;
    lang: Lang;
    accountFields: KVRow[];
    onLangChange: (lang: Lang) => void;
  } = $props();
</script>

<div class="settings-grid">
  <section class="panel account-panel">
    <PanelHeader label={t.account.panel} title={t.account.title} icon={KeyRound} />
    <div class="runtime-summary">
      {#each accountFields as row}
        <div><span>{row.label}</span><strong>{row.value}</strong></div>
      {/each}
    </div>
    <div class="notice">
      <LockKeyhole size={16} />
      <span>{t.account.notice}</span>
    </div>
  </section>
  <section class="panel">
    <PanelHeader label={t.systemSettings.panel} title={t.systemSettings.title} icon={Settings} />
    <div class="settings-row">
      <span class="settings-row-label"><Languages size={13} />{t.systemSettings.langLabel}</span>
      <select class="lang-select" value={lang} onchange={(event) => onLangChange((event.currentTarget as HTMLSelectElement).value as Lang)} aria-label={t.systemSettings.langLabel}>
        <option value="zh-CN">{t.systemSettings.langOptions.zh}</option>
        <option value="en">{t.systemSettings.langOptions.en}</option>
      </select>
    </div>
  </section>
</div>
