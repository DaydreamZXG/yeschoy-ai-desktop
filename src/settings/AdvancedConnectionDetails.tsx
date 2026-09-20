import { useState } from "react";
import i18n from "i18next";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import {
  CONFIGURATION_LINES,
  CONFIGURATION_TOOLS,
  type ConfigurationToolId,
} from "../configuration/preview";
import {
  connectionLabel,
  useConnections,
  type ToolConnection,
} from "../configuration/connections";
import type { ActivationToolId } from "../configuration/activation";
import { WORKBENCH_APPS } from "../workbench/appCatalog";
import { RecentRequest } from "../configuration/RecentRequest";

/**
 * #11 PRD 6.8 高级模式：设置页折叠区，展示已接入应用的实际连接参数。
 * 只读、默认折叠、不提供编辑。密钥本体存于系统安全存储，界面仅说明
 * 存放位置与撤销路径（掩码/创建时间属服务端数据，按客户端自治原则范围缩减）。
 */
const TOOL_INFO: Record<
  ActivationToolId,
  { previewId?: ConfigurationToolId; appType?: string }
> = {
  claude_desktop: { appType: "claude-desktop" },
  codex_desktop: { previewId: "codex", appType: "codex" },
  dsh_web: { previewId: "dsh" },
  pi: { previewId: "pi", appType: "pi" },
  claude_code: { previewId: "claude", appType: "claude" },
  workbuddy: { previewId: "workbuddy" },
};

const CONNECTED_STATES = ["connected", "changed", "legacy"] as const;

function toolName(toolId: ActivationToolId) {
  return WORKBENCH_APPS.find((app) => app.id === toolId)?.name ?? toolId;
}

function baseUrlOf(connection: ToolConnection) {
  const info = TOOL_INFO[connection.toolId];
  if (!connection.lineId || !info?.previewId) return null;
  const tool = CONFIGURATION_TOOLS.find((t) => t.id === info.previewId);
  const line = CONFIGURATION_LINES.find((l) => l.id === connection.lineId);
  return tool && line ? `${line.rootUrl}${tool.protocolSuffix}` : null;
}

function diagnosticsText(connection: ToolConnection, requestId?: string) {
  // 这份报告是给客服看的，所以跟着界面语言走：用英文界面的人复制出来的是英文。
  const d = (key: string) => i18n.t(`advancedDetails.${key}`);
  const info = TOOL_INFO[connection.toolId];
  const targetFile = info?.previewId
    ? (CONFIGURATION_TOOLS.find((t) => t.id === info.previewId)?.targetFile ??
      d("notRecorded"))
    : d("folderLaunch");
  const last = connection.lastRequest;
  return [
    d("reportTitle"),
    `${d("tool")}: ${toolName(connection.toolId)} (${connection.toolId})`,
    `${d("state")}: ${connectionLabel(connection.state)} (${connection.state})`,
    `Base URL: ${baseUrlOf(connection) ?? d("noLine")}`,
    `${d("model")}: ${connection.modelId || d("noModel")}`,
    `${d("billingGroup")}: ${connection.billingGroup || d("defaultGroup")}`,
    `${d("configFile")}: ${targetFile}`,
    `${d("lastWrite")}: ${
      connection.updatedAtEpochMs
        ? new Date(connection.updatedAtEpochMs).toISOString()
        : d("none")
    }`,
    last
      ? `${d("lastRequest")}: ${last.outcome} · HTTP ${last.httpStatus} · ${new Date(
          last.observedAtEpochMs,
        ).toISOString()}`
      : `${d("lastRequest")}: ${d("none")}`,
    `${d("readId")}: ${requestId ?? d("none")}`,
  ].join("\n");
}

export function AdvancedConnectionDetails() {
  const { t } = useTranslation();
  const connections = useConnections();
  const [revealFailed, setRevealFailed] = useState<ActivationToolId | null>(
    null,
  );
  const [copied, setCopied] = useState(false);

  const connected = (connections?.connections ?? []).filter((connection) =>
    (CONNECTED_STATES as readonly string[]).includes(connection.state),
  );

  const reveal = async (toolId: ActivationToolId) => {
    const appType = TOOL_INFO[toolId]?.appType;
    if (!appType) return;
    setRevealFailed(null);
    try {
      await invoke("open_config_folder", { app: appType });
    } catch {
      setRevealFailed(toolId);
    }
  };

  const copy = async (connection: ToolConnection) => {
    try {
      await navigator.clipboard.writeText(
        diagnosticsText(connection, connections?.requestId),
      );
      setCopied(true);
      setTimeout(() => setCopied(false), 2500);
    } catch {
      setCopied(false);
    }
  };

  return (
    <details className="advanced-connection-details">
      <summary>
        <span>{t("yeschoySettings.advanced.summary")}</span>
        <small>{t("yeschoySettings.advanced.hint")}</small>
      </summary>
      {connected.length === 0 ? (
        <p className="advanced-empty">{t("yeschoySettings.advanced.empty")}</p>
      ) : (
        <div className="advanced-tool-list">
          {connected.map((connection) => {
            const info = TOOL_INFO[connection.toolId];
            const targetFile = info?.previewId
              ? CONFIGURATION_TOOLS.find((t) => t.id === info.previewId)
                  ?.targetFile
              : undefined;
            return (
              <article key={connection.toolId} className="advanced-tool-card">
                <header>
                  <h3>{toolName(connection.toolId)}</h3>
                  <span>{connectionLabel(connection.state)}</span>
                  <button
                    type="button"
                    className="text-button"
                    onClick={() => void copy(connection)}
                  >
                    {copied
                      ? t("yeschoySettings.advanced.copied")
                      : t("yeschoySettings.advanced.copy")}
                  </button>
                </header>
                <dl>
                  <div>
                    <dt>{t("yeschoySettings.advanced.baseUrl")}</dt>
                    <dd>
                      <code>{baseUrlOf(connection)}</code>
                    </dd>
                  </div>
                  <div>
                    <dt>{t("yeschoySettings.advanced.model")}</dt>
                    <dd>
                      <code>{connection.modelId || "—"}</code>
                    </dd>
                  </div>
                  <div>
                    <dt>{t("yeschoySettings.advanced.targetFile")}</dt>
                    <dd>
                      {targetFile ? (
                        <code>{targetFile}</code>
                      ) : connection.toolId === "dsh_web" ? (
                        <span>{t("yeschoySettings.advanced.dshFileNote")}</span>
                      ) : (
                        <span>{t("yeschoySettings.advanced.folderNote")}</span>
                      )}
                      {info?.appType && (
                        <button
                          type="button"
                          className="text-button"
                          onClick={() => void reveal(connection.toolId)}
                        >
                          {t("yeschoySettings.advanced.reveal")}
                        </button>
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("yeschoySettings.advanced.key")}</dt>
                    <dd>
                      <span>{t("yeschoySettings.advanced.keyNote")}</span>
                    </dd>
                  </div>
                  <div>
                    <dt>{t("yeschoySettings.advanced.lastResult")}</dt>
                    <dd>
                      {connection.updatedAtEpochMs
                        ? new Date(connection.updatedAtEpochMs).toLocaleString()
                        : t("yeschoySettings.advanced.never")}
                    </dd>
                  </div>
                </dl>
                {revealFailed === connection.toolId && (
                  <p className="advanced-reveal-failed" role="alert">
                    {t("yeschoySettings.advanced.revealFailed")}
                  </p>
                )}
                <RecentRequest
                  value={connection.lastRequest}
                  toolId={connection.toolId}
                />
              </article>
            );
          })}
        </div>
      )}
    </details>
  );
}
