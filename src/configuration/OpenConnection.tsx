import { useId, useRef, useState } from "react";
import { ArrowUpRight, Check, LoaderCircle, Terminal, X } from "lucide-react";
import { useConnections, type ToolConnection } from "./connections";
import { OPENABLE_TOOLS, type OpenStatus } from "./launchApi";
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
  launch_failed: "没能打开应用。请确认应用安装完整、未被系统阻止，再重试。",
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
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const openable = (OPENABLE_TOOLS as readonly string[]).includes(
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
          if (!openable) {
            dialog.current?.showModal();
            return;
          }
          if (inFlight.current || !controller) return;
          inFlight.current = true;
          setStatus(null);
          try {
            setStatus(await controller.open(connection.toolId));
          } catch {
            setStatus("error");
          } finally {
            inFlight.current = false;
          }
        }}
      >
        {busy ? (
          <LoaderCircle className="is-spinning" />
        ) : openable ? (
          <ArrowUpRight />
        ) : (
          <Terminal />
        )}
        {busy ? "正在打开…" : openable ? "打开使用" : "使用方式"}
      </button>
      {status && (
        <div
          className="open-feedback"
          role={status === "opened" ? "status" : "alert"}
        >
          {status === "opened" && <Check />}
          <span>
            {status === "error"
              ? "打开结果暂时无法确认，可以重试；未重新配置应用。"
              : messages[status]}
          </span>
          {status !== "opened" && (
            <button className="text-button" onClick={onAdjust}>
              查看接入设置
            </button>
          )}
        </div>
      )}
      {!openable && (
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
            在终端中运行，不会出现独立桌面窗口。打开电脑的终端，在你的项目文件夹中运行：
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
