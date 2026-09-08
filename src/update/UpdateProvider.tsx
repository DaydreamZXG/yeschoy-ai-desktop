import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { runNativeUpdate } from "./api";
import {
  decodeUpdateProjection,
  idleUpdateProjection,
  UPDATE_PROGRESS_EVENT,
  updateBusy,
  type UpdateProjection,
} from "./contract";
import { canStartInstall, shouldCheckAfterReturning } from "./scheduling";

const BACKGROUND_CHECK_DELAY_MS = 1_500;
const CHECK_REPLY_DEADLINE_MS = 15_000;

interface UpdateContextValue {
  projection: UpdateProjection;
  attention: boolean;
  working: boolean;
  check: () => void;
  install: () => void;
}

const UpdateContext = createContext<UpdateContextValue>({
  projection: idleUpdateProjection(),
  attention: false,
  working: false,
  check: () => undefined,
  install: () => undefined,
});

let requestSequence = 0;
const nextRequestId = (prefix: "check" | "install") => {
  requestSequence += 1;
  return `update-${prefix}-${Date.now().toString(36)}-${requestSequence}`;
};

export function UpdateProvider({ children }: { children: ReactNode }) {
  const [projection, setProjection] = useState(idleUpdateProjection);
  const [attention, setAttention] = useState(false);
  const activeRequest = useRef("");
  const phase = useRef(projection.phase);
  const mounted = useRef(true);
  const deadline = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastCheckStartedAt = useRef(0);

  const clearDeadline = useCallback(() => {
    if (deadline.current) clearTimeout(deadline.current);
    deadline.current = null;
  }, []);

  useEffect(() => {
    phase.current = projection.phase;
  }, [projection.phase]);

  const apply = useCallback(
    (next: UpdateProjection, source: "check" | "install" | "event") => {
      if (!mounted.current || next.requestId !== activeRequest.current) return;
      if (next.phase !== "checking") clearDeadline();
      phase.current = next.phase;
      setProjection(next);
      if (next.phase === "available") setAttention(true);
      if (next.phase === "current" || next.phase === "unavailable") {
        setAttention(false);
      }
      if (next.phase === "failed" && source !== "check") setAttention(true);
    },
    [clearDeadline],
  );

  const check = useCallback(() => {
    if (updateBusy(phase.current)) return;
    // Ref updates are synchronous, unlike React state. This closes the small
    // double-click/focus-event window that could otherwise start two checks.
    phase.current = "checking";
    lastCheckStartedAt.current = Date.now();
    const requestId = nextRequestId("check");
    activeRequest.current = requestId;
    clearDeadline();
    setProjection((current) => ({
      ...current,
      requestId,
      phase: "checking",
      downloadedBytes: 0,
      totalBytes: 0,
      reasonCode: "checking",
    }));
    deadline.current = setTimeout(() => {
      if (!mounted.current || activeRequest.current !== requestId) return;
      activeRequest.current = "";
      phase.current = "unavailable";
      setAttention(false);
      setProjection((current) => ({
        ...current,
        requestId,
        phase: "unavailable",
        reasonCode: "response_timeout",
      }));
    }, CHECK_REPLY_DEADLINE_MS);
    void runNativeUpdate(requestId, "check")
      .then((next) => apply(next, "check"))
      .catch(() => {
        if (!mounted.current || activeRequest.current !== requestId) return;
        clearDeadline();
        phase.current = "unavailable";
        setAttention(false);
        setProjection((current) => ({
          ...current,
          requestId,
          phase: "unavailable",
          reasonCode: "native_unavailable",
        }));
      });
  }, [apply, clearDeadline]);

  const checkAfterReturning = useCallback(() => {
    if (
      !shouldCheckAfterReturning(
        document.visibilityState,
        Date.now(),
        lastCheckStartedAt.current,
      )
    )
      return;
    check();
  }, [check]);

  const install = useCallback(() => {
    // `projection` changes on React's schedule. The ref is the synchronous
    // admission authority, so two clicks in the same render cannot launch two
    // native installers for the same release.
    if (!canStartInstall(phase.current, projection.availableVersion)) return;
    const requestId = nextRequestId("install");
    const expectedVersion = projection.availableVersion;
    activeRequest.current = requestId;
    phase.current = "downloading";
    clearDeadline();
    setAttention(true);
    setProjection((current) => ({
      ...current,
      requestId,
      phase: "downloading",
      downloadedBytes: 0,
      totalBytes: 0,
      reasonCode: "downloading",
    }));
    void runNativeUpdate(requestId, "install", expectedVersion)
      .then((next) => apply(next, "install"))
      .catch(() => {
        if (!mounted.current || activeRequest.current !== requestId) return;
        phase.current = "failed";
        setProjection((current) => ({
          ...current,
          requestId,
          phase: "failed",
          reasonCode: "native_unavailable",
        }));
      });
  }, [apply, clearDeadline, projection.availableVersion]);

  useEffect(() => {
    mounted.current = true;
    let unlisten: (() => void) | undefined;
    void listen<unknown>(UPDATE_PROGRESS_EVENT, (event) => {
      const next = decodeUpdateProjection(event.payload);
      if (next) apply(next, "event");
    })
      .then((stop) => {
        if (mounted.current) unlisten = stop;
        else stop();
      })
      .catch(() => undefined);
    const timer = setTimeout(check, BACKGROUND_CHECK_DELAY_MS);
    window.addEventListener("focus", checkAfterReturning);
    document.addEventListener("visibilitychange", checkAfterReturning);
    return () => {
      mounted.current = false;
      clearTimeout(timer);
      clearDeadline();
      unlisten?.();
      window.removeEventListener("focus", checkAfterReturning);
      document.removeEventListener("visibilitychange", checkAfterReturning);
    };
  }, [apply, check, checkAfterReturning, clearDeadline]);

  const value = useMemo<UpdateContextValue>(
    () => ({
      projection,
      attention,
      working: updateBusy(projection.phase),
      check,
      install,
    }),
    [attention, check, install, projection],
  );
  return (
    <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>
  );
}

export function useDesktopUpdate() {
  return useContext(UpdateContext);
}
