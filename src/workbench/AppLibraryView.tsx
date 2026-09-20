import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowRight,
  ChevronRight,
  CircleAlert,
  Plus,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { AccountSummary } from "./WorkbenchChrome";
import { AppGlyph } from "./AppGlyph";
import { WORKBENCH_APPS } from "./appCatalog";
import type { AccountSessionController } from "../account/useAccountSession";
import { balanceAlert } from "../account/finance";
import { LowBalanceBanner } from "./LowBalanceBanner";
import { useWalletRecharge } from "./useWalletRecharge";
import {
  scanActivationTargets,
  type ActivationTargetScan,
  type ActivationToolId,
} from "../configuration/activation";
import { connectionLabel, useConnections } from "../configuration/connections";
import { RestoreConnection } from "../configuration/RestoreConnection";
import { OpenConnection } from "../configuration/OpenConnection";
import { ConnectionStatusNotice } from "../configuration/ConnectionStatusNotice";
import type { SetupAction } from "../configuration/setupIntent";
import { useWorkbenchCopy } from "./copy";
import { Users } from "lucide-react";

interface Props {
  onOpenAccount: () => void;
  onOpenSetup: (appId: ActivationToolId, action?: SetupAction) => void;
  // 交流群对话框的状态提到了 App：侧边栏和这里的页脚都要能打开它，
  // 两份各自的 useState 会变成两个互不知情的对话框。
  onOpenCommunity: () => void;
  accountSession: AccountSessionController;
}
/**
 * 后端整个够不着的时候，账户、本机扫描、接入状态会同时失败，页面就叠出三条
 * 措辞不同的说明和三个重试入口，底下每张应用卡片再各给一个「检查并修复」。
 * 一个根因，九个入口，而且三条文案都在暗示这是三件不同的事。
 *
 * 但只有三者**同时**失败才合并。单独一条失败时，它自己的说明是真有用的
 * —— 「读不到账户」和「读不到本机应用」要做的事完全不同 —— 那时候合并成
 * 一句笼统的「连不上」反而把信息丢掉了。
 */
export function outageIsTotal(
  account: boolean,
  scan: boolean,
  connections: boolean,
): boolean {
  return account && scan && connections;
}

export function AppLibraryView({
  onOpenAccount,
  onOpenSetup,
  onOpenCommunity,
  accountSession,
}: Props) {
  const { t } = useTranslation();
  const c = useWorkbenchCopy();
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
  const accountStatus = accountSession.projection?.status;
  const signedIn = accountStatus === "signed_in";
  // A failed/unfinished inspection is not evidence that the user signed out.
  // Only explicit terminal authentication states may offer the login action;
  // otherwise a route hiccup produces the contradictory error + login UI.
  const loginRequired = [
    "signed_out",
    "session_expired",
    "cancelled",
    "denied",
    "expired",
  ].includes(accountStatus ?? "");
  const balanceIssue = signedIn
    ? balanceAlert(accountSession.projection?.money)
    : null;
  const recharge = useWalletRecharge(accountSession.openWallet);
  const configured =
    connections?.connections.filter((v) =>
      ["connected", "legacy", "changed"].includes(v.state),
    ).length ?? 0;
  const hasConnectionSnapshot = !!connections?.connections.length;
  const partialConnections = connections?.connections.some(
    (c) => c.state === "unavailable",
  );
  const staleConnections = !!connections?.error && hasConnectionSnapshot;
  // #22 断电恢复横幅：启动读取到 recovery_pending 时，首页顶部引导进入恢复。
  const pendingRecovery = (() => {
    const pending = connections?.connections.find(
      (c) => c.state === "recovery_pending",
    );
    if (!pending) return null;
    const app = WORKBENCH_APPS.find((v) => v.id === pending.toolId);
    return {
      id: pending.toolId,
      name: app?.name ?? pending.toolId,
    };
  })();
  // #18 模型下线检测：账户模型目录就绪时，校验已接入工具的模型仍可用。
  // 目录为空（数据未返回）不校验，避免把加载失败误报成下线。
  const offlineModels = (() => {
    if (!signedIn) return null;
    const catalog = accountSession.projection?.models ?? [];
    if (!catalog.length) return null;
    const offline = (connections?.connections ?? []).filter(
      (c) =>
        ["connected", "legacy", "changed"].includes(c.state) &&
        !!c.modelId &&
        !catalog.some((m) => m.id === c.modelId),
    );
    if (!offline.length) return null;
    return offline.map((c) => ({
      id: c.toolId,
      name: WORKBENCH_APPS.find((v) => v.id === c.toolId)?.name ?? c.toolId,
    }));
  })();
  const checking = scanning || connections?.loading || connections?.refreshing;
  const everythingUnavailable = outageIsTotal(
    !!accountSession.lastError,
    scanError,
    !!connections?.error,
  );
  const retryEverything = () => {
    void accountSession.refresh();
    void refresh();
    void connections?.refresh();
  };
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

  // 每个应用的状态算一次，列表渲染直接用。
  const rows = visibleApps.map((app) => {
    const target = scan?.targets.find((v) => v.toolId === app.id);
    const connection = connections?.connections.find(
      (v) => v.toolId === app.id,
    );
    const active =
      connection &&
      !["not_connected", "unavailable"].includes(connection.state);
    const installed = target && target.status !== "not_found";
    const unknownConnection = !connection || connection.state === "unavailable";
    const label = active
      ? `${staleConnections ? c.libraryStalePrefix : ""}${connectionLabel(connection.state)}`
      : unknownConnection
        ? connections?.loading
          ? c.libraryStateLoading
          : c.libraryStateUnknown
        : staleConnections
          ? c.libraryStaleNotConnected
          : scanning
            ? c.libraryScanning
            : installed
              ? c.libraryPending
              : scanError
                ? c.libraryNeedsCheck
                : c.libraryNotFound;
    return {
      app,
      target,
      connection,
      active,
      installed,
      unknownConnection,
      label,
    };
  });

  // **状态读不到的时候不分组。** 按一个我们并不知道的状态给应用归类，
  // 是在假装知道：连接状态读不到（`unknownConnection`）或还在扫描
  // （`scanning`，装没装都还不知道）时，平铺回今天的样子才是诚实的。
  // 那个「六张卡全是状态待确认」的降级态不是设计没想清楚，
  // 那就是「读不到」应该有的样子。
  const canGroup = !scanning && rows.every((row) => !row.unknownConnection);
  const groups = canGroup
    ? [
        {
          key: "active",
          title: c.libraryGroupActive,
          apps: rows.filter((r) => r.active),
        },
        {
          key: "ready",
          title: c.libraryGroupReady,
          apps: rows.filter((r) => !r.active && r.installed),
        },
        {
          key: "missing",
          title: c.libraryGroupMissing,
          apps: rows.filter((r) => !r.active && !r.installed),
        },
      ].filter((group) => group.apps.length > 0)
    : [{ key: "all", title: "", apps: rows }];

  return (
    <div
      className="workbench-page desktop-home"
      data-testid="candidate-home-view"
    >
      <header className="workbench-page-heading">
        <div>
          <p className="eyebrow">{c.libraryEyebrow}</p>
          <h1>{c.libraryTitle}</h1>
          <p>
            {!hasConnectionSnapshot
              ? c.librarySubtitleUnknown
              : configured
                ? c.librarySubtitleConfigured
                : c.librarySubtitleEmpty}
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
          {checking ? c.libraryCheckingAction : c.libraryCheckAction}
        </button>
      </header>
      {pendingRecovery && (
        <p
          className="workbench-notice"
          role="alert"
          data-testid="recovery-banner"
        >
          <CircleAlert />
          {t("workbench.libraryRecoveryBanner", { app: pendingRecovery.name })}
          <button onClick={() => onOpenSetup(pendingRecovery.id, "repair")}>
            {c.libraryRecoveryAction}
          </button>
        </p>
      )}
      {offlineModels && (
        <p
          className="workbench-notice"
          role="alert"
          data-testid="offline-model-banner"
        >
          <CircleAlert />
          {t("yeschoyHome.offlineModelBanner", {
            apps: offlineModels.map((app) => app.name).join("、"),
          })}
          <button onClick={() => onOpenSetup(offlineModels[0].id)}>
            {t("yeschoyHome.offlineModelAction")}
          </button>
        </p>
      )}
      {everythingUnavailable && (
        <p className="workbench-notice" role="alert">
          <CircleAlert />
          {c.libraryOfflineNotice}
          <button disabled={checking} onClick={retryEverything}>
            {checking ? c.libraryRetrying : c.libraryRetry}
          </button>
        </p>
      )}
      {!everythingUnavailable && accountSession.lastError && (
        <p className="workbench-notice" role="status">
          <CircleAlert />
          {signedIn ? c.libraryAccountStale : c.libraryAccountUnavailable}
          <button onClick={() => void accountSession.refresh()}>
            {c.libraryRetry}
          </button>
        </p>
      )}
      {balanceIssue && (
        <LowBalanceBanner
          alert={balanceIssue}
          onRecharge={() => void recharge()}
        />
      )}
      {loginRequired ? (
        <section className="welcome-strip">
          <span className="welcome-symbol">
            <ShieldCheck />
          </span>
          <div>
            <h2>{c.librarySignInTitle}</h2>
            <p>{c.librarySignInBody}</p>
          </div>
          <button className="primary-action" onClick={onOpenAccount}>
            {c.librarySignInAction}
            <ArrowRight />
          </button>
        </section>
      ) : signedIn ? (
        <div className="library-heading">
          <span>
            {!hasConnectionSnapshot ||
            (partialConnections && configured === 0) ? (
              connections?.loading ? (
                c.libraryConnectionLoading
              ) : (
                c.libraryConnectionUnknown
              )
            ) : (
              <>
                {staleConnections
                  ? c.libraryCountStalePrefix
                  : partialConnections
                    ? c.libraryCountConfirmedPrefix
                    : ""}
                <b>{configured}</b>
                {c.libraryCountConnected}
                {partialConnections ? c.libraryCountPartial : ""}
              </>
            )}{" "}
            <span className="quiet-separator">/</span> {detected ?? "—"}
            {c.libraryDetected}
          </span>
        </div>
      ) : null}
      {!everythingUnavailable && scanError && (
        <p className="workbench-notice" role="alert">
          <CircleAlert />
          {c.libraryScanError}
        </p>
      )}
      {!everythingUnavailable &&
        connections &&
        (connections.error || partialConnections) && (
          <ConnectionStatusNotice
            issue={connections.errorInfo}
            stale={staleConnections}
            refreshing={connections.refreshing}
            onRetry={() => void connections.refresh()}
          />
        )}
      {!scanning && !scanError && visibleApps.length === 0 && (
        <section className="library-empty">
          <h2>{c.libraryEmptyTitle}</h2>
          <p>{c.libraryEmptyBody}</p>
          <button
            className="subtle-button"
            onClick={() => setShowAllApps(true)}
          >
            {c.libraryEmptyAction}
          </button>
        </section>
      )}
      {/* 用户在这一页问的是「哪些接上了、我要去哪个」。所以：
          状态决定分组，而不是每张卡里一行灰字；每行只放这个状态下有用的
          那一个事实，而不是六行重复的同一段话。
          详情（线路、计费分组、最近一次请求）归「管理」——
          列表负责导航，详情归详情页。 */}
      {/* **一个 section，标题行与 `<article>` 做兄弟**，而不是每组一个 section。
          分组只是视觉上的，DOM 上仍是一条列表：状态从「不知道」变成「知道」时
          应用会换组，若换了父节点，React 会卸载重建那个 `<article>`——
          而多处测试（含 ru076 那条回归守卫）先抓住 article 再断言后续状态，
          节点一被重建它们拿到的就是已脱离文档的旧节点。
          同一个父节点下带 key 的兄弟，React 只移动不重建。 */}
      <section
        className="connection-library"
        aria-label={c.libraryRegionLabel}
        aria-busy={scanning}
      >
        {groups.flatMap((group, groupIndex) => [
          ...(group.title
            ? [
                <h2
                  key={`heading-${group.key}`}
                  className="connection-group-heading"
                >
                  {group.title}
                  <span>{group.apps.length}</span>
                </h2>,
              ]
            : []),
          ...group.apps.map(
            (
              { app, target, connection, active, unknownConnection, label },
              index,
            ) => (
              <article
                key={app.id}
                className="connection-row stagger-enter"
                style={
                  { "--rail-index": groupIndex * 10 + index } as CSSProperties
                }
                data-connected={!!active}
              >
                <span className="configuration-app-icon" data-app={app.id}>
                  {app.icon ? (
                    <AppGlyph source={app.icon} />
                  ) : (
                    <b>{app.mark}</b>
                  )}
                </span>
                <div className="connection-row-main">
                  <h3>{app.name}</h3>
                  {active && connection ? (
                    <p className="connection-row-fact">
                      {/* 模型名本身可点 = 换模型。这是上一轮特意修的：
                          换模型是接入之后最常见的需求，而当时整张卡上最弱的
                          元素才是入口。压成一行时不许把它压没。 */}
                      <button
                        type="button"
                        className="connection-model-switch"
                        // 按钮的可访问名是它**做什么**，而不是它显示什么。
                        // 模型名 + 提示合成一个按钮之后，默认可访问名会变成
                        // 「GLM-5.3 换模型与分组」，而多处导航靠
                        // `getByRole("button", { name: "换模型与分组" })` 找它。
                        aria-label={t("yeschoyDaily.changeModel")}
                        onClick={() => onOpenSetup(app.id, "change-model")}
                      >
                        <code className="connection-model">
                          {connection.modelId || c.libraryModelPending}
                        </code>
                        <span className="connection-model-switch-hint">
                          {t("yeschoyDaily.changeModel")}
                          <ChevronRight aria-hidden="true" />
                        </span>
                      </button>
                      {(connection.models?.length ?? 0) > 1 && (
                        <span className="connection-row-extra">
                          {t("workbench.libraryMoreModels", {
                            count: connection.models!.length - 1,
                          })}
                        </span>
                      )}
                    </p>
                  ) : (
                    <p className="connection-row-fact">
                      {target?.installations[0]?.version ? (
                        <>
                          <span>{c.libraryInstalled}</span>
                          {/* 版本号必须独占一个元素、并保留「版本」前缀：
                              15 个测试用 `getByText("版本 1.40609.1")` 找它，
                              而 testing-library 的 getByText 比的是元素的
                              完整文本。把它并进「已安装 · 1.40609.1」
                              一行里，那 15 条会一起红。 */}
                          <span className="connection-row-version">
                            {t("workbench.libraryVersion", {
                              version: target.installations[0].version,
                            })}
                          </span>
                        </>
                      ) : (
                        <span>
                          {t(`yeschoyCatalog.appDescription.${app.id}`)}
                        </span>
                      )}
                    </p>
                  )}
                </div>
                <span
                  className="connection-state"
                  data-state={
                    unknownConnection ? "unavailable" : connection?.state
                  }
                >
                  {label}
                </span>
                <div className="connection-row-actions">
                  <RestoreConnection connection={connection} name={app.name} />
                  {active &&
                  connection &&
                  !["recovery_pending", "changed"].includes(
                    connection.state,
                  ) ? (
                    <OpenConnection
                      connection={connection}
                      name={app.name}
                      onAdjust={() => onOpenSetup(app.id, "repair")}
                    />
                  ) : (
                    <button
                      className={
                        active ? "subtle-button" : "connect-app-button"
                      }
                      // 后端整个够不着时，这个按钮点了也只会在接入页再失败一次。
                      // 六行给出六个一模一样的假入口，真正的重试反而被淹没；
                      // 上面那条唯一的说明已经说清楚了怎么办。
                      disabled={everythingUnavailable}
                      onClick={() =>
                        onOpenSetup(
                          app.id,
                          active || unknownConnection ? "repair" : "configure",
                        )
                      }
                    >
                      {active || unknownConnection
                        ? t("yeschoyDaily.checkAndRepair")
                        : target && target.status !== "not_found"
                          ? c.libraryConnectAction
                          : ["claude_desktop", "codex_desktop"].includes(app.id)
                            ? c.libraryInstallAndConnect
                            : c.libraryHowToInstall}
                      <ArrowRight />
                    </button>
                  )}
                </div>
              </article>
            ),
          ),
        ])}
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
            <Plus />{" "}
            {t("workbench.libraryShowMore", {
              count: apps.length - visibleApps.length,
            })}
          </button>
        )}
      {showAllApps && (
        <button
          className="library-expand"
          aria-expanded="true"
          onClick={() => setShowAllApps(false)}
        >
          {c.libraryShowLocalOnly}
        </button>
      )}
      {signedIn && (
        <details className="account-overview">
          <summary>
            <span>{t("yeschoyDaily.accountOverview")}</span>
            <small>{t("yeschoyDaily.accountOverviewHint")}</small>
          </summary>
          <AccountSummary
            detected={detected}
            scanning={scanning}
            onOpenAccount={onOpenAccount}
            onOpenApps={() =>
              document
                .querySelector<HTMLElement>(".connection-library")
                ?.scrollIntoView({ block: "start" })
            }
            accountProjection={accountSession.projection}
            accountLoading={accountSession.loading}
          />
        </details>
      )}
      <footer className="library-footer">
        <ShieldCheck />
        <span>{c.libraryFooterTrust}</span>
        <span className="library-footer-actions">
          <button
            type="button"
            className="text-button join-group-link"
            onClick={onOpenCommunity}
          >
            <Users aria-hidden="true" />
            {c.joinGroup}
          </button>
          <button
            className="text-button"
            onClick={() => onOpenSetup("claude_desktop")}
          >
            <Plus />
            {c.libraryFooterConnect}
          </button>
        </span>
      </footer>
    </div>
  );
}
