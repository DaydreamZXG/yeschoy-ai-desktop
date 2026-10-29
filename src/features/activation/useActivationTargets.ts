import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { queryClient } from "../../lib/query/queryClient";
import {
  scanActivationTargets,
  type ActivationToolId,
  type ActivationTargetScan,
} from "../../configuration/activation";
import type { ScanPhase } from "./previewRules";

export const ACTIVATION_TARGET_QUERY_KEY = [
  "activation-targets",
  "v1",
] as const;

/** Read-only discovery shares the existing QueryClient; user choices stay local. */
export function useActivationTargets(enableLocalActivation: boolean) {
  const query = useQuery(
    {
      queryKey: ACTIVATION_TARGET_QUERY_KEY,
      queryFn: scanActivationTargets,
      enabled: false,
      retry: false,
      networkMode: "always",
    },
    queryClient,
  );
  const refetch = query.refetch;
  const [scanPhase, setScanPhase] = useState<ScanPhase>("loading");
  const [targetScan, setTargetScan] = useState<ActivationTargetScan | null>(
    null,
  );
  const [installations, setInstallations] = useState<
    Partial<Record<ActivationToolId, string>>
  >({});
  const targetScanSequence = useRef(0);
  const refreshTargets = useCallback(async () => {
    targetScanSequence.current += 1;
    const currentScan = targetScanSequence.current;
    setScanPhase("loading");
    try {
      // A rescan after installation/manual selection needs a new observation,
      // not the older in-flight read. Cancel query ownership, never a native write.
      void queryClient.cancelQueries({ queryKey: ACTIVATION_TARGET_QUERY_KEY });
      const reply = await refetch({ cancelRefetch: true });
      if (!reply.isSuccess) throw reply.error;
      const result = reply.data;
      if (targetScanSequence.current !== currentScan) return;
      setTargetScan(result);
      setInstallations((current) => {
        const next = { ...current };
        for (const candidate of result.targets) {
          const currentId = current[candidate.toolId];
          const currentStillWorks = candidate.installations.some(
            (installation) =>
              installation.installationId === currentId &&
              installation.supported,
          );
          if (!currentStillWorks) {
            next[candidate.toolId] =
              candidate.installations.find(
                (installation) => installation.recommended,
              )?.installationId ??
              candidate.installations.find(
                (installation) => installation.supported,
              )?.installationId ??
              "";
          }
        }
        return next;
      });
      setScanPhase("ready");
    } catch {
      if (targetScanSequence.current !== currentScan) return;
      setTargetScan(null);
      setScanPhase("error");
    }
  }, [refetch]);

  useEffect(() => {
    if (!enableLocalActivation) {
      setScanPhase("ready");
      return;
    }
    void refreshTargets();
  }, [enableLocalActivation, refreshTargets]);

  useEffect(
    () => () => {
      targetScanSequence.current += 1;
    },
    [],
  );
  const selectInstallation = (
    toolId: ActivationToolId,
    installationId: string,
  ) =>
    setInstallations((current) => ({ ...current, [toolId]: installationId }));
  return {
    scanPhase,
    targetScan,
    installations,
    selectInstallation,
    refreshTargets,
  };
}
