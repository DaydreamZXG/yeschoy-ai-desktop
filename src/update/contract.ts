export const UPDATE_PROGRESS_EVENT = "yeschoy://update-progress";

export type NativeUpdatePhase =
  | "checking"
  | "current"
  | "available"
  | "downloading"
  | "restarting"
  | "unavailable"
  | "failed";

export type UpdatePhase = "idle" | NativeUpdatePhase;

export interface UpdateProjection {
  schemaVersion: 1;
  requestId: string;
  phase: UpdatePhase;
  currentVersion: string;
  availableVersion: string;
  notes: string;
  downloadedBytes: number;
  totalBytes: number;
  reasonCode: string;
}

const PHASES = new Set<NativeUpdatePhase>([
  "checking",
  "current",
  "available",
  "downloading",
  "restarting",
  "unavailable",
  "failed",
]);

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const boundedString = (
  value: unknown,
  max: number,
  allowEmpty = true,
): value is string =>
  typeof value === "string" &&
  value.length <= max &&
  (allowEmpty || value.length > 0);

const boundedCount = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;

export const idleUpdateProjection = (): UpdateProjection => ({
  schemaVersion: 1,
  requestId: "idle",
  phase: "idle",
  currentVersion: "",
  availableVersion: "",
  notes: "",
  downloadedBytes: 0,
  totalBytes: 0,
  reasonCode: "idle",
});

export function decodeUpdateProjection(
  value: unknown,
  expectedRequestId?: string,
): UpdateProjection | null {
  if (!isRecord(value) || value.schemaVersion !== 1) return null;
  if (
    !boundedString(value.requestId, 64, false) ||
    !/^[A-Za-z0-9_-]+$/.test(value.requestId)
  ) {
    return null;
  }
  if (expectedRequestId && value.requestId !== expectedRequestId) return null;
  if (
    typeof value.phase !== "string" ||
    !PHASES.has(value.phase as NativeUpdatePhase)
  ) {
    return null;
  }
  if (
    !boundedString(value.currentVersion, 40, false) ||
    !boundedString(value.availableVersion, 40) ||
    !boundedString(value.notes, 600) ||
    !boundedString(value.reasonCode, 80, false) ||
    !/^[a-z0-9_]+$/.test(value.reasonCode) ||
    !boundedCount(value.downloadedBytes) ||
    !boundedCount(value.totalBytes)
  ) {
    return null;
  }
  return {
    schemaVersion: 1,
    requestId: value.requestId,
    phase: value.phase as NativeUpdatePhase,
    currentVersion: value.currentVersion,
    availableVersion: value.availableVersion,
    notes: value.notes,
    downloadedBytes: value.downloadedBytes,
    totalBytes: value.totalBytes,
    reasonCode: value.reasonCode,
  };
}

export const updateBusy = (phase: UpdatePhase) =>
  phase === "checking" || phase === "downloading" || phase === "restarting";
