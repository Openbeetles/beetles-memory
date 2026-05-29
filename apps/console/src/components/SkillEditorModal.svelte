<script lang="ts">
  import { FileText, LoaderCircle, Pencil } from "lucide-svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import { modalBackdrop, modalPanel } from "../lib/modal-transition";
  import type { SkillForm, SkillModal } from "../lib/types";

  type SkillEditorMode = Exclude<SkillModal, "delete" | null>;

  let {
    t,
    mode,
    form,
    error,
    loading = false,
    onClose,
    onSubmit,
    onFieldChange,
  }: {
    t: ConsoleCopy;
    mode: SkillEditorMode;
    form: SkillForm;
    error: string;
    loading?: boolean;
    onClose: () => void;
    onSubmit: (event: SubmitEvent) => void;
    onFieldChange: (field: keyof SkillForm, value: string) => void;
  } = $props();
</script>

<div class="modal-backdrop" aria-hidden="true" transition:modalBackdrop></div>
<div class="modal skill-editor-modal" role="dialog" aria-modal="true" aria-labelledby="skill-editor-title" transition:modalPanel>
  <div class="modal-header">
    <h3 id="skill-editor-title">
      <Pencil size={14} />
      {t.skillsPanel.modalTitle[mode]}
    </h3>
    <button class="modal-close" type="button" onclick={onClose} aria-label={t.addDevice.closeLabel} disabled={loading}>✕</button>
  </div>
  <form class="modal-body" onsubmit={onSubmit}>
    <div class="skill-form-grid">
      <label>
        <span>{t.skillsPanel.titleLabel}</span>
        <input value={form.title} oninput={(event) => onFieldChange("title", (event.currentTarget as HTMLInputElement).value)} required autocomplete="off" disabled={loading} />
      </label>
      <label>
        <span>{t.skillsPanel.topic}</span>
        <input value={form.topic} oninput={(event) => onFieldChange("topic", (event.currentTarget as HTMLInputElement).value)} required autocomplete="off" disabled={loading} />
      </label>
    </div>
    <label>
      <span>{t.skillsPanel.summary}</span>
      <textarea value={form.summary} oninput={(event) => onFieldChange("summary", (event.currentTarget as HTMLTextAreaElement).value)} rows="3" required disabled={loading}></textarea>
    </label>
    <label>
      <span>{t.skillsPanel.procedure}</span>
      <textarea value={form.procedure} oninput={(event) => onFieldChange("procedure", (event.currentTarget as HTMLTextAreaElement).value)} rows="8" required disabled={loading}></textarea>
    </label>
    <label>
      <span>{t.skillsPanel.citationsInput}</span>
      <textarea value={form.citations} oninput={(event) => onFieldChange("citations", (event.currentTarget as HTMLTextAreaElement).value)} rows="3" disabled={loading}></textarea>
    </label>
    {#if error}<p class="modal-error">{error}</p>{/if}
    <div class="modal-footer">
      <button class="ghost-button" type="button" onclick={onClose} disabled={loading}>{t.actions.cancel}</button>
      <button class="primary-button" type="submit" disabled={loading}>
        {#if loading}<LoaderCircle class="spin-icon" size={13} />{:else}<FileText size={13} />{/if}
        {t.actions.save}
      </button>
    </div>
  </form>
</div>
