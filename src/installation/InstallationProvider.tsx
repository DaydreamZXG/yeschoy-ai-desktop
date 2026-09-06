import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  installationAction,
  installationActive,
  type InstallationAction,
  type InstallationIntent,
  type InstallationProgress,
} from "./api";
import type { ActivationToolId } from "../configuration/activation";

export interface InstallationController {
  progress: InstallationProgress | null;
  working: boolean;
  error: boolean;
  run: (
    tool: ActivationToolId,
    action: InstallationAction,
    intent?: InstallationIntent,
  ) => Promise<InstallationProgress | null>;
}
export const InstallationContext = createContext<InstallationController | null>(
  null,
);
export const useInstallation = () => useContext(InstallationContext);

export function InstallationProvider({ children }: { children: ReactNode }) {
  const [progress, setProgress] = useState<InstallationProgress | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState(false);
  const current = useRef(progress);
  current.current = progress;
  const inFlight = useRef(false);
  const sequence = useRef(0);
  const mounted = useRef(true);
  const run = useCallback(
    async (
      tool: ActivationToolId,
      action: InstallationAction,
      intent?: InstallationIntent,
    ) => {
      if (inFlight.current) return null;
      const mutation = action !== "inspect" && action !== "help";
      if (mutation) {
        inFlight.current = true;
        setWorking(true);
      }
      const request = ++sequence.current;
      const jobId = ["cancel", "confirm", "reopen"].includes(action)
        ? (current.current?.jobId ?? "")
        : "";
      try {
        const result = await installationAction(tool, action, jobId, intent);
        if (mounted.current && request === sequence.current) {
          setProgress(result);
          setError(false);
        }
        return result;
      } catch {
        // A lost start response is an observation problem, not permission to
        // start another installer. Recover the native job by a read-only inspect.
        if (action === "start") {
          try {
            const recovered = await installationAction(tool, "inspect");
            if (mounted.current && request === sequence.current) {
              setProgress(recovered);
              setError(false);
            }
            return recovered;
          } catch {
            /* Keep the last known state until explicit/polled inspection. */
          }
        }
        if (mounted.current && request === sequence.current) setError(true);
        return null;
      } finally {
        if (mutation) {
          inFlight.current = false;
          if (mounted.current) setWorking(false);
        }
      }
    },
    [],
  );
  useEffect(() => {
    mounted.current = true;
    void run("claude_desktop", "inspect");
    return () => {
      mounted.current = false;
      ++sequence.current;
    };
  }, [run]);
  useEffect(() => {
    if (!progress || !installationActive(progress) || working) return;
    // Keep observing after repeated failures even when React state is unchanged.
    // A mutation owns the next snapshot; polling never replays an install action.
    let cancelled = false;
    let timer: number;
    const poll = async () => {
      await run(progress.toolId, "inspect");
      if (!cancelled) timer = window.setTimeout(poll, 2500);
    };
    timer = window.setTimeout(poll, error ? 2500 : 900);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [progress, working, error, run]);
  return (
    <InstallationContext.Provider value={{ progress, working, error, run }}>
      {children}
    </InstallationContext.Provider>
  );
}
