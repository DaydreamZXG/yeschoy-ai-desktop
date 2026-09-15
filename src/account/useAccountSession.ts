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

export function useAccountSession(lineId: ConfigurationLineId) {
  const [projection, setProjection] = useState<AccountProjection | null>(null);
  const [loading, setLoading] = useState(true);
  const [lastError, setLastError] = useState<string | null>(null);
  const [pollRevision, setPollRevision] = useState(0);
  const projectionRef = useRef<AccountProjection | null>(null);
  const authorizationLine = useRef(lineId);
  const latest = useRef(0);
  projectionRef.current = projection;

  const execute = useCallback(
    async (command: AccountCommand) => {
      latest.current += 1;
      const current = latest.current;
      setLoading(true);
      if (command === "account_begin_authorization_v2")
        authorizationLine.current = lineId;
      try {
        const result = await runAccountCommand(
          command,
          command === "account_poll_authorization_v2"
            ? authorizationLine.current
            : lineId,
        );
        if (latest.current === current) {
          const transient = [
            "backend_unavailable",
            "incompatible_server",
            "secure_storage_unavailable",
            "network_error",
            "invalid_response",
          ].includes(result.status);
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
            (previous?.status === "signed_in" ||
              previous?.status === "authorization_pending")
              ? previous
              : result,
          );
        }
        return result;
      } catch {
        if (latest.current === current) setLastError("network_error");
        return null;
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
