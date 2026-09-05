import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { LogOut } from "lucide-react";
import { useConnections } from "../configuration/connections";
import { WORKBENCH_APPS } from "../workbench/appCatalog";
export function QuitAssistant() {
  const connections = useConnections();
  const affected =
    connections?.connections.filter(
      (c) =>
        c.requiresBackground &&
        ["connected", "changed", "legacy"].includes(c.state),
    ) ?? [];
  const unknown = !connections || connections.loading || connections.error;
  const dialog = useRef<HTMLDialogElement>(null);
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);
  return (
    <div className="quit-assistant">
      <div>
        <strong>后台运行</strong>
        <p>关闭窗口会最小化，保持应用连接。需要完全退出时，请使用右侧按钮。</p>
      </div>
      <button type="button" onClick={() => dialog.current?.showModal()}>
        <LogOut />
        退出野菜助手
      </button>
      <dialog
        className="restore-dialog"
        ref={dialog}
        aria-labelledby="quit-title"
        onCancel={(event) => {
          if (busy) event.preventDefault();
        }}
      >
        <h2 id="quit-title">退出野菜助手？</h2>
        {unknown ? (
          <p>
            暂时无法确认哪些应用需要助手运行。完全退出可能中断正在使用的连接，设置不会被删除。
          </p>
        ) : affected.length ? (
          <>
            <p>以下应用的连接需要助手运行，退出后会暂时中断：</p>
            <ul>
              {affected.map((c) => (
                <li key={c.toolId}>
                  {WORKBENCH_APPS.find((a) => a.id === c.toolId)?.name ??
                    c.toolId}
                </li>
              ))}
            </ul>
            <p>再次使用时，打开野菜助手，从应用卡片继续使用。不用重新接入。</p>
          </>
        ) : (
          <p>
            当前没有发现需要助手运行的连接。退出不会删除应用设置和聊天记录。
          </p>
        )}
        {error && <p role="alert">暂时无法退出，请重试。</p>}
        <footer>
          <button
            autoFocus
            disabled={busy}
            onClick={() => dialog.current?.close()}
          >
            继续运行
          </button>
          <button
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              try {
                await invoke("quit_desktop_assistant");
              } catch {
                setError(true);
                setBusy(false);
              }
            }}
          >
            {busy ? "正在退出…" : "确认退出"}
          </button>
        </footer>
      </dialog>
    </div>
  );
}
