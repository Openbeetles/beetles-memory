<script lang="ts">
  import { KeyRound, LoaderCircle, ShieldAlert } from "lucide-svelte";
  import { accessibleDialog } from "../lib/dialog-focus";
  import type { ConsoleCopy } from "../lib/i18n";
  import { modalBackdrop, modalPanel } from "../lib/modal-transition";

  let {
    t,
    dialog,
    copied,
    loading = false,
    onClose,
    onCopy,
  }: {
    t: ConsoleCopy;
    dialog: { deviceId: string; label: string; appKey: string };
    copied: boolean;
    loading?: boolean;
    onClose: () => void;
    onCopy: () => void;
  } = $props();
</script>

<div class="modal-host">
  <div class="modal-backdrop" aria-hidden="true" transition:modalBackdrop></div>
  <div class="modal key-modal key-ceremony" role="dialog" aria-modal="true" aria-labelledby="issued-key-title" tabindex="-1" use:accessibleDialog={{ onClose }} transition:modalPanel>
    <div class="modal-header">
      <h3 id="issued-key-title"><KeyRound size={14} /> {t.addDevice.keyDialogTitle}</h3>
      <button class="modal-close" type="button" onclick={onClose} aria-label={t.addDevice.closeLabel} disabled={loading}>✕</button>
    </div>
    <div class="modal-body">
      <p class="key-ceremony-warn">
        <ShieldAlert size={14} style="display:inline;vertical-align:-2px;margin-right:6px" />
        {t.addDevice.keyCeremonyWarn}
      </p>
      <div class="issued-key-meta">
        <span>{t.addDevice.issuedKeyNotice}</span>
        <strong>{dialog.label}</strong>
        <small>{dialog.deviceId}</small>
      </div>
      <code class="issued-key-code">{dialog.appKey}</code>
      <p class="modal-hint">{t.addDevice.keyDialogDesc}</p>
      <div class="modal-footer">
        <button data-dialog-autofocus class="ghost-button" type="button" onclick={onCopy} disabled={loading}>
          {#if loading}<LoaderCircle class="spin-icon" size={13} />{/if}
          {copied ? t.addDevice.copied : t.addDevice.copyKey}
        </button>
        <button class="primary-button" class:success-primary={copied} type="button" onclick={onClose} disabled={loading}>
          {copied ? t.addDevice.acknowledgeClose : t.addDevice.closeLabel}
        </button>
      </div>
    </div>
  </div>
</div>
