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

export const installationStage = (p: InstallationProgress) =>
  p.reasonCode === "system_handoff_dismissed"
    ? "本次安装引导已结束"
    : {
        idle: "准备安装",
        checking: "正在检查电脑",
        downloading: "正在下载应用",
        verifying: "正在检查安装包",
        installing: "正在安装应用",
        checking_install: "正在检查安装结果",
        awaiting_system_confirmation: "请在系统窗口中完成安装",
        installed: "应用已找到",
        cancelled: "下载已暂停",
        failed: "这次安装没有完成",
      }[p.phase];
function explanation(code: string) {
  switch (code) {
    case "disk_full":
      return "电脑空间不足。腾出一些空间后再试，原有应用和设置不会被删除。";
    case "permission_denied":
      return "系统暂时不允许写入安装位置。可查看官方安装方式，安装后回来检查。";
    case "source_challenge":
      return "官网暂时要求浏览器确认。请打开官方安装页完成安装，再回来检查。";
    case "signature_invalid":
    case "identity_mismatch":
    case "invalid_download":
    case "download_changed":
      return "安装包未通过检查，这次没有继续安装。请重新下载，或查看官方安装方式。";
    case "system_security_blocked":
      return "系统暂时没有允许安装。请查看系统提示或官方安装说明，不需要关闭系统安全保护。";
    case "system_installer_unavailable":
      return "未能打开或确认系统安装程序。请检查 Windows 的应用安装程序和系统提示，也可以查看官方安装方式。";
    case "installation_not_detected":
      return "还没找到装好的应用。请先在系统窗口完成安装，再点击下方检查。";
    case "system_handoff_dismissed":
      return "已结束野菜的安装引导，不会取消 Windows 安装。你可以安装其他应用，或装好后回来重新检查。";
    case "cache_full":
      return "已保留两个可能仍在使用的安装包，暂时不能下载其他版本。可使用官方安装方式，装好后回来检查；不会删除仍可能使用中的安装包。";
    case "installation_location_conflict":
      return "安装位置已有同名文件，但还不能确认是这个应用。我们没有替换它；可使用官方安装方式选择其他位置，装好后回来检查。";
    case "already_installed":
      return "电脑上已经有这个应用，不需要重装。重新检查后即可选择模型接入。";
    case "mount_cleanup_pending":
      return "应用已保留，但安装磁盘尚未退出。请先在访达中推出安装磁盘，再重新检查应用。";
    case "installation_unconfirmed":
    case "presence_unconfirmed":
      return "暂时不能确认应用的安装位置。请重新检查应用；我们没有删除或替换已有应用。";
    case "none":
    case "cancelled":
      return "";
    default:
      return "暂时没能完成下载或安装。可以重试；已下载的部分会尽量保留，原有应用和设置不会被删除。";
  }
}
const size = (bytes: number) => `${(bytes / 1024 ** 2).toFixed(1)} MB`;
export const installationSource = (source: InstallationProgress["source"]) =>
  source === "mirror"
    ? "下载来源：野菜国内加速 · 安装前校验厂商签名"
    : source === "official"
      ? "下载来源：厂商官网 · 安装前校验厂商签名"
      : "优先使用野菜国内加速，不可用时自动改走厂商官网；安装前校验厂商签名。";

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
      aria-label={`${name} 安装`}
      aria-busy={installer.working}
    >
      <header>
        <span className="app-install-symbol">
          <Download size={20} />
        </span>
        <div>
          <h2>
            {other
              ? "另一个应用正在安装"
              : installed
                ? `${name} 已找到`
                : active
                  ? `${name} · 安装进度`
                  : current?.phase === "failed"
                    ? `${name} · 安装未完成`
                    : `还没有安装 ${name}？`}
          </h2>
          <p>
            {other
              ? "先完成当前安装，再安装其他应用。"
              : guided
                ? "按官方步骤安装，完成后野菜帮你接入模型。"
                : "安装包自动选好，不用找版本，也不用输入命令。"}
          </p>
        </div>
        <span className="app-install-trust">
          <ShieldCheck size={14} /> 官方原版
        </span>
      </header>
      {current && !guided && current.jobId && (
        <>
          <ol className="app-install-steps" aria-label="安装进度">
            {["下载应用", "完成安装", "连接模型"].map((label, i) => (
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
              aria-label="应用下载进度"
              value={current.downloadedBytes}
              max={current.totalBytes || undefined}
            />
          )}
          {awaiting && (
            <div className="app-install-hint">
              <strong>请在 Windows 窗口中点击“安装”</strong>
              <p>
                确认发布者为 {tool === "codex_desktop" ? "OpenAI" : "Anthropic"}
                。如果出现“重新安装”或“替换”，请取消，回来直接接入已有应用。
              </p>
              <p>系统会检查安装包的签名。关闭野菜不会取消这个系统安装窗口。</p>
            </div>
          )}
          {installed && (
            <p className="app-install-copy">
              应用安装与模型连接是两步。只有收到真实测试结果后，才会显示接入成功。
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
            ? "目前官方提供的 Codex 安装包适用于苹果芯片 Mac。这台电脑可以先选择 Claude；已经装好的 Codex 仍可重新检查并接入。"
            : "这个应用目前没有适合这台电脑的自动安装包。可查看官方支持范围，或选择其他应用。"}
        </p>
      )}
      {selectionChanged && (
        <p className="app-install-warning">
          你的选择已更改。应用可以继续安装，装好后请确认当前模型，再点接入。
        </p>
      )}
      {installer.error && (
        <p className="app-install-warning" role="alert">
          安装进度暂时没有更新。请检查进度，不要重复启动安装。
        </p>
      )}
      <footer className="app-install-actions">
        {installer.error ? (
          <button
            className="primary-action"
            onClick={() => void installer.run(tool, "inspect")}
          >
            <RefreshCw size={15} />
            检查安装进度
          </button>
        ) : other ? null : guided ? (
          <button className="primary-action" onClick={help}>
            查看官方安装方式 <ExternalLink size={15} />
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
            {canConnect ? "检查安装并继续" : "检查安装结果"}
          </button>
        ) : installed ? (
          <button className="subtle-button" onClick={recheck}>
            <RefreshCw size={15} />
            重新检查应用
          </button>
        ) : active ? (
          current?.canCancel && (
            <button
              className="subtle-button"
              onClick={() => void installer.run(tool, "cancel")}
            >
              <Pause size={15} />
              暂停下载
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
              ? "重试安装"
              : current?.phase === "cancelled" &&
                  current.reasonCode !== "system_handoff_dismissed"
                ? "继续下载"
                : canConnect
                  ? "安装并接入"
                  : "先安装应用"}
          </button>
        )}
        {awaiting && (
          <button
            className="text-button"
            disabled={installer.working}
            onClick={() => void installer.run(tool, "reopen")}
          >
            重新打开安装窗口
          </button>
        )}
        {awaiting && (
          <button
            className="text-button"
            disabled={installer.working}
            onClick={() => void installer.run(tool, "cancel")}
          >
            结束本次引导（不取消 Windows 安装）
          </button>
        )}
        {!guided && (
          <button className="text-button" onClick={help}>
            官方安装方式 <ExternalLink size={13} />
          </button>
        )}
        {!active && !installed && (
          <button className="text-button" onClick={recheck}>
            我已安装，重新检查
          </button>
        )}
      </footer>
      {!active && !installed && !guided && (
        <p className="app-install-copy">
          {canConnect
            ? "安装完成后连接当前选择的模型，并发送一条测试消息，按所选分组计费。"
            : "可以先安装应用，再登录野菜、选择模型并接入。"}{" "}
          {!current?.jobId && installationSource("none")}
        </p>
      )}
      {awaiting && !canConnect && (
        <p className="app-install-copy">
          无需登录也能完成安装和检查。装好后再登录野菜、选择模型接入。
        </p>
      )}
    </section>
  );
}

export function InstallationNotice({
  onOpen,
}: {
  onOpen: (tool: ActivationToolId) => void;
}) {
  const installer = useInstallation();
  const p = installer?.progress;
  if (!p?.jobId || !installationActive(p)) return null;
  const name =
    WORKBENCH_APPS.find((app) => app.id === p.toolId)?.name ?? "应用";
  return (
    <aside className="app-install-notice" aria-label="正在进行的安装">
      <Download size={18} />
      <div>
        <strong>{name}</strong>
        <span>{installationStage(p)}</span>
      </div>
      <button className="text-button" onClick={() => onOpen(p.toolId)}>
        查看进度 <ArrowRight size={14} />
      </button>
    </aside>
  );
}
