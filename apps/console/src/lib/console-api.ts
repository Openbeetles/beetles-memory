import { apiJson } from "../api";
import type {
  ConsoleApiCapabilities,
  ConsoleApiDevice,
  ConsoleApiLlmGateway,
  ConsoleApiLlmGatewaySmokeRunReport,
  ConsoleApiOllamaTransition,
  ConsoleApiOllamaTransparentStatus,
  ConsoleApiOverview,
  ConsoleApiSession,
  ConsoleApiSkillDetail,
  ConsoleApiSkillList,
  ConsoleApiSkillMutation,
  ConsoleApiTransport,
  ConsoleApiWorkbenchReport,
  Device,
  StatusKind,
  Transport,
  TransportId,
} from "./types";
import { fromApiDevice, fromApiTransport } from "./view-model";

export type ConsoleSnapshot = {
  capabilities: ConsoleApiCapabilities;
  overview: ConsoleApiOverview;
  skills: ConsoleApiSkillList;
  llmGateway: ConsoleApiLlmGateway;
  ollamaTransparent: ConsoleApiOllamaTransparentStatus | null;
  transports: Transport[];
  devices: Device[];
  session: ConsoleApiSession;
};

export type SkillEditInput = {
  title: string;
  topic: string;
  summary: string;
  procedure: string;
  citations: string[];
};

export async function loadConsoleSnapshot(): Promise<ConsoleSnapshot> {
  const capabilitiesResponse = await apiJson<{ capabilities: ConsoleApiCapabilities }>("/console/capabilities");
  const ollamaTransparentAppVisible =
    capabilitiesResponse.capabilities.features.ollamaTransparentApp?.visible === true;
  const [
    overviewResponse,
    skillResponse,
    llmGatewayResponse,
    ollamaTransparentResponse,
    transportResponse,
    deviceResponse,
    sessionResponse,
  ] = await Promise.all([
    apiJson<{ overview: ConsoleApiOverview }>("/console/overview"),
    apiJson<{ skills: ConsoleApiSkillList }>("/console/skills"),
    apiJson<{ llmGateway: ConsoleApiLlmGateway }>("/console/llm-gateway"),
    ollamaTransparentAppVisible
      ? apiJson<{ ollamaTransparent: ConsoleApiOllamaTransparentStatus }>("/console/ollama-transparent/status")
      : Promise.resolve({ ollamaTransparent: null }),
    apiJson<{ transports: ConsoleApiTransport[] }>("/console/transports"),
    apiJson<{ devices: ConsoleApiDevice[] }>("/console/devices"),
    apiJson<{ session: ConsoleApiSession }>("/console/session"),
  ]);
  return {
    capabilities: capabilitiesResponse.capabilities,
    overview: overviewResponse.overview,
    skills: skillResponse.skills,
    llmGateway: llmGatewayResponse.llmGateway,
    ollamaTransparent: ollamaTransparentResponse.ollamaTransparent,
    transports: transportResponse.transports.map(fromApiTransport),
    devices: deviceResponse.devices.map(fromApiDevice),
    session: sessionResponse.session,
  };
}

export async function fetchWorkbenchReport(): Promise<ConsoleApiWorkbenchReport> {
  const response = await apiJson<{ workbenchReport: ConsoleApiWorkbenchReport }>("/console/workbench/report");
  return response.workbenchReport;
}

export async function fetchSkill(name: string): Promise<ConsoleApiSkillDetail> {
  const response = await apiJson<{ skill: ConsoleApiSkillDetail }>(`/console/skills/${encodeURIComponent(name)}`);
  return response.skill;
}

export async function editSkill(name: string, input: SkillEditInput): Promise<ConsoleApiSkillMutation> {
  const response = await apiJson<{ mutation: ConsoleApiSkillMutation }>(
    `/console/skills/${encodeURIComponent(name)}`,
    {
      method: "PATCH",
      body: JSON.stringify(input),
    },
  );
  return response.mutation;
}

export async function setSkillEnabled(name: string, enabled: boolean): Promise<ConsoleApiSkillMutation> {
  const response = await apiJson<{ mutation: ConsoleApiSkillMutation }>(`/console/skills/${encodeURIComponent(name)}/enabled`, {
    method: "PATCH",
    body: JSON.stringify({ enabled }),
  });
  return response.mutation;
}

export async function deleteSkill(name: string): Promise<ConsoleApiSkillMutation> {
  const response = await apiJson<{ mutation: ConsoleApiSkillMutation }>(`/console/skills/${encodeURIComponent(name)}`, {
    method: "DELETE",
  });
  return response.mutation;
}

export async function createDevice(label: string): Promise<{ device: ConsoleApiDevice; appKeyOnce: string }> {
  return await apiJson<{ device: ConsoleApiDevice; appKeyOnce: string }>("/console/devices", {
    method: "POST",
    body: JSON.stringify({ label }),
  });
}

export async function updateTransportConfig(id: TransportId, update: Partial<Transport>): Promise<Transport> {
  const response = await apiJson<{ transport: ConsoleApiTransport }>(`/console/transports/${id}`, {
    method: "PATCH",
    body: JSON.stringify({
      enabled: update.enabled,
      endpoint: update.endpoint,
    }),
  });
  return fromApiTransport(response.transport);
}

export async function rotateDeviceKey(deviceId: string): Promise<{ device: ConsoleApiDevice; appKeyOnce: string }> {
  return await apiJson<{ device: ConsoleApiDevice; appKeyOnce: string }>(`/console/devices/${deviceId}/rotate-key`, {
    method: "POST",
    body: "{}",
  });
}

export async function updateDeviceStatus(deviceId: string, status: StatusKind): Promise<Device> {
  const response = await apiJson<{ device: ConsoleApiDevice }>(`/console/devices/${deviceId}`, {
    method: "PATCH",
    body: JSON.stringify({ status }),
  });
  return fromApiDevice(response.device);
}

export async function runLlmGatewaySmokeCheck(id: string): Promise<ConsoleApiLlmGatewaySmokeRunReport> {
  const response = await apiJson<{ result: ConsoleApiLlmGatewaySmokeRunReport }>(
    `/console/llm-gateway/smoke-checks/${encodeURIComponent(id)}/run`,
    {
      method: "POST",
      body: "{}",
    },
  );
  return response.result;
}

export async function enableOllamaTransparent(): Promise<ConsoleApiOllamaTransition> {
  const response = await apiJson<{ transition: ConsoleApiOllamaTransition }>("/console/ollama-transparent/enable", {
    method: "POST",
    body: JSON.stringify({
      allowStopOfficialOllama: true,
      openApp: true,
    }),
  });
  return response.transition;
}

export async function disableOllamaTransparent(): Promise<ConsoleApiOllamaTransition> {
  const response = await apiJson<{ transition: ConsoleApiOllamaTransition }>("/console/ollama-transparent/disable", {
    method: "POST",
    body: JSON.stringify({
      restoreOfficialApp: true,
    }),
  });
  return response.transition;
}

export async function openOllamaApp(): Promise<void> {
  await apiJson<{ action: unknown }>("/console/ollama-transparent/open-app", {
    method: "POST",
    body: "{}",
  });
}
