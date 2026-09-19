import { useEffect, useId, useRef, useState } from "react";
import { ArrowUpRight, Check, LoaderCircle, Terminal, X } from "lucide-react";
import { useConnections, type ToolConnection } from "./connections";
import { TERMINAL_TOOLS, type OpenStatus } from "./launchApi";
import type { ActivationToolId } from "./activation";
import { useTranslation } from "react-i18next";
import { RecoveryNotice } from "./RecoveryNotice";

const messages: Record<OpenStatus, string> = {
  opened: "已发送打开请求，不会重新配置或发送测试消息。",
  not_connected: "接入信息已移除，请重新接入。",
  settings_changed:
    "应用的连接设置已变化，没有覆盖你的修改。可调整接入，或恢复原设置。",
  recovery_pending:
    "上次接入尚未完成。请进入接入设置重新接入，助手会先自动恢复未完成的操作，无需手动修改配置。",
  secure_storage_unavailable:
    "暂时无法读取系统保存的接入信息。请解锁系统钥匙串或凭据管理器后重试。",
  tool_not_found: "没有找到可以启动的应用，请检查安装后重试。",
  launch_failed:
    "没能打开应用或终端。请确认应用可以手动打开、未被系统阻止，再重新检查。",
  busy: "另一个应用操作还在进行，结束后可重试。",
};

/**
 * 「打开使用」的失败原因。原生侧过去把每一种失败都压成 `launch_failed`，这里
 * 只能说同一句「请确认应用可以手动打开」——对 Claude Code 和 Pi 尤其不对：
 * 它们是终端工具，助手的价值就是替用户把带参数的终端拉起来，「手动打开应用」
 * 并不是可执行的下一步。没列到的原因仍然回落到 `messages.launch_failed`。
 */
const launchReasons: Record<string, string> = {
  terminal_launch_failed:
    "没能打开终端窗口。接入设置没有改动，你可以自己打开终端启动它，或者重试。",
  terminal_unavailable:
    "没有找到可用的终端程序。macOS 请确认「终端」还在，Windows 请确认系统目录可以正常访问。接入设置没有改动。",
  workspace_unavailable:
    "工作目录不可用，可能已被移动、删除或没有写入权限。请在接入设置里重新选择工作目录。",
  invalid_launch_target:
    "这个应用的安装路径无法使用（不是绝对路径，或含有特殊字符）。请把它安装到常规位置后重新检查应用。",
  launch_target_missing:
    "没有找到这个应用的可执行文件，它可能已被移动或卸载。请重新检查应用。",
  desktop_launch_start_failed:
    "这个应用没能启动。请确认它还在原来的位置、并且可以手动打开。",
  desktop_launch_exit_failed:
    "这个应用没能退出，新设置要重开之后才生效。请手动退出它再试一次。",
  desktop_launch_wait_failed:
    "没能确认这个应用是否已经退出。请手动确认它已关闭，再试一次。",
  desktop_launch_access_denied:
    "系统阻止了这次启动。请在「系统设置 → 隐私与安全性」里允许它运行，或者手动打开一次。",
  desktop_start_unconfirmed:
    "没能确认这个应用是否已经打开。请手动看一下；接入设置没有改动。",
  desktop_launch_target_changed:
    "这个应用的安装位置刚刚发生了变化。请重新检查应用后再打开。",
};

function openMessage(status: OpenStatus, reasonCode: string): string {
  if (status === "launch_failed" && launchReasons[reasonCode])
    return launchReasons[reasonCode];
  return messages[status];
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
  const { t } = useTranslation();
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
        {busy ? "正在打开…" : terminal ? "打开终端使用" : "打开使用"}
      </button>
      {terminal && (
        <button
          type="button"
          className="text-button"
          onClick={() => dialog.current?.showModal()}
        >
          使用说明
        </button>
      )}
      {status === "opened" ? (
        <div className="open-feedback" role="status">
          <Check aria-hidden="true" />
          <span>
            {terminal
              ? "已请求打开终端，请在新窗口中使用。不会重新配置或发送测试消息。"
              : messages.opened}
          </span>
        </div>
      ) : (
        status && (
          <RecoveryNotice
            className="open-feedback"
            title={t("yeschoyDaily.openUnconfirmed")}
            action={{
              label:
                status === "recovery_pending" ? "前往接入修复" : "查看接入设置",
              run: onAdjust,
            }}
          >
            <p>
              {status === "error"
                ? "打开结果暂时无法确认，可以重试；未重新配置应用。"
                : status === "timed_out"
                  ? "等待应用响应超时，页面已经恢复操作。应用仍可能稍后打开；若没有，请重试。接入设置没有被修改。"
                  : openMessage(status, reasonCode)}
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
            <h2 id={titleId}>使用 {name}</h2>
            <button
              aria-label="关闭使用说明"
              onClick={() => dialog.current?.close()}
            >
              <X />
            </button>
          </header>
          <p>
            {name}{" "}
            在终端中运行，不会出现独立桌面窗口。点击“打开终端使用”，助手会为你启动它；也可以在项目文件夹的终端中运行：
          </p>
          <code className="terminal-use-command">{terminalCommand}</code>
          <p>
            模型和接入信息已保存在应用中，不需要复制密钥。已有会话可能仍沿用原设置，请新建会话后使用。
          </p>
          {connection.requiresBackground && <p>使用期间请保持野菜助手运行。</p>}
          <footer>
            <button autoFocus onClick={() => dialog.current?.close()}>
              知道了
            </button>
          </footer>
        </dialog>
      )}
    </div>
  );
}
