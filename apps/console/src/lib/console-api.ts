import { apiJson } from "../api";
import type {
  ConsoleApiDevice,
  ConsoleApiOverview,
  ConsoleApiSession,
  ConsoleApiSkillDetail,
  ConsoleApiSkillList,
  ConsoleApiSkillMutation,
  ConsoleApiTransport,
  Device,
  StatusKind,
  Transport,
  TransportId,
} from "./types";
import { fromApiDevice, fromApiTransport } from "./view-model";

export type ConsoleSnapshot = {
  overview: ConsoleApiOverview;
  skills: ConsoleApiSkillList;
  transports: Transport[];
  devices: Device[];
  session: ConsoleApiSession;
};

export type SkillUpsertInput = {
  title: string;
  topic: string;
  summary: string;
  procedure: string;
  citations: string[];
};

export async function loadConsoleSnapshot(): Promise<ConsoleSnapshot> {
  const [overviewResponse, skillResponse, transportResponse, deviceResponse, sessionResponse] = await Promise.all([
    apiJson<{ overview: ConsoleApiOverview }>("/console/overview"),
    apiJson<{ skills: ConsoleApiSkillList }>("/console/skills"),
    apiJson<{ transports: ConsoleApiTransport[] }>("/console/transports"),
    apiJson<{ devices: ConsoleApiDevice[] }>("/console/devices"),
    apiJson<{ session: ConsoleApiSession }>("/console/session"),
  ]);
  return {
    overview: overviewResponse.overview,
    skills: skillResponse.skills,
    transports: transportResponse.transports.map(fromApiTransport),
    devices: deviceResponse.devices.map(fromApiDevice),
    session: sessionResponse.session,
  };
}

export async function fetchSkill(name: string): Promise<ConsoleApiSkillDetail> {
  const response = await apiJson<{ skill: ConsoleApiSkillDetail }>(`/console/skills/${encodeURIComponent(name)}`);
  return response.skill;
}

export async function upsertSkill(name: string | undefined, input: SkillUpsertInput): Promise<ConsoleApiSkillMutation> {
  const response = await apiJson<{ mutation: ConsoleApiSkillMutation }>(
    name ? `/console/skills/${encodeURIComponent(name)}` : "/console/skills",
    {
      method: name ? "PATCH" : "POST",
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
