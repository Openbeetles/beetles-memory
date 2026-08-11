<script lang="ts">
  import { Archive, BookOpen, LoaderCircle, Pencil, Search } from "lucide-svelte";
  import { ConsoleApiResponseError } from "../api";
  import SkillRetireModal from "../components/SkillRetireModal.svelte";
  import SkillEditorModal from "../components/SkillEditorModal.svelte";
  import StatusBadge from "../components/StatusBadge.svelte";
  import { editSkill, fetchSkill, retireSkill, setSkillEnabled } from "../lib/console-api";
  import type { ConsoleCopy } from "../lib/i18n";
  import type {
    ConsoleApiSkillDetail,
    ConsoleApiSkillList,
    ConsoleApiSkillSummary,
    SkillForm,
    SkillModal,
    SkillStatusFilter,
  } from "../lib/types";
  import {
    citationsText,
    filterSkills,
    skillQuality,
    statusLabel,
  } from "../lib/view-model";

  let {
    t,
    skillReport,
    skills,
    backendConnected,
    onRefresh,
    onBackendDisconnected,
  }: {
    t: ConsoleCopy;
    skillReport: ConsoleApiSkillList | null;
    skills: ConsoleApiSkillSummary[];
    backendConnected: boolean;
    onRefresh: () => Promise<void>;
    onBackendDisconnected: () => void;
  } = $props();

  let selectedSkillOwnerId: string | null = $state(null);
  let selectedSkill = $state<ConsoleApiSkillDetail | null>(null);
  let skillModal: SkillModal = $state(null);
  let skillForm: SkillForm = $state({ title: "", topic: "", summary: "", procedure: "", citations: "" });
  let skillError = $state("");
  let skillSearch = $state("");
  let skillStatusFilter: SkillStatusFilter = $state("all");
  let selectingSkillOwnerId: string | null = $state(null);
  let skillFormSubmitting = $state(false);
  let skillToggleBusyOwnerId: string | null = $state(null);
  let skillRetiring = $state(false);

  const selectedSkillSummary = $derived(skills.find((skill) => skill.ownerId === selectedSkillOwnerId) ?? selectedSkill?.summary ?? null);
  const filteredSkills = $derived(filterSkills(skills, skillSearch, skillStatusFilter));
  const skillOperationBusy = $derived(
    selectingSkillOwnerId !== null || skillFormSubmitting || skillToggleBusyOwnerId !== null || skillRetiring,
  );
  const skillFilters: SkillStatusFilter[] = ["all", "active", "disabled", "retired"];

  $effect(() => {
    if (selectedSkillOwnerId && !skills.some((skill) => skill.ownerId === selectedSkillOwnerId)) {
      selectedSkillOwnerId = null;
      selectedSkill = null;
    }
  });

  function resetSkillForm() {
    skillForm = { title: "", topic: "", summary: "", procedure: "", citations: "" };
    skillError = "";
  }

  function skillFilterCount(filter: SkillStatusFilter): number {
    if (filter === "all") return skillReport?.total ?? skills.length;
    if (filter === "active") return skillReport?.active ?? 0;
    if (filter === "disabled") return skillReport?.disabled ?? 0;
    return skills.filter((skill) => skill.status === "retired").length;
  }

  function selectSkillFilter(filter: SkillStatusFilter) {
    skillStatusFilter = filter;
  }

  function handleSkillFilterKeydown(event: KeyboardEvent, filter: SkillStatusFilter) {
    const currentIndex = skillFilters.indexOf(filter);
    let nextIndex: number | null = null;
    if (event.key === "ArrowRight") nextIndex = (currentIndex + 1) % skillFilters.length;
    else if (event.key === "ArrowLeft") nextIndex = (currentIndex - 1 + skillFilters.length) % skillFilters.length;
    else if (event.key === "Home") nextIndex = 0;
    else if (event.key === "End") nextIndex = skillFilters.length - 1;
    if (nextIndex === null) return;

    event.preventDefault();
    const nextFilter = skillFilters[nextIndex];
    selectSkillFilter(nextFilter);
    document.getElementById(`skill-filter-${nextFilter}`)?.focus();
  }

  function setSkillFormField(field: keyof SkillForm, value: string) {
    skillForm = { ...skillForm, [field]: value };
  }

  function openSkillEdit() {
    if (!selectedSkill || skillOperationBusy) return;
    skillForm = {
      title: selectedSkill.summary.title,
      topic: selectedSkill.summary.topic,
      summary: selectedSkill.summaryText,
      procedure: selectedSkill.procedureText,
      citations: citationsText(selectedSkill.citations),
    };
    skillError = "";
    skillModal = "edit";
  }

  function openSkillRetire() {
    if (!selectedSkillSummary || skillOperationBusy) return;
    skillError = "";
    skillModal = "retire";
  }

  function closeSkillModal() {
    if (skillFormSubmitting || skillRetiring) return;
    resetSkillModal();
  }

  function resetSkillModal() {
    skillModal = null;
    skillError = "";
  }

  async function selectSkill(skill: ConsoleApiSkillSummary) {
    if (selectingSkillOwnerId !== null) return;
    selectedSkillOwnerId = skill.ownerId;
    selectedSkill = null;
    if (!backendConnected) return;
    selectingSkillOwnerId = skill.ownerId;
    try {
      selectedSkill = await fetchSkill(skill.locator);
    } catch (error) {
      selectedSkill = null;
      if (!(error instanceof ConsoleApiResponseError)) onBackendDisconnected();
    } finally {
      if (selectingSkillOwnerId === skill.ownerId) selectingSkillOwnerId = null;
    }
  }

  async function submitSkillForm(e: SubmitEvent) {
    e.preventDefault();
    if (skillFormSubmitting) return;
    if (!backendConnected) {
      skillError = t.labels.backendOffline;
      return;
    }
    const title = skillForm.title.trim();
    const topic = skillForm.topic.trim();
    const summary = skillForm.summary.trim();
    const procedure = skillForm.procedure.trim();
    if (!title || !topic || !summary || !procedure) {
      skillError = t.skillsPanel.requiredFieldsError;
      return;
    }
    const locator = selectedSkill?.summary.locator;
    if (!locator) {
      skillError = t.skillsPanel.selectFirstError;
      return;
    }
    skillFormSubmitting = true;
    try {
      const mutation = await editSkill(locator, {
        title,
        topic,
        summary,
        procedure,
        editReason: "console_runtime_skill_edit",
      });
      resetSkillModal();
      await onRefresh();
      selectedSkillOwnerId = mutation.ownerId;
      selectedSkill = await fetchSkill(mutation.currentLocator);
    } catch (error) {
      skillError = error instanceof Error ? error.message : String(error);
      if (!(error instanceof ConsoleApiResponseError)) onBackendDisconnected();
    } finally {
      skillFormSubmitting = false;
    }
  }

  async function toggleSkillEnabled(skill: ConsoleApiSkillSummary) {
    if (!backendConnected || skillToggleBusyOwnerId !== null) return;
    skillToggleBusyOwnerId = skill.ownerId;
    try {
      const mutation = await setSkillEnabled(skill.locator, !skill.enabled);
      await onRefresh();
      if (selectedSkillOwnerId === skill.ownerId) {
        selectedSkill = await fetchSkill(mutation.currentLocator);
      }
    } catch (error) {
      skillError = error instanceof Error ? error.message : String(error);
      if (!(error instanceof ConsoleApiResponseError)) onBackendDisconnected();
    } finally {
      if (skillToggleBusyOwnerId === skill.ownerId) skillToggleBusyOwnerId = null;
    }
  }

  async function runSkillRetire(skill: ConsoleApiSkillSummary) {
    if (skillRetiring) return;
    if (!backendConnected) {
      skillError = t.labels.backendOffline;
      return;
    }
    skillRetiring = true;
    try {
      await retireSkill(skill.locator);
      selectedSkillOwnerId = null;
      selectedSkill = null;
      await onRefresh();
    } catch (error) {
      skillError = error instanceof Error ? error.message : String(error);
      if (!(error instanceof ConsoleApiResponseError)) onBackendDisconnected();
    } finally {
      skillRetiring = false;
    }
  }
</script>

<div class="skill-top panel">
  <div class="skill-filter-bar">
    <div class="skill-lifecycle" role="tablist" aria-label={t.skillsPanel.label}>
      {#each skillFilters as filter}
        <button
          id={`skill-filter-${filter}`}
          type="button"
          role="tab"
          aria-selected={skillStatusFilter === filter}
          aria-controls="skill-list-panel"
          tabindex={skillStatusFilter === filter ? 0 : -1}
          class:active={skillStatusFilter === filter}
          onclick={() => selectSkillFilter(filter)}
          onkeydown={(event) => handleSkillFilterKeydown(event, filter)}
        >
          <span>{t.skillsPanel[filter]}</span><strong>{skillFilterCount(filter)}</strong>
        </button>
      {/each}
    </div>
    <div class="skill-learned-chip" title={t.skillsPanel.runtimeLearned}>
      <span>{t.skillsPanel.runtimeLearned}</span>
      <strong>{skillReport?.runtimeLearned ?? 0}</strong>
    </div>
  </div>
  <div class="skill-toolbar">
    <div class="skill-search">
      <span class="skill-search-icon-wrap"><Search size={13} /></span>
      <input aria-label={t.skillsPanel.search} value={skillSearch} placeholder={t.skillsPanel.search} oninput={(event) => (skillSearch = (event.currentTarget as HTMLInputElement).value)} />
    </div>
  </div>
</div>

{#if skillError && skillModal === null}
  <p class="panel-action-error" role="alert">{skillError}</p>
{/if}

<div class="skill-layout">
  <div id="skill-list-panel" role="tabpanel" aria-labelledby={`skill-filter-${skillStatusFilter}`} class="panel skill-list-panel">
    <div class="skill-list">
      {#if filteredSkills.length === 0}
        <div class="skill-empty">{t.skillsPanel.empty}</div>
      {:else}
        {#each filteredSkills as skill}
          <button
            class:active={selectedSkillOwnerId === skill.ownerId}
            class="skill-row"
            type="button"
            disabled={selectingSkillOwnerId !== null}
            onclick={() => void selectSkill(skill)}
          >
            <span class="skill-row-main">
              <strong>{skill.title}</strong>
              <small>{skill.topic} · {skill.ownerId}</small>
            </span>
            <span class="skill-row-meta">
              {#if selectingSkillOwnerId === skill.ownerId}<LoaderCircle class="spin-icon" size={12} />{/if}
              <StatusBadge {t} status={skill.enabled ? skill.status : "disabled"} />
              <span>{t.skillsPanel.quality}: {skillQuality(skill)}</span>
              <span>{t.skillsPanel.uses}: {skill.useCount}</span>
            </span>
          </button>
        {/each}
      {/if}
    </div>
  </div>

  <article class="panel skill-detail-panel">
    {#if selectedSkill && selectedSkillSummary}
      <div class="panel-title">
        <h3>{selectedSkillSummary.title}</h3>
        <div class="panel-title-actions">
          <button class="ghost-button" type="button" onclick={() => void toggleSkillEnabled(selectedSkillSummary)} disabled={!backendConnected || skillOperationBusy}>
            {#if skillToggleBusyOwnerId === selectedSkillSummary.ownerId}<LoaderCircle class="spin-icon" size={13} />{/if}
            {selectedSkillSummary.enabled ? t.actions.disable : t.actions.enable}
          </button>
          <button class="ghost-button" type="button" onclick={openSkillEdit} disabled={skillOperationBusy}><Pencil size={13} /> {t.actions.edit}</button>
          <button class="ghost-button danger-button" type="button" onclick={openSkillRetire} disabled={skillOperationBusy}>
            {#if skillRetiring}<LoaderCircle class="spin-icon" size={13} />{:else}<Archive size={13} />{/if}
            {t.skillsPanel.retireTitle}
          </button>
        </div>
      </div>

      <div class="skill-meta-grid">
        <div><span>{t.skillsPanel.name}</span><strong>{selectedSkillSummary.ownerId}</strong></div>
        <div><span>{t.skillsPanel.topic}</span><strong>{selectedSkillSummary.topic}</strong></div>
        <div><span>{t.skillsPanel.quality}</span><strong>{skillQuality(selectedSkillSummary)}</strong></div>
        <div><span>{t.skillsPanel.uses}</span><strong>{selectedSkillSummary.useCount}</strong></div>
        <div><span>{t.skillsPanel.successes}</span><strong>{selectedSkillSummary.validatedSuccessCount}</strong></div>
        <div><span>{t.skillsPanel.mismatches}</span><strong>{selectedSkillSummary.mismatchCount}</strong></div>
        <div><span>{t.skillsPanel.revisionPending}</span><strong>{selectedSkillSummary.revisionPending ? t.workbenchPanel.yes : t.workbenchPanel.no}</strong></div>
        <div><span>{t.labels.status}</span><strong>{selectedSkillSummary.enabled ? statusLabel(t, selectedSkillSummary.status) : statusLabel(t, "disabled")}</strong></div>
      </div>

      <div class="skill-detail">
        <section>
          <h4>{t.skillsPanel.summary}</h4>
          <p>{selectedSkill.summaryText}</p>
        </section>
        <section>
          <h4>{t.skillsPanel.procedure}</h4>
          <pre>{selectedSkill.procedureText}</pre>
        </section>
        <section>
          <h4>{t.skillsPanel.citations}</h4>
          {#if selectedSkill.citations.length === 0}
            <p>-</p>
          {:else}
            <div class="chips">{#each selectedSkill.citations as citation}<span>{citation}</span>{/each}</div>
          {/if}
        </section>
        <section>
          <h4>{t.skillsPanel.lineage}</h4>
          {#if selectedSkill.lineage.length === 0}
            <p>-</p>
          {:else}
            <ul>{#each selectedSkill.lineage as item}<li>{item}</li>{/each}</ul>
          {/if}
        </section>
        <section>
          <h4>{t.skillsPanel.strategyDiffs}</h4>
          {#if selectedSkill.strategyDiffs.length === 0}
            <p>-</p>
          {:else}
            <ul>{#each selectedSkill.strategyDiffs as item}<li>{item}</li>{/each}</ul>
          {/if}
        </section>
      </div>
    {:else}
      <div class="skill-empty detail-empty">
        <BookOpen size={28} />
        <span>{t.skillsPanel.emptyDetail}</span>
      </div>
    {/if}
  </article>
</div>

{#if skillModal === "edit"}
  <SkillEditorModal
    {t}
    mode={skillModal}
    form={skillForm}
    error={skillError}
    loading={skillFormSubmitting}
    onClose={closeSkillModal}
    onSubmit={(event) => void submitSkillForm(event)}
    onFieldChange={setSkillFormField}
  />
{/if}

{#if skillModal === "retire" && selectedSkillSummary}
  <SkillRetireModal
    {t}
    skill={selectedSkillSummary}
    onClose={closeSkillModal}
    onRetire={() => void runSkillRetire(selectedSkillSummary)}
  />
{/if}
