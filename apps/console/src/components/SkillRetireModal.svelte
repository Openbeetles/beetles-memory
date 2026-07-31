<script lang="ts">
  import { Archive } from "lucide-svelte";
  import { flushSync } from "svelte";
  import type { ConsoleCopy } from "../lib/i18n";
  import type { ConsoleApiSkillSummary } from "../lib/types";

  let {
    t,
    skill,
    onClose,
    onRetire,
  }: {
    t: ConsoleCopy;
    skill: ConsoleApiSkillSummary;
    onClose: () => void;
    onRetire: () => void;
  } = $props();

  let closing = $state(false);

  function retireAndClose() {
    if (closing) return;
    closing = true;
    onClose();
    flushSync();
    onRetire();
  }
</script>

<div class="modal-backdrop" aria-hidden="true"></div>
<div class="modal" role="dialog" aria-modal="true" aria-labelledby="skill-retire-title">
  <div class="modal-header">
    <h3 id="skill-retire-title"><Archive size={14} /> {t.skillsPanel.retireTitle}</h3>
    <button class="modal-close" type="button" onclick={onClose} aria-label={t.addDevice.closeLabel}>✕</button>
  </div>
  <div class="modal-body">
    <div class="issued-key-meta">
      <span>{t.skillsPanel.retireDesc}</span>
      <strong>{skill.title}</strong>
      <small>{skill.ownerId}</small>
    </div>
    <div class="modal-footer">
      <button class="ghost-button" type="button" onclick={onClose}>{t.actions.cancel}</button>
      <button class="primary-button danger-primary" type="button" disabled={closing} onclick={retireAndClose}>{t.skillsPanel.retireTitle}</button>
    </div>
  </div>
</div>
