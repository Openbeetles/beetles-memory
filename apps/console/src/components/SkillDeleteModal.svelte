<script lang="ts">
  import { Trash2 } from "lucide-svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { ConsoleApiSkillSummary } from "../lib/types";

  let {
    t,
    skill,
    error,
    onClose,
    onDelete,
  }: {
    t: ConsoleCopy;
    skill: ConsoleApiSkillSummary;
    error: string;
    onClose: () => void;
    onDelete: () => void;
  } = $props();
</script>

<button class="modal-backdrop" type="button" onclick={onClose} aria-label={t.addDevice.closeLabel}></button>
<div class="modal" role="dialog" aria-modal="true" aria-labelledby="skill-delete-title">
  <div class="modal-header">
    <h3 id="skill-delete-title"><Trash2 size={14} /> {t.skillsPanel.deleteTitle}</h3>
    <button class="modal-close" type="button" onclick={onClose} aria-label={t.addDevice.closeLabel}>✕</button>
  </div>
  <div class="modal-body">
    <div class="issued-key-meta">
      <span>{t.skillsPanel.deleteDesc}</span>
      <strong>{skill.title}</strong>
      <small>{skill.name}</small>
    </div>
    {#if error}<p class="modal-error">{error}</p>{/if}
    <div class="modal-footer">
      <button class="ghost-button" type="button" onclick={onClose}>{t.actions.cancel}</button>
      <button class="primary-button danger-primary" type="button" onclick={onDelete}>{t.actions.delete}</button>
    </div>
  </div>
</div>
