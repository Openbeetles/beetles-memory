<script lang="ts">
  import { FileText, LoaderCircle, Pencil, Plus, Upload } from "lucide-svelte";
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
    readFileLoading = false,
    onClose,
    onSubmit,
    onReadFile,
    onFieldChange,
  }: {
    t: ConsoleCopy;
    mode: SkillEditorMode;
    form: SkillForm;
    error: string;
    loading?: boolean;
    readFileLoading?: boolean;
    onClose: () => void;
    onSubmit: (event: SubmitEvent) => void;
    onReadFile: (event: Event) => void;
    onFieldChange: (field: keyof SkillForm, value: string) => void;
  } = $props();
</script>

<div class="modal-backdrop" aria-hidden="true" transition:modalBackdrop></div>
<div class="modal skill-editor-modal" role="dialog" aria-modal="true" aria-labelledby="skill-editor-title" transition:modalPanel>
  <div class="modal-header">
    <h3 id="skill-editor-title">
      {#if mode === "import"}<Upload size={14} />{:else if mode === "edit"}<Pencil size={14} />{:else}<Plus size={14} />{/if}
      {t.skillsPanel.modalTitle[mode]}
    </h3>
    <button class="modal-close" type="button" onclick={onClose} aria-label={t.addDevice.closeLabel} disabled={loading || readFileLoading}>✕</button>
  </div>
  <form class="modal-body" onsubmit={onSubmit}>
    <div class="skill-form-grid">
      <label>
        <span>{t.skillsPanel.titleLabel}</span>
        <input value={form.title} oninput={(event) => onFieldChange("title", (event.currentTarget as HTMLInputElement).value)} required autocomplete="off" disabled={loading || readFileLoading} />
      </label>
      <label>
        <span>{t.skillsPanel.topic}</span>
        <input value={form.topic} oninput={(event) => onFieldChange("topic", (event.currentTarget as HTMLInputElement).value)} required autocomplete="off" disabled={loading || readFileLoading} />
      </label>
    </div>
    <label>
      <span>{t.skillsPanel.summary}</span>
      <textarea value={form.summary} oninput={(event) => onFieldChange("summary", (event.currentTarget as HTMLTextAreaElement).value)} rows="3" required disabled={loading || readFileLoading}></textarea>
    </label>
    <label>
      <span>{t.skillsPanel.procedure}</span>
      <textarea value={form.procedure} oninput={(event) => onFieldChange("procedure", (event.currentTarget as HTMLTextAreaElement).value)} rows="8" required disabled={loading || readFileLoading}></textarea>
    </label>
    {#if mode === "import"}
      <label class="file-reader">
        <span>{t.skillsPanel.file}{#if readFileLoading}<LoaderCircle class="spin-icon inline-loading-icon" size={12} />{/if}</span>
        <input type="file" accept=".md,.txt,text/plain,text/markdown" onchange={onReadFile} disabled={loading || readFileLoading} />
      </label>
    {/if}
    <label>
      <span>{t.skillsPanel.citationsInput}</span>
      <textarea value={form.citations} oninput={(event) => onFieldChange("citations", (event.currentTarget as HTMLTextAreaElement).value)} rows="3" disabled={loading || readFileLoading}></textarea>
    </label>
    {#if error}<p class="modal-error">{error}</p>{/if}
    <div class="modal-footer">
      <button class="ghost-button" type="button" onclick={onClose} disabled={loading || readFileLoading}>{t.actions.cancel}</button>
      <button class="primary-button" type="submit" disabled={loading || readFileLoading}>
        {#if loading}<LoaderCircle class="spin-icon" size={13} />{:else}<FileText size={13} />{/if}
        {t.actions.save}
      </button>
    </div>
  </form>
</div>
