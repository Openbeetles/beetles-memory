import type { StatusKind } from "./types";

/** Semantic tone used by badges, borders, and dots. Single mapping source. */
export type StatusTone = "ready" | "limited" | "blocked" | "locked" | "neutral";

const STATUS_TONE: Record<StatusKind, StatusTone> = {
  ready: "ready",
  allowed: "ready",
  active: "ready",
  limited: "limited",
  draft: "limited",
  stale: "limited",
  low_value: "limited",
  locked: "locked",
  blocked: "blocked",
  disabled: "blocked",
  retired: "blocked",
  superseded: "limited",
};

export function statusTone(status: StatusKind): StatusTone {
  return STATUS_TONE[status] ?? "neutral";
}
