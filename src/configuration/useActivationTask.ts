import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  ACTIVATION_PROGRESS_EVENT,
  activateDesktopTool,
  cancelDesktopToolActivation,
  decodeActivationProgress,
  type ActivationProgress,
  type ToolActivationProjection,
} from "./activation";
import type { ConfigurationLineId } from "./preview";

export interface ActivationContext {
  key: string;
  account: string;
  app: string;
  model: string;
  group: string;
  line: ConfigurationLineId;
}

type Input = Omit<Parameters<typeof activateDesktopTool>[0], "onRequestId">;

/** Owns one native task, independent of the page's currently edited selection. */
export function useActivationTask(onComplete: () => void) {
  const [phase, setPhase] = useState<"idle" | "applying" | "finished">("idle");
  const [context, setContext] = useState<ActivationContext | null>(null);
  const [result, setResult] = useState<ToolActivationProjection | null>(null);
  const [progress, setProgress] = useState<ActivationProgress | null>(null);
  const [cancelRequested, setCancelRequested] = useState(false);
  const [cancelFailed, setCancelFailed] = useState(false);
  const inFlight = useRef(false);
  const request = useRef("");
  const cancelling = useRef(false);
  const mounted = useRef(true);
  const complete = useRef(onComplete);
  complete.current = onComplete;

  useEffect(() => {
    mounted.current = true;
    let live = true;
    let unlisten: (() => void) | undefined;
    void listen<unknown>(ACTIVATION_PROGRESS_EVENT, ({ payload }) => {
      const next = decodeActivationProgress(payload);
      if (live && next && next.requestId === request.current) setProgress(next);
    })
      .then((stop) => {
        if (live) unlisten = stop;
        else stop();
      })
      .catch(() => undefined);
    return () => {
      live = false;
      mounted.current = false;
      unlisten?.();
    };
  }, []);

  const reset = useCallback(() => {
    // A selection reset must never erase an active task's progress/cancel ID.
    if (inFlight.current) return;
    setPhase("idle");
    setResult(null);
    setContext(null);
    setProgress(null);
    setCancelRequested(false);
    setCancelFailed(false);
  }, []);

  const run = useCallback(async (input: Input, captured: ActivationContext) => {
    if (inFlight.current) return;
    inFlight.current = true;
    cancelling.current = false;
    setPhase("applying");
    setContext(captured);
    setResult(null);
    setProgress(null);
    setCancelRequested(false);
    setCancelFailed(false);
    let outcome: ToolActivationProjection;
    try {
      outcome = await activateDesktopTool({
        ...input,
        onRequestId: (id) => {
          request.current = id;
        },
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      outcome = {
        requestId: "local",
        schemaVersion: 3,
        status: "configuration_failed",
        toolId: input.toolId,
        modelId: input.modelId,
        billingGroup: input.billingGroup,
        observedAtEpochMs: Date.now(),
        reasonCode:
          message === "activation_request_timed_out"
            ? message
            : message.includes("activation_already_running")
              ? "activation_already_running"
              : "invalid_response",
      };
    } finally {
      inFlight.current = false;
      request.current = "";
      cancelling.current = false;
    }
    if (mounted.current) {
      setResult(outcome);
      setPhase("finished");
      setCancelRequested(false);
      setCancelFailed(false);
      complete.current();
    }
    return outcome;
  }, []);

  const cancel = useCallback(async () => {
    const id = request.current;
    if (!id || cancelling.current) return;
    cancelling.current = true;
    setCancelRequested(true);
    setCancelFailed(false);
    try {
      const status = await cancelDesktopToolActivation(id);
      if (mounted.current && request.current === id && status === "not_found") {
        cancelling.current = false;
        setCancelRequested(false);
        setCancelFailed(true);
      }
    } catch {
      if (mounted.current && request.current === id) {
        cancelling.current = false;
        setCancelRequested(false);
        setCancelFailed(true);
      }
    }
  }, []);

  return {
    phase,
    context,
    result,
    progress,
    cancelRequested,
    cancelFailed,
    inFlight,
    run,
    reset,
    cancel,
  };
}
