import { useEffect, useState } from "react";
import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import { askToQuit } from "./Shutdown";

// 窗口没有系统标题栏：画面顶到最上面，这一条高度以内按住就能拖动窗口。
const GRIP_HEIGHT = 36;
// 按下后移动超过这么多才算拖动，顶到最上面的角色条和按钮照常能点。
const GRIP_SLOP = 4;

export type WindowFrame = "own" | "mac";

/** macOS 留着系统的红黄绿三个圆点；别的系统由我们自己画窗口按钮。 */
export function windowFrame(): WindowFrame {
  return /Mac/i.test(navigator.userAgent) ? "mac" : "own";
}

function nativeWindow(): Window | null {
  try {
    return getCurrentWindow();
  } catch {
    // 不在客户端窗口里（比如测试）。
    return null;
  }
}

const quiet = (work: Promise<unknown> | undefined) =>
  void work?.catch(() => undefined);

function typing(target: EventTarget | null) {
  return (
    target instanceof Element &&
    !!target.closest("input, textarea, select, [contenteditable], .wc")
  );
}

function pressable(target: EventTarget | null) {
  return target instanceof Element && !!target.closest("button, a");
}

export function WindowBar({ frame }: { frame: WindowFrame }) {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let from: { x: number; y: number } | null = null;
    const down = (event: MouseEvent) => {
      from =
        event.button === 0 &&
        event.clientY < GRIP_HEIGHT &&
        !typing(event.target)
          ? { x: event.clientX, y: event.clientY }
          : null;
    };
    const move = (event: MouseEvent) => {
      if (!from) return;
      if (event.buttons !== 1) {
        from = null;
        return;
      }
      if (
        Math.hypot(event.clientX - from.x, event.clientY - from.y) < GRIP_SLOP
      )
        return;
      from = null;
      quiet(nativeWindow()?.startDragging());
    };
    const up = () => {
      from = null;
    };
    const twice = (event: MouseEvent) => {
      if (
        event.clientY < GRIP_HEIGHT &&
        !typing(event.target) &&
        !pressable(event.target)
      )
        quiet(nativeWindow()?.toggleMaximize());
    };
    document.addEventListener("mousedown", down);
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", up);
    document.addEventListener("dblclick", twice);
    return () => {
      document.removeEventListener("mousedown", down);
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", up);
      document.removeEventListener("dblclick", twice);
    };
  }, []);

  useEffect(() => {
    if (frame !== "own") return;
    const native = nativeWindow();
    if (!native) return;
    let live = true;
    const read = () =>
      void native
        .isMaximized()
        .then((value) => {
          if (live) setMaximized(value);
        })
        .catch(() => undefined);
    read();
    const stop = native.onResized(read);
    return () => {
      live = false;
      void stop.then((off) => off()).catch(() => undefined);
    };
  }, [frame]);

  if (frame !== "own") return null;
  return (
    <div className="wc" data-testid="kaifan-window-controls">
      <button
        type="button"
        aria-label={t("kaifan.window.minimize")}
        onClick={() => quiet(nativeWindow()?.minimize())}
      >
        <svg viewBox="0 0 12 12" aria-hidden="true">
          <path d="M1.5 6.5h9" />
        </svg>
      </button>
      <button
        type="button"
        aria-label={t(
          maximized ? "kaifan.window.restore" : "kaifan.window.maximize",
        )}
        onClick={() => quiet(nativeWindow()?.toggleMaximize())}
      >
        <svg viewBox="0 0 12 12" aria-hidden="true">
          {maximized ? (
            <path d="M3.5 3.5v-2h7v7h-2M1.5 3.5h7v7h-7z" />
          ) : (
            <path d="M1.5 1.5h9v9h-9z" />
          )}
        </svg>
      </button>
      <button
        type="button"
        className="shut"
        aria-label={t("kaifan.window.close")}
        // 直接问界面里那个退出框；不走系统关窗，否则 Windows 上弹的是系统自带的提示框。
        onClick={askToQuit}
      >
        <svg viewBox="0 0 12 12" aria-hidden="true">
          <path d="M1.5 1.5l9 9M10.5 1.5l-9 9" />
        </svg>
      </button>
    </div>
  );
}
