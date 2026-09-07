import { useState } from "react";
import { createRoot } from "react-dom/client";
import { InstallationPanel } from "../../../src/installation/InstallationPanel";
import { InstallationContext } from "../../../src/installation/InstallationProvider";
import type { InstallationProgress } from "../../../src/installation/api";
import brand from "../../../src/assets/brand/yecai-logo.png";
import "../../../src/index.css";
import "../../../src/workbench/workbench-v2.css";

function Preview() {
  const [phase, setPhase] = useState<InstallationProgress["phase"]>("idle");
  const [dark, setDark] = useState(false);
  const p: InstallationProgress = {
    schemaVersion: 2,
    requestId: "visual-fixture",
    toolId: "codex_desktop",
    jobId: phase === "idle" ? "" : "fixture-only",
    mode: "system_assisted",
    phase,
    downloadedBytes: phase === "downloading" ? 136314880 : 0,
    totalBytes: phase === "downloading" ? 524288000 : 0,
    canCancel: phase === "downloading",
    reasonCode: phase === "failed" ? "signature_invalid" : "none",
    source: "official",
    platform: "windows",
    architecture: "x64",
    disposition: phase === "installed" ? "confirmed" : "none",
    installationId: phase === "installed" ? "fixture" : "",
  };
  return (
    <main data-theme={dark ? "dark" : "light"} className="installer-preview">
      <header className="installer-preview-brand">
        <img src={brand} alt="野菜" />
        <div>
          <strong>野菜 API</strong>
          <span>桌面助手</span>
        </div>
        <button
          className="subtle-button"
          onClick={() => {
            setDark(!dark);
            document.documentElement.dataset.theme = dark ? "light" : "dark";
          }}
        >
          {dark ? "浅色" : "深色"}
        </button>
      </header>
      <div className="installer-preview-intro">
        <p>应用接入</p>
        <h1>装好应用，就能开始</h1>
        <p>野菜帮你选择安装包，装好后继续连接你选好的模型。</p>
      </div>
      <div className="installer-preview-grid">
        <InstallationContext.Provider
          value={{
            progress: p,
            working: false,
            error: false,
            run: async (_tool, action) => {
              if (action === "cancel") setPhase("cancelled");
              return p;
            },
          }}
        >
          <InstallationPanel
            tool="codex_desktop"
            name="Codex Desktop"
            canConnect
            onStart={() => setPhase("downloading")}
            onConfirm={() => setPhase("installed")}
            onRefresh={() => setPhase("idle")}
          />
        </InstallationContext.Provider>
        <aside className="installer-preview-choice">
          <span>安装后继续</span>
          <h2>你的选择</h2>
          <dl>
            <dt>应用</dt>
            <dd>Codex Desktop</dd>
            <dt>模型</dt>
            <dd>示例模型 A</dd>
            <dt>计费分组</dt>
            <dd>标准分组</dd>
            <dt>网络线路</dt>
            <dd>大陆优化</dd>
          </dl>
          <p>只连接你选好的应用与模型。原来的设置会保留，随时可以恢复。</p>
        </aside>
      </div>
      <nav aria-label="预览状态" className="installer-preview-states">
        {(
          [
            ["idle", "准备安装"],
            ["downloading", "下载进度"],
            ["awaiting_system_confirmation", "Windows 确认"],
            ["installed", "已安装"],
            ["failed", "失败恢复"],
          ] as const
        ).map(([value, label]) => (
          <button
            className="subtle-button"
            aria-pressed={phase === value}
            onClick={() => setPhase(value)}
            key={value}
          >
            {label}
          </button>
        ))}
      </nav>
      <p className="installer-preview-disclaimer">
        组件预览 · 全部为示例数据 · 不会下载、安装或连接真实账户
      </p>
      <style>{`.installer-preview {max-width:1100px; margin:0 auto; padding:36px 32px; color:var(--ink); min-height:100vh;} body{background:var(--bg);} .installer-preview-brand {display:flex; align-items:center; gap:12px; padding-bottom:28px; border-bottom:1px solid var(--line);} .installer-preview-brand img{width:44px;height:44px;border-radius:12px;} .installer-preview-brand div{display:flex;flex-direction:column;gap:3px;} .installer-preview-brand span{font-size:11px;color:var(--muted);} .installer-preview-brand button{margin-left:auto;} .installer-preview-intro{margin:32px 0 8px;} .installer-preview-intro>p:first-child{font-size:12px;color:var(--accent);margin:0 0 8px;} .installer-preview-intro h1{font-size:27px;letter-spacing:-.6px; margin:0 0 12px;} .installer-preview-intro p{font-size:13px; color:var(--muted);} .installer-preview-grid{display:grid;grid-template-columns:minmax(0,1fr) 250px;gap:20px;align-items:start;} .installer-preview-choice{border:1px solid var(--line); background:var(--surface);padding:23px;border-radius:14px;margin:20px 0;} .installer-preview-choice>span{font-size:11px;color:var(--muted);} .installer-preview-choice h2{font-size:16px;margin:8px 0 22px;} .installer-preview-choice dt{font-size:11px;color:var(--muted);margin:15px 0 4px;} .installer-preview-choice dd{font-size:13px;margin:0;font-weight:600;} .installer-preview-choice p{font-size:12px;line-height:1.7;color:var(--muted);border-top:1px solid var(--line);padding-top:18px;margin:22px 0 0;} .installer-preview-states{display:flex;flex-wrap:wrap;gap:9px;margin-top:14px;} .installer-preview-states button[aria-pressed=true]{border-color:var(--accent);background:var(--accent-soft);color:var(--accent);} .installer-preview-disclaimer{font-size:11px;color:var(--muted);margin-top:16px;} @media(max-width:760px){.installer-preview{padding:20px 16px}.installer-preview-grid{grid-template-columns:1fr}.installer-preview-choice{margin:0}.installer-preview-intro h1{font-size:23px}}`}</style>
    </main>
  );
}
createRoot(document.getElementById("root")!).render(<Preview />);
