import { useEffect, useId, useRef, useState } from "react";
import { ArrowUpRight, Check, LoaderCircle, Terminal, X } from "lucide-react";
import { useConnections, type ToolConnection } from "./connections";
import { TERMINAL_TOOLS, type OpenStatus } from "./launchApi";
import type { ActivationToolId } from "./activation";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { RecoveryNotice } from "./RecoveryNotice";

/**
 * 「打开使用」的失败原因。原生侧过去把每一种失败都压成 `launch_failed`，只能说
 * 同一句「请确认应用可以手动打开」——对 Claude Code 和 Pi 尤其不对：它们是终端
 * 工具，助手的价值就是替用户把带参数的终端拉起来，「手动打开应用」并不是可执行的
 * 下一步。`openConnection.launchReason` 里没有的原因回落到 `status.launch_failed`。
 *
 * 用 `i18n.exists` 判断，而不是在这里另留一张认得的原因码清单：那张清单和语言
 * 文件必然漂移，而这样只有一个来源。
 */
function openMessage(
  status: OpenStatus,
  reasonCode: string,
  t: TFunction,
  i18n: { exists: (key: string) => boolean },
): string {
  const reasonKey = `openConnection.launchReason.${reasonCode}`;
  if (status === "launch_failed" && i18n.exists(reasonKey)) return t(reasonKey);
  return t(`openConnection.status.${status}`);
}
export function OpenConnection({
  connection,
  name,
  onAdjust,
  disabled = false,
}: {
  connection: ToolConnection;
  name: string;
  onAdjust: () => void;
  disabled?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const controller = useConnections();
  const [status, setStatus] = useState<
    OpenStatus | "error" | "timed_out" | null
  >(null);
  const [reasonCode, setReasonCode] = useState("");
  const inFlight = useRef(false);
  const identity = `${connection.toolId}:${connection.modelId}:${connection.lineId}:${connection.billingGroup}:${connection.updatedAtEpochMs}`;
  const latestIdentity = useRef(identity);
  latestIdentity.current = identity;
  useEffect(() => {
    setStatus(null);
    setReasonCode("");
  }, [identity]);
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const terminal = (TERMINAL_TOOLS as readonly string[]).includes(
    connection.toolId,
  );
  const busy = controller?.opening === connection.toolId;
  const terminalCommand = (
    {
      claude_code: "claude",
      pi: "pi",
    } as Partial<Record<ActivationToolId, string>>
  )[connection.toolId];
  return (
    <div className="open-connection-control">
      <button
        type="button"
        className="primary-action"
        disabled={
          disabled ||
          !!controller?.opening ||
          !!controller?.restoring ||
          !controller
        }
        onClick={async () => {
          if (inFlight.current || !controller) return;
          const openingIdentity = identity;
          inFlight.current = true;
          setStatus(null);
          setReasonCode("");
          try {
            const result = await controller.open(connection.toolId);
            if (latestIdentity.current === openingIdentity) {
              setStatus(result.status);
              setReasonCode(result.reasonCode);
            }
          } catch (cause) {
            if (latestIdentity.current === openingIdentity)
              setStatus(
                cause instanceof Error &&
                  cause.message === "open_request_timed_out"
                  ? "timed_out"
                  : "error",
              );
          } finally {
            inFlight.current = false;
          }
        }}
      >
        {busy ? (
          <LoaderCircle className="is-spinning" />
        ) : terminal ? (
          <Terminal />
        ) : (
          <ArrowUpRight />
        )}
        {busy
          ? t("openConnection.opening")
          : terminal
            ? t("openConnection.openTerminal")
            : t("openConnection.open")}
      </button>
      {terminal && (
        <button
          type="button"
          className="text-button"
          onClick={() => dialog.current?.showModal()}
        >
          {t("openConnection.howToUse")}
        </button>
      )}
      {status === "opened" ? (
        <div className="open-feedback" role="status">
          <Check aria-hidden="true" />
          <span>
            {terminal
              ? t("openConnection.openedTerminal")
              : t("openConnection.status.opened")}
          </span>
        </div>
      ) : (
        status && (
          <RecoveryNotice
            className="open-feedback"
            title={t("yeschoyDaily.openUnconfirmed")}
            action={{
              label: t(
                status === "recovery_pending"
                  ? "openConnection.actionRepair"
                  : "openConnection.actionSettings",
              ),
              run: onAdjust,
            }}
          >
            <p>
              {status === "error"
                ? t("openConnection.unconfirmedError")
                : status === "timed_out"
                  ? t("openConnection.unconfirmedTimeout")
                  : openMessage(status, reasonCode, t, i18n)}
            </p>
          </RecoveryNotice>
        )
      )}
      {terminal && (
        <dialog
          ref={dialog}
          className="restore-dialog"
          aria-labelledby={titleId}
        >
          <header>
            <h2 id={titleId}>{t("openConnection.dialogTitle", { name })}</h2>
            <button
              aria-label={t("openConnection.dialogClose")}
              onClick={() => dialog.current?.close()}
            >
              <X />
            </button>
          </header>
          <p>{t("openConnection.dialogIntro", { name })}</p>
          <code className="terminal-use-command">{terminalCommand}</code>
          <p>{t("openConnection.dialogSaved")}</p>
          {connection.requiresBackground && (
            <p>{t("openConnection.dialogBackground")}</p>
          )}
          <footer>
            <button autoFocus onClick={() => dialog.current?.close()}>
              {t("openConnection.dialogDismiss")}
            </button>
          </footer>
        </dialog>
      )}
    </div>
  );
}
