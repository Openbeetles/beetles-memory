import { invoke } from "@tauri-apps/api/core";

type DesktopConsoleResponse = {
  statusCode: number;
  body: string;
};

export class ConsoleApiResponseError extends Error {
  constructor(public readonly statusCode: number, message: string) {
    super(message);
  }
}

export async function apiJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  if (isTauriRuntime()) {
    return await tauriJson<T>(path, init);
  }
  return await httpJson<T>(path, init);
}

async function httpJson<T>(path: string, init: RequestInit): Promise<T> {
  const headers = new Headers(init.headers);
  headers.set("content-type", "application/json");
  headers.set("x-loopback", "true");
  const response = await fetch(path, {
    ...init,
    headers,
  });
  if (!response.ok) {
    throw new ConsoleApiResponseError(response.status, `${response.status} ${response.statusText}`);
  }
  return await response.json() as T;
}

async function tauriJson<T>(path: string, init: RequestInit): Promise<T> {
  const response = await invoke<DesktopConsoleResponse>("console_request", {
    request: {
      method: (init.method ?? "GET").toUpperCase(),
      path,
      body: requestBody(init.body),
    },
  });
  if (response.statusCode < 200 || response.statusCode >= 300) {
    throw new ConsoleApiResponseError(response.statusCode, `${response.statusCode} ${response.body}`);
  }
  return JSON.parse(response.body) as T;
}

function requestBody(body: BodyInit | null | undefined): string {
  if (body == null) return "";
  if (typeof body === "string") return body;
  throw new Error("Tauri console API only accepts string JSON bodies");
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined"
    && ("__TAURI_INTERNALS__" in window || "__TAURI__" in window);
}
