import { useTranslation } from "react-i18next";
import i18n from "i18next";
import {
  Download,
  ExternalLink,
  LoaderCircle,
  ShieldCheck,
  Check,
  ArrowRight,
  Pause,
  RefreshCw,
} from "lucide-react";
import { installationActive, type InstallationProgress } from "./api";
import { useInstallation } from "./InstallationProvider";
import { WORKBENCH_APPS } from "../workbench/appCatalog";
import type { ActivationToolId } from "../configuration/activation";
import "./installation.css";

/**
 * 这三个是模块级函数，不是组件，拿不到 hook —— 和 `connectionLabel` 同一种情况，
 * 所以同样走 i18next 单例。渲染它们的组件都调了 `useTranslation()`，
 * 语言一变就会重新调用。
 */
export const installationStage = (p: InstallationProgress) =>
  i18n.t(
    p.reasonCode === "system_handoff_dismissed"
      ? "installation.stage.handoffDismissed"
      : `installation.stage.${p.phase}`,
  );
function explanation(code: string) {
  switch (code) {
    case "disk_full":
      return i18n.t("installation.reason.diskFull");
    case "permission_denied":
      return i18n.t("installation.reason.permissionDenied");
    case "source_challenge":
      return i18n.t("installation.reason.sourceChallenge");
    case "signature_invalid":
    case "identity_mismatch":
    case "invalid_download":
    case "download_changed":
      return i18n.t("installation.reason.packageRejected");
    case "system_security_blocked":
      return i18n.t("installation.reason.securityBlocked");
    case "system_installer_unavailable":
      return i18n.t("installation.reason.installerUnavailable");
    case "installation_not_detected":
      return i18n.t("installation.reason.notDetected");
    case "system_handoff_dismissed":
      return i18n.t("installation.reason.handoffDismissed");
    case "cache_full":
      return i18n.t("installation.reason.cacheFull");
    case "installation_location_conflict":
      return i18n.t("installation.reason.locationConflict");
    case "already_installed":
      return i18n.t("installation.reason.alreadyInstalled");
    case "mount_cleanup_pending":
      return i18n.t("installation.reason.mountCleanupPending");
    case "installation_unconfirmed":
    case "presence_unconfirmed":
      return i18n.t("installation.reason.unconfirmed");
    case "none":
    case "cancelled":
      return "";
    default:
      return i18n.t("installation.reason.unknown");
  }
}
const size = (bytes: number) => `${(bytes / 1024 ** 2).toFixed(1)} MB`;
export const installationSource = (source: InstallationProgress["source"]) =>
  // `none` 那条不重复「安装前校验厂商签名」：头部徽章「厂商原版 · 安装前验签」
  // 已经说过，同一条信任承诺在一屏里说两遍，第二遍不会更可信。
  i18n.t(
    `installation.source.${source === "mirror" || source === "official" ? source : "none"}`,
  );

interface Props {
  tool: ActivationToolId;
  name: string;
  canConnect: boolean;
  onStart: () => void;
  onConfirm: () => void;
  onRefresh: () => void;
  selectionChanged?: boolean;
}
export function InstallationPanel({
  tool,
  name,
  canConnect,
  onStart,
  onConfirm,
  onRefresh,
  selectionChanged,
}: Props) {
  const { t } = useTranslation();
  const installer = useInstallation();
  if (!installer) return null;
  const p = installer.progress;
  const other = p && p.toolId !== tool && installationActive(p);
  const current = p?.toolId === tool ? p : null;
  const active = installationActive(current);
  const awaiting = current?.phase === "awaiting_system_confirmation";
  const installed = current?.phase === "installed";
  const guided = current && ["guided", "unsupported"].includes(current.mode);
  const step =
    current?.phase === "downloading" || current?.phase === "checking"
      ? 0
      : installed
        ? 2
        : active
          ? 1
          : 0;
  const help = () => void installer.run(tool, "help");
  const recheck = () => {
    void installer.run(tool, "inspect");
    onRefresh();
  };
  return (
    <section
      className="app-install-panel"
      data-failed={current?.phase === "failed"}
      aria-label={t("installation.regionLabel", { name })}
      aria-busy={installer.working}
    >
      <header>
        <span className="app-install-symbol">
          <Download size={20} />
        </span>
        <div>
          <h2>
            {other
              ? t("installation.titleOther")
              : installed
                ? t("installation.titleFound", { name })
                : active
                  ? t("installation.titleProgress", { name })
                  : current?.phase === "failed"
                    ? t("installation.titleFailed", { name })
                    : // 第 1 步的卡片刚说过「未在这台电脑找到该应用」。
                      // 这里再问一遍「还没有安装 X？」，是把同一个事实
                      // 在 100px 内说两遍；标题该说这块面板要做什么。
                      t("installation.titleInstall", { name })}
          </h2>
          <p>
            {other
              ? t("installation.leadOther")
              : guided
                ? t("installation.leadGuided")
                : t("installation.leadAuto")}
          </p>
        </div>
        <span className="app-install-trust">
          <ShieldCheck size={14} />
          {current?.mode === "guided"
            ? t("installation.trustGuided")
            : current?.mode === "unsupported"
              ? t("installation.trustUnsupported")
              : t("installation.trustDefault")}
        </span>
      </header>
      {current && !guided && current.jobId && (
        <>
          <ol
            className="app-install-steps"
            aria-label={t("installation.stepsLabel")}
          >
            {[
              t("installation.stepDownload"),
              t("installation.stepInstall"),
              t("installation.stepConnect"),
            ].map((label, i) => (
              <li
                key={label}
                data-current={i === step}
                data-complete={i < step}
              >
                <span>{i < step ? <Check size={13} /> : i + 1}</span>
                {label}
              </li>
            ))}
          </ol>
          <div className="app-install-status">
            <strong role="status">{installationStage(current)}</strong>
            {current.phase === "downloading" && (
              <span>
                {size(current.downloadedBytes)}
                {current.totalBytes > 0 ? ` / ${size(current.totalBytes)}` : ""}
              </span>
            )}
          </div>
          <p className="app-install-copy">
            {installationSource(current.source)}
          </p>
          {current.phase === "downloading" && (
            <progress
              aria-label={t("installation.downloadProgressLabel")}
              value={current.downloadedBytes}
              max={current.totalBytes || undefined}
            />
          )}
          {awaiting && (
            <div className="app-install-hint">
              <strong>{t("installation.awaitingTitle")}</strong>
              <p>
                {t("installation.awaitingPublisher", {
                  publisher: tool === "codex_desktop" ? "OpenAI" : "Anthropic",
                })}
              </p>
              <p>{t("installation.awaitingSignature")}</p>
            </div>
          )}
          {installed && (
            <p className="app-install-copy">
              {t("installation.installedNote")}
            </p>
          )}
          {explanation(current.reasonCode) && (
            <p className="app-install-warning" role="alert">
              {explanation(current.reasonCode)}
            </p>
          )}
        </>
      )}
      {current?.mode === "unsupported" && (
        <p className="app-install-hint">
          {tool === "codex_desktop" &&
          current.platform === "macos" &&
          current.architecture === "x64"
            ? t("installation.unsupportedCodexArm")
            : t("installation.unsupportedGeneric")}
        </p>
      )}
      {selectionChanged && (
        <p className="app-install-warning">
          {t("installation.selectionChanged")}
        </p>
      )}
      {installer.error && (
        <p className="app-install-warning" role="alert">
          {t("installation.progressStale")}
        </p>
      )}
      <footer className="app-install-actions">
        {installer.error ? (
          <button
            className="primary-action"
            onClick={() => void installer.run(tool, "inspect")}
          >
            <RefreshCw size={15} />
            {t("installation.checkProgress")}
          </button>
        ) : other ? null : guided ? (
          <button className="primary-action" onClick={help}>
            {t("installation.viewOfficial")} <ExternalLink size={15} />
          </button>
        ) : awaiting ? (
          <button
            className="primary-action"
            disabled={installer.working}
            onClick={onConfirm}
          >
            {installer.working ? (
              <LoaderCircle className="is-spinning" size={15} />
            ) : (
              <Check size={15} />
            )}
            {canConnect
              ? t("installation.confirmAndConnect")
              : t("installation.confirmInstall")}
          </button>
        ) : installed ? (
          <button className="subtle-button" onClick={recheck}>
            <RefreshCw size={15} />
            {t("installation.recheckApp")}
          </button>
        ) : active ? (
          current?.canCancel && (
            <button
              className="subtle-button"
              onClick={() => void installer.run(tool, "cancel")}
            >
              <Pause size={15} />
              {t("installation.pauseDownload")}
            </button>
          )
        ) : (
          <button
            className="primary-action"
            disabled={installer.working || !current}
            onClick={onStart}
          >
            <Download size={15} />
            {current?.phase === "failed"
              ? t("installation.retryInstall")
              : current?.phase === "cancelled" &&
                  current.reasonCode !== "system_handoff_dismissed"
                ? t("installation.resumeDownload")
                : canConnect
                  ? t("installation.installAndConnect")
                  : t("installation.installFirst")}
          </button>
        )}
        {awaiting && (
          <button
            className="text-button"
            disabled={installer.working}
            onClick={() => void installer.run(tool, "reopen")}
          >
            {t("installation.reopenInstaller")}
          </button>
        )}
        {awaiting && (
          <button
            className="text-button"
            disabled={installer.working}
            onClick={() => void installer.run(tool, "cancel")}
          >
            {t("installation.endGuide")}
          </button>
        )}
        {!guided && (
          <button className="text-button" onClick={help}>
            {t("installation.officialInstall")} <ExternalLink size={13} />
          </button>
        )}
        {!active && !installed && (
          <button className="text-button" onClick={recheck}>
            {t("installation.alreadyInstalled")}
          </button>
        )}
      </footer>
      {!active && !installed && !guided && (
        <p className="app-install-copy">
          {canConnect
            ? t("installation.footNoteConnect")
            : t("installation.footNoteSignIn")}
        </p>
      )}
      {/* 下载从哪儿来是另一件事，不该和「接下来怎么走」连在同一句里。 */}
      {!active && !installed && !guided && !current?.jobId && (
        <p className="app-install-copy">{installationSource("none")}</p>
      )}
      {awaiting && !canConnect && (
        <p className="app-install-copy">{t("installation.noSignInNeeded")}</p>
      )}
    </section>
  );
}

export function InstallationNotice({
  onOpen,
}: {
  onOpen: (tool: ActivationToolId) => void;
}) {
  const { t } = useTranslation();
  const installer = useInstallation();
  const p = installer?.progress;
  if (!p?.jobId || !installationActive(p)) return null;
  const name =
    WORKBENCH_APPS.find((app) => app.id === p.toolId)?.name ??
    t("installation.noticeFallbackName");
  return (
    <aside
      className="app-install-notice"
      aria-label={t("installation.noticeLabel")}
    >
      <Download size={18} />
      <div>
        <strong>{name}</strong>
        <span>{installationStage(p)}</span>
      </div>
      <button className="text-button" onClick={() => onOpen(p.toolId)}>
        {t("installation.viewProgress")} <ArrowRight size={14} />
      </button>
    </aside>
  );
}
