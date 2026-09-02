import {
  act,
  fireEvent,
  render,
  screen,
  within,
  cleanup,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import tw from "../i18n/locales/zh-TW.json";
import en from "../i18n/locales/en.json";
import ja from "../i18n/locales/ja.json";
import App from "../App";
import { CandidateHomeView } from "../candidate/CandidateHomeView";
import { APPEARANCE_KEY, readAppearance } from "./appearance";
import { workbenchCopies } from "./copy";
import { catalogFixture } from "../service-catalog/test-fixtures";
import { readFileSync } from "node:fs";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);
const locales = { zh, "zh-TW": tw, en, ja };
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
      },
    ],
    comparisonFx: "1",
    reasonCode: "none",
  };
}
const accountSession = () => ({
  projection: null,
  loading: false,
  refresh: vi.fn(async () => null),
  beginAuthorization: vi.fn(async () => null),
  cancelAuthorization: vi.fn(async () => null),
  logout: vi.fn(async () => null),
  openWallet: vi.fn(async () => true),
});
const callbacks = () => ({
  onOpenAccount: vi.fn(),
  onOpenSetup: vi.fn(),
  onOpenDiagnostics: vi.fn(),
  onOpenTools: vi.fn(),
  onOpenSettings: vi.fn(),
  accountSession: accountSession(),
});
type Args = {
  request: {
    requestId: string;
    lineId: "mainland_optimized" | "global_accelerated";
  };
};
let systemDark = false;
let appearanceListener: () => void = () => {};
beforeEach(async () => {
  native.mockReset();
  native.mockImplementation(async (command, args) => {
    const request = (args as Args).request;
    if (command === "scan_desktop_apps_read_only")
      return discovery(request.requestId);
    if (command === "read_public_service_catalog")
      return catalogFixture(request.requestId, request.lineId);
    if (command === "account_inspect_v2") return signedOut(request.requestId);
    throw Error("Not available in test");
  });
  for (const [language, resource] of Object.entries(locales))
    i18n.addResourceBundle(language, "translation", resource, true, true);
  await i18n.changeLanguage("zh");
  localStorage.removeItem(APPEARANCE_KEY);
  systemDark = false;
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      get matches() {
        return systemDark;
      },
      addEventListener: (_name: string, callback: () => void) => {
        appearanceListener = callback;
      },
      removeEventListener: vi.fn(),
    })),
  );
  vi.stubGlobal("scrollTo", vi.fn());
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("official workbench", () => {
  it.each(["zh", "zh-TW", "en", "ja"] as const)(
    "uses user-facing copy without engineering or release checklists in %s",
    async (language) => {
      await i18n.changeLanguage(language);
      const { container } = render(<App />);
      await screen.findByText("1.2.3");
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
          ["scan_desktop_apps_read_only", "account_inspect_v2"].includes(
            command,
          ),
        ),
      ).toBe(true);
      expect(
        commands.filter((command) => command === "account_inspect_v2"),
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
  it("renders only real discovery evidence and unavailable account values", async () => {
    render(<App />);
    await screen.findByText("1.2.3");
    const stats = screen.getByRole("region", { name: "用量账单" });
    expect(within(stats).getAllByLabelText("暂无账户数据")).toHaveLength(3);
    expect(stats).toHaveTextContent("1个");
    const card = screen
      .getByRole("heading", { name: "Claude Desktop" })
      .closest("article")!;
    expect(card).toHaveTextContent("尚未读取");
    expect(card).toHaveTextContent("连接状态未确认");
    expect(card).not.toHaveTextContent("已接入");
    expect(screen.queryByText("¥128.60")).not.toBeInTheDocument();
    expect(native.mock.calls.map((call) => call[0])).toEqual([
      "scan_desktop_apps_read_only",
      "account_inspect_v2",
    ]);
  });
  it("takes the application navigation to the working setup flow", async () => {
    render(<App />);
    await screen.findByText("1.2.3");
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
    expect(native).toHaveBeenCalledTimes(2);
  });
  it("rejects an incomplete or mismatched result, shows unknown not absent, and retries", async () => {
    native.mockResolvedValueOnce(discovery("wrong-request"));
    render(<CandidateHomeView {...callbacks()} />);
    await screen.findByRole("alert");
    expect(screen.getAllByText("状态未知")).toHaveLength(2);
    expect(screen.queryByText("未发现应用")).not.toBeInTheDocument();
    expect(
      screen.getByRole("region", { name: "用量账单" }),
    ).not.toHaveTextContent("0个");
    fireEvent.click(screen.getByRole("button", { name: "重新检查" }));
    await screen.findByText("1.2.3");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
  it.each([
    {
      status: "unsupported_platform",
      count: undefined,
      note: "当前系统尚不支持识别此应用。",
    },
    {
      status: "multiple_installations",
      count: 2,
      note: "发现多个安装，尚不能确认要使用哪一个。",
    },
    {
      status: "not_found",
      count: 0,
      note: "未发现安装信息，可先了解接入方式。",
    },
  ])(
    "keeps the summary honest for $status",
    async ({ status, count, note }) => {
      native.mockImplementationOnce(async (_command, args) => {
        const result = discovery((args as Args).request.requestId);
        result.apps = result.apps.map((app) => ({
          ...app,
          status,
          version: "",
          bundleIdentifier: "",
          candidateCount: status === "multiple_installations" ? 2 : 0,
          locationHint:
            status === "multiple_installations"
              ? "multiple"
              : status === "unsupported_platform"
                ? "unsupported"
                : "none",
          configurationStatus:
            status === "multiple_installations"
              ? "documented_unverified"
              : "not_applicable",
          reasonCode:
            status === "multiple_installations"
              ? "multiple_desktop_apps_found"
              : status === "unsupported_platform"
                ? "desktop_platform_not_supported"
                : "desktop_app_not_found",
        }));
        return result;
      });
      render(<CandidateHomeView {...callbacks()} />);
      await screen.findAllByText(note);
      const stats = screen.getByRole("region", { name: "用量账单" });
      if (count === undefined) {
        expect(within(stats).getByLabelText("暂时无法读取")).toHaveTextContent(
          "—",
        );
        expect(stats).not.toHaveTextContent("0个");
      } else {
        expect(stats).toHaveTextContent(`${count}个`);
      }
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    },
  );
  it("does not present a partial discovery as a complete total", async () => {
    native.mockImplementationOnce(async (_command, args) => {
      const result = discovery((args as Args).request.requestId);
      Object.assign(result.apps[1], {
        status: "unsupported_platform",
        locationHint: "unsupported",
        reasonCode: "desktop_platform_not_supported",
      });
      return result;
    });
    render(<CandidateHomeView {...callbacks()} />);
    await screen.findByText("1.2.3");
    const stats = screen.getByRole("region", { name: "用量账单" });
    expect(within(stats).getByLabelText("暂时无法读取")).toHaveTextContent("—");
    expect(stats).not.toHaveTextContent("1个");
  });
  it("ignores an old completion after unmount and preserves the newest mount result", async () => {
    let resolveOld: (value: unknown) => void = () => {};
    let oldRequest = "";
    native.mockImplementationOnce((_command, args) => {
      oldRequest = (args as Args).request.requestId;
      return new Promise((resolve) => {
        resolveOld = resolve;
      });
    });
    const first = render(<CandidateHomeView {...callbacks()} />);
    first.unmount();
    render(<CandidateHomeView {...callbacks()} />);
    await screen.findByText("1.2.3");
    await act(async () => resolveOld(discovery(oldRequest, "old-0.0.1")));
    expect(screen.queryByText("old-0.0.1")).not.toBeInTheDocument();
    expect(screen.getByText("1.2.3")).toBeInTheDocument();
  });
  it("keeps the chosen desktop application and directs signed-out users to login", async () => {
    render(<App />);
    await screen.findByText("1.2.3");
    const codex = screen
      .getByRole("heading", { name: "Codex" })
      .closest("article")!;
    const openSetup = within(codex).getByRole("button", { name: "查看接入" });
    openSetup.focus();
    fireEvent.click(openSetup);
    expect(screen.getByRole("heading", { level: 1 })).toHaveFocus();
    expect(
      screen.getByRole("button", { name: /Codex ChatGPT/ }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("configuration-apply-action")).toBeEnabled();
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "先去登录",
    );
    expect(native).toHaveBeenCalledTimes(2);
  });
  it("uses one shared signed-out account state across billing and pricing", async () => {
    render(<App />);
    await screen.findByText("1.2.3");
    fireEvent.click(screen.getByRole("button", { name: "用量账单" }));
    expect(
      await screen.findByRole("button", { name: "网页登录" }),
    ).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "模型与价格" }));
    expect(
      await screen.findAllByText("登录后可查看当前账号可用模型和实际价格。"),
    ).toHaveLength(2);
    expect(native.mock.calls.map((call) => call[0])).toEqual([
      "scan_desktop_apps_read_only",
      "account_inspect_v2",
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
    native.mockImplementation(async (command, args) => {
      const request = (args as Args).request;
      if (command === "scan_desktop_apps_read_only")
        return discovery(request.requestId);
      if (command === "account_inspect_v2") return signedIn(request.requestId);
      if (command === "read_public_service_catalog")
        return catalogFixture(request.requestId, request.lineId);
      throw Error("Not available in test");
    });
    render(<App />);
    await screen.findByText("1.2.3");
    fireEvent.click(screen.getByRole("button", { name: "用量账单" }));
    expect(await screen.findAllByText("野菜测试用户")).toHaveLength(2);
    expect(screen.getByText(/0\.70/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /去充值/ })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "模型与价格" }));
    expect((await screen.findAllByText("glm-5.3")).length).toBeGreaterThan(1);
    expect(screen.getByText("¥2.00")).toBeInTheDocument();
    expect(screen.getByText("¥1.00")).toBeInTheDocument();
    expect(screen.getByText("1 USD = 1 CNY")).toBeInTheDocument();
    expect(screen.getByText(/50%/)).toBeInTheDocument();
  });
  it("keeps the signed-in account visible while a new route is refreshing", async () => {
    let inspections = 0;
    let finishRouteRefresh: () => void = () => {};
    native.mockImplementation(async (command, args) => {
      const request = (args as Args).request;
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
  it("configures the selected desktop app with the signed-in account model", async () => {
    native.mockImplementation(async (command, args) => {
      const request = (args as Args).request;
      if (command === "scan_desktop_apps_read_only")
        return discovery(request.requestId);
      if (command === "account_inspect_v2") return signedIn(request.requestId);
      if (command === "configure_desktop_tool_v1")
        return {
          requestId: request.requestId,
          schemaVersion: 1,
          status: "configured",
          toolId: "codex_desktop",
          modelId: "glm-5.3",
          observedAtEpochMs: 1_788_195_600_000,
          reasonCode: "none",
        };
      throw Error("Not available in test");
    });
    render(<App />);
    await screen.findByText("1.2.3");
    const codex = screen
      .getByRole("heading", { name: "Codex" })
      .closest("article")!;
    fireEvent.click(within(codex).getByRole("button", { name: "查看接入" }));
    await screen.findByRole("button", { name: "一键接入" });
    fireEvent.click(screen.getByTestId("configuration-apply-action"));
    expect(await screen.findByText("接入成功")).toBeInTheDocument();
    expect(
      screen.getByText("Codex 已连接野菜API，重新打开应用即可使用。"),
    ).toBeInTheDocument();
    expect(native).toHaveBeenCalledWith("configure_desktop_tool_v1", {
      request: {
        requestId: expect.stringMatching(/^activate-/),
        lineId: "mainland_optimized",
        toolId: "codex_desktop",
        modelId: "glm-5.3",
      },
    });
  });
  it("syncs appearance controls, follows system changes and persists only an enum", async () => {
    const save = vi.spyOn(Storage.prototype, "setItem");
    render(<App />);
    await screen.findByText("1.2.3");
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
    act(() => appearanceListener());
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(native).toHaveBeenCalledTimes(2);
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
    await screen.findByText("1.2.3");
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
      "min-width: 320px",
      "max-width: 420px",
      "tabular-nums",
      "overflow-wrap: anywhere",
    ])
      expect(css).toContain(token);
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
