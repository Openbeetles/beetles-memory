<script lang="ts">
  import { Plus } from "lucide-svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import { modalBackdrop, modalPanel } from "../lib/modal-transition";

  let {
    t,
    label,
    error,
    onClose,
    onSubmit,
    onLabelChange,
  }: {
    t: ConsoleCopy;
    label: string;
    error: string;
    onClose: () => void;
    onSubmit: (event: SubmitEvent) => void;
    onLabelChange: (value: string) => void;
  } = $props();
</script>

<div class="modal-backdrop" aria-hidden="true" transition:modalBackdrop></div>
<div class="modal" role="dialog" aria-modal="true" aria-labelledby="add-device-title" transition:modalPanel>
  <div class="modal-header">
    <h3 id="add-device-title"><Plus size={14} /> {t.addDevice.title}</h3>
    <button class="modal-close" type="button" onclick={onClose} aria-label={t.addDevice.closeLabel}>✕</button>
  </div>
  <form class="modal-body" onsubmit={onSubmit}>
    <label>
      <span>{t.addDevice.nameLabel}</span>
      <input value={label} oninput={(event) => onLabelChange((event.currentTarget as HTMLInputElement).value)} placeholder={t.addDevice.namePlaceholder} required autocomplete="off" />
    </label>
    {#if error}<p class="modal-error">{error}</p>{/if}
    <div class="modal-footer">
      <button class="ghost-button" type="button" onclick={onClose}>{t.addDevice.cancel}</button>
      <button class="primary-button" type="submit">{t.addDevice.save}</button>
    </div>
  </form>
</div>
