<script lang="ts">
  import { onMount } from "svelte";
  import AddDeviceModal from "./components/AddDeviceModal.svelte";
  import ConfirmActionModal from "./components/ConfirmActionModal.svelte";
  import IssuedKeyModal from "./components/IssuedKeyModal.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import StatusBar from "./components/StatusBar.svelte";
  import Topbar from "./components/Topbar.svelte";
  import {
    createDevice,
    loadConsoleSnapshot,
    rotateDeviceKey,
    updateDeviceStatus,
    updateTransportConfig,
  } from "./lib/console-api";
  import { copy, readLang, readTheme, STORAGE_KEYS, writeStorage } from "./lib/i18n";
  import type {
    ConsoleApiCapabilities,
    ConsoleApiDevice,
    ConsoleApiLlmGateway,
    ConsoleApiOllamaTransparentStatus,
    ConsoleApiOverview,
    ConsoleApiSession,
    ConsoleApiSkillList,
    ConsoleApiSkillSummary,
    Device,
    DeviceConfirmAction,
    Lang,
    PageId,
    Theme,
    Transport,
    TransportId,
  } from "./lib/types";
  import {
    accountRows,
    createOverviewCards,
    displaySystemInfo,
    fromApiDevice,
    localizedEvents,
    localizedKernelRows,
    localizedMemoryContextRows,
    mapDev,
    mapId,
    pagesWithCounts,
    systemInfoSpecRows,
    transportStatRows,
  } from "./lib/view-model";
  import AccountPage from "./pages/AccountPage.svelte";
  import DevicesPage from "./pages/DevicesPage.svelte";
  import LlmGatewayPage from "./pages/LlmGatewayPage.svelte";
  import OverviewPage from "./pages/OverviewPage.svelte";
  import SkillMemoryPage from "./pages/SkillMemoryPage.svelte";
  import TransportsPage from "./pages/TransportsPage.svelte";

  const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const isMacOS   = isTauri && typeof navigator !== "undefined" && navigator.userAgent.includes("Mac OS X");
  const isWindows = isTauri && typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
  type DeviceBusyAction = DeviceConfirmAction | "enable";

  let activePage: PageId = $state("overview");
  let theme: Theme = $state(readTheme());
  let lang: Lang = $state(readLang());
  let backendConnected = $state(false);
  let consoleLoading = $state(false);
  let consoleLoadSeq = 0;

  let overviewData: ConsoleApiOverview | null = $state(null);
  let consoleCapabilities: ConsoleApiCapabilities | null = $state(null);
  let llmGateway: ConsoleApiLlmGateway | null = $state(null);
  let ollamaTransparent: ConsoleApiOllamaTransparentStatus | null = $state(null);
  let sessionData: ConsoleApiSession | null = $state(null);
  let transports: Transport[] = $state([]);
  let devices: Device[] = $state([]);

  let skillReport: ConsoleApiSkillList | null = $state(null);
  let skills: ConsoleApiSkillSummary[] = $state([]);

  let addDeviceOpen = $state(false);
  let addDeviceSaving = $state(false);
  let newDevice = $state({ label: "" });
  let formError = $state("");
  let issuedKeyDialog: { deviceId: string; label: string; appKey: string } | null = $state(null);
  let issuedKeyCopied = $state(false);
  let issuedKeyCopying = $state(false);
  let deviceConfirm: { action: DeviceConfirmAction; device: Device } | null = $state(null);
  let deviceActionError = $state("");
  let deviceBusy: { deviceId: string; action: DeviceBusyAction } | null = $state(null);
  let transportBusy: { id: TransportId; action: "toggle" | "save" } | null = $state(null);

  const t = $derived(copy[lang]);
  const systemInfo = $derived(displaySystemInfo(overviewData?.systemInfo, lang));
  const enabledTransportCount = $derived(transports.filter((transport) => transport.enabled).length);
  const activeDeviceCount = $derived(devices.filter((device) => device.status !== "disabled").length);
  const skillCount = $derived(skillReport?.total ?? skills.length);
  const pages = $derived(pagesWithCounts(t, skillCount, transports.length, devices.length));
  const currentPage = $derived(pages.find((page) => page.id === activePage) ?? pages[0]);
  const overviewCards = $derived(createOverviewCards(t, overviewData, systemInfo, lang));
  const transportStats = $derived(transportStatRows(t, enabledTransportCount, transports.length, activeDeviceCount, devices.length));
  const accountFields = $derived(accountRows(t, sessionData, lang));
  const recentEvents = $derived(localizedEvents(overviewData?.recentEvents, t.recentEvents, lang));
  const kernelRows = $derived(localizedKernelRows(overviewData?.kernel, t.kernel.rows, lang));
  const memoryContextRows = $derived(localizedMemoryContextRows(overviewData?.memoryContext, lang));
  const systemInfoSpecs = $derived(systemInfoSpecRows(systemInfo, lang));

  $effect(() => writeStorage(STORAGE_KEYS.theme, theme));
  $effect(() => writeStorage(STORAGE_KEYS.lang, lang));

  onMount(() => {
    void loadConsoleData();
  });

  function setActivePage(page: PageId) {
    activePage = page;
  }

  function setTheme(nextTheme: Theme) {
    theme = nextTheme;
  }

  function setLang(nextLang: Lang) {
    lang = nextLang;
  }

  async function loadConsoleData() {
    const loadId = ++consoleLoadSeq;
    consoleLoading = true;
    try {
      const snapshot = await loadConsoleSnapshot();
      consoleCapabilities = snapshot.capabilities;
      overviewData = snapshot.overview;
      llmGateway = snapshot.llmGateway;
      ollamaTransparent = snapshot.ollamaTransparent;
      skillReport = snapshot.skills;
      skills = snapshot.skills.skills;
      transports = snapshot.transports;
      devices = snapshot.devices;
      sessionData = snapshot.session;
      backendConnected = true;
    } catch {
      overviewData = null;
      consoleCapabilities = null;
      llmGateway = null;
      ollamaTransparent = null;
      skillReport = null;
      skills = [];
      transports = [];
      devices = [];
      sessionData = null;
      backendConnected = false;
    } finally {
      if (consoleLoadSeq === loadId) {
        consoleLoading = false;
      }
    }
  }

  function openAddDevice() {
    if (addDeviceSaving) return;
    newDevice = { label: "" };
    formError = "";
    addDeviceOpen = true;
  }

  function closeAddDeviceModal() {
    if (addDeviceSaving) return;
    addDeviceOpen = false;
  }

  function setNewDeviceLabel(label: string) {
    newDevice = { label };
  }

  function showIssuedKey(device: ConsoleApiDevice, appKey: string) {
    issuedKeyCopied = false;
    issuedKeyDialog = {
      deviceId: device.deviceId,
      label: device.label,
      appKey,
    };
  }

  function closeIssuedKeyDialog() {
    if (issuedKeyCopying) return;
    issuedKeyDialog = null;
    issuedKeyCopied = false;
  }

  function deviceName(device: Device): string {
    return device.label?.trim() || device.deviceId;
  }

  function openDeviceConfirm(action: DeviceConfirmAction, deviceId: string) {
    const device = devices.find((item) => item.deviceId === deviceId);
    if (!device) return;
    deviceActionError = "";
    deviceConfirm = { action, device };
  }

  function closeDeviceConfirm() {
    deviceConfirm = null;
  }

  async function copyIssuedKey() {
    if (!issuedKeyDialog || issuedKeyCopying || typeof navigator === "undefined" || !navigator.clipboard) return;
    issuedKeyCopying = true;
    try {
      await navigator.clipboard.writeText(issuedKeyDialog.appKey);
      issuedKeyCopied = true;
    } finally {
      issuedKeyCopying = false;
    }
  }

  async function saveDevice(e: SubmitEvent) {
    e.preventDefault();
    if (addDeviceSaving) return;
    if (!newDevice.label.trim()) {
      formError = lang === "zh-CN" ? "设备名称不能为空" : "Device name is required";
      return;
    }
    if (!backendConnected) {
      formError = t.labels.backendOffline;
      return;
    }
    addDeviceSaving = true;
    try {
      const response = await createDevice(newDevice.label.trim());
      devices = [...devices, fromApiDevice(response.device)];
      addDeviceOpen = false;
      showIssuedKey(response.device, response.appKeyOnce);
      void loadConsoleData();
    } catch (error) {
      formError = error instanceof Error ? error.message : String(error);
      backendConnected = false;
    } finally {
      addDeviceSaving = false;
    }
  }

  async function updateTransport(id: TransportId, update: Partial<Transport>) {
    if (!backendConnected || transportBusy !== null) return;
    const action = Object.prototype.hasOwnProperty.call(update, "enabled") ? "toggle" : "save";
    transportBusy = { id, action };
    try {
      const transport = await updateTransportConfig(id, update);
      transports = mapId(transports, id, () => transport);
      void loadConsoleData();
    } catch {
      backendConnected = false;
    } finally {
      transportBusy = null;
    }
  }

  function toggleTransport(id: TransportId) {
    if (!backendConnected || transportBusy !== null) return;
    const current = transports.find((transport) => transport.id === id);
    if (!current || current.editable === false) return;
    void updateTransport(id, { enabled: !current.enabled });
  }

  function saveTransportEndpoint(id: TransportId, endpoint: string) {
    if (transportBusy !== null) return;
    void updateTransport(id, { endpoint });
  }

  async function rotateAppKey(deviceId: string) {
    if (!backendConnected || deviceBusy !== null) return;
    deviceActionError = "";
    deviceBusy = { deviceId, action: "rotate_key" };
    try {
      const response = await rotateDeviceKey(deviceId);
      devices = mapDev(devices, deviceId, () => fromApiDevice(response.device));
      showIssuedKey(response.device, response.appKeyOnce);
      void loadConsoleData();
    } catch (error) {
      deviceActionError = error instanceof Error ? error.message : String(error);
      backendConnected = false;
    } finally {
      deviceBusy = null;
    }
  }

  async function disableDevice(deviceId: string) {
    if (!backendConnected || deviceBusy !== null) return;
    deviceActionError = "";
    deviceBusy = { deviceId, action: "disable" };
    try {
      const device = await updateDeviceStatus(deviceId, "disabled");
      devices = mapDev(devices, deviceId, () => device);
      void loadConsoleData();
    } catch (error) {
      deviceActionError = error instanceof Error ? error.message : String(error);
      backendConnected = false;
    } finally {
      deviceBusy = null;
    }
  }

  function runConfirmedDeviceAction(action: DeviceConfirmAction, deviceId: string) {
    if (deviceBusy !== null) return;
    if (!backendConnected) {
      deviceActionError = t.labels.backendOffline;
      return;
    }
    if (action === "rotate_key") {
      void rotateAppKey(deviceId);
    } else {
      void disableDevice(deviceId);
    }
  }

  function toggleDevice(deviceId: string) {
    if (!backendConnected || deviceBusy !== null) return;
    const current = devices.find((device) => device.deviceId === deviceId);
    if (!current) return;
    if (current.status !== "disabled") {
      openDeviceConfirm("disable", deviceId);
      return;
    }
    deviceActionError = "";
    deviceBusy = { deviceId, action: "enable" };
    void updateDeviceStatus(deviceId, "allowed")
      .then((device) => {
        devices = mapDev(devices, deviceId, () => device);
        void loadConsoleData();
      })
      .catch((error) => {
        deviceActionError = error instanceof Error ? error.message : String(error);
        backendConnected = false;
      })
      .finally(() => {
        deviceBusy = null;
      });
  }
</script>

<svelte:head>
  <title>{t.headTitle}</title>
</svelte:head>

<main class:light={theme === "light"} class:tauri={isTauri} class:macos={isMacOS} class:windows={isWindows} class="shell">
  <Sidebar pages={pages} activePage={activePage} {isTauri} {isMacOS} brand={t.brand} onSelectPage={setActivePage} />

  <section class="workspace">
    <Topbar consoleLabel={t.labels.console} currentPage={currentPage} />
    <div class="page-shell">
      {#if activePage === "overview"}
        <OverviewPage
          {t}
          {overviewCards}
          {overviewData}
          {systemInfoSpecs}
          {recentEvents}
          {transportStats}
          {kernelRows}
          {memoryContextRows}
        />
      {:else if activePage === "skills"}
        <SkillMemoryPage
          {t}
          {lang}
          {skillReport}
          {skills}
          {backendConnected}
          onRefresh={loadConsoleData}
          onBackendDisconnected={() => (backendConnected = false)}
        />
      {:else if activePage === "llm-gateway"}
        <LlmGatewayPage
          {t}
          {llmGateway}
          {consoleCapabilities}
          {ollamaTransparent}
          {backendConnected}
          onRefresh={loadConsoleData}
          onBackendDisconnected={() => (backendConnected = false)}
        />
      {:else if activePage === "account"}
        <AccountPage {t} {lang} {accountFields} onLangChange={setLang} />
      {:else if activePage === "transports"}
        <TransportsPage
          {t}
          {transports}
          {backendConnected}
          busyTransportId={transportBusy?.id ?? null}
          busyTransportAction={transportBusy?.action ?? null}
          onToggleTransport={toggleTransport}
          onSaveTransportEndpoint={saveTransportEndpoint}
        />
      {:else if activePage === "devices"}
        <DevicesPage
          {t}
          {devices}
          {backendConnected}
          actionError={deviceActionError}
          busyDeviceId={deviceBusy?.deviceId ?? null}
          busyDeviceAction={deviceBusy?.action ?? null}
          onOpenAddDevice={openAddDevice}
          onRotateAppKey={(deviceId) => openDeviceConfirm("rotate_key", deviceId)}
          onToggleDevice={toggleDevice}
        />
      {/if}
    </div>
  </section>

  <StatusBar
    {t}
    {theme}
    {skillCount}
    {enabledTransportCount}
    transportCount={transports.length}
    {activeDeviceCount}
    deviceCount={devices.length}
    loading={consoleLoading}
    onThemeChange={setTheme}
    onRefresh={() => void loadConsoleData()}
  />

  {#if addDeviceOpen}
    <AddDeviceModal
      {t}
      label={newDevice.label}
      error={formError}
      loading={addDeviceSaving}
      onClose={closeAddDeviceModal}
      onSubmit={(event) => void saveDevice(event)}
      onLabelChange={setNewDeviceLabel}
    />
  {/if}

  {#if issuedKeyDialog}
    <IssuedKeyModal
      {t}
      dialog={issuedKeyDialog}
      copied={issuedKeyCopied}
      loading={issuedKeyCopying}
      onClose={closeIssuedKeyDialog}
      onCopy={() => void copyIssuedKey()}
    />
  {/if}

  {#if deviceConfirm}
    {@const confirmAction = deviceConfirm.action}
    {@const confirmDeviceId = deviceConfirm.device.deviceId}
    <ConfirmActionModal
      title={confirmAction === "rotate_key" ? t.deviceConfirm.rotateTitle : t.deviceConfirm.disableTitle}
      description={confirmAction === "rotate_key" ? t.deviceConfirm.rotateDesc : t.deviceConfirm.disableDesc}
      subjectLabel={t.deviceConfirm.deviceLabel}
      subject={deviceName(deviceConfirm.device)}
      meta={confirmDeviceId}
      confirmLabel={confirmAction === "rotate_key" ? t.deviceConfirm.rotateConfirm : t.deviceConfirm.disableConfirm}
      cancelLabel={t.actions.cancel}
      closeLabel={t.addDevice.closeLabel}
      onClose={closeDeviceConfirm}
      onConfirm={() => runConfirmedDeviceAction(confirmAction, confirmDeviceId)}
    />
  {/if}
</main>
