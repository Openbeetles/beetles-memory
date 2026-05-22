<script lang="ts">
  import { AlertTriangle } from "lucide-svelte";
  import { modalBackdrop, modalPanel } from "../lib/modal-transition";

  let {
    title,
    description,
    subjectLabel,
    subject,
    meta,
    confirmLabel,
    cancelLabel,
    closeLabel,
    error = "",
    danger = true,
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
    error?: string;
    danger?: boolean;
    onClose: () => void;
    onConfirm: () => void;
  } = $props();
</script>

<div class="modal-backdrop" aria-hidden="true" transition:modalBackdrop></div>
<div class="modal confirm-modal" role="dialog" aria-modal="true" aria-labelledby="confirm-action-title" transition:modalPanel>
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
    {#if error}<p class="modal-error">{error}</p>{/if}
    <div class="modal-footer">
      <button class="ghost-button" type="button" onclick={onClose}>{cancelLabel}</button>
      <button class:danger-primary={danger} class="primary-button" type="button" onclick={onConfirm}>{confirmLabel}</button>
    </div>
  </div>
</div>
