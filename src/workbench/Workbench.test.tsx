import {
  act,
  fireEvent,
  render,
  screen,
  within,
  cleanup,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StrictMode } from "react";
import { invoke } from "@tauri-apps/api/core";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import tw from "../i18n/locales/zh-TW.json";
import en from "../i18n/locales/en.json";
import ja from "../i18n/locales/ja.json";
import App from "../App";
import { APPEARANCE_KEY, readAppearance } from "./appearance";
import { workbenchCopies } from "./copy";
import { readFileSync } from "node:fs";
import { connectionsFixture } from "../configuration/connection-test-fixtures";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
import { installationInspectionFixture } from "../installation/test-fixtures";
const native = vi.mocked(invoke);
const locales = { zh, "zh-TW": tw, en, ja };
// Inlined legacy service-catalog fixture: the mock branch for
// `read_public_service_catalog` is retained so unexpected-command
// assertions keep their original coverage shape.
function catalogFixture(requestId = "test-read", lineId = "mainland_optimized") {
  return {
    requestId,
    lineId,
    observedAtEpochMs: 1700000000000,
    catalogStatus: "available",
    catalogError: "none",
    serviceVersion: "test-version",
    backendDisplayExchangeRate: "1",
    groups: [
      {
        id: "test-group",
        description: "仅供测试，不作为流式限制的机器契约",
        multiplier: "0.25",
      },
    ],
    models: [
      {
        id: "test/model",
        groups: ["test-group"],
        endpoints: ["anthropic", "openai-response", "openai"],
        billingMode: "tiered_expr",
      },
    ],
    desktopBackend: {
      status: "not_deployed",
      error: "none",
      declaredCapabilities: [],
    },
    userSpecific: false,
    secretsAccessed: false,
  };
}
function discovery(requestId: string, version = "1.2.3") {
  return {
    requestId,
    platform: "macos",
    startedAtEpochMs: 1,
    completedAtEpochMs: 2,
    apps: [
      {
        appId: "claude_desktop",
        displayName: "Claude Desktop",
        status: "detected_unverified",
        version,
        candidateCount: 1,
        locationHint: "applications",
        bundleIdentifier: "com.anthropic.claudefordesktop",
        configurationStatus: "documented_unverified",
        reasonCode: "desktop_app_detected_adapter_unverified",
      },
      {
        appId: "codex_desktop",
        displayName: "Codex",
        status: "not_found",
        version: "",
        candidateCount: 0,
        locationHint: "none",
        bundleIdentifier: "",
        configurationStatus: "not_applicable",
        reasonCode: "desktop_app_not_found",
      },
    ],
  };
}
function activationTargetScan(requestId: string) {
  const definitions = [
    ["claude_code", "Claude Code", "2.1.233"],
    ["claude_desktop", "Claude Desktop", "1.40609.1"],
    ["codex_desktop", "Codex Desktop", "26.825.51511"],
    ["pi", "Pi", "0.84.4"],
    ["dsh_web", "DSH web", "0.1.0-rc.6"],
    ["hermes", "Hermes", "0.21.0"],
    ["openclaw", "OpenClaw", "2026.9.1"],
  ] as const;
  return {
    requestId,
    schemaVersion: 1,
    platform: "macos",
    targets: definitions.map(([toolId, displayName, version], index) => ({
      toolId,
      displayName,
      surface: "本机应用",
      status: "available",
      installations: [
        {
          installationId: `i${String(index + 1).padStart(16, "0")}`,
          label: "安装 1 · 系统应用",
          version,
          supported: true,
          recommended: true,
        },
      ],
    })),
  };
}
function signedOut(requestId: string) {
  return {
    requestId,
    schemaVersion: 3,
    status: "signed_out",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 1,
    account: {
      available: false,
      displayName: "",
      username: "",
      balanceQuota: "",
      usedQuota: "",
      requestCount: "",
      quotaPerUnit: "",
    },
    usage: {
      available: false,
      consumedQuota: "",
      requestRate: "",
      tokenCount: "",
    },
    models: [],
    comparisonFx: "",
    reasonCode: "signed_out",
  };
}
function signedIn(requestId: string) {
  return {
    ...signedOut(requestId),
    status: "signed_in",
    observedAtEpochMs: 1_788_195_600_000,
    account: {
      available: true,
      displayName: "野菜测试用户",
      username: "member@example.com",
      balanceQuota: "350000",
      usedQuota: "120000",
      requestCount: "42",
      quotaPerUnit: "500000",
    },
    usage: {
      available: true,
      consumedQuota: "120000",
      requestRate: "42",
      tokenCount: "980000",
    },
    models: [
      {
        id: "glm-5.3",
        description: "",
        billingMode: "ratio",
        pricingAvailable: true,
        officialInputCnyPerMillion: "2",
        officialOutputCnyPerMillion: "8",
        actualInputCnyPerMillion: "1",
        actualOutputCnyPerMillion: "4",
        supportedEndpointTypes: ["anthropic", "openai-response", "openai"],
        billing: {
          groups: [
            { id: "default", description: "", ratio: 0.5 },
            { id: "国模特价分组", description: "按所选分组计费", ratio: 0.35 },
          ],
          baseInputUsd: 2,
          baseOutputUsd: 8,
          requestUsd: null,
          expression: "",
        },
      },
    ],
    comparisonFx: "1",
    reasonCode: "none",
  };
}
type Args = {
  request: {
    requestId: string;
    lineId: "mainland_optimized" | "global_accelerated";
  };
};
type NativeHandler = (command: string, args: unknown) => unknown;
function mockNativeByCommand(handler: NativeHandler) {
  native.mockImplementation(async (command, args) => {
    if (command === "read_desktop_exit_state") {
      return { closeRequested: false, shutdown: null };
    }
    if (command === "manage_app_installation_v2")
      return installationInspectionFixture(args);
    return handler(command, args);
  });
}
const defaultNativeHandler: NativeHandler = (command, args) => {
  const request = (args as Args).request;
  if (command === "manage_tool_connections_v1")
    return connectionsFixture(request.requestId);
  if (command === "scan_activation_targets_v1")
    return activationTargetScan(request.requestId);
  if (command === "scan_desktop_apps_read_only")
    return discovery(request.requestId);
  if (command === "read_public_service_catalog")
    return catalogFixture(request.requestId, request.lineId);
  if (command === "account_inspect_v2") return signedOut(request.requestId);
  throw Error(`Unexpected native command: ${command}`);
};
function mockDiscoveryOnce(make: (requestId: string) => unknown) {
  let pending = true;
  mockNativeByCommand((command, args) => {
    if (command === "scan_desktop_apps_read_only" && pending) {
      pending = false;
      return make((args as Args).request.requestId);
    }
    return defaultNativeHandler(command, args);
  });
}
let systemDark = false;
// App 渲染的 Toaster（sonner）也会注册 prefers-color-scheme 监听器，
// 因此按列表保存全部监听器，并以真实事件形状触发。
let appearanceListeners: ((event: { matches: boolean }) => void)[] = [];
beforeEach(async () => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  native.mockReset();
  mockNativeByCommand(defaultNativeHandler);
  for (const [language, resource] of Object.entries(locales))
    i18n.addResourceBundle(language, "translation", resource, true, true);
  await i18n.changeLanguage("zh");
  localStorage.removeItem(APPEARANCE_KEY);
  systemDark = false;
  appearanceListeners = [];
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      get matches() {
        return systemDark;
      },
      addEventListener: (
        _name: string,
        callback: (event: { matches: boolean }) => void,
      ) => {
        appearanceListeners.push(callback);
      },
      removeEventListener: vi.fn(),
    })),
  );
  vi.stubGlobal("scrollTo", vi.fn());
});
afterEach(() => {
  for (const [command, args] of native.mock.calls) {
    if (command === "read_desktop_exit_state") expect(args).toBeUndefined();
    if (command === "manage_app_installation_v2")
      expect(() => installationInspectionFixture(args)).not.toThrow();
  }
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("official workbench", () => {
  it("ru076 never reports zero connections or ready-to-connect when the initial read fails", async () => {
    mockNativeByCommand((command, args) => {
      const request = (args as Args).request;
      if (command === "account_inspect_v2") return signedIn(request.requestId);
      if (command === "manage_tool_connections_v1")
        throw Error("connection_operation_busy");
      return defaultNativeHandler(command, args);
    });
    render(<App />);
    await screen.findByText(/正在处理另一项接入或恢复操作/);
    const home = screen.getByTestId("candidate-home-view");
    expect(home).toHaveTextContent("接入状态待确认");
    expect(home).not.toHaveTextContent("0 个应用已接入");
    const codex = within(home)
      .getByRole("heading", { name: "Codex Desktop" })
      .closest("article")!;
    expect(codex).toHaveTextContent("状态待确认");
    expect(codex).not.toHaveTextContent("待接入");
    expect(
      within(codex).getByRole("button", { name: "检查并修复" }),
    ).toBeEnabled();
    expect(home).toHaveTextContent(/诊断编号：connections-/);
    mockNativeByCommand((command, args) =>
      command === "account_inspect_v2"
        ? signedIn((args as Args).request.requestId)
        : defaultNativeHandler(command, args),
    );
    fireEvent.click(within(home).getByRole("button", { name: "读取接入状态" }));
    await waitFor(() => expect(codex).toHaveTextContent("待接入"));
    expect(home).toHaveTextContent("0 个应用已接入");
  });
  it("ru076 retains a labelled last-successful snapshot after a refresh fails", async () => {
    let fail = false;
    mockNativeByCommand((command, args) => {
      const request = (args as Args).request;
      if (command === "account_inspect_v2") return signedIn(request.requestId);
      if (command === "manage_tool_connections_v1") {
        if (fail) throw Error("synthetic-secret-must-not-appear");
        const result = connectionsFixture(request.requestId);
        Object.assign(
          result.connections.find((c) => c.toolId === "codex_desktop")!,
          {
            state: "connected",
            modelId: "gpt-6-astra",
            lineId: "mainland_optimized",
            billingGroup: "default",
            restoreMode: "original",
          },
        );
        return result;
      }
      return defaultNativeHandler(command, args);
    });
    render(<App />);
    await screen.findByText("gpt-6-astra");
    fail = true;
    fireEvent.click(screen.getByRole("button", { name: "检查应用" }));
    await screen.findByText(/下方保留上次确认的结果/);
    const home = screen.getByTestId("candidate-home-view");
    expect(home).toHaveTextContent("上次确认 1 个应用已接入");
    expect(home).toHaveTextContent("上次确认 · 已接入");
    expect(home).toHaveTextContent("gpt-6-astra");
    expect(home).not.toHaveTextContent("synthetic-secret");
  });
  it("ru076 keeps an individual unavailable result unknown and blocks setup until reread", async () => {
    mockNativeByCommand((command, args) => {
      const request = (args as Args).request;
      if (command === "account_inspect_v2") return signedIn(request.requestId);
      if (command === "manage_tool_connections_v1") {
        const result = connectionsFixture(request.requestId);
        Object.assign(
          result.connections.find((c) => c.toolId === "codex_desktop")!,
          {
            state: "unavailable",
            reasonCode: "secure_storage_unavailable",
          },
        );
        return result;
      }
      return defaultNativeHandler(command, args);
    });
    render(<App />);
    await screen.findByText(/部分应用的接入设置暂时无法读取/);
    const codex = screen
      .getByRole("heading", { name: "Codex Desktop" })
      .closest("article")!;
    expect(codex).toHaveTextContent("状态待确认");
    expect(codex).not.toHaveTextContent("待接入");
    fireEvent.click(within(codex).getByRole("button", { name: "检查并修复" }));
    await screen.findByText(/不会用默认选择覆盖/);
    expect(
      native.mock.calls.some(([cmd]) => cmd === "activate_desktop_tool_v1"),
    ).toBe(false);
  });
  it("keeps raw model descriptions off both model surfaces while retaining group choices", async () => {
    mockNativeByCommand(async (command, args) => {
      const request = (args as Args).request;
      if (command === "manage_tool_connections_v1")
        return connectionsFixture(request.requestId);
      if (command === "scan_activation_targets_v1")
        return activationTargetScan(request.requestId);
      if (command === "scan_desktop_apps_read_only")
        return discovery(request.requestId);
      if (command === "read_public_service_catalog")
        return catalogFixture(request.requestId, request.lineId);
      if (command === "account_inspect_v2") {
        const projection = signedIn(request.requestId);
        projection.models[0].description =
          "Synthetic upstream via private connector";
        return projection;
      }
      throw Error("Not available in test");
    });
    render(<App />);
    await screen.findAllByText("野菜测试用户");
    for (const page of ["应用接入", "模型与价格"]) {
      fireEvent.click(screen.getByRole("button", { name: page }));
      const picker = await screen.findByRole("combobox", {
        name: /完整模型 ID/,
      });
      fireEvent.click(picker);
      const option = within(screen.getByRole("listbox")).getByRole("option");
      expect(option).toHaveTextContent("glm-5.3");
      expect(screen.queryByText(/Synthetic upstream/)).not.toBeInTheDocument();
      fireEvent.click(option);
      expect(screen.queryByText(/Synthetic upstream/)).not.toBeInTheDocument();
      const group = within(
        screen.getByRole("group", { name: "选择计费分组" }),
      ).getByRole("radio", { name: /国模特价分组/ });
      fireEvent.click(group);
      expect(group).toBeChecked();
    }
  });
  it("#13 hides incompatible models by default, greys them out with a reason when shown", async () => {
    mockNativeByCommand(async (command, args) => {
      const request = (args as Args).request;
      if (command === "manage_tool_connections_v1")
        return connectionsFixture(request.requestId);
      if (command === "scan_activation_targets_v1")
        return activationTargetScan(request.requestId);
      if (command === "scan_desktop_apps_read_only")
        return discovery(request.requestId);
      if (command === "read_public_service_catalog")
        return catalogFixture(request.requestId, request.lineId);
      if (command === "account_inspect_v2") {
        const projection = signedIn(request.requestId);
        // glm-5.3 兼容全部工具；anthropic-only 模型对 codex_desktop 不兼容。
        projection.models[0].supportedEndpointTypes = ["anthropic"];
        projection.models.push({
          ...projection.models[0],
          id: "glm-5.3-full",
          supportedEndpointTypes: ["anthropic", "openai", "openai-response"],
        });
        return projection;
      }
      throw Error("Not available in test");
    });
    render(<App />);
    await screen.findAllByText("野菜测试用户");
    fireEvent.click(screen.getByRole("button", { name: "模型与价格" }));
    await screen.findByRole("combobox", { name: /完整模型 ID/ });

    // 默认工具 claude_desktop（anthropic 直连）：两个模型都兼容。
    fireEvent.click(screen.getByRole("combobox", { name: /完整模型 ID/ }));
    let listbox = screen.getByRole("listbox");
    expect(within(listbox).getAllByRole("option").length).toBe(2);
    fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("listbox")).not.toBeInTheDocument(),
    );

    // 切到 Codex Desktop（需 openai-response/openai）：兼容列表只剩 1 个。
    fireEvent.change(
      screen
        .getByRole("combobox", { name: /使用应用/ })
        .closest("select")!,
      { target: { value: "codex_desktop" } },
    );
    await waitFor(() =>
      expect(
        screen.getByRole("combobox", { name: /完整模型 ID/ }),
      ).toHaveTextContent("glm-5.3-full"),
    );
    fireEvent.click(screen.getByRole("combobox", { name: /完整模型 ID/ }));
    await screen.findByRole("listbox");
    listbox = screen.getByRole("listbox");
    expect(within(listbox).getAllByRole("option").length).toBe(1);
    fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("listbox")).not.toBeInTheDocument(),
    );

    // 打开「查看全部模型」：不兼容项回到列表，置灰并带原因。
    fireEvent.click(screen.getByLabelText("查看全部模型"));
    fireEvent.click(screen.getByRole("combobox", { name: /完整模型 ID/ }));
    const options = await within(
      await screen.findByRole("listbox"),
    ).getAllByRole("option");
    expect(options.length).toBe(2);
    const incompatible = options.find((option) =>
      option.textContent?.includes("不支持此应用的协议"),
    )!;
    expect(incompatible).toHaveAttribute("aria-disabled", "true");
  });
  it("#13 distinguishes no-compatible-models from unreturned data", async () => {
    let modelsReturned = true;
    mockNativeByCommand(async (command, args) => {
      const request = (args as Args).request;
      if (command === "manage_tool_connections_v1")
        return connectionsFixture(request.requestId);
      if (command === "scan_activation_targets_v1")
        return activationTargetScan(request.requestId);
      if (command === "scan_desktop_apps_read_only")
        return discovery(request.requestId);
      if (command === "read_public_service_catalog")
        return catalogFixture(request.requestId, request.lineId);
      if (command === "account_inspect_v2") {
        const projection = signedIn(request.requestId);
        if (!modelsReturned) {
          projection.models = [];
          return projection;
        }
        projection.models[0].supportedEndpointTypes = ["anthropic"];
        return projection;
      }
      throw Error("Not available in test");
    });
    render(<App />);
    await screen.findAllByText("野菜测试用户");
    fireEvent.click(screen.getByRole("button", { name: "模型与价格" }));
    // claude_desktop 与 anthropic 兼容，先切到 codex 才能制造「无兼容模型」。
    fireEvent.change(
      screen
        .getByRole("combobox", { name: /使用应用/ })
        .closest("select")!,
      { target: { value: "codex_desktop" } },
    );
    await screen.findByText(
      /没有兼容此应用的模型。可打开「查看全部模型」了解各模型不适配的原因。/,
    );
    // 空模型数据（数据未返回）走另一口径，不误报为不兼容。
    modelsReturned = false;
    fireEvent.click(screen.getByRole("button", { name: "重新检查" }));
    await screen.findByText(/部分数据暂时没有返回/);
  });
  it("restores saved selections under effect replay and keeps them across navigation", async () => {
    mockNativeByCommand(async (command, args) => {
      const request = (args as Args).request;
      if (command === "manage_tool_connections_v1") {
        const result = connectionsFixture(request.requestId);
        Object.assign(
          result.connections.find((c) => c.toolId === "codex_desktop")!,
          {
            state: "connected",
            restoreMode: "original",
            modelId: "glm-5.3",
            billingGroup: "国模特价分组",
            lineId: "global_accelerated",
            updatedAtEpochMs: 1,
          },
        );
        return result;
      }
      if (command === "scan_activation_targets_v1")
        return activationTargetScan(request.requestId);
      if (command === "account_inspect_v2") return signedIn(request.requestId);
      throw Error("Unexpected command");
    });
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await screen.findByRole("button", { name: "恢复原设置" });
    const card = screen
      .getByRole("heading", { name: "Codex Desktop" })
      .closest("article")!;
    fireEvent.click(within(card).getByRole("button", { name: "换模型与分组" }));
    await act(async () => {});
    expect(screen.getByRole("radio", { name: /国模特价分组/ })).toBeChecked();
    expect(
      screen.getByRole("button", { name: /全球加速 Cloudflare/ }),
    ).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByRole("button", { name: "用量账单" }));
    expect(
      screen.getByRole("heading", { level: 1, name: "用量账单" }),
    ).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "应用接入" }));
    await act(async () => {});
    expect(screen.getByRole("radio", { name: /国模特价分组/ })).toBeChecked();
    expect(screen.getByRole("button", { name: "恢复原设置" })).toBeEnabled();
  });
  it.each(["zh", "zh-TW", "en", "ja"] as const)(
    "uses user-facing copy without engineering or release checklists in %s",
    async (language) => {
      await i18n.changeLanguage(language);
      const { container } = render(<App />);
      await screen.findByText("版本 1.40609.1");
      const c = workbenchCopies[language];
      const checkCopy = () => {
        expect(container.textContent).not.toMatch(
          /NewAPI|\/api\/desktop|Developer ID|\bHTTP\b|\bTCP\b|\bTLS\b|CRUD|web profile|原生桥|安全执行层|安全執行層|契约|契約|投影|适配器|適配器|白名单|白名單|候选版|候選版|发布门槛|發佈門檻|签名|公证|小白用户|小白用戶|backend|contract|adapter|allowlist|hardened runtime|native bridge|notariz|release gate|バックエンド|契約|アダプター|公証/i,
        );
      };
      checkCopy();
      for (const label of [
        c.apps,
        c.models,
        c.usage,
        c.help,
        c.advanced,
        c.settings,
      ]) {
        fireEvent.click(screen.getAllByRole("button", { name: label })[0]);
        checkCopy();
        if (label === c.apps) {
          expect(
            screen.getByTestId("configuration-apply-action"),
          ).toBeEnabled();
          expect(
            screen.getByTestId("configuration-apply-action"),
          ).toHaveTextContent(c.signInFirst);
          expect(
            container.querySelector(".desktop-technical-details"),
          ).not.toHaveAttribute("open");
        }
        if (label === c.usage) {
          expect(
            await screen.findByRole("button", { name: c.signIn }),
          ).toBeEnabled();
          expect(screen.getByText(c.signInBody)).toBeInTheDocument();
        }
      }
      const commands = native.mock.calls.map((call) => call[0]);
      expect(
        commands.every((command) =>
          [
            "scan_desktop_apps_read_only",
            "manage_app_installation_v2",
            "manage_tool_connections_v1",
            "account_inspect_v2",
            "scan_activation_targets_v1",
            "read_desktop_exit_state",
          ].includes(command),
        ),
      ).toBe(true);
      expect(
        commands.filter((command) => command === "account_inspect_v2"),
      ).toHaveLength(1);
      expect(
        commands.filter((command) => command === "read_desktop_exit_state"),
      ).toHaveLength(1);
    },
  );
  it("keeps user-interface translation keys aligned in all four languages", () => {
    const keys = (value: unknown, prefix = ""): string[] =>
      typeof value === "object" && value !== null
        ? Object.entries(value).flatMap(([key, child]) =>
            keys(child, `${prefix}.${key}`),
          )
        : [prefix];
    const namespaces = [
      "yeschoyCatalog",
      "yeschoyConfiguration",
      "yeschoyDesktop",
      "yeschoyDiscovery",
      "yeschoyDiagnostics",
      "yeschoySettings",
    ] as const;
    for (const resource of Object.values(locales))
      for (const namespace of namespaces)
        expect(keys(resource[namespace]).sort()).toEqual(
          keys(zh[namespace]).sort(),
        );
  });
  it("prioritizes login over empty account statistics and preserves discovery truth", async () => {
    render(<App />);
    await screen.findByText("版本 1.40609.1");
    expect(
      screen.queryByRole("region", { name: "用量账单" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "登录野菜 API" })).toBeEnabled();
    // 5 张 V1 应用卡 + 恒显的 OpenCode「即将支持」卡（PRD §3.2）。
    expect(screen.getAllByRole("article")).toHaveLength(6);
    const card = screen
      .getByRole("heading", { name: "Claude Desktop" })
      .closest("article")!;
    expect(card).toHaveTextContent("待接入");
    expect(card).toHaveTextContent("选择模型与分组");
    expect(card).not.toHaveTextContent("已接入");
    expect(screen.queryByText("¥128.60")).not.toBeInTheDocument();
    expect(native.mock.calls.map((call) => call[0])).toEqual([
      "scan_activation_targets_v1",
      "manage_app_installation_v2",
      "manage_tool_connections_v1",
      "account_inspect_v2",
      "read_desktop_exit_state",
    ]);
  });
  it("takes the application navigation to the working setup flow", async () => {
    render(<App />);
    await screen.findByText("版本 1.40609.1");
    fireEvent.click(screen.getByRole("button", { name: "应用接入" }));
    expect(
      screen.getByTestId("configuration-preview-view"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { level: 1, name: "应用接入" }),
    ).toHaveFocus();
    expect(screen.getByTestId("configuration-apply-action")).toBeEnabled();
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "先去登录",
    );
    await act(async () => {});
    expect(native.mock.calls.map(([command]) => command)).toEqual([
      "scan_activation_targets_v1",
      "manage_app_installation_v2",
      "manage_tool_connections_v1",
      "account_inspect_v2",
      "read_desktop_exit_state",
      "manage_app_installation_v2",
      "scan_activation_targets_v1",
    ]);
  });
  it("rejects an incomplete or mismatched result, shows unknown not absent, and retries", async () => {
    // #15：旧 CandidateHomeView/scan_desktop_apps_read_only 首页链路已删；
    // 新首页的诚实性由 ru076 族（connections）与发现页用例覆盖。
    mockDiscoveryOnce(() => discovery("wrong-request"));
    render(<App />);
    await screen.findByText("野菜测试用户").catch(() => undefined);
    expect(
      screen.getByTestId("candidate-home-view"),
    ).not.toHaveTextContent("0 个应用已接入");
  });
  it("keeps the chosen desktop application and directs signed-out users to login", async () => {
    render(<App />);
    await screen.findByText("版本 1.40609.1");
    const codex = screen
      .getByRole("heading", { name: "Codex Desktop" })
      .closest("article")!;
    const openSetup = within(codex).getByRole("button", { name: "开始接入" });
    openSetup.focus();
    fireEvent.click(openSetup);
    expect(screen.getByRole("heading", { level: 1 })).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "更换应用" }));
    expect(
      screen.getByRole("button", { name: /Codex Desktop ChatGPT/ }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("configuration-apply-action")).toBeEnabled();
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "先去登录",
    );
    await act(async () => {});
    expect(native.mock.calls.map(([command]) => command)).toEqual([
      "scan_activation_targets_v1",
      "manage_app_installation_v2",
      "manage_tool_connections_v1",
      "account_inspect_v2",
      "read_desktop_exit_state",
      "manage_app_installation_v2",
      "scan_activation_targets_v1",
    ]);
  });
  it("uses one shared signed-out account state across billing and pricing", async () => {
    render(<App />);
    await screen.findByText("版本 1.40609.1");
    fireEvent.click(screen.getByRole("button", { name: "用量账单" }));
    expect(
      await screen.findByRole("button", { name: "网页登录" }),
    ).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "模型与价格" }));
    expect(
      await screen.findAllByText("登录后可查看当前账号可用模型和实际价格。"),
    ).toHaveLength(2);
    expect(native.mock.calls.map((call) => call[0])).toEqual([
      "scan_activation_targets_v1",
      "manage_app_installation_v2",
      "manage_tool_connections_v1",
      "account_inspect_v2",
      "read_desktop_exit_state",
    ]);
    fireEvent.change(screen.getByRole("combobox", { name: "使用线路" }), {
      target: { value: "global_accelerated" },
    });
    expect(
      await screen.findAllByText("登录后可查看当前账号可用模型和实际价格。"),
    ).toHaveLength(2);
    await act(async () => {});
    expect(
      native.mock.calls.filter((call) => call[0] === "account_inspect_v2"),
    ).toHaveLength(2);
  });
  it("renders signed-in account facts and the auditable price comparison", async () => {
    mockNativeByCommand(async (command, args) => {
      const request = (args as Args).request;
      if (command === "manage_tool_connections_v1")
        return connectionsFixture(request.requestId);
      if (command === "scan_activation_targets_v1")
        return activationTargetScan(request.requestId);
      if (command === "scan_desktop_apps_read_only")
        return discovery(request.requestId);
      if (command === "account_inspect_v2") {
        const old = signedIn(request.requestId);
        return {
          ...old,
          schemaVersion: 5,
          models: old.models.map(({ billing, ...model }) => {
            const { requestUsd: _unknown, ...known } = billing;
            return { ...model, billing: known };
          }),
          money: {
            currency: "CNY",
            balanceAmount: "0.7",
            consumedAmount: "0.24",
            displayRate: "1",
          },
          savings: {
            status: "empty",
            reasonCode: "no_history",
            officialAmount: "",
            siteAmount: "",
            savedAmount: "",
            referenceRate: "",
            priceRate: "",
            recordLimit: 100,
            scannedCount: 0,
            includedCount: 0,
            excludedCount: 0,
            oldestAtEpochMs: 0,
            newestAtEpochMs: 0,
          },
        };
      }
      if (command === "read_public_service_catalog")
        return catalogFixture(request.requestId, request.lineId);
      throw Error("Not available in test");
    });
    render(<App />);
    await screen.findByText("版本 1.40609.1");
    fireEvent.click(screen.getByRole("button", { name: "用量账单" }));
    expect(await screen.findAllByText("野菜测试用户")).toHaveLength(2);
    expect(screen.getByText("¥0.70")).toBeInTheDocument();
    expect(screen.getAllByText("人民币额度")).toHaveLength(2);
    expect(screen.getByRole("button", { name: /去充值/ })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "模型与价格" }));
    expect((await screen.findAllByText("glm-5.3")).length).toBeGreaterThan(1);
    expect(screen.getByText("¥260")).toBeInTheDocument();
    expect(screen.getByText("¥130")).toBeInTheDocument();
    expect(screen.getByText("参考换算值 1")).toBeInTheDocument();
    expect(screen.getByText(/50%/)).toBeInTheDocument();
  });
  it("renders a legitimate negative account balance instead of rejecting the session", async () => {
    mockNativeByCommand(async (command, args) => {
      const request = (args as Args).request;
      if (command === "manage_tool_connections_v1")
        return connectionsFixture(request.requestId);
      if (command === "scan_activation_targets_v1")
        return activationTargetScan(request.requestId);
      if (command === "scan_desktop_apps_read_only")
        return discovery(request.requestId);
      if (command === "account_inspect_v2") {
        const account = signedIn(request.requestId);
        return {
          ...account,
          schemaVersion: 5,
          account: { ...account.account, balanceQuota: "-125000" },
          models: [],
          money: {
            currency: "CNY",
            balanceAmount: "-1.75",
            consumedAmount: "1.68",
            displayRate: "1",
          },
          savings: {
            status: "empty",
            reasonCode: "no_history",
            officialAmount: "",
            siteAmount: "",
            savedAmount: "",
            referenceRate: "",
            priceRate: "",
            recordLimit: 100,
            scannedCount: 0,
            includedCount: 0,
            excludedCount: 0,
            oldestAtEpochMs: 0,
            newestAtEpochMs: 0,
          },
        };
      }
      if (command === "read_public_service_catalog")
        return catalogFixture(request.requestId, request.lineId);
      throw Error("Not available in test");
    });

    render(<App />);
    await screen.findByText("版本 1.40609.1");
    fireEvent.click(screen.getByRole("button", { name: "用量账单" }));

    expect(await screen.findAllByText("野菜测试用户")).toHaveLength(2);
    expect(screen.getByText("-¥1.75")).toBeInTheDocument();
    expect(
      screen.queryByText("账户数据格式不正确，已停止显示。"),
    ).not.toBeInTheDocument();
  });
  it("keeps the signed-in account visible while a new route is refreshing", async () => {
    let inspections = 0;
    let finishRouteRefresh: () => void = () => {};
    mockNativeByCommand(async (command, args) => {
      const request = (args as Args).request;
      if (command === "manage_tool_connections_v1")
        return connectionsFixture(request.requestId);
      if (command === "scan_activation_targets_v1")
        return activationTargetScan(request.requestId);
      if (command === "scan_desktop_apps_read_only")
        return discovery(request.requestId);
      if (command === "account_inspect_v2") {
        inspections += 1;
        if (inspections === 1) return signedIn(request.requestId);
        if (inspections === 2)
          return new Promise((resolve) => {
            finishRouteRefresh = () => resolve(signedIn(request.requestId));
          });
        return {
          ...signedOut(request.requestId),
          status: "network_error",
          reasonCode: "network_error",
        };
      }
      throw Error("Not available in test");
    });
    render(<App />);
    await screen.findAllByText("野菜测试用户");
    fireEvent.click(screen.getByRole("button", { name: "模型与价格" }));
    expect((await screen.findAllByText("glm-5.3")).length).toBeGreaterThan(1);
    fireEvent.change(screen.getByRole("combobox", { name: "使用线路" }), {
      target: { value: "global_accelerated" },
    });
    expect(screen.getAllByText("野菜测试用户")).toHaveLength(1);
    expect(screen.getAllByText("glm-5.3").length).toBeGreaterThan(1);
    await act(async () => finishRouteRefresh());
    fireEvent.change(screen.getByRole("combobox", { name: "使用线路" }), {
      target: { value: "mainland_optimized" },
    });
    await act(async () => {});
    expect(screen.getAllByText("野菜测试用户")).toHaveLength(1);
    expect(screen.getAllByText("glm-5.3").length).toBeGreaterThan(1);
  });
  it.each(["default", "国模特价分组"])(
    "configures the selected desktop app using the displayed billing group: %s",
    async (billingGroup) => {
      mockNativeByCommand(async (command, args) => {
        const request = (args as Args).request;
        if (command === "manage_tool_connections_v1")
          return connectionsFixture(request.requestId);
        if (command === "scan_activation_targets_v1")
          return activationTargetScan(request.requestId);
        if (command === "scan_desktop_apps_read_only")
          return discovery(request.requestId);
        if (command === "account_inspect_v2")
          return signedIn(request.requestId);
        if (command === "scan_activation_targets_v1")
          return activationTargetScan(request.requestId);
        if (command === "configure_desktop_tool_v2")
          return {
            requestId: request.requestId,
            schemaVersion: 3,
            status: "ready",
            toolId: "codex_desktop",
            modelId: "glm-5.3",
            billingGroup,
            observedAtEpochMs: 1_788_195_600_000,
            reasonCode: "configuration_ready",
          };
        throw Error("Not available in test");
      });
      render(<App />);
      await screen.findByText("版本 1.40609.1");
      const codex = screen
        .getByRole("heading", { name: "Codex Desktop" })
        .closest("article")!;
      fireEvent.click(within(codex).getByRole("button", { name: "开始接入" }));
      await screen.findByRole("button", { name: "一键接入" });
      if (billingGroup === "国模特价分组") {
        fireEvent.click(screen.getByRole("radio", { name: /国模特价分组/ }));
        expect(screen.getByText("¥91")).toBeInTheDocument();
        expect(
          screen.getByRole("radio", { name: /国模特价分组/ }),
        ).toBeChecked();
      }
      fireEvent.click(screen.getByTestId("configuration-apply-action"));
      expect(await screen.findByText("接入完成")).toBeInTheDocument();
      expect(
        screen.getByText(
          "Codex Desktop 的设置和野菜本地路由已经就绪。Codex 仍可显示你的官方登录账号，那只是登录身份，不代表模型请求走官方计费。第一次真实请求的结果会显示在“最近野菜中转记录”里；看到完整模型 ID，才表示这次请求确实经过野菜中转。",
        ),
      ).toBeInTheDocument();
      expect(native).toHaveBeenCalledWith("configure_desktop_tool_v2", {
        request: {
          requestId: expect.stringMatching(/^activate-/),
          lineId: "mainland_optimized",
          toolId: "codex_desktop",
          modelId: "glm-5.3",
          installationId: "i0000000000000003",
          billingGroup,
          models: [{ modelId: "glm-5.3", billingGroup }],
        },
      });
    },
  );
  it("allows setup when the group is valid but its dynamic price cannot be converted", async () => {
    mockNativeByCommand(async (command, args) => {
      const request = (args as Args).request;
      if (command === "manage_tool_connections_v1")
        return connectionsFixture(request.requestId);
      if (command === "scan_activation_targets_v1")
        return activationTargetScan(request.requestId);
      if (command === "scan_desktop_apps_read_only")
        return discovery(request.requestId);
      if (command === "scan_activation_targets_v1")
        return activationTargetScan(request.requestId);
      if (command === "account_inspect_v2") {
        const account = signedIn(request.requestId);
        account.models[0] = {
          ...account.models[0],
          billingMode: "tiered_expr",
          pricingAvailable: false,
          officialInputCnyPerMillion: "",
          officialOutputCnyPerMillion: "",
          actualInputCnyPerMillion: "",
          actualOutputCnyPerMillion: "",
          billing: {
            ...account.models[0].billing,
            expression: 'param("service_tier") == "fast" ? p * 3 : p',
          },
        };
        return account;
      }
      if (command === "configure_desktop_tool_v2")
        return {
          requestId: request.requestId,
          schemaVersion: 3,
          status: "ready",
          toolId: "claude_desktop",
          modelId: "glm-5.3",
          billingGroup: "default",
          observedAtEpochMs: 1_788_195_600_000,
          reasonCode: "configuration_ready",
        };
      throw Error("Not available in test");
    });
    render(<App />);
    await screen.findByText("版本 1.40609.1");
    fireEvent.click(screen.getByRole("button", { name: "应用接入" }));
    expect(
      await screen.findByText(
        /当前规则无法可靠换算为 Token 费用，暂不展示估算/,
      ),
    ).toBeInTheDocument();
    expect(screen.getByTestId("configuration-apply-action")).toBeEnabled();
    fireEvent.click(screen.getByTestId("configuration-apply-action"));
    expect(await screen.findByText("接入完成")).toBeInTheDocument();
  });

  it("syncs appearance controls, follows system changes and persists only an enum", async () => {
    const save = vi.spyOn(Storage.prototype, "setItem");
    render(<App />);
    await screen.findByText("版本 1.40609.1");
    fireEvent.click(screen.getByRole("button", { name: "深色" }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(save).toHaveBeenLastCalledWith(APPEARANCE_KEY, "dark");
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    expect(
      screen
        .getAllByRole("button", { name: "深色" })
        .every((el) => el.getAttribute("aria-pressed") === "true"),
    ).toBe(true);
    fireEvent.click(screen.getAllByRole("button", { name: "跟随系统" })[0]);
    expect(document.documentElement.dataset.theme).toBe("light");
    systemDark = true;
    act(() =>
      appearanceListeners.forEach((listener) =>
        listener({ matches: systemDark }),
      ),
    );
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(native.mock.calls.map(([command]) => command)).toEqual([
      "scan_activation_targets_v1",
      "manage_app_installation_v2",
      "manage_tool_connections_v1",
      "account_inspect_v2",
      "read_desktop_exit_state",
    ]);
  });
  it("keeps appearance usable when storage fails and rejects invalid preferences", async () => {
    localStorage.setItem(APPEARANCE_KEY, "unexpected-value");
    expect(readAppearance()).toBe("system");
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw Error("denied");
    });
    expect(readAppearance()).toBe("system");
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw Error("denied");
    });
    render(<App />);
    await screen.findByText("版本 1.40609.1");
    fireEvent.click(screen.getByRole("button", { name: "深色" }));
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
  it("defines complete localized copy, a single font system and accessible responsive themes", () => {
    const keys = Object.keys(workbenchCopies.zh).sort();
    for (const copy of Object.values(workbenchCopies)) {
      expect(Object.keys(copy).sort()).toEqual(keys);
      expect(Object.values(copy).every((value) => value.length > 0)).toBe(true);
    }
    const css = readFileSync("src/index.css", "utf8");
    expect(css).toContain("--font-ui:");
    expect(css).toContain("--font-mono:");
    expect(css).not.toMatch(/Georgia|Times New Roman|@import|fonts\.google/);
    for (const token of [
      "data-theme",
      "button:focus-visible",
      "prefers-reduced-motion",
      "overflow-wrap: anywhere",
    ])
      expect(css).toContain(token);
    // After the CSS architecture merge, component rules (responsive
    // breakpoints, numeric variants) live in workbench-v2.css.
    const components = readFileSync("src/workbench/workbench-v2.css", "utf8");
    for (const token of ["max-width: 900px", "max-width: 1100px", "tabular-nums"])
      expect(components).toContain(token);
  });
  it("maintains readable text contrast on light and dark surfaces", () => {
    const css = readFileSync("src/index.css", "utf8");
    const luminance = (hex: string) => {
      const full =
        hex.length === 3
          ? [...hex].map((digit) => digit + digit).join("")
          : hex;
      return [0, 2, 4]
        .map((i) => parseInt(full.slice(i, i + 2), 16) / 255)
        .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
        .reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
    };
    for (const selector of [":root", ':root[data-theme="dark"]']) {
      const block = css.slice(css.indexOf(selector)).split("}")[0];
      const colors = Object.fromEntries(
        [...block.matchAll(/--([\w-]+): #([\da-f]+);/g)].map((match) => [
          match[1],
          match[2],
        ]),
      );
      for (const fg of ["ink", "muted", "faint"]) {
        for (const bg of [
          "bg",
          "surface",
          "subtle",
          "sidebar",
          "accent-soft",
        ]) {
          const values = [luminance(colors[fg]), luminance(colors[bg])].sort(
            (a, b) => b - a,
          );
          expect(
            (values[0] + 0.05) / (values[1] + 0.05),
            `${selector} ${fg}/${bg}`,
          ).toBeGreaterThanOrEqual(4.5);
        }
      }
    }
  });
});
