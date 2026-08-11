<script lang="ts">
  import { AlertTriangle } from "lucide-svelte";
  import { flushSync } from "svelte";
  import { accessibleDialog } from "../lib/dialog-focus";

  let {
    title,
    description,
    subjectLabel,
    subject,
    meta,
    confirmLabel,
    cancelLabel,
    closeLabel,
    danger = true,
    success = false,
    onClose,
    onConfirm,
  }: {
    title: string;
    description: string;
    subjectLabel: string;
    subject: string;
    meta?: string;
    confirmLabel: string;
    cancelLabel: string;
    closeLabel: string;
    danger?: boolean;
    success?: boolean;
    onClose: () => void;
    onConfirm: () => void;
  } = $props();

  let closing = $state(false);

  function confirmAndClose() {
    if (closing) return;
    closing = true;
    onClose();
    flushSync();
    onConfirm();
  }
</script>

<div class="modal-host">
  <div class="modal-backdrop" aria-hidden="true"></div>
  <div class="modal confirm-modal" role="dialog" aria-modal="true" aria-labelledby="confirm-action-title" tabindex="-1" use:accessibleDialog={{ onClose }}>
    <div class="modal-header">
      <h3 id="confirm-action-title"><AlertTriangle size={14} /> {title}</h3>
      <button class="modal-close" type="button" onclick={onClose} aria-label={closeLabel}>✕</button>
    </div>
    <div class="modal-body confirm-body">
      <p>{description}</p>
      <div class="issued-key-meta">
        <span>{subjectLabel}</span>
        <strong>{subject}</strong>
        {#if meta}<small>{meta}</small>{/if}
      </div>
      <div class="modal-footer">
        <button data-dialog-autofocus class="ghost-button" type="button" onclick={onClose}>{cancelLabel}</button>
        <button class:danger-primary={danger} class:success-primary={success} class="primary-button" type="button" disabled={closing} onclick={confirmAndClose}>{confirmLabel}</button>
      </div>
    </div>
  </div>
</div>
