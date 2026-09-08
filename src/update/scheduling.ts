import type { UpdatePhase } from "./contract";

const FOREGROUND_CHECK_MIN_INTERVAL_MS = 5 * 60_000;

export function canStartInstall(phase: UpdatePhase, availableVersion: string) {
  return phase === "available" && availableVersion.length > 0;
}

export function shouldCheckAfterReturning(
  visibility: DocumentVisibilityState,
  now: number,
  lastCheckStartedAt: number,
) {
  return (
    visibility === "visible" &&
    now - lastCheckStartedAt >= FOREGROUND_CHECK_MIN_INTERVAL_MS
  );
}
