<script lang="ts">
  import { BrainCircuit, CircleCheck, LoaderCircle, PlugZap } from "lucide-svelte";
  import PanelHeader from "../components/PanelHeader.svelte";
  import StatusBadge from "../components/StatusBadge.svelte";
  import { saveGovernanceModelConfig, testGovernanceModelConnection } from "../lib/console-api";
  import type { ConsoleCopy } from "../lib/i18n";
  import type {
    ConsoleApiGovernanceModel,
    GovernanceModelConfigInput,
    GovernanceModelProtocol,
  } from "../lib/types";

  let {
    t,
    governanceModel,
    backendConnected,
    onUpdated,
    onBackendDisconnected,
  }: {
    t: ConsoleCopy;
    governanceModel: ConsoleApiGovernanceModel | null;
    backendConnected: boolean;
    onUpdated: (binding: ConsoleApiGovernanceModel) => void;
    onBackendDisconnected: () => void;
  } = $props();

  let enabled = $state(true);
  let protocol: GovernanceModelProtocol = $state("open_ai_compatible");
  let endpoint = $state("http://127.0.0.1:11434/v1");
  let model = $state("");
  let credentialEnv = $state("");
  let requestTimeoutMs = $state(30_000);
  let maxInputTokens = $state(8_192);
  let maxOutputTokens = $state(1_024);
  let saving = $state(false);
  let testing = $state(false);
  let message = $state("");
  let messageTone: "ready" | "blocked" | "limited" = $state("limited");
  let loadedRevision: number | null | undefined = undefined;

  $effect(() => {
    const revision = governanceModel?.configRevision ?? null;
    if (loadedRevision === revision || saving) return;
    loadedRevision = revision;
    if (!governanceModel?.configured) return;
    enabled = governanceModel.enabled;
    protocol = governanceModel.protocol ?? "open_ai_compatible";
    endpoint = governanceModel.endpoint ?? defaultEndpoint(protocol);
    model = governanceModel.model ?? "";
    credentialEnv = governanceModel.credentialEnv ?? "";
    requestTimeoutMs = governanceModel.requestTimeoutMs ?? 30_000;
    maxInputTokens = governanceModel.maxInputTokens ?? 8_192;
    maxOutputTokens = governanceModel.maxOutputTokens ?? 1_024;
  });

  function defaultEndpoint(next: GovernanceModelProtocol): string {
    return next === "ollama_native" ? "http://127.0.0.1:11434/api" : "http://127.0.0.1:11434/v1";
  }

  function selectProtocol(next: GovernanceModelProtocol) {
    const previousDefault = defaultEndpoint(protocol);
    protocol = next;
    if (!endpoint.trim() || endpoint === previousDefault) endpoint = defaultEndpoint(next);
    if (next === "ollama_native" && !governanceModel?.credentialConfigured) credentialEnv = "";
    message = "";
  }

  function input(): GovernanceModelConfigInput {
    return {
      enabled,
      protocol,
      endpoint: endpoint.trim(),
      model: model.trim(),
      authMode: credentialEnv.trim()
        ? { kind: "credential_env", credentialEnv: credentialEnv.trim() }
        : { kind: "local_unauthenticated" },
      requestTimeoutMs,
      maxInputTokens,
      maxOutputTokens,
    };
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!backendConnected || saving || testing) return;
    saving = true;
    message = "";
    try {
      const saved = await saveGovernanceModelConfig(input());
      onUpdated(saved);
      loadedRevision = saved.configRevision;
      message = t.governanceModel.saved;
      messageTone = "ready";
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
      messageTone = "blocked";
      if (message.startsWith("5") || message.includes("Failed to fetch")) onBackendDisconnected();
    } finally {
      saving = false;
    }
  }

  async function testConnection() {
    if (!backendConnected || !governanceModel?.configured || saving || testing) return;
    testing = true;
    message = "";
    try {
      const result = await testGovernanceModelConnection();
      message = t.governanceModel.testReady(result.durationMs);
      messageTone = "ready";
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
      messageTone = "blocked";
      if (message.startsWith("5") || message.includes("Failed to fetch")) onBackendDisconnected();
    } finally {
      testing = false;
    }
  }
</script>

<div class="model-config-layout">
  <section class="panel model-config-panel">
    <PanelHeader title={t.governanceModel.title} icon={BrainCircuit} />

    <form class="model-config-form" onsubmit={save}>
      <div class="model-config-switch-row">
        <button
          aria-label={t.governanceModel.enabled}
          aria-pressed={enabled}
          class:enabled
          class="switch"
          type="button"
          disabled={!backendConnected || saving || testing}
          onclick={() => (enabled = !enabled)}
        ><span></span></button>
        <strong>{t.governanceModel.enabled}</strong>
        <StatusBadge t={t} status={governanceModel?.configured ? (enabled ? "ready" : "disabled") : "draft"} />
      </div>

      <fieldset class="model-protocol-picker">
        <legend>{t.governanceModel.protocol}</legend>
        <button
          type="button"
          class:active={protocol === "open_ai_compatible"}
          aria-pressed={protocol === "open_ai_compatible"}
          onclick={() => selectProtocol("open_ai_compatible")}
        >{t.governanceModel.protocolOpenAi}</button>
        <button
          type="button"
          class:active={protocol === "ollama_native"}
          aria-pressed={protocol === "ollama_native"}
          onclick={() => selectProtocol("ollama_native")}
        >{t.governanceModel.protocolOllama}</button>
      </fieldset>

      <div class="model-config-fields">
        <label class="wide">
          <span>{t.governanceModel.endpoint}</span>
          <input bind:value={endpoint} required autocomplete="url" spellcheck="false" />
        </label>
        <label>
          <span>{t.governanceModel.model}</span>
          <input bind:value={model} required autocomplete="off" spellcheck="false" />
        </label>
        <label>
          <span>{t.governanceModel.credentialEnv}</span>
          <input bind:value={credentialEnv} autocomplete="off" spellcheck="false" placeholder={protocol === "ollama_native" ? "" : "OPENAI_API_KEY"} />
        </label>
        <label>
          <span>{t.governanceModel.timeout}</span>
          <input bind:value={requestTimeoutMs} type="number" min="1000" max="600000" step="1000" />
        </label>
        <label>
          <span>{t.governanceModel.maxInput}</span>
          <input bind:value={maxInputTokens} type="number" min="1" />
        </label>
        <label>
          <span>{t.governanceModel.maxOutput}</span>
          <input bind:value={maxOutputTokens} type="number" min="1" />
        </label>
      </div>

      {#if !backendConnected}
        <p class="model-config-feedback blocked" role="alert">{t.governanceModel.offline}</p>
      {:else if message}
        <p class="model-config-feedback {messageTone}" role="status">
          {#if messageTone === "ready"}<CircleCheck size={14} />{/if}{message}
        </p>
      {/if}

      <div class="model-config-actions">
        <button class="ghost-button" type="button" disabled={!backendConnected || !governanceModel?.configured || saving || testing} onclick={testConnection}>
          {#if testing}<LoaderCircle class="spin-icon" size={14} />{:else}<PlugZap size={14} />{/if}
          {testing ? t.governanceModel.testing : t.governanceModel.test}
        </button>
        <button class="primary-button" type="submit" disabled={!backendConnected || saving || testing || !model.trim() || !endpoint.trim()}>
          {#if saving}<LoaderCircle class="spin-icon" size={14} />{/if}
          {saving ? t.governanceModel.saving : t.governanceModel.save}
        </button>
      </div>
    </form>
  </section>
</div>
