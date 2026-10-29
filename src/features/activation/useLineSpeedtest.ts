import { useCallback, useEffect, useRef, useState } from "react";
import type { ConnectivityLineResult } from "../../diagnostics/contract";
import type { ConfigurationLineId } from "../../configuration/preview";
import { checkLineConnectivity } from "../../shared/api/connectivity";
import {
  lineLatencyMs,
  recommendedLineId,
  type LineSpeedtestPhase,
} from "./previewRules";

/** Opt-in only: editing the selection must never trigger a network probe. */
export function useLineSpeedtest() {
  const [phase, setPhase] = useState<LineSpeedtestPhase>("idle");
  const [lines, setLines] = useState<ConnectivityLineResult[]>([]);
  const sequence = useRef(0);
  useEffect(
    () => () => {
      sequence.current += 1;
    },
    [],
  );

  const run = useCallback(async () => {
    const current = ++sequence.current;
    setPhase("running");
    try {
      const decoded = await checkLineConnectivity();
      if (sequence.current !== current) return;
      setLines(decoded.response.lines);
      setPhase("done");
    } catch {
      if (sequence.current === current) setPhase("error");
    }
  }, []);

  const recommendedLine = phase === "done" ? recommendedLineId(lines) : null;
  const lineLatencies: Partial<Record<ConfigurationLineId, number>> =
    phase === "done"
      ? Object.fromEntries(
          lines
            .map((line) => [line.lineId, lineLatencyMs(line)] as const)
            .filter(
              (entry): entry is [ConfigurationLineId, number] =>
                entry[1] !== undefined,
            ),
        )
      : {};
  return { phase, run, recommendedLine, lineLatencies };
}
