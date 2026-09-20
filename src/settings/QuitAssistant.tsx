import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { LogOut } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useConnections } from "../configuration/connections";
import { WORKBENCH_APPS } from "../workbench/appCatalog";

const OPEN_CHOICE_EVENT = "yeschoy:open-exit-choice";
const CLOSE_CHOICE_EVENT = "yeschoy://close-choice";
const EXIT_PROGRESS_EVENT = "yeschoy://exit-progress";
// This matches the native account request bound. It is only a progress notice,
// never a renderer instruction to interrupt a native transaction.
const FINISHING_NOTICE_MS = 20_000;

type ExitStatus =
  | "exiting"
  | "finishing_operation"
  | "restoring_settings"
  | "restore_failed"
  | "retryable_error";
type ExitPhase = "choice" | ExitStatus;
type ExitResponse = { status: ExitStatus; failedTools?: string[] };
type ExitState = { closeRequested: boolean; shutdown: ExitResponse | null };

function exitResponse(raw: unknown): ExitResponse | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const value = raw as Record<string, unknown>;
  if (
    Object.keys(value).some(
      (key) => !["status", "failedTools"].includes(key),
    ) ||
    ("failedTools" in value &&
      (!Array.isArray(value.failedTools) ||
        value.failedTools.length > 6 ||
        value.failedTools.some(
          (tool) =>
            ![
              "claude_code",
              "claude_desktop",
              "codex_desktop",
              "pi",
              "dsh_web",
              "workbuddy",
            ].includes(String(tool)),
        ))) ||
    ![
      "exiting",
      "finishing_operation",
      "restoring_settings",
      "restore_failed",
      "retryable_error",
    ].includes(String(value.status))
  )
    return null;
  return {
    status: value.status as ExitStatus,
    failedTools: value.failedTools as string[] | undefined,
  };
}

function exitState(raw: unknown): ExitState | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const value = raw as Record<string, unknown>;
  if (typeof value.closeRequested !== "boolean") return null;
  const shutdown =
    value.shutdown === null ? null : exitResponse(value.shutdown);
  if (value.shutdown !== null && !shutdown) return null;
  return { closeRequested: value.closeRequested, shutdown };
}

/** Settings entry point; the only dialog belongs to the always-mounted host. */
export function QuitAssistant() {
  const { t } = useTranslation();
  return (
    <div className="quit-assistant">
      <div>
        <strong>{t("quit.settingsTitle")}</strong>
        <p>{t("quit.settingsBody")}</p>
      </div>
      <button
        type="button"
        onClick={() => window.dispatchEvent(new Event(OPEN_CHOICE_EVENT))}
      >
        <LogOut />
        {t("quit.settingsAction")}
      </button>
    </div>
  );
}

/** Mount once inside ConnectionProvider, including when Settings is not open. */
export function ShutdownProvider({ children }: { children: ReactNode }) {
  return (
    <>
      {children}
      <ShutdownHost />
    </>
  );
}

/** Alternative sibling mount; do not mount both this and ShutdownProvider. */
export function ShutdownHost() {
  const { t } = useTranslation();
  const connections = useConnections();
  const affected =
    connections?.connections.filter(
      (connection) =>
        connection.requiresBackground &&
        ["connected", "changed", "legacy", "recovery_pending"].includes(
          connection.state,
        ),
    ) ?? [];
  const unknown = !connections || connections.loading || connections.error;
  const dialog = useRef<HTMLDialogElement>(null);
  const mounted = useRef(false);
  const requestSequence = useRef(0);
  const [visible, setVisible] = useState(false);
  const [phase, setPhase] = useState<ExitPhase>("choice");
  const [error, setError] = useState(false);
  const [failedTools, setFailedTools] = useState<string[]>([]);
  const restoreChoice = useRef(true);
  const submitting = useRef(false);

  const applyProgress = useCallback((raw: unknown) => {
    const response = exitResponse(raw);
    if (!mounted.current || !response) return false;
    setPhase(response.status);
    setFailedTools(response.failedTools ?? []);
    setError(response.status === "retryable_error");
    return true;
  }, []);

  const refreshState = useCallback(
    async (openIfRequested = false) => {
      try {
        const raw = await invoke<unknown>("read_desktop_exit_state");
        const state = exitState(raw);
        if (!mounted.current || !state) return;
        if (state.shutdown) {
          applyProgress(state.shutdown);
        }
        if (openIfRequested && state.closeRequested) setVisible(true);
      } catch {
        // A renderer-only preview has no native bridge. Explicit actions below
        // still report failure instead of pretending to exit.
      }
    },
    [applyProgress],
  );

  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    const install = async <T,>(
      event: string,
      callback: (payload: T) => void,
    ) => {
      try {
        const unlisten = await listen<T>(event, ({ payload }) => {
          if (!disposed) callback(payload);
        });
        if (disposed) unlisten();
        else unlisteners.push(unlisten);
      } catch {
        // Native close intent is retained for the mount/readback handshake.
      }
    };
    const openChoice = () => {
      setVisible(true);
      setError(false);
      void refreshState();
    };
    window.addEventListener(OPEN_CHOICE_EVENT, openChoice);
    void Promise.all([
      install(CLOSE_CHOICE_EVENT, openChoice),
      install<unknown>(EXIT_PROGRESS_EVENT, applyProgress),
    ]).then(() => {
      if (!disposed) void refreshState(true);
    });
    return () => {
      disposed = true;
      mounted.current = false;
      requestSequence.current += 1;
      window.removeEventListener(OPEN_CHOICE_EVENT, openChoice);
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [applyProgress, refreshState]);

  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (visible && !element.open) element.showModal();
    if (!visible && element.open) element.close();
  }, [visible]);

  useEffect(() => {
    if (
      phase !== "exiting" &&
      phase !== "restoring_settings" &&
      phase !== "finishing_operation"
    )
      return;
    // Native events are primary. This fallback keeps a missing IPC response
    // from leaving a permanently disabled or inescapable progress dialog.
    const timer = setInterval(() => {
      setPhase((current) =>
        current === "exiting" ? "finishing_operation" : current,
      );
      void refreshState();
    }, FINISHING_NOTICE_MS);
    return () => clearInterval(timer);
  }, [phase, refreshState]);

  const dismiss = () => {
    setVisible(false);
    void invoke("dismiss_desktop_exit_prompt").catch(() => {});
    // Hiding progress after confirmation never revokes shutdown or reopens
    // operation admission. Local cleanup still completes before native exit.
  };

  const background = async () => {
    try {
      await invoke("background_desktop_assistant");
      if (mounted.current) setVisible(false);
    } catch {
      if (mounted.current) setError(true);
    }
  };

  const confirmExit = async (restoreSettings = restoreChoice.current) => {
    if (submitting.current) return;
    submitting.current = true;
    restoreChoice.current = restoreSettings;
    const sequence = ++requestSequence.current;
    setError(false);
    setPhase("exiting");
    try {
      const response = await invoke<unknown>("quit_desktop_assistant", {
        restoreSettings,
      });
      if (!mounted.current || sequence !== requestSequence.current) return;
      if (!applyProgress(response)) {
        setPhase("retryable_error");
        setError(true);
      }
    } catch {
      if (mounted.current && sequence === requestSequence.current) {
        setPhase("retryable_error");
        setError(true);
      }
    } finally {
      submitting.current = false;
    }
  };

  return (
    <dialog
      className="restore-dialog"
      ref={dialog}
      aria-labelledby="quit-title"
      onCancel={(event) => {
        event.preventDefault();
        dismiss();
      }}
    >
      <h2 id="quit-title">
        {t(phase === "choice" ? "quit.titleChoice" : "quit.titleExiting")}
      </h2>
      {phase === "choice" ? (
        <>
          {/* 这个对话框积了两层东西：自动恢复本身，和当年「退不掉客户端」
              时期加的防御性说明。结果是八句话四个按钮，而其中大半在「没有
              连接需要助手运行」时根本不适用 —— 界面自己就那么写着。
              现在先说这次会发生什么，细则收进折叠里。 */}
          {unknown ? (
            <p>{t("quit.unknownAffected")}</p>
          ) : affected.length ? (
            <>
              <p>{t("quit.affectedIntro")}</p>
              <ul>
                {affected.map((connection) => (
                  <li key={connection.toolId}>
                    {WORKBENCH_APPS.find((app) => app.id === connection.toolId)
                      ?.name ?? connection.toolId}
                  </li>
                ))}
              </ul>
              <p>{t("quit.affectedHint")}</p>
            </>
          ) : (
            <p>{t("quit.noneAffected")}</p>
          )}
          <details className="quit-detail">
            <summary>{t("quit.detailSummary")}</summary>
            <p>{t("quit.detailClose")}</p>
            <p>{t("quit.detailKeep")}</p>
            <p>{t("quit.detailBackground")}</p>
          </details>
        </>
      ) : phase === "restore_failed" ? (
        <div role="alert">
          <p>{t("quit.restoreFailedIntro")}</p>
          <ul>
            {failedTools.map((tool) => (
              <li key={tool}>
                {WORKBENCH_APPS.find((app) => app.id === tool)?.name ?? tool}
              </li>
            ))}
          </ul>
          <p>{t("quit.restoreFailedHint")}</p>
        </div>
      ) : phase === "restoring_settings" ? (
        <p role="status">{t("quit.restoringSettings")}</p>
      ) : phase === "finishing_operation" ? (
        <p role="status">{t("quit.finishingOperation")}</p>
      ) : phase === "exiting" ? (
        <p role="status">{t("quit.exiting")}</p>
      ) : null}
      {error && (
        <p role="alert">
          {t(phase === "choice" ? "quit.errorBackground" : "quit.errorExit")}
        </p>
      )}
      {/* 四个同等重量的按钮里，用户真正要选的只有两个：走还是留。
          「保留接入并退出」是给知道自己在做什么的人的出口，降成文字按钮 ——
          但它必须还在：这个客户端有过退不掉的历史，任何一条退出路径都不删。 */}
      <footer>
        <button
          type="button"
          className="subtle-button"
          autoFocus
          onClick={dismiss}
        >
          {t(phase === "choice" ? "quit.cancel" : "quit.dismiss")}
        </button>
        {(phase === "choice" || phase === "restore_failed") && (
          <button
            type="button"
            className="text-button quit-keep-connected"
            onClick={() => {
              void confirmExit(false);
            }}
          >
            {t(
              phase === "choice" ? "quit.keepAndExit" : "quit.keepRestAndExit",
            )}
          </button>
        )}
        <button type="button" className="subtle-button" onClick={background}>
          {t(phase === "choice" ? "quit.background" : "quit.waitInBackground")}
        </button>
        {phase === "choice" ||
        phase === "restore_failed" ||
        phase === "retryable_error" ||
        phase === "finishing_operation" ? (
          <button
            type="button"
            className="primary-action"
            onClick={() => {
              void confirmExit(
                phase === "restore_failed" ? true : restoreChoice.current,
              );
            }}
          >
            {phase === "choice"
              ? t("quit.restoreAndExit")
              : phase === "restore_failed"
                ? t("quit.retryRestoreAndExit")
                : t("quit.retryExit")}
          </button>
        ) : (
          <button
            type="button"
            onClick={() => {
              void refreshState();
            }}
          >
            {t("quit.viewProgress")}
          </button>
        )}
      </footer>
    </dialog>
  );
}
