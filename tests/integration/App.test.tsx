/** Current Yecai journeys: real App, hooks, providers and IPC decoders.
 * Retired CC Switch homepage expectations are replaced, not skipped.
 * useProviderActions/PiProviderForm/OpenClawProviderActions/UnifiedSkillsPanel
 * and sync-section component suites remain in the full repository run.
 */
import { StrictMode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import i18n from "i18next";
import App from "@/App";
import zh from "@/i18n/locales/zh.json";
import {
  ACTIVATION_TOOL_IDS,
  type ActivationToolId,
} from "@/configuration/activation";
import type { AccountProjection } from "@/account/session";
import {
  installationActive,
  type InstallationProgress,
} from "@/installation/api";
import { installationInspectionFixture } from "@/installation/test-fixtures";
import { connectionsFixture } from "@/configuration/connection-test-fixtures";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);
type Request = {
  requestId: string;
  toolId: ActivationToolId;
  action: string;
  operation: string;
  lineId: string;
};
let signedIn: boolean, connected: boolean, scanFails: boolean;
let job: InstallationProgress | null;
let unexpected: string[];
const tick = async () => {
  await act(async () => {});
};
const requests = (command: string) =>
  native.mock.calls
    .filter(([c]) => c === command)
    .map(([, args]) => (args as { request: Request }).request);
function account(requestId: string): AccountProjection {
  return {
    schemaVersion: 3,
    requestId,
    status: signedIn ? "signed_in" : "signed_out",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 1000,
    account: {
      available: signedIn,
      displayName: signedIn ? "测试用户" : "",
      username: signedIn ? "fixture-user" : "",
      balanceQuota: "500000",
      usedQuota: "1000",
      requestCount: "2",
      quotaPerUnit: "500000",
    },
    usage: {
      available: false,
      consumedQuota: "",
      requestRate: "",
      tokenCount: "",
    },
    comparisonFx: "1",
    reasonCode: signedIn ? "none" : "signed_out",
    models: signedIn
      ? [
          {
            id: "model-a",
            description: "Private upstream via internal connector",
            billingMode: "ratio",
            supportedEndpointTypes: ["openai", "anthropic", "openai-response"],
            pricingAvailable: false,
            officialInputCnyPerMillion: "",
            officialOutputCnyPerMillion: "",
            actualInputCnyPerMillion: "",
            actualOutputCnyPerMillion: "",
            billing: {
              groups: [
                { id: "default", description: "标准", ratio: 0.7 },
                { id: "优惠组", description: "账户价格", ratio: 0.15 },
              ],
              baseInputUsd: 2,
              baseOutputUsd: 8,
              cacheReadUsd: 0.2,
              requestUsd: null,
              expression: "",
            },
          },
        ]
      : [],
  };
}
function plan(r: Request) {
  return installationInspectionFixture({
    request: {
      requestId: r.requestId,
      action: "inspect",
      toolId: r.toolId,
      jobId: "",
    },
  });
}
function card(name: string) {
  const article = screen.getByRole("heading", { name }).closest("article");
  expect(article).not.toBeNull();
  return within(article!);
}
async function home() {
  render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
  await tick();
  expect(screen.getByRole("button", { name: "检查应用" })).toBeEnabled();
}
async function selectMissingCodex() {
  await home();
  fireEvent.click(
    card("Codex Desktop").getByRole("button", { name: "安装并接入" }),
  );
  await tick();
  return within(screen.getByRole("region", { name: "Codex Desktop 安装" }));
}
beforeEach(async () => {
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  await i18n.changeLanguage("zh");
  localStorage.clear();
  signedIn = false;
  connected = false;
  scanFails = false;
  job = null;
  unexpected = [];
  vi.stubGlobal("scrollTo", vi.fn());
  // jsdom does not implement modal dialogs; emulate the browser open state.
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  native.mockReset();
  native.mockImplementation(async (command, args) => {
    if (command === "read_desktop_exit_state")
      return { closeRequested: false, shutdown: null };
    const r = (args as { request: Request }).request;
    if (command === "account_begin_authorization_v2") {
      signedIn = true;
      return account(r.requestId);
    }
    if (command === "account_inspect_v2") return account(r.requestId);
    if (command === "scan_activation_targets_v1") {
      if (scanFails) throw Error("fixture scan unavailable");
      return {
        schemaVersion: 1,
        requestId: r.requestId,
        platform: "macos",
        targets: ACTIVATION_TOOL_IDS.map((toolId, i) => ({
          toolId,
          displayName: toolId,
          surface: "本机应用",
          status:
            connected && toolId === "codex_desktop" ? "available" : "not_found",
          installations:
            connected && toolId === "codex_desktop"
              ? [
                  {
                    installationId: "i" + String(i + 1).padStart(16, "0"),
                    label: "本机应用",
                    version: "1.0.0",
                    supported: true,
                    recommended: true,
                  },
                ]
              : [],
        })),
      };
    }
    if (command === "manage_tool_connections_v1") {
      if (r.operation === "restore") {
        expect(r.toolId).toBe("codex_desktop");
        connected = false;
      }
      const result = connectionsFixture(r.requestId);
      if (connected)
        Object.assign(
          result.connections.find((c) => c.toolId === "codex_desktop")!,
          {
            state: "connected",
            modelId: "model-a",
            lineId: "mainland_optimized",
            billingGroup: "优惠组",
            restoreMode: "original",
            updatedAtEpochMs: 1,
          },
        );
      return {
        ...result,
        status: r.operation === "restore" ? "restored" : "ok",
      };
    }
    if (command === "manage_app_installation_v2") {
      if (r.action === "start")
        job = {
          ...plan(r),
          jobId: "job-1",
          phase: "downloading",
          canCancel: true,
        };
      if (r.action === "cancel" && job)
        job = { ...job, phase: "cancelled", canCancel: false };
      return {
        ...(job && (job.toolId === r.toolId || installationActive(job))
          ? job
          : plan(r)),
        requestId: r.requestId,
      };
    }
    unexpected.push(command);
    throw Error("Unexpected native boundary: " + command);
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  expect(unexpected).toEqual([]);
});

describe("ru051 current App integration", () => {
  it("shows installation choices on a clean computer and performs only passive reads", async () => {
    await home();
    expect(screen.getAllByRole("button", { name: "安装并接入" })).toHaveLength(
      2,
    );
    expect(screen.queryByTestId("provider-list")).not.toBeInTheDocument();
    expect(new Set(native.mock.calls.map(([c]) => c))).toEqual(
      new Set([
        "read_desktop_exit_state",
        "account_inspect_v2",
        "manage_tool_connections_v1",
        "scan_activation_targets_v1",
        "manage_app_installation_v2",
      ]),
    );
    expect(
      requests("manage_app_installation_v2").every(
        (r) => r.action === "inspect",
      ),
    ).toBe(true);
    expect(
      requests("manage_tool_connections_v1").every(
        (r) => r.operation === "inspect",
      ),
    ).toBe(true);
  });
  it("opens the chosen missing app with an enabled install action, not the startup Claude plan", async () => {
    const panel = await selectMissingCodex();
    expect(panel.getByRole("button", { name: "先安装应用" })).toBeEnabled();
    fireEvent.click(panel.getByRole("button", { name: "先安装应用" }));
    await tick();
    expect(panel.getByRole("status")).toHaveTextContent("正在下载应用");
    expect(
      requests("manage_app_installation_v2").filter(
        (r) => r.action === "start",
      ),
    ).toEqual([expect.objectContaining({ toolId: "codex_desktop" })]);
    expect(
      native.mock.calls.some(([c]) => c === "configure_desktop_tool_v2"),
    ).toBe(false);
  });
  it("keeps an installation visible across navigation without replaying start", async () => {
    const panel = await selectMissingCodex();
    fireEvent.click(panel.getByRole("button", { name: "先安装应用" }));
    await tick();
    fireEvent.click(screen.getByRole("button", { name: "用量账单" }));
    await tick();
    const notice = within(
      screen.getByRole("complementary", { name: "正在进行的安装" }),
    );
    expect(notice.getByText("Codex Desktop")).toBeInTheDocument();
    fireEvent.click(notice.getByRole("button", { name: "查看进度" }));
    await tick();
    expect(
      screen.getByRole("region", { name: "Codex Desktop 安装" }),
    ).toBeVisible();
    expect(
      requests("manage_app_installation_v2").filter(
        (r) => r.action === "start",
      ),
    ).toHaveLength(1);
  });
  it("turns a failed download into an explicit retry without claiming a connection", async () => {
    vi.useFakeTimers();
    const panel = await selectMissingCodex();
    fireEvent.click(panel.getByRole("button", { name: "先安装应用" }));
    await tick();
    job = {
      ...job!,
      phase: "failed",
      canCancel: false,
      reasonCode: "source_unavailable",
    };
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200);
    });
    expect(panel.getByRole("button", { name: "重试安装" })).toBeEnabled();
    expect(screen.queryByText(/^接入成功$/)).not.toBeInTheDocument();
    fireEvent.click(panel.getByRole("button", { name: "重试安装" }));
    await tick();
    expect(
      requests("manage_app_installation_v2").filter(
        (r) => r.action === "start",
      ),
    ).toHaveLength(2);
  });
  it("retains login across both network lines without restarting authorization", async () => {
    await home();
    fireEvent.click(screen.getByRole("button", { name: "用量账单" }));
    await tick();
    fireEvent.click(screen.getByRole("button", { name: "网页登录" }));
    await tick();
    expect(screen.getAllByText("测试用户").length).toBeGreaterThan(0);
    fireEvent.change(screen.getByRole("combobox", { name: "使用线路" }), {
      target: { value: "global_accelerated" },
    });
    await tick();
    expect(screen.getAllByText("测试用户").length).toBeGreaterThan(0);
    expect(
      screen.queryByRole("button", { name: "网页登录" }),
    ).not.toBeInTheDocument();
    expect(requests("account_begin_authorization_v2")).toHaveLength(1);
    expect(requests("account_inspect_v2").at(-1)?.lineId).toBe(
      "global_accelerated",
    );
  });
  it("restores only the explicitly confirmed app even when logged out", async () => {
    connected = true;
    await home();
    fireEvent.click(
      card("Codex Desktop").getByRole("button", { name: "恢复原设置" }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "先不恢复",
      }),
    );
    expect(
      requests("manage_tool_connections_v1").filter(
        (r) => r.operation === "restore",
      ),
    ).toHaveLength(0);
    fireEvent.click(
      card("Codex Desktop").getByRole("button", { name: "恢复原设置" }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "恢复原设置",
      }),
    );
    await tick();
    expect(
      requests("manage_tool_connections_v1").filter(
        (r) => r.operation === "restore",
      ),
    ).toEqual([expect.objectContaining({ toolId: "codex_desktop" })]);
    expect(card("Codex Desktop").getByRole("status")).toHaveTextContent(
      "已恢复接入前的设置",
    );
    expect(requests("account_begin_authorization_v2")).toHaveLength(0);
  });
  it("keeps recovery available when detection fails and supports rechecking", async () => {
    connected = true;
    scanFails = true;
    await home();
    expect(
      card("Codex Desktop").getByRole("button", { name: "恢复原设置" }),
    ).toBeEnabled();
    scanFails = false;
    fireEvent.click(screen.getByRole("button", { name: "检查应用" }));
    await tick();
    expect(
      card("Codex Desktop").getByRole("button", { name: "恢复原设置" }),
    ).toBeEnabled();
    expect(
      requests("manage_tool_connections_v1").every(
        (r) => r.operation === "inspect",
      ),
    ).toBe(true);
  });
  it("offers guided installation for other apps only after an explicit choice", async () => {
    await home();
    fireEvent.click(screen.getByRole("button", { name: "查看其他 5 个应用" }));
    fireEvent.click(card("Pi").getByRole("button", { name: "查看安装方式" }));
    await tick();
    fireEvent.click(screen.getByRole("button", { name: "查看官方安装方式" }));
    await tick();
    expect(
      requests("manage_app_installation_v2").filter(
        (r) => r.action !== "inspect",
      ),
    ).toEqual([expect.objectContaining({ toolId: "pi", action: "help" })]);
  });
  it("exposes model and group choices without private upstream labels", async () => {
    signedIn = true;
    await selectMissingCodex();
    fireEvent.click(screen.getByRole("combobox", { name: /完整模型 ID/ }));
    expect(
      within(screen.getByRole("listbox")).getByRole("option"),
    ).toHaveTextContent("model-a");
    expect(screen.queryByText(/Private upstream/)).not.toBeInTheDocument();
    fireEvent.click(within(screen.getByRole("listbox")).getByRole("option"));
    const discount = within(
      screen.getByRole("group", { name: "选择计费分组" }),
    ).getByRole("radio", { name: /优惠组/ });
    fireEvent.click(discount);
    expect(discount).toBeChecked();
    expect(
      requests("manage_app_installation_v2").every(
        (r) => r.action === "inspect",
      ),
    ).toBe(true);
  });
});
