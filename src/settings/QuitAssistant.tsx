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
import { useConnections } from "../configuration/connections";
import { WORKBENCH_APPS } from "../workbench/appCatalog";

const OPEN_CHOICE_EVENT = "yeschoy:open-exit-choice";
const CLOSE_CHOICE_EVENT = "yeschoy://close-choice";
const EXIT_PROGRESS_EVENT = "yeschoy://exit-progress";
// This matches the native account request bound. It is only a progress notice,
// never a renderer instruction to interrupt a native transaction.
const FINISHING_NOTICE_MS = 20_000;

type ExitStatus = "exiting" | "finishing_operation" | "retryable_error";
type ExitPhase = "choice" | ExitStatus;
type ExitResponse = { status: ExitStatus };
type ExitState = { closeRequested: boolean; shutdown: ExitResponse | null };

function exitResponse(raw: unknown): ExitResponse | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const value = raw as Record<string, unknown>;
  if (
    Object.keys(value).length !== 1 ||
    !["exiting", "finishing_operation", "retryable_error"].includes(
      String(value.status),
    )
  )
    return null;
  return { status: value.status as ExitStatus };
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
  return (
    <div className="quit-assistant">
      <div>
        <strong>后台运行</strong>
        <p>
          关闭窗口时可选择后台运行或完全退出。完全退出会暂停需要助手的连接。
        </p>
      </div>
      <button
        type="button"
        onClick={() => window.dispatchEvent(new Event(OPEN_CHOICE_EVENT))}
      >
        <LogOut />
        退出野菜助手
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

  const applyProgress = useCallback((raw: unknown) => {
    const response = exitResponse(raw);
    if (!mounted.current || !response) return false;
    setPhase(response.status);
    setError(response.status === "retryable_error");
    return true;
  }, []);

  const refreshState = useCallback(async (openIfRequested = false) => {
    try {
      const raw = await invoke<unknown>("read_desktop_exit_state");
      const state = exitState(raw);
      if (!mounted.current || !state) return;
      if (state.shutdown) {
        setPhase(state.shutdown.status);
        setError(state.shutdown.status === "retryable_error");
      }
      if (openIfRequested && state.closeRequested) setVisible(true);
    } catch {
      // A renderer-only preview has no native bridge. Explicit actions below
      // still report failure instead of pretending to exit.
    }
  }, []);

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
    if (phase !== "exiting") return;
    // Native events are primary. This fallback keeps a missing IPC response
    // from leaving a permanently disabled or inescapable progress dialog.
    const timer = setTimeout(
      () => setPhase("finishing_operation"),
      FINISHING_NOTICE_MS,
    );
    return () => clearTimeout(timer);
  }, [phase]);

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

  const confirmExit = async () => {
    const sequence = ++requestSequence.current;
    setError(false);
    setPhase("exiting");
    try {
      const response = await invoke<unknown>("quit_desktop_assistant");
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
        {phase === "choice" ? "关闭野菜助手？" : "正在退出野菜助手"}
      </h2>
      {phase === "choice" ? (
        <>
          <p>后台运行会最小化窗口并保持连接；完全退出会停止助手的后台连接。</p>
          {unknown ? (
            <p>
              暂时无法确认哪些应用需要助手运行。完全退出可能中断正在使用的连接，设置不会被删除。
            </p>
          ) : affected.length ? (
            <>
              <p>以下应用的连接需要助手运行，退出后会暂时中断：</p>
              <ul>
                {affected.map((connection) => (
                  <li key={connection.toolId}>
                    {WORKBENCH_APPS.find((app) => app.id === connection.toolId)
                      ?.name ?? connection.toolId}
                  </li>
                ))}
              </ul>
              <p>
                再次使用时，打开野菜助手，从应用卡片继续使用。不用重新接入。
              </p>
            </>
          ) : (
            <p>
              当前没有发现需要助手运行的连接。退出不会删除应用设置和聊天记录。
            </p>
          )}
        </>
      ) : phase === "finishing_operation" ? (
        <p role="status">
          正在完成当前接入或账号操作，避免设置损坏。安全收尾后会自动退出；你可以收起提示或在后台等待。
        </p>
      ) : phase === "exiting" ? (
        <p role="status">
          正在安全结束操作并关闭助手的后台连接，不会强制关闭其他应用。
        </p>
      ) : null}
      {error && (
        <p role="alert">
          {phase === "choice"
            ? "暂时无法切换到后台，请重试。"
            : "暂时无法确认退出状态，请重试。不会强制中断正在保存的设置。"}
        </p>
      )}
      <footer>
        <button type="button" autoFocus onClick={dismiss}>
          {phase === "choice" ? "取消" : "收起提示"}
        </button>
        <button type="button" onClick={background}>
          {phase === "choice" ? "后台运行" : "在后台等待"}
        </button>
        {phase === "choice" ||
        phase === "retryable_error" ||
        phase === "finishing_operation" ? (
          <button type="button" onClick={confirmExit}>
            {phase === "choice" ? "确认退出" : "重试退出"}
          </button>
        ) : (
          <button
            type="button"
            onClick={() => {
              void refreshState();
            }}
          >
            查看退出进度
          </button>
        )}
      </footer>
    </dialog>
  );
}
