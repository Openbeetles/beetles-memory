<script lang="ts">
  import { KeyRound } from "lucide-svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import { modalBackdrop, modalPanel } from "../lib/modal-transition";

  let {
    t,
    dialog,
    copied,
    onClose,
    onCopy,
  }: {
    t: ConsoleCopy;
    dialog: { deviceId: string; label: string; appKey: string };
    copied: boolean;
    onClose: () => void;
    onCopy: () => void;
  } = $props();
</script>

<div class="modal-backdrop" aria-hidden="true" transition:modalBackdrop></div>
<div class="modal key-modal" role="dialog" aria-modal="true" aria-labelledby="issued-key-title" transition:modalPanel>
  <div class="modal-header">
    <h3 id="issued-key-title"><KeyRound size={14} /> {t.addDevice.keyDialogTitle}</h3>
    <button class="modal-close" type="button" onclick={onClose} aria-label={t.addDevice.closeLabel}>✕</button>
  </div>
  <div class="modal-body">
    <div class="issued-key-meta">
      <span>{t.addDevice.issuedKeyNotice}</span>
      <strong>{dialog.label}</strong>
      <small>{dialog.deviceId}</small>
    </div>
    <code class="issued-key-code">{dialog.appKey}</code>
    <p class="modal-hint">{t.addDevice.keyDialogDesc}</p>
    <div class="modal-footer">
      <button class="ghost-button" type="button" onclick={onCopy}>{copied ? t.addDevice.copied : t.addDevice.copyKey}</button>
      <button class="primary-button" type="button" onclick={onClose}>{t.addDevice.closeLabel}</button>
    </div>
  </div>
</div>
