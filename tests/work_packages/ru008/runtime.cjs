// Read-only acceptance: execute the actual modules in memory; no Vite cache,
// package downloads, source rewriting, live HTTP, or private credentials.
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
const load = (relative) => require(path.join(root, relative));
const { catalogFixture } = load("src/service-catalog/test-fixtures.ts");
const { isServiceCatalog } = load("src/service-catalog/contract.ts");
const { createToolAccessPlan } = load("src/service-catalog/access-plan.ts");
let checks = 0;
const check = (condition, message) => {
  assert.ok(condition, message);
  checks++;
};

function contract() {
  const valid = (value) => isServiceCatalog(value, "test-read", "mainland_optimized");
  check(valid(catalogFixture()), "valid public catalog");
  check(valid({ ...catalogFixture(), models: [], groups: [] }), "empty catalog");
  for (const bad of [
    null, [], {}, catalogFixture("old"), catalogFixture("test-read", "global_accelerated"),
    { ...catalogFixture(), userSpecific: true },
    { ...catalogFixture(), secretsAccessed: true },
    { ...catalogFixture(), observedAtEpochMs: NaN },
    { ...catalogFixture(), extraSecret: "synthetic" },
    { ...catalogFixture(), catalogStatus: "ready" },
    { ...catalogFixture(), catalogError: "unknown" },
    { ...catalogFixture(), catalogStatus: "unavailable", catalogError: "http_error" },
    { ...catalogFixture(), groups: [...catalogFixture().groups, ...catalogFixture().groups] },
    { ...catalogFixture(), models: [...catalogFixture().models, ...catalogFixture().models] },
  ]) check(!valid(bad), "invalid catalog rejected");
  for (const multiplier of ["", "0", "-1", "NaN", "Infinity", "0.2 extra", 0.2]) {
    const sample = catalogFixture();
    sample.groups[0].multiplier = multiplier;
    check(!valid(sample), "invalid multiplier rejected");
  }
  for (const change of [
    { groups: ["unknown"] }, { groups: [] }, { id: "x".repeat(201) },
    { id: "hidden\u202e" }, { endpoints: ["openai", "openai"] },
    { billingMode: "free" }, { billing_expr: "synthetic" },
  ]) {
    const sample = catalogFixture();
    Object.assign(sample.models[0], change);
    check(!valid(sample), "invalid model rejected");
  }
  const huge = catalogFixture();
  huge.models = Array.from({ length: 2049 }, (_, i) => ({ ...huge.models[0], id: `model-${i}` }));
  check(!valid(huge), "oversized catalog rejected");
  const recognized = catalogFixture();
  recognized.desktopBackend = { status: "contract_recognized", error: "none", declaredCapabilities: ["account_read"] };
  check(valid(recognized), "recognition is a valid public observation");
  check(recognized.userSpecific === false && recognized.secretsAccessed === false, "recognition grants no authority");
  recognized.desktopBackend.declaredCapabilities = ["admin"];
  check(!valid(recognized), "unknown capability rejected");
}

function access() {
  const expected = { claude: "anthropic", codex: "openai-response", opencode: "openai", pi: "openai" };
  for (const line of ["mainland_optimized", "global_accelerated"]) {
    for (const [tool, protocol] of Object.entries(expected)) {
      const sample = catalogFixture("test-read", line);
      const plan = createToolAccessPlan(sample, tool, line, "test-group", "test/model");
      check(plan.requiredProtocol === protocol && plan.status === "protocol_declared", "per-tool protocol");
      check(!plan.applyAllowed && plan.accountAccess === "unverified" && plan.groupRestrictions === "unverified" && plan.exactToolVersion === "unverified", "public data never authorizes writes");
      const origin = line === "mainland_optimized" ? "https://yeschoy.com" : "https://api.yeschoy.com";
      check(plan.baseUrl === origin + (tool === "claude" ? "" : "/v1"), "canonical endpoint");
      sample.models[0].endpoints = [];
      check(createToolAccessPlan(sample, tool, line, "test-group", "test/model").status === "protocol_not_declared", "missing protocol abstains");
    }
    const sample = catalogFixture("test-read", line);
    const dsh = createToolAccessPlan(sample, "dsh", line, "test-group", "test/model");
    check(dsh.status === "dsh_unverified" && dsh.baseUrl === "" && !dsh.applyAllowed, "DSH abstains");
    for (const [group, model] of [["other", "test/model"], ["test-group", "other"]]) {
      assert.throws(() => createToolAccessPlan(sample, "pi", line, group, model)); checks++;
    }
  }
}

async function ui(scenario) {
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
  let handler = async () => { throw new Error("unconfigured test IPC"); };
  const calls = [];
  const originalLoad = Module._load;
  Module._load = function (id, parent, main) {
    if (id === "@tauri-apps/api/core") return { invoke: (command, args) => {
      calls.push({ command, args });
      return handler(command, args);
    } };
    if (id === "react-i18next") return { useTranslation: () => translation };
    return originalLoad.call(this, id, parent, main);
  };
  const React = require(path.join(root, "node_modules/react"));
  const { render, screen, fireEvent, cleanup, act } = require(path.join(root, "node_modules/@testing-library/react"));
  const { ServiceCatalogPanel } = load("src/service-catalog/ServiceCatalogPanel.tsx");
  const { ConfigurationPreviewView } = load("src/configuration/ConfigurationPreviewView.tsx");
  const cb = { onPlanChange: (plan) => plans.push(plan), onReadAttempt: () => {} };
  const plans = [];
  const props = { toolId: "pi", lineId: "mainland_optimized", ...cb };
  const element = (component, props) => React.createElement(component, props);
  const read = () => fireEvent.click(screen.getByRole("button", { name: "读取模型目录" }));
  const refresh = () => fireEvent.click(screen.getByRole("button", { name: "重新读取" }));
  const ready = () => screen.findByRole("combobox", { name: /服务分组/ });
  const choose = () => {
    fireEvent.change(screen.getByRole("combobox", { name: /服务分组/ }), { target: { value: "test-group" } });
    fireEvent.change(screen.getByRole("combobox", { name: /模型 ID/ }), { target: { value: "test/model" } });
  };
  const respond = () => { handler = async (_, { request }) => catalogFixture(request.requestId, request.lineId); };
  try {
    if (scenario === "recovery") {
      render(element(ServiceCatalogPanel, props));
      check(calls.length === 0, "no read on mount");
      handler = async () => { throw "synthetic-private-error <script>bad()</script>"; };
      read();
      await screen.findByRole("alert");
      check(!document.body.textContent.includes("synthetic-private-error"), "raw error hidden");
      handler = async () => ({ guessed: true });
      refresh();
      await screen.findByText(/返回的数据不符合目录契约/);
      check(screen.queryByRole("combobox") === null, "malformed data never becomes models");
      handler = async (_, { request }) => {
        const sample = catalogFixture(request.requestId, request.lineId);
        sample.groups[0].description = '<img src="x" onerror="steal()">';
        sample.models[0].endpoints = [];
        return sample;
      };
      refresh(); await ready(); choose();
      check(document.body.textContent.includes('<img src="x" onerror="steal()">') && document.querySelector("img") === null, "untrusted prose is text");
      check(plans.at(-1).status === "protocol_not_declared" && !plans.at(-1).applyAllowed, "no guessed protocol or write access");
      handler = async (_, { request }) => ({ ...catalogFixture(request.requestId, request.lineId), models: [], groups: [] });
      refresh();
      await screen.findByText(/当前公开目录没有可选模型/);
      check(plans.at(-1) === null && screen.queryByRole("combobox") === null, "refresh clears selection and supports empty");
      respond(); refresh(); await ready();
      check(calls.every(({ command, args }) => command === "read_public_service_catalog" && Object.keys(args.request).sort().join() === "lineId,requestId"), "only narrow public IPC called");
    } else {
      let finishOld;
      let oldRequest;
      handler = (_, { request }) => { oldRequest = request; return new Promise((resolve) => { finishOld = resolve; }); };
      const view = render(element(ServiceCatalogPanel, props));
      read();
      check(screen.getByRole("button", { name: "正在读取…" }).disabled, "pending read cannot duplicate");
      view.rerender(element(ServiceCatalogPanel, { ...props, lineId: "global_accelerated" }));
      respond(); read(); await ready(); choose();
      const old = catalogFixture(oldRequest.requestId, oldRequest.lineId);
      old.models[0].id = "obsolete-model";
      await act(async () => finishOld(old));
      check(!document.body.textContent.includes("obsolete-model") && plans.at(-1).lineId === "global_accelerated", "late old line response discarded");
      let finishUnmounted;
      let unmountedRequest;
      handler = (_, { request }) => { unmountedRequest = request; return new Promise((resolve) => { finishUnmounted = resolve; }); };
      refresh();
      check(plans.at(-1) === null && screen.queryByRole("combobox") === null, "refresh immediately invalidates selection");
      view.unmount();
      const count = plans.length;
      await act(async () => finishUnmounted(catalogFixture(unmountedRequest.requestId, unmountedRequest.lineId)));
      check(plans.length === count, "unmount discards pending response");
      respond();
      render(element(ConfigurationPreviewView, { onOpenAccount: () => {}, onOpenTools: () => {} }));
      read(); await ready(); choose();
      const preview = screen.getByRole("region", { name: "先看清，再决定" });
      check(preview.textContent.includes("test/model"), "real model reaches parent preview");
      fireEvent.click(screen.getByRole("button", { name: /大陆优化 中国大陆网络优先/ }));
      check(preview.textContent.includes("test/model") && preview.textContent.includes("test-group"), "same line preserves parent preview");
      check(screen.getByRole("combobox", { name: /模型 ID/ }).value === "test/model", "same line preserves selection");
      fireEvent.click(screen.getByRole("button", { name: /全球加速 Cloudflare 全球线路/ }));
      check(!preview.textContent.includes("test/model") && screen.queryByRole("combobox") === null, "different line clears both panes");
      check(screen.getByTestId("configuration-apply-blocked").disabled, "application remains blocked");
    }
  } finally {
    cleanup(); Module._load = originalLoad; dom.window.close();
  }
}

async function main() {
  const scenario = process.argv[2];
  if (scenario === "contract") contract();
  else if (scenario === "access") access();
  else if (["recovery", "race"].includes(scenario)) await ui(scenario);
  else throw new Error("Unknown acceptance scenario");
  process.stdout.write(JSON.stringify({ scenario, checks, passed: true }));
}
main().catch((error) => { console.error(error); process.exitCode = 1; });
