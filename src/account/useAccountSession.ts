import { useCallback, useEffect, useRef, useState } from "react";
import type { ConfigurationLineId } from "../configuration/preview";
import {
  openAccountWallet,
  runAccountCommand,
  type AccountCommand,
  type AccountProjection,
} from "./session";

export function useAccountSession(lineId: ConfigurationLineId) {
  const [projection, setProjection] = useState<AccountProjection | null>(null);
  const [loading, setLoading] = useState(true);
  const latest = useRef(0);

  const execute = useCallback(
    async (command: AccountCommand) => {
      latest.current += 1;
      const current = latest.current;
      setLoading(true);
      try {
        const result = await runAccountCommand(command, lineId);
        if (latest.current === current) setProjection(result);
        return result;
      } catch {
        if (latest.current === current) setProjection(null);
        return null;
      } finally {
        if (latest.current === current) setLoading(false);
      }
    },
    [lineId],
  );

  useEffect(() => {
    setProjection(null);
    void execute("account_inspect_v2");
    return () => {
      latest.current += 1;
    };
  }, [execute]);

  useEffect(() => {
    if (projection?.status !== "authorization_pending") return;
    const timer = window.setTimeout(
      () => void execute("account_poll_authorization_v2"),
      Math.max(1, projection.pollAfterSeconds) * 1000,
    );
    return () => window.clearTimeout(timer);
  }, [execute, projection]);

  return {
    projection,
    loading,
    refresh: () => execute("account_inspect_v2"),
    beginAuthorization: () => execute("account_begin_authorization_v2"),
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
