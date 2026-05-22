<script lang="ts">
  import { BookOpen, Pencil, Plus, Search, Trash2, Upload } from "lucide-svelte";
  import SkillDeleteModal from "../components/SkillDeleteModal.svelte";
  import SkillEditorModal from "../components/SkillEditorModal.svelte";
  import { deleteSkill, fetchSkill, setSkillEnabled, upsertSkill } from "../lib/console-api";
  import type { ConsoleCopy } from "../lib/i18n";
  import type {
    ConsoleApiSkillDetail,
    ConsoleApiSkillList,
    ConsoleApiSkillSummary,
    Lang,
    SkillForm,
    SkillModal,
  } from "../lib/types";
  import {
    citationsText,
    filterSkills,
    parseCitations,
    skillKindLabel,
    skillOriginLabel,
    skillQuality,
    statusLabel,
  } from "../lib/view-model";

  let {
    t,
    lang,
    skillReport,
    skills,
    backendConnected,
    onRefresh,
    onBackendDisconnected,
  }: {
    t: ConsoleCopy;
    lang: Lang;
    skillReport: ConsoleApiSkillList | null;
    skills: ConsoleApiSkillSummary[];
    backendConnected: boolean;
    onRefresh: () => Promise<void>;
    onBackendDisconnected: () => void;
  } = $props();

  let selectedSkillName: string | null = $state(null);
  let selectedSkill: ConsoleApiSkillDetail | null = $state(null);
  let skillModal: SkillModal = $state(null);
  let skillForm: SkillForm = $state({ title: "", topic: "", summary: "", procedure: "", citations: "" });
  let skillError = $state("");
  let skillSearch = $state("");
  let skillStatusFilter: "all" | "active" | "disabled" | "retired" = $state("all");
  let skillOriginFilter: "all" | "user_provided" | "runtime_learned" = $state("all");

  const selectedSkillSummary = $derived(skills.find((skill) => skill.name === selectedSkillName) ?? selectedSkill?.summary ?? null);
  const filteredSkills = $derived(filterSkills(skills, skillSearch, skillStatusFilter, skillOriginFilter));

  $effect(() => {
    if (selectedSkillName && !skills.some((skill) => skill.name === selectedSkillName)) {
      selectedSkillName = null;
      selectedSkill = null;
    }
  });

  function resetSkillForm() {
    skillForm = { title: "", topic: "", summary: "", procedure: "", citations: "" };
    skillError = "";
  }

  function setSkillFormField(field: keyof SkillForm, value: string) {
    skillForm = { ...skillForm, [field]: value };
  }

  function openSkillCreate() {
    resetSkillForm();
    skillModal = "create";
  }

  function openSkillImport() {
    resetSkillForm();
    skillModal = "import";
  }

  function openSkillEdit() {
    if (!selectedSkill) return;
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

  function openSkillDelete() {
    if (!selectedSkillSummary) return;
    skillError = "";
    skillModal = "delete";
  }

  function closeSkillModal() {
    skillModal = null;
    skillError = "";
  }

  async function selectSkill(name: string) {
    selectedSkillName = name;
    if (!backendConnected) return;
    try {
      selectedSkill = await fetchSkill(name);
    } catch {
      selectedSkill = null;
      onBackendDisconnected();
    }
  }

  async function submitSkillForm(e: SubmitEvent) {
    e.preventDefault();
    if (!backendConnected) {
      skillError = t.labels.backendOffline;
      return;
    }
    const title = skillForm.title.trim();
    const topic = skillForm.topic.trim();
    const summary = skillForm.summary.trim();
    const procedure = skillForm.procedure.trim();
    if (!title || !topic || !summary || !procedure) {
      skillError = lang === "zh-CN" ? "标题、主题、摘要和过程都不能为空" : "Title, topic, summary, and procedure are required";
      return;
    }
    const name = skillModal === "edit" ? selectedSkill?.summary.name : undefined;
    try {
      const mutation = await upsertSkill(name, {
        title,
        topic,
        summary,
        procedure,
        citations: parseCitations(skillForm.citations),
      });
      closeSkillModal();
      await onRefresh();
      await selectSkill(mutation.name);
    } catch (error) {
      skillError = error instanceof Error ? error.message : String(error);
      onBackendDisconnected();
    }
  }

  async function toggleSkillEnabled(skill: ConsoleApiSkillSummary) {
    if (!backendConnected) return;
    try {
      await setSkillEnabled(skill.name, !skill.enabled);
      await onRefresh();
      if (selectedSkillName === skill.name) await selectSkill(skill.name);
    } catch {
      onBackendDisconnected();
    }
  }

  async function deleteSelectedSkill() {
    if (!selectedSkillSummary || !backendConnected) return;
    try {
      await deleteSkill(selectedSkillSummary.name);
      closeSkillModal();
      selectedSkillName = null;
      selectedSkill = null;
      await onRefresh();
    } catch (error) {
      skillError = error instanceof Error ? error.message : String(error);
      onBackendDisconnected();
    }
  }

  async function readSkillFile(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    const text = await file.text();
    skillForm = {
      ...skillForm,
      title: skillForm.title || file.name.replace(/\.[^.]+$/, ""),
      procedure: text,
      summary: skillForm.summary || text.trim().split(/\r?\n/).find(Boolean)?.slice(0, 180) || "",
    };
    input.value = "";
  }
</script>

<div class="skill-top panel">
  <div class="panel-title">
    <div>
      <p class="panel-label">{t.skillsPanel.label}</p>
      <h3>{t.skillsPanel.title}</h3>
    </div>
    <div class="panel-title-actions">
      <button class="ghost-button" type="button" onclick={openSkillImport}>
        <Upload size={13} /> {t.actions.importSkill}
      </button>
      <button class="primary-button" type="button" onclick={openSkillCreate}>
        <Plus size={13} /> {t.actions.createSkill}
      </button>
    </div>
  </div>
  <div class="skill-stats">
    <div><span>{t.skillsPanel.total}</span><strong>{skillReport?.total ?? 0}</strong></div>
    <div><span>{t.skillsPanel.active}</span><strong>{skillReport?.active ?? 0}</strong></div>
    <div><span>{t.skillsPanel.disabled}</span><strong>{skillReport?.disabled ?? 0}</strong></div>
    <div><span>{t.skillsPanel.runtimeLearned}</span><strong>{skillReport?.runtimeLearned ?? 0}</strong></div>
    <div><span>{t.skillsPanel.userProvided}</span><strong>{skillReport?.userProvided ?? 0}</strong></div>
  </div>
  <div class="skill-toolbar">
    <div class="skill-search">
      <span class="skill-search-icon-wrap"><Search size={13} /></span>
      <input value={skillSearch} placeholder={t.skillsPanel.search} oninput={(event) => (skillSearch = (event.currentTarget as HTMLInputElement).value)} />
    </div>
    <div class="skill-filters">
      <select value={skillStatusFilter} onchange={(event) => (skillStatusFilter = (event.currentTarget as HTMLSelectElement).value as typeof skillStatusFilter)}>
        <option value="all">{t.skillsPanel.all}</option>
        <option value="active">{t.skillsPanel.active}</option>
        <option value="disabled">{t.skillsPanel.disabled}</option>
        <option value="retired">{t.skillsPanel.retired}</option>
      </select>
      <select value={skillOriginFilter} onchange={(event) => (skillOriginFilter = (event.currentTarget as HTMLSelectElement).value as typeof skillOriginFilter)}>
        <option value="all">{t.skillsPanel.all}</option>
        <option value="user_provided">{t.skillsPanel.userProvided}</option>
        <option value="runtime_learned">{t.skillsPanel.runtimeLearned}</option>
      </select>
    </div>
  </div>
</div>

<div class="skill-layout">
  <article class="panel skill-list-panel">
    <div class="skill-list">
      {#if filteredSkills.length === 0}
        <div class="skill-empty">{t.skillsPanel.empty}</div>
      {:else}
        {#each filteredSkills as skill}
          <button
            class:active={selectedSkillName === skill.name}
            class="skill-row"
            type="button"
            onclick={() => void selectSkill(skill.name)}
          >
            <span class="skill-row-main">
              <strong>{skill.title}</strong>
              <small>{skill.topic} · {skill.name}</small>
            </span>
            <span class="skill-row-meta">
              <span class={`badge ${skill.enabled ? skill.status : "disabled"}`}>{skill.enabled ? statusLabel(t, skill.status) : statusLabel(t, "disabled")}</span>
              <span>{skillOriginLabel(t, skill.origin)}</span>
              <span>{t.skillsPanel.quality}: {skillQuality(skill)}</span>
              <span>{t.skillsPanel.uses}: {skill.useCount}</span>
            </span>
          </button>
        {/each}
      {/if}
    </div>
  </article>

  <article class="panel skill-detail-panel">
    {#if selectedSkill && selectedSkillSummary}
      <div class="panel-title">
        <div>
          <p class="panel-label">{skillOriginLabel(t, selectedSkillSummary.origin)} · {skillKindLabel(selectedSkillSummary.kind, lang)}</p>
          <h3>{selectedSkillSummary.title}</h3>
        </div>
        <div class="panel-title-actions">
          <button class="ghost-button" type="button" onclick={() => void toggleSkillEnabled(selectedSkillSummary)} disabled={!backendConnected}>
            {selectedSkillSummary.enabled ? t.actions.disable : t.actions.enable}
          </button>
          <button class="ghost-button" type="button" onclick={openSkillEdit}><Pencil size={13} /> {t.actions.edit}</button>
          <button class="ghost-button danger-button" type="button" onclick={openSkillDelete}><Trash2 size={13} /> {t.actions.delete}</button>
        </div>
      </div>

      <div class="skill-meta-grid">
        <div><span>{t.skillsPanel.name}</span><strong>{selectedSkillSummary.name}</strong></div>
        <div><span>{t.skillsPanel.topic}</span><strong>{selectedSkillSummary.topic}</strong></div>
        <div><span>{t.skillsPanel.quality}</span><strong>{skillQuality(selectedSkillSummary)}</strong></div>
        <div><span>{t.skillsPanel.uses}</span><strong>{selectedSkillSummary.useCount}</strong></div>
        <div><span>{t.skillsPanel.successes}</span><strong>{selectedSkillSummary.validatedSuccessCount}</strong></div>
        <div><span>{t.skillsPanel.mismatches}</span><strong>{selectedSkillSummary.mismatchCount}</strong></div>
        <div><span>{t.skillsPanel.revisionPending}</span><strong>{selectedSkillSummary.revisionPending ? "YES" : "NO"}</strong></div>
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

{#if skillModal === "create" || skillModal === "import" || skillModal === "edit"}
  <SkillEditorModal
    {t}
    mode={skillModal}
    form={skillForm}
    error={skillError}
    onClose={closeSkillModal}
    onSubmit={(event) => void submitSkillForm(event)}
    onReadFile={(event) => void readSkillFile(event)}
    onFieldChange={setSkillFormField}
  />
{/if}

{#if skillModal === "delete" && selectedSkillSummary}
  <SkillDeleteModal
    {t}
    skill={selectedSkillSummary}
    error={skillError}
    onClose={closeSkillModal}
    onDelete={() => void deleteSelectedSkill()}
  />
{/if}
