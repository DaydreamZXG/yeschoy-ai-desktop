export const STATION_CONNECT_PROGRESS_EVENT =
  "yeschoy://station-connect-progress";

export type StationConnectPhase =
  | "connecting"
  | "signed_in"
  | "failed"
  | "rejected";

export interface StationConnectProgressEvent {
  phase: StationConnectPhase;
  origin?: string;
  lineId?: string;
  reasonCode?: string;
  recoverable: boolean;
}

function object(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

export function decodeStationConnectProgress(
  value: unknown,
): StationConnectProgressEvent | null {
  if (!object(value)) return null;
  const phase = value.phase;
  if (
    phase !== "connecting" &&
    phase !== "signed_in" &&
    phase !== "failed" &&
    phase !== "rejected"
  ) {
    return null;
  }
  if (typeof value.recoverable !== "boolean") return null;
  const origin =
    value.origin === undefined
      ? undefined
      : typeof value.origin === "string"
        ? value.origin
        : null;
  if (origin === null) return null;
  const lineId =
    value.lineId === undefined
      ? undefined
      : typeof value.lineId === "string"
        ? value.lineId
        : null;
  if (lineId === null) return null;
  const reasonCode =
    value.reasonCode === undefined
      ? undefined
      : typeof value.reasonCode === "string"
        ? value.reasonCode
        : null;
  if (reasonCode === null) return null;
  return {
    phase,
    origin,
    lineId,
    reasonCode,
    recoverable: value.recoverable,
  };
}
