const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");
const Module = require("node:module");

const root = path.resolve(__dirname, "../../..");
const ts = require(path.join(root, "node_modules/typescript"));
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    const result = ts.transpileModule(fs.readFileSync(filename, "utf8"), {
      fileName: filename,
      compilerOptions: {
        module: ts.ModuleKind.CommonJS,
        target: ts.ScriptTarget.ES2020,
        jsx: ts.JsxEmit.ReactJSX,
        esModuleInterop: true,
      },
    });
    module._compile(result.outputText, filename);
  };
}
require.extensions[".svg"] = (module, filename) => {
  module.exports = filename;
};

const load = (relative) => require(path.join(root, relative));
const { isDesktopAppScanResponse } = load("src/desktop-apps/contract.ts");
let checks = 0;
const check = (condition, message) => {
  assert.ok(condition, message);
  checks++;
};

const result = (requestId = "desktop-test", overrides = {}) => ({
  requestId,
  platform: "macos",
  startedAtEpochMs: 100,
  completedAtEpochMs: 120,
  apps: [
    {
      appId: "claude_desktop",
      displayName: "Claude Desktop",
      status: "detected_unverified",
      version: "1.40609.0",
      candidateCount: 1,
      locationHint: "applications",
      bundleIdentifier: "com.anthropic.claudefordesktop",
      configurationStatus: "documented_unverified",
      reasonCode: "desktop_app_detected_adapter_unverified",
    },
    {
      appId: "codex_desktop",
      displayName: "Codex",
      status: "detected_unverified",
      version: "26.825.51511",
      candidateCount: 1,
      locationHint: "applications",
      bundleIdentifier: "com.openai.codex",
      configurationStatus: "documented_unverified",
      reasonCode: "desktop_app_detected_adapter_unverified",
    },
  ],
  ...overrides,
});

function contract() {
  check(isDesktopAppScanResponse(result(), "desktop-test"), "valid two-app projection");
  const notFound = result();
  notFound.apps[0] = {
    ...notFound.apps[0],
    status: "not_found",
    version: "",
    candidateCount: 0,
    locationHint: "none",
    bundleIdentifier: "",
    configurationStatus: "not_applicable",
    reasonCode: "desktop_app_not_found",
  };
  check(isDesktopAppScanResponse(notFound, "desktop-test"), "truthful not-found state");
  const unsupported = result();
  unsupported.apps = unsupported.apps.map((app) => ({
    ...app,
    status: "unsupported_platform",
    version: "",
    candidateCount: 0,
    locationHint: "unsupported",
    bundleIdentifier: "",
    configurationStatus: "not_applicable",
    reasonCode: "desktop_platform_not_supported",
  }));
  check(isDesktopAppScanResponse(unsupported, "desktop-test"), "truthful unsupported state");
  for (const bad of [
    null,
    {},
    result("old"),
    { ...result(), apps: [result().apps[0]] },
    { ...result(), apps: [...result().apps].reverse() },
    { ...result(), apps: [...result().apps, result().apps[0]] },
    { ...result(), completedAtEpochMs: 99 },
    { ...result(), secret: "synthetic" },
  ]) check(!isDesktopAppScanResponse(bad, "desktop-test"), "invalid envelope rejected");
  for (const change of [
    { displayName: "Claude Code" },
    { status: "connected" },
    { version: "x".repeat(129) },
    { candidateCount: 2 },
    { locationHint: "/Users/private/Claude.app" },
    { bundleIdentifier: "com.example.fake" },
    { configurationStatus: "configured" },
    { reasonCode: "ready" },
    { rawPath: "/Users/private/Claude.app" },
  ]) {
    const sample = result();
    sample.apps[0] = { ...sample.apps[0], ...change };
    check(!isDesktopAppScanResponse(sample, "desktop-test"), "invalid app fact rejected");
  }
}

async function ui() {
  const { JSDOM } = require(path.join(root, "node_modules/jsdom"));
  const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/" });
  for (const name of ["window", "document", "HTMLElement", "Element", "Node", "Event", "MouseEvent", "MutationObserver"])
    global[name] = dom.window[name];
  Object.defineProperty(global, "navigator", { value: dom.window.navigator, configurable: true });
  global.IS_REACT_ACT_ENVIRONMENT = true;
  const translations = load("src/i18n/locales/zh.json");
  const translation = {
    i18n: { resolvedLanguage: "zh" },
    t: (key, values = {}) => {
      const text = key.split(".").reduce((value, part) => value?.[part], translations) ?? key;
      return String(text).replace(/\{\{(\w+)\}\}/g, (_, name) => String(values[name] ?? ""));
    },
  };
  const pending = [];
  const calls = [];
  const originalLoad = Module._load;
  Module._load = function (id, parent, main) {
    if (id === "@tauri-apps/api/core") return { invoke: (command, args) => {
      calls.push({ command, args });
      return new Promise((resolve, reject) => pending.push({ resolve, reject, args }));
    } };
    if (id === "react-i18next") return { useTranslation: () => translation };
    return originalLoad.call(this, id, parent, main);
  };
  const React = require(path.join(root, "node_modules/react"));
  const { render, screen, fireEvent, cleanup, act } = require(path.join(root, "node_modules/@testing-library/react"));
  const { CandidateHomeView } = load("src/candidate/CandidateHomeView.tsx");
  const selected = [];
  try {
    const view = render(React.createElement(CandidateHomeView, {
      onOpenAccount: () => {},
      onOpenSetup: (app) => selected.push(app),
      onOpenDiagnostics: () => {},
      onOpenTools: () => {},
      onOpenSettings: () => {},
    }));
    check(calls.length === 1, "one bounded startup discovery");
    check(calls[0].command === "scan_desktop_apps_read_only", "desktop command only");
    check(Object.keys(calls[0].args.request).join() === "requestId", "requestId-only IPC");
    const firstRequest = pending[0].args.request.requestId;
    await act(async () => pending[0].resolve(result(firstRequest)));
    check(screen.getAllByText("已安装").length === 2, "both desktop apps detected");
    check(document.body.textContent.includes("Claude Desktop") && document.body.textContent.includes("Codex"), "desktop identities visible");
    check(!document.body.textContent.includes("Base URL"), "beginner home hides technical terms");
    fireEvent.click(screen.getAllByRole("button", { name: /开始接入/ })[1]);
    check(selected.at(-1) === "codex_desktop", "Codex desktop selection reaches setup");
    fireEvent.click(screen.getByRole("button", { name: "重新检查" }));
    check(calls.length === 2, "refresh creates a new request");
    const secondRequest = pending[1].args.request.requestId;
    const stale = result(firstRequest);
    await act(async () => pending[0].resolve(stale));
    check(screen.getByText("正在识别桌面应用…"), "old completion cannot end current loading");
    await act(async () => pending[1].reject(new Error("synthetic private path")));
    check(screen.getByRole("alert").textContent.includes("暂时无法读取"), "safe recovery error shown");
    check(!document.body.textContent.includes("synthetic private path"), "raw error hidden");
    check(secondRequest !== firstRequest, "refresh identity changes");
    view.unmount();
  } finally {
    cleanup();
    Module._load = originalLoad;
    dom.window.close();
  }
}

async function main() {
  const scenario = process.argv[2];
  if (scenario === "contract") contract();
  else if (scenario === "ui") await ui();
  else throw new Error("Unknown RU-011 runtime scenario");
  process.stdout.write(JSON.stringify({ scenario, checks, passed: true }));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
