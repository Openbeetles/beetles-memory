<script lang="ts">
  import { Trash2 } from "lucide-svelte";
  import { flushSync } from "svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { ConsoleApiSkillSummary } from "../lib/types";

  let {
    t,
    skill,
    onClose,
    onDelete,
  }: {
    t: ConsoleCopy;
    skill: ConsoleApiSkillSummary;
    onClose: () => void;
    onDelete: () => void;
  } = $props();

  let closing = $state(false);

  function deleteAndClose() {
    if (closing) return;
    closing = true;
    onClose();
    flushSync();
    onDelete();
  }
</script>

<div class="modal-backdrop" aria-hidden="true"></div>
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
    <div class="modal-footer">
      <button class="ghost-button" type="button" onclick={onClose}>{t.actions.cancel}</button>
      <button class="primary-button danger-primary" type="button" disabled={closing} onclick={deleteAndClose}>{t.actions.delete}</button>
    </div>
  </div>
</div>
