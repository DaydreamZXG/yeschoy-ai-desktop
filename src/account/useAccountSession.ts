import { useCallback, useEffect, useRef, useState } from "react";
import type { ConfigurationLineId } from "../configuration/preview";
import {
  clearSessionAgeRecord,
  noteSessionActivity,
} from "./sessionAge";
import {
  openAccountWallet,
  runAccountCommand,
  type AccountCommand,
  type AccountProjection,
} from "./session";

/// Masking a transient failure keeps a brief route hiccup from looking like a
/// logout. Masking it indefinitely is worse: an authorization the user already
/// approved in the browser keeps rendering as "waiting", with the real reason
/// never shown, until the device code expires ten minutes later. Tolerate a
/// short run of failures, then let the real status through.
const TRANSIENT_TOLERANCE = 3;

// A desktop launch can race the local service or the selected route becoming
// ready. Do not turn that short startup window into a visible account error.
// These delays keep the retry bounded while giving the service enough time to
// settle. Structural failures are deliberately excluded because retrying them
// cannot repair a mismatched server or an unavailable secure store.
const INITIAL_INSPECTION_RETRY_DELAYS_MS = [250, 750, 1_500] as const;
const RETRYABLE_INSPECTION_STATUSES: AccountProjection["status"][] = [
  "backend_unavailable",
  "network_error",
  "invalid_response",
];
const TRANSIENT_STATUSES: AccountProjection["status"][] = [
  ...RETRYABLE_INSPECTION_STATUSES,
  "incompatible_server",
  "secure_storage_unavailable",
];

function isTransientStatus(status: AccountProjection["status"]): boolean {
  return TRANSIENT_STATUSES.includes(status);
}

function isRetryableInspectionStatus(
  status: AccountProjection["status"],
): boolean {
  return RETRYABLE_INSPECTION_STATUSES.includes(status);
}

function waitForRetry(delayMs: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, delayMs));
}

export function useAccountSession(lineId: ConfigurationLineId) {
  const [projection, setProjection] = useState<AccountProjection | null>(null);
  const [loading, setLoading] = useState(true);
  const [lastError, setLastError] = useState<string | null>(null);
  const [pollRevision, setPollRevision] = useState(0);
  const projectionRef = useRef<AccountProjection | null>(null);
  const authorizationLine = useRef(lineId);
  const latest = useRef(0);
  const transientStreak = useRef(0);
  projectionRef.current = projection;

  const execute = useCallback(
    async (command: AccountCommand) => {
      latest.current += 1;
      const current = latest.current;
      setLoading(true);
      if (command === "account_begin_authorization_v2") {
        authorizationLine.current = lineId;
        transientStreak.current = 0;
      }
      try {
        let result: AccountProjection | null = null;
        let transportFailure = false;
        const previous = projectionRef.current;
        const retryDelays =
          command === "account_inspect_v2" &&
          (!previous || isTransientStatus(previous.status))
            ? INITIAL_INSPECTION_RETRY_DELAYS_MS
            : [];

        for (let attempt = 0; attempt <= retryDelays.length; attempt += 1) {
          try {
            result = await runAccountCommand(
              command,
              command === "account_poll_authorization_v2"
                ? authorizationLine.current
                : lineId,
            );
            transportFailure = false;
          } catch {
            result = null;
            transportFailure = true;
          }

          if (latest.current !== current) return result;
          const retryable = result
            ? isRetryableInspectionStatus(result.status)
            : transportFailure;
          if (!retryable || attempt === retryDelays.length) break;
          await waitForRetry(retryDelays[attempt]);
          if (latest.current !== current) return result;
        }

        if (latest.current === current) {
          if (!result) {
            transientStreak.current += 1;
            setLastError("network_error");
            return null;
          }
          const transient = isTransientStatus(result.status);
          transientStreak.current = transient ? transientStreak.current + 1 : 0;
          setLastError(transient ? result.reasonCode : null);
          // 本地记账（#23）：signed_in 滚动最近使用时间；会话确认消失则清除。
          if (result.status === "signed_in") noteSessionActivity();
          else if (
            result.status === "signed_out" ||
            result.status === "session_expired"
          )
            clearSessionAgeRecord();
          setProjection((previous) =>
            transient &&
            transientStreak.current <= TRANSIENT_TOLERANCE &&
            (previous?.status === "signed_in" ||
              previous?.status === "authorization_pending")
              ? previous
              : result,
          );
        }
        return result;
      } finally {
        if (latest.current === current) {
          setLoading(false);
          // Retry scheduling must not depend on a changed projection object:
          // transport exceptions and cached state can leave it identical.
          setPollRevision((value) => value + 1);
        }
      }
    },
    [lineId],
  );

  useEffect(() => {
    void execute(
      projectionRef.current?.status === "authorization_pending"
        ? "account_poll_authorization_v2"
        : "account_inspect_v2",
    );
    return () => {
      latest.current += 1;
    };
  }, [execute]);

  useEffect(() => {
    if (projection?.status !== "authorization_pending" || loading) return;
    if (projection.expiresAtEpochMs <= Date.now()) {
      setProjection({
        ...projection,
        status: "expired",
        reasonCode: "authorization_expired",
      });
      return;
    }
    const timer = window.setTimeout(
      () => void execute("account_poll_authorization_v2"),
      Math.max(1, projection.pollAfterSeconds) * 1000,
    );
    return () => window.clearTimeout(timer);
  }, [execute, projection, pollRevision, loading]);

  return {
    projection,
    loading,
    lastError,
    refresh: () => execute("account_inspect_v2"),
    beginAuthorization: () => execute("account_begin_authorization_v2"),
    openAuthorization: () => execute("account_open_authorization_v2"),
    cancelAuthorization: () => execute("account_cancel_authorization_v2"),
    logout: () => execute("account_logout_v2"),
    openWallet: async () => {
      try {
        await openAccountWallet(lineId);
        return true;
      } catch {
        return false;
      }
    },
  };
}

export type AccountSessionController = Omit<
  ReturnType<typeof useAccountSession>,
  "lastError"
> & { lastError?: string | null };
