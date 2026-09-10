import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowRight,
  CircleAlert,
  Globe2,
  Plus,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { AccountSummary } from "./WorkbenchChrome";
import { AppGlyph } from "./AppGlyph";
import { WORKBENCH_APPS } from "./appCatalog";
import type { AccountSessionController } from "../account/useAccountSession";
import {
  scanActivationTargets,
  type ActivationTargetScan,
  type ActivationToolId,
} from "../configuration/activation";
import { connectionLabel, useConnections } from "../configuration/connections";
import { RestoreConnection } from "../configuration/RestoreConnection";
import { OpenConnection } from "../configuration/OpenConnection";
import { groupLabel } from "../configuration/BillingGroupPicker";
import { RecentRequest } from "../configuration/RecentRequest";
import { ConnectionStatusNotice } from "../configuration/ConnectionStatusNotice";

interface Props {
  onOpenAccount: () => void;
  onOpenSetup: (appId: ActivationToolId) => void;
  onOpenDiagnostics: () => void;
  accountSession: AccountSessionController;
}
export function AppLibraryView({
  onOpenAccount,
  onOpenSetup,
  onOpenDiagnostics,
  accountSession,
}: Props) {
  const connections = useConnections();
  const [scan, setScan] = useState<ActivationTargetScan | null>(null);
  const [scanning, setScanning] = useState(true);
  const [scanError, setScanError] = useState(false);
  const [showAllApps, setShowAllApps] = useState(false);
  const sequence = useRef(0);
  const refresh = useCallback(async () => {
    const current = ++sequence.current;
    setScanning(true);
    try {
      const result = await scanActivationTargets();
      if (current === sequence.current) {
        setScan(result);
        setScanError(false);
      }
    } catch {
      if (current === sequence.current) setScanError(true);
    } finally {
      if (current === sequence.current) setScanning(false);
    }
  }, []);
  useEffect(() => {
    void refresh();
    return () => {
      ++sequence.current;
    };
  }, [refresh]);
  const signedIn = accountSession.projection?.status === "signed_in";
  const configured =
    connections?.connections.filter((v) =>
      ["connected", "legacy", "changed"].includes(v.state),
    ).length ?? 0;
  const hasConnectionSnapshot = !!connections?.connections.length;
  const partialConnections = connections?.connections.some(
    (c) => c.state === "unavailable",
  );
  const staleConnections = !!connections?.error && hasConnectionSnapshot;
  const checking = scanning || connections?.loading || connections?.refreshing;
  const detected = scan?.targets.filter((v) => v.status !== "not_found").length;
  const apps = [...WORKBENCH_APPS].sort((a, b) => {
    const rank = (id: ActivationToolId) => {
      const connection = connections?.connections.find((v) => v.toolId === id);
      if (
        connection &&
        connection.state !== "not_connected" &&
        connection.state !== "unavailable"
      )
        return 0;
      return scan?.targets.find((v) => v.toolId === id)?.status === "not_found"
        ? 2
        : 1;
    };
    return rank(a.id) - rank(b.id);
  });
  const visibleApps = apps.filter(
    (app) =>
      showAllApps ||
      !scan ||
      scanError ||
      (detected === 0 &&
        ["claude_desktop", "codex_desktop"].includes(app.id)) ||
      scan.targets.find((target) => target.toolId === app.id)?.status !==
        "not_found" ||
      connections?.connections.some(
        (c) =>
          c.toolId === app.id &&
          !["not_connected", "unavailable"].includes(c.state),
      ),
  );
  return (
    <div
      className="workbench-page desktop-home"
      data-testid="candidate-home-view"
    >
      <header className="workbench-page-heading">
        <div>
          <p className="eyebrow">野菜 API · 你的 AI 工作台</p>
          <h1>我的应用</h1>
          <p>
            {!hasConnectionSnapshot
              ? "先确认本机应用的接入状态，再继续使用或调整设置。"
              : configured
                ? "从这里打开应用，接着上次的工作。"
                : "选一个应用，安装并连接你想用的模型。"}
          </p>
        </div>
        <button
          className="subtle-button"
          type="button"
          disabled={checking}
          onClick={() => {
            void refresh();
            void connections?.refresh();
          }}
        >
          <RefreshCw className={checking ? "is-spinning" : ""} />
          {checking ? "正在检查" : "检查应用"}
        </button>
      </header>
      {signedIn && (
        <AccountSummary
          detected={detected}
          scanning={scanning}
          onOpenAccount={onOpenAccount}
          onOpenApps={() => onOpenSetup("claude_desktop")}
          accountProjection={accountSession.projection}
          accountLoading={accountSession.loading}
        />
      )}
      {accountSession.lastError && (
        <p className="workbench-notice" role="status">
          <CircleAlert />
          {signedIn
            ? "账户数据暂未更新，保留上次结果。"
            : "暂时无法获取账户状态，请重试。"}
          <button onClick={() => void accountSession.refresh()}>重试</button>
        </p>
      )}
      {!signedIn ? (
        <section className="welcome-strip">
          <span className="welcome-symbol">
            <ShieldCheck />
          </span>
          <div>
            <h2>登录一次，接入你的 AI 应用</h2>
            <p>自动获取可用模型和账户价格，不用复制密钥。恢复设置无需登录。</p>
          </div>
          <button className="primary-action" onClick={onOpenAccount}>
            登录野菜 API
            <ArrowRight />
          </button>
        </section>
      ) : (
        <div className="library-heading">
          <span>
            {!hasConnectionSnapshot ||
            (partialConnections && configured === 0) ? (
              connections?.loading ? (
                "正在读取接入状态"
              ) : (
                "接入状态待确认"
              )
            ) : (
              <>
                {staleConnections
                  ? "上次确认 "
                  : partialConnections
                    ? "已确认 "
                    : ""}
                <b>{configured}</b> 个应用已接入
                {partialConnections ? "，部分状态待确认" : ""}
              </>
            )}{" "}
            <span className="quiet-separator">/</span> {detected ?? "—"}{" "}
            个已发现
          </span>
          <button className="text-button" onClick={onOpenDiagnostics}>
            <Globe2 />
            连接诊断
          </button>
        </div>
      )}
      {scanError && (
        <p className="workbench-notice" role="alert">
          <CircleAlert />
          暂时无法确认已安装的应用，请点击“检查应用”重试。接入设置不会因此删除。
        </p>
      )}
      {connections && (connections.error || partialConnections) && (
        <ConnectionStatusNotice
          issue={connections.errorInfo}
          stale={staleConnections}
          refreshing={connections.refreshing}
          onRetry={() => void connections.refresh()}
        />
      )}
      {!scanning && !scanError && visibleApps.length === 0 && (
        <section className="library-empty">
          <h2>还没有找到 AI 应用</h2>
          <p>
            先安装一个想用的应用，再点“检查应用”。已有接入的设置和恢复记录不会因此删除。
          </p>
          <button
            className="subtle-button"
            onClick={() => setShowAllApps(true)}
          >
            查看支持的应用
          </button>
        </section>
      )}
      <section
        className="connection-library"
        aria-label="本机应用"
        aria-busy={scanning}
      >
        {visibleApps.map((app) => {
          const target = scan?.targets.find((v) => v.toolId === app.id);
          const connection = connections?.connections.find(
            (v) => v.toolId === app.id,
          );
          const active =
            connection &&
            !["not_connected", "unavailable"].includes(connection.state);
          const installed = target && target.status !== "not_found";
          const unknownConnection =
            !connection || connection.state === "unavailable";
          const label = active
            ? `${staleConnections ? "上次确认 · " : ""}${connectionLabel(connection.state)}`
            : unknownConnection
              ? connections?.loading
                ? "正在读取状态"
                : "状态待确认"
              : staleConnections
                ? "上次确认 · 未接入"
                : scanning
                  ? "正在查找"
                  : installed
                    ? "待接入"
                    : scanError
                      ? "待检查"
                      : "未发现应用";
          return (
            <article
              key={app.id}
              className="connection-card"
              data-connected={!!active}
            >
              <header>
                <span className="configuration-app-icon" data-app={app.id}>
                  {app.icon ? (
                    <AppGlyph source={app.icon} />
                  ) : (
                    <b>{app.mark}</b>
                  )}
                </span>
                <div>
                  <h2>{app.name}</h2>
                  <p>{app.description}</p>
                </div>
                <span
                  className="connection-state"
                  data-state={
                    unknownConnection ? "unavailable" : connection?.state
                  }
                >
                  {label}
                </span>
              </header>
              <div className="connection-card-body">
                {active ? (
                  <>
                    <span className="field-caption">
                      {connection.models?.length
                        ? "接入时默认模型"
                        : "当前模型"}
                    </span>
                    <code className="connection-model">
                      {connection.modelId || "待确认"}
                    </code>
                    <div className="connection-tags">
                      <span>
                        {connection.lineId === "global_accelerated"
                          ? "全球加速"
                          : connection.lineId
                            ? "大陆优化"
                            : "线路待确认"}
                      </span>
                      <span>
                        {connection.billingGroup
                          ? groupLabel(connection.billingGroup)
                          : "分组待确认"}
                      </span>
                    </div>
                    <p className="connection-footnote">
                      {connection.state === "recovery_pending"
                        ? "上次操作未完成，可恢复后重新接入。"
                        : connection.state === "changed"
                          ? "检测到设置有变化，保留你的修改。"
                          : connection.requiresBackground
                            ? "使用时请保持野菜助手运行。"
                            : "设置已保存；可随时换回原来的服务。"}
                    </p>
                    {(connection.models?.length ?? 0) > 1 && (
                      <p className="connection-footnote">
                        已配置 {connection.models!.length}{" "}
                        个常用模型，可在应用内切换。
                      </p>
                    )}
                    {connection.lastRequest && (
                      <details className="connection-last-result">
                        <summary>
                          {connection.lastRequest.outcome === "ok"
                            ? "最近一次已确认经野菜中转完成"
                            : "最近一次请求未完成 · 查看原因"}
                        </summary>
                        <RecentRequest
                          value={connection.lastRequest}
                          toolId={app.id}
                        />
                      </details>
                    )}
                  </>
                ) : (
                  <div className="connection-empty">
                    <p>
                      {unknownConnection
                        ? "尚未确认接入设置，检查成功后再调整；已有设置不会因此删除。"
                        : installed
                          ? "选择模型与分组，助手帮你完成配置。"
                          : ["claude_desktop", "codex_desktop"].includes(app.id)
                            ? "野菜帮你选择安装包，装好后继续接入模型。"
                            : "查看官方安装步骤，安装后由野菜完成模型接入。"}
                    </p>
                    <span>
                      {target?.installations[0]?.version
                        ? `版本 ${target.installations[0].version}`
                        : "不需要了解配置文件"}
                    </span>
                  </div>
                )}
              </div>
              <footer>
                <div className="connection-manage-actions">
                  <RestoreConnection connection={connection} name={app.name} />
                  {active && (
                    <button
                      className="text-button"
                      onClick={() => onOpenSetup(app.id)}
                    >
                      调整接入
                    </button>
                  )}
                </div>
                {active && connection.state !== "recovery_pending" ? (
                  <OpenConnection
                    connection={connection}
                    name={app.name}
                    onAdjust={() => onOpenSetup(app.id)}
                  />
                ) : (
                  <button
                    className={active ? "subtle-button" : "connect-app-button"}
                    onClick={() => onOpenSetup(app.id)}
                  >
                    {active || unknownConnection
                      ? "检查接入"
                      : installed
                        ? "开始接入"
                        : ["claude_desktop", "codex_desktop"].includes(app.id)
                          ? "安装并接入"
                          : "查看安装方式"}
                    <ArrowRight />
                  </button>
                )}
              </footer>
            </article>
          );
        })}
      </section>
      {scan &&
        !scanError &&
        apps.length > visibleApps.length &&
        visibleApps.length > 0 && (
          <button
            className="library-expand"
            aria-expanded="false"
            onClick={() => setShowAllApps(true)}
          >
            <Plus /> 查看其他 {apps.length - visibleApps.length} 个应用
          </button>
        )}
      {showAllApps && (
        <button
          className="library-expand"
          aria-expanded="true"
          onClick={() => setShowAllApps(false)}
        >
          只看本机应用
        </button>
      )}
      <footer className="library-footer">
        <ShieldCheck />
        <span>只改你选择的应用，随时可以恢复。不会删除聊天记录。</span>
        <button
          className="text-button"
          onClick={() => onOpenSetup("claude_desktop")}
        >
          <Plus />
          接入应用
        </button>
      </footer>
    </div>
  );
}
