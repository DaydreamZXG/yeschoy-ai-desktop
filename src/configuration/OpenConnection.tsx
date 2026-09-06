import { useEffect, useId, useRef, useState } from "react";
import { ArrowUpRight, Check, LoaderCircle, Terminal, X } from "lucide-react";
import { useConnections, type ToolConnection } from "./connections";
import { TERMINAL_TOOLS, type OpenStatus } from "./launchApi";
import type { ActivationToolId } from "./activation";

const messages: Record<OpenStatus, string> = {
  opened: "已发送打开请求，不会重新配置或发送测试消息。",
  not_connected: "接入信息已移除，请重新接入。",
  settings_changed:
    "应用的连接设置已变化，没有覆盖你的修改。可调整接入，或恢复原设置。",
  recovery_pending: "上次操作尚未结束，请先恢复原设置。",
  secure_storage_unavailable:
    "暂时无法读取系统保存的接入信息。请解锁系统钥匙串或凭据管理器后重试。",
  tool_not_found: "没有找到可以启动的应用，请检查安装后重试。",
  launch_failed:
    "没能打开应用或终端。请确认应用可以手动打开、未被系统阻止，再重新检查。",
  busy: "另一个应用操作还在进行，结束后可重试。",
};
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
  const controller = useConnections();
  const [status, setStatus] = useState<OpenStatus | "error" | null>(null);
  const inFlight = useRef(false);
  const identity = `${connection.toolId}:${connection.modelId}:${connection.lineId}:${connection.billingGroup}:${connection.updatedAtEpochMs}`;
  const latestIdentity = useRef(identity);
  latestIdentity.current = identity;
  useEffect(() => {
    setStatus(null);
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
      hermes: "hermes",
      openclaw: "openclaw",
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
          try {
            const result = await controller.open(connection.toolId);
            if (latestIdentity.current === openingIdentity) setStatus(result);
          } catch {
            if (latestIdentity.current === openingIdentity) setStatus("error");
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
      {status && (
        <div
          className="open-feedback"
          role={status === "opened" ? "status" : "alert"}
        >
          {status === "opened" && <Check />}
          <span>
            {status === "error"
              ? "打开结果暂时无法确认，可以重试；未重新配置应用。"
              : status === "opened" && terminal
                ? "已请求打开终端，请在新窗口中使用。不会重新配置或发送测试消息。"
                : messages[status]}
          </span>
          {status !== "opened" && (
            <button className="text-button" onClick={onAdjust}>
              查看接入设置
            </button>
          )}
        </div>
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
