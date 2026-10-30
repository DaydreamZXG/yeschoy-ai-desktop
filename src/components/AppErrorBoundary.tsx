import { Component, type ErrorInfo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import "./error-boundary.css";

/**
 * 最后一道界面兜底：渲染期抛出的异常只让这一棵子树失败，而不是留下一个
 * 再也点不动的白窗口。
 *
 * 错误详情只写本机 console，不渲染到界面上——异常对象的 message 里可能带
 * 本地路径或 IPC 载荷，产品不变量要求渲染层不展示这些内容。
 */
function AppErrorFallback({ onReload }: { onReload: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="app-error-boundary" role="alert">
      <div className="app-error-card">
        <p className="app-error-eyebrow">{t("appError.eyebrow")}</p>
        <h1>{t("appError.title")}</h1>
        <p className="app-error-body">{t("appError.body")}</p>
        <p className="app-error-hint">{t("appError.hint")}</p>
        <button type="button" className="app-error-reload" onClick={onReload}>
          {t("appError.reload")}
        </button>
      </div>
    </div>
  );
}

interface Props {
  children: ReactNode;
  /** 测试与将来「只重挂这一屏」的入口；默认整页重载。 */
  onReload?: () => void;
}

interface State {
  failed: boolean;
}

export class AppErrorBoundary extends Component<Props, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("app_render_failed", error, info.componentStack);
  }

  private reload = () => {
    if (this.props.onReload) this.props.onReload();
    else window.location.reload();
  };

  render() {
    if (!this.state.failed) return this.props.children;
    return <AppErrorFallback onReload={this.reload} />;
  }
}
