import { useEffect, useId, useRef, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  LoaderCircle,
  RotateCcw,
  X,
} from "lucide-react";
import { useConfigurationCopy } from "./copy";
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
  const c = useConfigurationCopy();
  const [open, setOpen] = useState(false);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);
  const [revokeKey, setRevokeKey] = useState(true);
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const revokeId = useId();
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
  const action = original ? c.restoreAction : c.revokeAction;
  const restore = async () => {
    if (!controller || !connection || busy) return;
    setFailed(false);
    try {
      const result = await controller.restore(connection.toolId, revokeKey);
      const error = result.status === "recovery_failed";
      setFailed(error);
      setMessage(
        error
          ? c.restoreFailed
          : result.reasonCode ===
              "local_settings_restored_token_cleanup_pending"
            ? c.tokenCleanupPending.replace("{{action}}", action)
            : result.reasonCode === "local_settings_restored_token_kept"
              ? c.tokenKept
              : result.status === "restored_with_changes"
                ? c.restoredWithChanges
                : original
                  ? c.restoredOriginal
                  : c.revokedLegacy,
      );
      if (!error) setOpen(false);
    } catch {
      setFailed(true);
      setMessage(c.restoreError);
    }
  };
  return (
    <div className="restore-control">
      {available && (
        <button
          className="restore-action"
          type="button"
          disabled={
            disabled || !!controller?.restoring || !!controller?.opening
          }
          onClick={() => {
            setMessage("");
            setRevokeKey(true);
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
            aria-label={c.closeLabel}
            disabled={busy}
            onClick={() => setOpen(false)}
          >
            <X />
          </button>
        </header>
        <h2 id={titleId}>
          {action} · {name}
        </h2>
        <p>{original ? c.introOriginal : c.introLegacy}</p>
        <ul>
          <li>{c.noteNoUninstall}</li>
          <li>{c.noteIsolated}</li>
          <li>{c.noteReopen.replace("{{app}}", name)}</li>
        </ul>
        <label className="restore-revoke-option" htmlFor={revokeId}>
          <input
            id={revokeId}
            type="checkbox"
            checked={revokeKey}
            disabled={busy}
            onChange={(event) => setRevokeKey(event.target.checked)}
          />
          <span>
            {c.revokeOptionTitle}
            <small>
              {revokeKey ? c.revokeOnHint : c.revokeOffHint}
            </small>
          </span>
        </label>
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
            {c.cancelRestore}
          </button>
          <button
            className="primary-action"
            type="button"
            disabled={busy}
            onClick={() => void restore()}
          >
            {busy ? <LoaderCircle className="is-spinning" /> : <RotateCcw />}
            {busy ? c.restoring : action}
          </button>
        </footer>
      </dialog>
    </div>
  );
}
