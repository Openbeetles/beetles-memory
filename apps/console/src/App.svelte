<script lang="ts">
  import { onMount } from "svelte";
  import AddDeviceModal from "./components/AddDeviceModal.svelte";
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
    ConsoleApiDevice,
    ConsoleApiOverview,
    ConsoleApiSession,
    ConsoleApiSkillList,
    ConsoleApiSkillSummary,
    Device,
    Lang,
    PageId,
    StatusKind,
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
    mapDev,
    mapId,
    pagesWithCounts,
    systemInfoSpecRows,
    transportStatRows,
  } from "./lib/view-model";
  import AccountPage from "./pages/AccountPage.svelte";
  import DevicesPage from "./pages/DevicesPage.svelte";
  import OverviewPage from "./pages/OverviewPage.svelte";
  import SkillMemoryPage from "./pages/SkillMemoryPage.svelte";
  import TransportsPage from "./pages/TransportsPage.svelte";

  const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  let activePage: PageId = $state("overview");
  let theme: Theme = $state(readTheme());
  let lang: Lang = $state(readLang());
  let backendConnected = $state(false);

  let overviewData: ConsoleApiOverview | null = $state(null);
  let sessionData: ConsoleApiSession | null = $state(null);
  let transports: Transport[] = $state([]);
  let devices: Device[] = $state([]);

  let skillReport: ConsoleApiSkillList | null = $state(null);
  let skills: ConsoleApiSkillSummary[] = $state([]);

  let addDeviceOpen = $state(false);
  let newDevice = $state({ label: "" });
  let formError = $state("");
  let issuedKeyDialog: { deviceId: string; label: string; appKey: string } | null = $state(null);
  let issuedKeyCopied = $state(false);

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
    try {
      const snapshot = await loadConsoleSnapshot();
      overviewData = snapshot.overview;
      skillReport = snapshot.skills;
      skills = snapshot.skills.skills;
      transports = snapshot.transports;
      devices = snapshot.devices;
      sessionData = snapshot.session;
      backendConnected = true;
    } catch {
      overviewData = null;
      skillReport = null;
      skills = [];
      transports = [];
      devices = [];
      sessionData = null;
      backendConnected = false;
    }
  }

  function openAddDevice() {
    newDevice = { label: "" };
    formError = "";
    addDeviceOpen = true;
  }

  function closeAddDeviceModal() {
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
    issuedKeyDialog = null;
    issuedKeyCopied = false;
  }

  async function copyIssuedKey() {
    if (!issuedKeyDialog || typeof navigator === "undefined" || !navigator.clipboard) return;
    await navigator.clipboard.writeText(issuedKeyDialog.appKey);
    issuedKeyCopied = true;
  }

  async function saveDevice(e: SubmitEvent) {
    e.preventDefault();
    if (!newDevice.label.trim()) {
      formError = lang === "zh-CN" ? "设备名称不能为空" : "Device name is required";
      return;
    }
    if (!backendConnected) {
      formError = t.labels.backendOffline;
      return;
    }
    try {
      const response = await createDevice(newDevice.label.trim());
      devices = [...devices, fromApiDevice(response.device)];
      closeAddDeviceModal();
      showIssuedKey(response.device, response.appKeyOnce);
      void loadConsoleData();
    } catch (error) {
      formError = error instanceof Error ? error.message : String(error);
      backendConnected = false;
    }
  }

  async function updateTransport(id: TransportId, update: Partial<Transport>) {
    if (!backendConnected) return;
    try {
      const transport = await updateTransportConfig(id, update);
      transports = mapId(transports, id, () => transport);
      void loadConsoleData();
    } catch {
      backendConnected = false;
    }
  }

  function toggleTransport(id: TransportId) {
    if (!backendConnected) return;
    const current = transports.find((transport) => transport.id === id);
    if (!current || current.editable === false) return;
    void updateTransport(id, { enabled: !current.enabled });
  }

  function saveTransportEndpoint(id: TransportId, endpoint: string) {
    void updateTransport(id, { endpoint });
  }

  async function rotateAppKey(deviceId: string) {
    if (!backendConnected) return;
    try {
      const response = await rotateDeviceKey(deviceId);
      devices = mapDev(devices, deviceId, () => fromApiDevice(response.device));
      showIssuedKey(response.device, response.appKeyOnce);
      void loadConsoleData();
    } catch {
      backendConnected = false;
    }
  }

  function toggleDevice(deviceId: string) {
    if (!backendConnected) return;
    const current = devices.find((device) => device.deviceId === deviceId);
    if (!current) return;
    const status: StatusKind = current.status === "disabled" ? "allowed" : "disabled";
    void updateDeviceStatus(deviceId, status)
      .then((device) => {
        devices = mapDev(devices, deviceId, () => device);
        void loadConsoleData();
      })
      .catch(() => {
        backendConnected = false;
      });
  }
</script>

<svelte:head>
  <title>{t.headTitle}</title>
</svelte:head>

<main class:light={theme === "light"} class:tauri={isTauri} class="shell">
  <Sidebar pages={pages} activePage={activePage} {isTauri} brand={t.brand} onSelectPage={setActivePage} />

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
      {:else if activePage === "account"}
        <AccountPage {t} {lang} {accountFields} onLangChange={setLang} />
      {:else if activePage === "transports"}
        <TransportsPage
          {t}
          {transports}
          {backendConnected}
          onToggleTransport={toggleTransport}
          onSaveTransportEndpoint={saveTransportEndpoint}
        />
      {:else if activePage === "devices"}
        <DevicesPage
          {t}
          {devices}
          {backendConnected}
          onOpenAddDevice={openAddDevice}
          onRotateAppKey={(deviceId) => void rotateAppKey(deviceId)}
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
    onThemeChange={setTheme}
    onRefresh={() => void loadConsoleData()}
  />

  {#if addDeviceOpen}
    <AddDeviceModal
      {t}
      label={newDevice.label}
      error={formError}
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
      onClose={closeIssuedKeyDialog}
      onCopy={() => void copyIssuedKey()}
    />
  {/if}
</main>
