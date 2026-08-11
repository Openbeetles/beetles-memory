<script lang="ts">
  import { LoaderCircle, Plus } from "lucide-svelte";
  import { accessibleDialog } from "../lib/dialog-focus";
  import type { ConsoleCopy } from "../lib/i18n";
  import { modalBackdrop, modalPanel } from "../lib/modal-transition";

  let {
    t,
    label,
    error,
    loading = false,
    onClose,
    onSubmit,
    onLabelChange,
  }: {
    t: ConsoleCopy;
    label: string;
    error: string;
    loading?: boolean;
    onClose: () => void;
    onSubmit: (event: SubmitEvent) => void;
    onLabelChange: (value: string) => void;
  } = $props();
</script>

<div class="modal-host">
  <div class="modal-backdrop" aria-hidden="true" transition:modalBackdrop></div>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="add-device-title" tabindex="-1" use:accessibleDialog={{ onClose }} transition:modalPanel>
    <div class="modal-header">
      <h3 id="add-device-title"><Plus size={14} /> {t.addDevice.title}</h3>
      <button class="modal-close" type="button" onclick={onClose} aria-label={t.addDevice.closeLabel} disabled={loading}>✕</button>
    </div>
    <form class="modal-body" onsubmit={onSubmit}>
      <label>
        <span>{t.addDevice.nameLabel}</span>
        <input data-dialog-autofocus value={label} oninput={(event) => onLabelChange((event.currentTarget as HTMLInputElement).value)} placeholder={t.addDevice.namePlaceholder} required autocomplete="off" disabled={loading} />
      </label>
      {#if error}<p class="modal-error" role="alert">{error}</p>{/if}
      <div class="modal-footer">
        <button class="ghost-button" type="button" onclick={onClose} disabled={loading}>{t.addDevice.cancel}</button>
        <button class="primary-button" type="submit" disabled={loading}>
          {#if loading}<LoaderCircle class="spin-icon" size={13} />{/if}
          {t.addDevice.save}
        </button>
      </div>
    </form>
  </div>
</div>
