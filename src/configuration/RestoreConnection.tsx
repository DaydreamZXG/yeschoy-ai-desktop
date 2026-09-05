import { useEffect, useId, useRef, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  LoaderCircle,
  RotateCcw,
  X,
} from "lucide-react";
import { useConnections, type ToolConnection } from "./connections";

export function RestoreConnection({
  connection,
  name,
  disabled = false,
}: {
  connection?: ToolConnection;
  name: string;
  disabled?: boolean;
}) {
  const controller = useConnections();
  const [open, setOpen] = useState(false);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const cancel = useRef<HTMLButtonElement>(null);
  const busy = controller?.restoring === connection?.toolId;
  const original = connection?.restoreMode === "original";
  const available = connection && connection.restoreMode !== "none";
  useEffect(() => {
    if (open) {
      dialog.current?.showModal();
      cancel.current?.focus();
    } else dialog.current?.close();
  }, [open]);
  if (!available && !message) return null;
  const action = original ? "恢复原设置" : "撤销野菜设置";
  const restore = async () => {
    if (!controller || !connection || busy) return;
    setFailed(false);
    try {
      const result = await controller.restore(connection.toolId);
      const error = result.status === "recovery_failed";
      setFailed(error);
      setMessage(
        error
          ? "恢复没有完成，现有记录已保留。请关闭目标应用后重试；不会强行覆盖你的修改。"
          : result.status === "restored_with_changes"
            ? "已恢复可还原的设置，你之后修改的内容已保留。重新打开应用后生效。"
            : original
              ? "已恢复接入前的设置。重新打开应用后生效。"
              : "已撤销野菜接入。旧版本没有保存原值，请在应用中选择你要用的账户或服务商。",
      );
      if (!error) setOpen(false);
    } catch {
      setFailed(true);
      setMessage("暂时无法恢复，请重试。恢复记录仍保留在这台电脑上。");
    }
  };
  return (
    <div className="restore-control">
      {available && (
        <button
          className="restore-action"
          type="button"
          disabled={disabled || !!controller?.restoring || !!controller?.opening}
          onClick={() => {
            setMessage("");
            setOpen(true);
          }}
        >
          <RotateCcw aria-hidden="true" />
          {action}
        </button>
      )}
      {message && !open && (
        <p
          className={failed ? "restore-feedback is-error" : "restore-feedback"}
          role={failed ? "alert" : "status"}
        >
          {failed ? <AlertCircle /> : <CheckCircle2 />}
          {message}
        </p>
      )}
      <dialog
        ref={dialog}
        className="restore-dialog"
        aria-labelledby={titleId}
        onCancel={(event) => {
          if (busy) event.preventDefault();
          else setOpen(false);
        }}
        onClose={() => setOpen(false)}
      >
        <header>
          <span className="restore-symbol">
            <RotateCcw />
          </span>
          <button
            type="button"
            aria-label="关闭"
            disabled={busy}
            onClick={() => setOpen(false)}
          >
            <X />
          </button>
        </header>
        <h2 id={titleId}>
          {action} · {name}
        </h2>
        <p>
          {original
            ? "恢复这个应用接入野菜前的模型、服务商和相关连接设置。你后来修改过的内容会保留。"
            : "这个接入来自旧版本，没有保存接入前的设置。只能撤销仍属于野菜的连接项，无法找回原来的值。"}
        </p>
        <ul>
          <li>不会卸载应用，也不会删除聊天记录。</li>
          <li>不影响其他应用、网站账户或余额。</li>
          <li>恢复后请重新打开 {name}。</li>
        </ul>
        {message && (
          <p className="restore-feedback is-error" role="alert">
            {message}
          </p>
        )}
        <footer>
          <button
            ref={cancel}
            type="button"
            disabled={busy}
            onClick={() => setOpen(false)}
          >
            先不恢复
          </button>
          <button
            className="primary-action"
            type="button"
            disabled={busy}
            onClick={() => void restore()}
          >
            {busy ? <LoaderCircle className="is-spinning" /> : <RotateCcw />}
            {busy ? "正在恢复…" : action}
          </button>
        </footer>
      </dialog>
    </div>
  );
}
