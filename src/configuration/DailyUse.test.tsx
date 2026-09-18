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
import { toast } from "sonner";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import { ConfigurationPreviewView } from "./ConfigurationPreviewView";
import { AppLibraryView } from "../workbench/AppLibraryView";
import { OpenConnection } from "./OpenConnection";
import { QuitAssistant, ShutdownProvider } from "../settings/QuitAssistant";
import { ACTIVATION_TOOL_IDS, type ActivationTargetScan } from "./activation";
import {
  ConnectionProvider,
  useToolConnections,
  type ToolConnection,
} from "./connections";
import { connectionsFixture } from "./connection-test-fixtures";
import type { AccountSessionController } from "../account/useAccountSession";
import type { AccountModel, AccountProjection } from "../account/session";
import { openConnection } from "./launchApi";
import type { SetupIntent } from "./setupIntent";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));
const native = vi.mocked(invoke);
const tick = async () => {
  await act(async () => {});
};
const model = (id = "model-a"): AccountModel => ({
  id,
  description: "",
  billingMode: "ratio",
  supportedEndpointTypes: ["openai", "openai-response", "anthropic"],
  pricingAvailable: false,
  officialInputCnyPerMillion: "",
  officialOutputCnyPerMillion: "",
  actualInputCnyPerMillion: "",
  actualOutputCnyPerMillion: "",
  billing: {
    groups: [
      { id: "default", description: "标准价", ratio: 0.7 },
      { id: "优惠组", description: "账户价格", ratio: 0.15 },
    ],
    baseInputUsd: 2,
    baseOutputUsd: 8,
    cacheReadUsd: 0.2,
    requestUsd: null,
    expression: "",
  },
});
function session(): AccountSessionController {
  const projection: AccountProjection = {
    requestId: "fixture",
    schemaVersion: 3,
    status: "signed_in",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 1000,
    account: {
      available: true,
      displayName: "普通用户",
      username: "user535",
      balanceQuota: "10",
      usedQuota: "1",
      requestCount: "1",
      quotaPerUnit: "500000",
    },
    usage: {
      available: false,
      consumedQuota: "",
      requestRate: "",
      tokenCount: "",
    },
    models: [model(), model("model-b")],
    comparisonFx: "6.75",
    reasonCode: "none",
  };
  return {
    projection,
    loading: false,
    refresh: vi.fn(async () => projection),
    beginAuthorization: vi.fn(async () => null),
    openAuthorization: vi.fn(async () => null),
    cancelAuthorization: vi.fn(async () => null),
    logout: vi.fn(async () => null),
    openWallet: vi.fn(async () => true),
  };
}
function local(saved: Partial<ToolConnection> = {}) {
  const connections = connectionsFixture("test").connections;
  Object.assign(
    connections.find((c) => c.toolId === "codex_desktop")!,
    {
      state: "connected",
      modelId: "model-a",
      lineId: "mainland_optimized",
      billingGroup: "优惠组",
      restoreMode: "original",
    },
    saved,
  );
  return {
    connections,
    loading: false,
    error: false,
    restoring: null,
    opening: null,
    refresh: vi.fn(async () => {}),
    restore: vi.fn(async () => connectionsFixture("restore")),
    open: vi.fn(async () => "opened" as const),
  };
}
function sessionWithMoney(balanceAmount: string): AccountSessionController {
  const account = session();
  account.projection = {
    ...account.projection!,
    money: {
      currency: "CNY",
      balanceAmount,
      consumedAmount: "13.50",
      displayRate: "6.75",
    },
  };
  return account;
}
function scan(requestId: string): ActivationTargetScan {
  return {
    requestId,
    schemaVersion: 1,
    platform: "macos",
    targets: ACTIVATION_TOOL_IDS.map((toolId, i) => ({
      toolId,
      displayName: toolId,
      surface: "本机应用",
      status: "available",
      installations: [
        {
          installationId: `i${String(i + 1).padStart(16, "0")}`,
          label: "本机应用",
          version: "9.0",
          supported: true,
          recommended: true,
        },
      ],
    })),
  };
}
// Existing lifecycle tests explicitly reapply saved settings. The primary
// "change model" action now opens the editor and must never submit by itself.
function applySavedOrSelected() {
  const primary = screen.getByTestId("configuration-apply-action");
  fireEvent.click(
    primary.textContent?.trim() === "换模型与分组"
      ? screen.getByRole("button", { name: "重新应用当前设置" })
      : primary,
  );
}
const callback = vi.fn();
function setupView(
  account = session(),
  connections = local(),
  line: "mainland_optimized" | "global_accelerated" = "mainland_optimized",
  toolId: (typeof ACTIVATION_TOOL_IDS)[number] = "codex_desktop",
  intent?: SetupIntent,
  active = true,
) {
  return (
    <StrictMode>
      <ConnectionProvider value={connections}>
        <ConfigurationPreviewView
          initialDesktopAppId={toolId}
          setupIntent={intent}
          active={active}
          enableLocalActivation
          lineId={line}
          onLineChange={callback}
          session={account}
          onOpenAccount={callback}
          onOpenTools={callback}
        />
      </ConnectionProvider>
    </StrictMode>
  );
}
beforeEach(async () => {
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  await i18n.changeLanguage("zh");
  callback.mockReset();
  vi.mocked(toast).mockReset();
  vi.mocked(toast.error).mockReset();
  native.mockReset();
  native.mockImplementation(async (command, args) => {
    const req = (
      args as {
        request: {
          requestId: string;
          toolId: string;
          modelId: string;
          billingGroup: string;
        };
      }
    ).request;
    if (command === "scan_activation_targets_v1") return scan(req.requestId);
    if (command === "manage_tool_connections_v1")
      return {
        ...connectionsFixture(req.requestId),
        connections: local().connections,
      };
    if (command === "open_tool_connection_v1")
      return {
        requestId: req.requestId,
        toolId: req.toolId,
        schemaVersion: 1,
        status: "opened",
      };
    if (command === "configure_desktop_tool_v2")
      return {
        ...req,
        schemaVersion: 3,
        status: "verification_failed",
        reasonCode: "endpoint_unavailable",
        observedAtEpochMs: 2000,
        lineId: undefined,
        installationId: undefined,
      };
    throw Error("unexpected command");
  });
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(cleanup);

describe("daily-use UX", () => {
  it("guides recharge on the home page when the balance runs low (PRD 6.2)", async () => {
    const account = sessionWithMoney("3.20");
    render(
      <ConnectionProvider value={local()}>
        <AppLibraryView
          accountSession={account}
          onOpenAccount={callback}
          onOpenSetup={callback}
          onOpenCommunity={callback}
        />
      </ConnectionProvider>,
    );
    await tick();
    expect(
      screen.getByText("当前余额 ¥3.20，为避免请求中断，请先充值。"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /去充值/ }));
    await tick();
    expect(account.openWallet).toHaveBeenCalled();
  });

  it("#22 shows a recovery banner on the home page when a transaction is pending", async () => {
    render(
      <ConnectionProvider value={local({ state: "recovery_pending" })}>
        <AppLibraryView
          accountSession={session()}
          onOpenAccount={callback}
          onOpenSetup={callback}
          onOpenCommunity={callback}
        />
      </ConnectionProvider>,
    );
    await tick();
    const banner = screen.getByTestId("recovery-banner");
    expect(banner).toHaveTextContent(
      "上次退出时有接入操作没有完成，Codex Desktop 的原设置需要先恢复",
    );
    fireEvent.click(screen.getByRole("button", { name: "前往恢复" }));
    expect(callback).toHaveBeenCalledWith("codex_desktop", "repair");
  });

  it("#18 flags retired models on the home page and routes to re-selection", async () => {
    render(
      <ConnectionProvider value={local({ modelId: "model-retired" })}>
        <AppLibraryView
          accountSession={session()}
          onOpenAccount={callback}
          onOpenSetup={callback}
          onOpenCommunity={callback}
        />
      </ConnectionProvider>,
    );
    await tick();
    const banner = screen.getByTestId("offline-model-banner");
    expect(banner).toHaveTextContent(
      "Codex Desktop 正在使用的模型已下线，需要重新选择模型后再继续使用。",
    );
    fireEvent.click(screen.getByRole("button", { name: "重新选择模型" }));
    expect(callback).toHaveBeenCalledWith("codex_desktop");
  });

  it("#18 keeps the home page quiet while every connected model is still listed", async () => {
    render(
      <ConnectionProvider value={local()}>
        <AppLibraryView
          accountSession={session()}
          onOpenAccount={callback}
          onOpenSetup={callback}
          onOpenCommunity={callback}
        />
      </ConnectionProvider>,
    );
    await tick();
    expect(screen.queryByTestId("offline-model-banner")).toBeNull();
  });

  it("guides recharge without blocking setup when the balance is depleted (PRD 6.2)", async () => {
    render(setupView(sessionWithMoney("-0.50")));
    await tick();
    expect(
      screen.getByText("余额已用尽，请求将无法完成，请先充值。"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/余额已用尽：写入设置本身免费，但调用会失败/),
    ).toBeInTheDocument();
    expect(screen.getByTestId("configuration-apply-action")).toBeEnabled();
  });

  it("shows the low-balance banner on the setup page without the depleted warning", async () => {
    render(setupView(sessionWithMoney("3.20")));
    await tick();
    expect(
      screen.getByText("当前余额 ¥3.20，为避免请求中断，请先充值。"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/写入设置本身免费/)).toBeNull();
  });

  it("announces when the line picker follows the app's last-used line", async () => {
    render(setupView(session(), local(), "global_accelerated"));
    await tick();
    expect(callback).toHaveBeenCalledWith("mainland_optimized");
    expect(toast).toHaveBeenCalledWith("已切换到此应用上次使用的线路");
  });

  it("opens the model editor without rewriting an unchanged connection", async () => {
    render(setupView());
    await tick();
    fireEvent.click(screen.getByTestId("configuration-apply-action"));
    await tick();
    expect(screen.getByRole("combobox", { name: /选择模型/ })).toHaveFocus();
    expect(document.querySelector(".setup-advanced")).toHaveAttribute("open");
    expect(
      native.mock.calls.filter(([c]) => c === "configure_desktop_tool_v2"),
    ).toHaveLength(0);
    expect(
      screen.getByRole("button", { name: "重新应用当前设置" }),
    ).toBeEnabled();
  });

  it("downgrades an unsaved selection on an established connection to a secondary prompt (PRD 6.3 #20)", async () => {
    const controller = local({
      models: [
        { modelId: "model-a", billingGroup: "优惠组" },
        { modelId: "model-b", billingGroup: "default" },
      ],
    });
    const view = render(
      setupView(session(), controller, "mainland_optimized", "codex_desktop", {
        appId: "codex_desktop",
        action: "change-model",
        revision: 1,
      }),
    );
    await tick();
    // 已接入且选择一致：换模型与分组（次级）。
    const steady = screen.getByTestId("configuration-apply-action");
    expect(steady).toHaveTextContent("换模型与分组");
    expect(steady.className).toContain("secondary-action");
    fireEvent.click(screen.getByRole("combobox", { name: /选择模型/ }));
    fireEvent.click(screen.getByRole("option", { name: "model-b" }));
    fireEvent.click(screen.getByRole("button", { name: "使用所选模型" }));
    await tick();
    // 选择变化后仍按「已接入」呈现：次级按钮 + 模型提示 + 可用连接卡，
    // 不回退成「未接入」的 primary「一键接入」。
    const action = screen.getByTestId("configuration-apply-action");
    expect(action).toHaveTextContent("保存并应用");
    expect(action.className).toContain("secondary-action");
    expect(action.className).not.toContain("primary-action");
    expect(
      screen.getAllByRole("status").some((node) =>
        node.textContent?.includes("已接入 model-a；当前选择尚未保存。"),
      ),
    ).toBe(true);
    expect(screen.getByRole("button", { name: "打开使用" })).toBeEnabled();
    // 对照：从未接入的应用仍走 primary「一键接入」。
    view.rerender(setupView(session(), local({
      state: "not_connected",
      modelId: "",
      lineId: "",
      billingGroup: "",
      models: [],
    })));
    await tick();
    const fresh = screen.getByTestId("configuration-apply-action");
    expect(fresh).toHaveTextContent("一键接入");
    expect(fresh.className).toContain("primary-action");
    expect(fresh.className).not.toContain("secondary-action");
  });

  it("changes the default explicitly while retaining each other model and its price group", async () => {
    const controller = local({
      models: [
        { modelId: "model-a", billingGroup: "优惠组" },
        { modelId: "model-b", billingGroup: "default" },
      ],
    });
    render(
      setupView(session(), controller, "mainland_optimized", "codex_desktop", {
        appId: "codex_desktop",
        action: "change-model",
        revision: 1,
      }),
    );
    await tick();
    fireEvent.click(screen.getByRole("combobox", { name: /选择模型/ }));
    fireEvent.click(screen.getByRole("option", { name: "model-b" }));
    expect(
      screen.getByRole("radio", { name: "默认模型 model-a" }),
    ).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "使用所选模型" }));
    expect(
      screen.getByRole("radio", { name: "默认模型 model-b" }),
    ).toBeChecked();
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "保存并应用",
    );
    expect(
      native.mock.calls.filter(([c]) => c === "configure_desktop_tool_v2"),
    ).toHaveLength(0);
    fireEvent.click(screen.getByTestId("configuration-apply-action"));
    await tick();
    expect(
      native.mock.calls.find(([c]) => c === "configure_desktop_tool_v2")?.[1],
    ).toMatchObject({
      request: {
        modelId: "model-b",
        billingGroup: "default",
        models: [
          { modelId: "model-a", billingGroup: "优惠组" },
          { modelId: "model-b", billingGroup: "default" },
        ],
      },
    });
    expect(controller.restore).not.toHaveBeenCalled();
  });

  it("honors repeat model-change visits and does not focus the hidden setup page", async () => {
    const account = session(),
      controller = local();
    const intent = {
      appId: "codex_desktop" as const,
      action: "change-model" as const,
      revision: 1,
    };
    const view = render(
      setupView(
        account,
        controller,
        "mainland_optimized",
        "codex_desktop",
        intent,
        false,
      ),
    );
    await tick();
    expect(
      screen.getByRole("combobox", { name: /选择模型/ }),
    ).not.toHaveFocus();
    view.rerender(
      setupView(
        account,
        controller,
        "mainland_optimized",
        "codex_desktop",
        intent,
      ),
    );
    await tick();
    expect(screen.getByRole("combobox", { name: /选择模型/ })).toHaveFocus();
    screen.getByRole("button", { name: "重新应用当前设置" }).focus();
    view.rerender(
      setupView(account, controller, "mainland_optimized", "codex_desktop", {
        ...intent,
        revision: 2,
      }),
    );
    await tick();
    expect(screen.getByRole("combobox", { name: /选择模型/ })).toHaveFocus();
    expect(
      native.mock.calls.filter(([c]) => c === "configure_desktop_tool_v2"),
    ).toHaveLength(0);
  });

  it("puts app actions before account details and routes changed settings to inspection without restoring", async () => {
    const controller = local({ state: "changed" });
    const open = vi.fn();
    const { container } = render(
      <ConnectionProvider value={controller}>
        <AppLibraryView
          accountSession={session()}
          onOpenAccount={callback}
          onOpenSetup={open}
          onOpenCommunity={open}
        />
      </ConnectionProvider>,
    );
    await tick();
    const card = screen
      .getByRole("heading", { name: "Codex Desktop" })
      .closest("article")!;
    const overview = container.querySelector(".account-overview")!;
    expect(
      card.compareDocumentPosition(overview) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(overview).not.toHaveAttribute("open");
    fireEvent.click(within(card).getByRole("button", { name: "检查并修复" }));
    expect(open).toHaveBeenLastCalledWith("codex_desktop", "repair");
    expect(controller.open).not.toHaveBeenCalled();
    expect(controller.restore).not.toHaveBeenCalled();
    fireEvent.click(within(card).getByRole("button", { name: "换模型与分组" }));
    expect(open).toHaveBeenLastCalledWith("codex_desktop", "change-model");
  });

  it("returns to the home-selected app even when its parent value did not change after an internal app switch", async () => {
    const account = session(),
      controller = local();
    const intent: SetupIntent = {
      appId: "claude_desktop",
      action: "change-model",
      revision: 1,
    };
    const view = render(
      setupView(
        account,
        controller,
        "mainland_optimized",
        "claude_desktop",
        intent,
      ),
    );
    await tick();
    fireEvent.click(screen.getByRole("button", { name: "更换应用" }));
    fireEvent.click(
      screen.getByRole("button", { name: /Codex Desktop.*已接入/ }),
    );
    await tick();
    expect(
      document.querySelector(".selected-app-bar strong"),
    ).toHaveTextContent("Codex Desktop");
    view.rerender(
      setupView(account, controller, "mainland_optimized", "claude_desktop", {
        ...intent,
        revision: 2,
      }),
    );
    await tick();
    expect(
      document.querySelector(".selected-app-bar strong"),
    ).toHaveTextContent("Claude Desktop");
    expect(screen.getByRole("combobox", { name: /选择模型/ })).toHaveFocus();
    expect(
      native.mock.calls.filter(([c]) => c === "configure_desktop_tool_v2"),
    ).toHaveLength(0);
  });

  it.each([false, true])(
    "keeps another app's saved connection usable after an external switch (queued: %s)",
    async (queued) => {
      const account = session(),
        controller = local();
      Object.assign(
        controller.connections.find((c) => c.toolId === "claude_desktop")!,
        {
          ...controller.connections.find((c) => c.toolId === "codex_desktop")!,
          toolId: "claude_desktop",
        },
      );
      let finish!: () => void;
      const original = native.getMockImplementation()!;
      native.mockImplementation(async (command, args) => {
        if (command !== "configure_desktop_tool_v2")
          return original(command, args);
        const req = (args as { request: Record<string, unknown> }).request;
        if (queued)
          await new Promise<void>((resolve) => {
            finish = resolve;
          });
        return {
          requestId: req.requestId,
          schemaVersion: 4,
          toolId: req.toolId,
          modelId: req.modelId,
          billingGroup: req.billingGroup,
          models: req.models,
          observedAtEpochMs: 2000,
          status: "ready",
          reasonCode: "desktop_start_observed",
        };
      });
      const view = render(setupView(account, controller));
      await tick();
      applySavedOrSelected();
      await tick();
      view.rerender(
        setupView(account, controller, "mainland_optimized", "claude_desktop", {
          appId: "claude_desktop",
          action: "change-model",
          revision: 1,
        }),
      );
      await tick();
      if (queued) {
        expect(
          document.querySelector(".selected-app-bar strong"),
        ).toHaveTextContent("Codex Desktop");
        expect(screen.getByTestId("configuration-apply-action")).toBeDisabled();
        await act(async () => finish());
      }
      await tick();
      expect(
        document.querySelector(".selected-app-bar strong"),
      ).toHaveTextContent("Claude Desktop");
      expect(screen.getByRole("button", { name: "打开使用" })).toBeEnabled();
      expect(
        screen.getByTestId("configuration-apply-action"),
      ).toHaveTextContent("换模型与分组");
      expect(
        screen.queryByText("配置已保存，已检查启动"),
      ).not.toBeInTheDocument();
      const calls = native.mock.calls.filter(
        ([c]) => c === "configure_desktop_tool_v2",
      );
      expect(calls).toHaveLength(1);
      expect(calls[0][1]).toMatchObject({
        request: { toolId: "codex_desktop" },
      });
      expect(controller.restore).not.toHaveBeenCalled();
    },
  );

  it("does not claim a direct connection needs the assistant running", async () => {
    render(setupView(session(), local({ requiresBackground: false })));
    await tick();
    expect(document.querySelector(".background-note")).toBeNull();
    expect(screen.queryByText(/已确认经野菜中转完成/)).toBeNull();
  });
  it("routes an interrupted open to automatic setup recovery without disconnecting", async () => {
    const controller = {
      ...local(),
      open: vi.fn(async () => "recovery_pending" as const),
    };
    render(
      <ConnectionProvider value={controller}>
        <OpenConnection
          connection={
            controller.connections.find((c) => c.toolId === "codex_desktop")!
          }
          name="Codex Desktop"
          onAdjust={callback}
        />
      </ConnectionProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "打开使用" }));
    await tick();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "先自动恢复未完成的操作",
    );
    expect(screen.getByRole("alert")).not.toHaveTextContent("先恢复原设置");
    fireEvent.click(screen.getByRole("button", { name: "前往接入修复" }));
    expect(callback).toHaveBeenCalledOnce();
    expect(controller.restore).not.toHaveBeenCalled();
  });

  it.each(["codex_desktop", "claude_desktop"] as const)(
    "repairs %s through save/quit confirmation without a manual restore step",
    async (toolId) => {
      let configureCount = 0;
      native.mockImplementation(async (command, args) => {
        const req = (args as { request: Record<string, unknown> }).request;
        if (command === "scan_activation_targets_v1")
          return scan(String(req.requestId));
        if (command !== "configure_desktop_tool_v2")
          throw Error("Unexpected fixture request");
        configureCount++;
        return {
          requestId: req.requestId,
          schemaVersion: 4,
          toolId: req.toolId,
          modelId: req.modelId,
          billingGroup: req.billingGroup,
          models: req.models,
          observedAtEpochMs: 2000,
          status:
            configureCount === 1
              ? "configuration_failed"
              : req.restartRunningApp
                ? "ready"
                : "application_running",
          reasonCode:
            configureCount === 1
              ? "desktop_recovery_waiting_for_exit"
              : req.restartRunningApp
                ? "desktop_start_observed"
                : "save_work_before_restart",
        };
      });
      const controller = local();
      render(setupView(session(), controller, "mainland_optimized", toolId));
      await tick();
      const primary = () => screen.getByTestId("configuration-apply-action");
      applySavedOrSelected();
      await tick();
      expect(primary()).toHaveTextContent("自动修复并重试");
      expect(screen.getByRole("alert")).toHaveTextContent("尚未恢复设置");
      applySavedOrSelected();
      await tick();
      const dialog = screen.getByRole("dialog");
      expect(dialog).toHaveTextContent("请先保存");
      const configureCalls = () =>
        native.mock.calls.filter(
          ([command]) => command === "configure_desktop_tool_v2",
        );
      expect(configureCalls().at(-1)?.[1]).not.toMatchObject({
        request: { restartRunningApp: true },
      });
      fireEvent.click(
        within(dialog).getByRole("button", { name: "已保存，退出并继续" }),
      );
      await tick();
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(screen.getByText("配置已保存，已检查启动")).toBeInTheDocument();
      expect(screen.getByText(/尚未验证应用是否完成加载/)).toBeInTheDocument();
      expect(controller.restore).not.toHaveBeenCalled();
      expect(configureCalls()).toHaveLength(3);
      expect(configureCalls().at(-1)?.[1]).toMatchObject({
        request: { restartRunningApp: true },
      });
    },
  );

  it.each(["codex_desktop", "claude_desktop"] as const)(
    "reconnect regression: %s can change models repeatedly after success without a stranded modal",
    async (toolId) => {
      let configured = false;
      let finish: (() => void) | undefined;
      native.mockImplementation(async (command, args) => {
        const req = (args as { request: Record<string, unknown> }).request;
        if (command === "scan_activation_targets_v1")
          return scan(String(req.requestId));
        if (command !== "configure_desktop_tool_v2")
          throw Error("Unexpected fixture request");
        const result = {
          requestId: req.requestId,
          schemaVersion: 4,
          status: "ready",
          toolId: req.toolId,
          modelId: req.modelId,
          billingGroup: req.billingGroup,
          models: req.models,
          observedAtEpochMs: 2000,
          reasonCode: "configuration_ready",
        };
        if (configured && !req.restartRunningApp)
          return {
            ...result,
            status: "application_running",
            reasonCode: "save_work_before_restart",
          };
        if (configured)
          await new Promise<void>((resolve) => {
            finish = resolve;
          });
        configured = true;
        return result;
      });
      render(setupView(session(), local(), "mainland_optimized", toolId));
      await tick();
      const applyButton = () =>
        screen.getByTestId("configuration-apply-action");
      const configureCalls = () =>
        native.mock.calls.filter(
          ([command]) => command === "configure_desktop_tool_v2",
        );
      applySavedOrSelected();
      await tick();
      expect(configureCalls()).toHaveLength(1);
      for (const [index, modelId] of ["model-b", "model-a"].entries()) {
        fireEvent.click(screen.getByRole("combobox", { name: /选择模型/ }));
        fireEvent.click(screen.getByRole("option", { name: modelId }));
        applySavedOrSelected();
        await tick();
        expect(screen.getByRole("dialog")).toHaveTextContent("请先保存");
        expect(configureCalls().at(-1)?.[1]).not.toMatchObject({
          request: { restartRunningApp: true },
        });
        const beforeCancel = configureCalls().length;
        fireEvent.click(
          within(screen.getByRole("dialog")).getByRole("button", {
            name: "暂不接入",
          }),
        );
        await tick();
        expect(configureCalls()).toHaveLength(beforeCancel);
        expect(getComputedStyle(document.body).pointerEvents).not.toBe("none");
        applySavedOrSelected();
        await tick();
        fireEvent.click(
          within(screen.getByRole("dialog")).getByRole("button", {
            name: "已保存，退出并继续",
          }),
        );
        await tick();
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
        expect(document.querySelector(".yeschoy-dialog-overlay")).toBeNull();
        expect(getComputedStyle(document.body).pointerEvents).not.toBe("none");
        expect(applyButton()).toBeDisabled();
        expect(configureCalls().at(-1)?.[1]).toMatchObject({
          request: { restartRunningApp: true, modelId },
        });
        fireEvent.click(screen.getByRole("button", { name: "查看其他工具" }));
        expect(callback).toHaveBeenCalled();
        expect(finish).toBeTypeOf("function");
        await act(async () => finish!());
        expect(applyButton()).toBeEnabled();
        expect(screen.getByText("接入完成")).toBeInTheDocument();
        // A cancelled prompt may be reopened from the already-known running
        // state; only the first preflight and consented apply invoke native.
        expect(configureCalls()).toHaveLength(1 + (index + 1) * 2);
      }
    },
  );

  it("ru056 enrolls explicit model group pairs without blocking on paid model probes", async () => {
    native.mockImplementation(async (command, args) => {
      const req = (args as { request: Record<string, unknown> }).request;
      if (command === "scan_activation_targets_v1")
        return scan(String(req.requestId));
      if (command === "configure_desktop_tool_v2")
        return {
          requestId: req.requestId,
          schemaVersion: 4,
          status: "ready",
          toolId: req.toolId,
          modelId: req.modelId,
          billingGroup: req.billingGroup,
          models: req.models,
          observedAtEpochMs: 2000,
          reasonCode: "configuration_ready",
        };
      throw Error("Unexpected fixture request");
    });
    render(setupView());
    await tick();
    fireEvent.click(screen.getByRole("button", { name: "加入常用模型" }));
    fireEvent.click(screen.getByRole("combobox", { name: /选择模型/ }));
    fireEvent.click(screen.getByRole("option", { name: "model-b" }));
    fireEvent.click(screen.getByRole("radio", { name: /标准方案/ }));
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "使用所选模型",
    );
    applySavedOrSelected();
    expect(
      native.mock.calls.some(([c]) => c === "configure_desktop_tool_v2"),
    ).toBe(false);
    expect(
      screen.getByRole("radio", { name: "默认模型 model-b" }),
    ).toBeChecked();
    applySavedOrSelected();
    await tick();
    const call = native.mock.calls.find(
      ([c]) => c === "configure_desktop_tool_v2",
    )!;
    expect(call[1]).toMatchObject({
      request: {
        modelId: "model-b",
        billingGroup: "default",
        models: [
          { modelId: "model-a", billingGroup: "优惠组" },
          { modelId: "model-b", billingGroup: "default" },
        ],
      },
    });
    expect(
      screen
        .getAllByRole("status")
        .map((node) => node.textContent)
        .join(" "),
    ).not.toContain("全部模型已验证");
    expect(screen.getByText(/常用模型已一起配置/)).toBeInTheDocument();
    expect(
      screen.getByText(/官方登录账号.*只是登录身份.*不代表模型请求走官方计费/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/每个模型的真实连接结果在首次使用后显示/),
    ).toBeInTheDocument();
  });

  it("ru056 asks before gracefully restarting a running desktop app and never grants that consent implicitly", async () => {
    let configureCount = 0;
    native.mockImplementation(async (command, args) => {
      const req = (args as { request: Record<string, unknown> }).request;
      if (command === "scan_activation_targets_v1")
        return scan(String(req.requestId));
      if (command === "configure_desktop_tool_v2") {
        configureCount += 1;
        return {
          requestId: req.requestId,
          schemaVersion: 4,
          status: configureCount === 1 ? "application_running" : "ready",
          toolId: req.toolId,
          modelId: req.modelId,
          billingGroup: req.billingGroup,
          models: req.models,
          observedAtEpochMs: 2000,
          reasonCode:
            configureCount === 1
              ? "save_work_before_restart"
              : "configuration_ready",
        };
      }
      throw Error("Unexpected fixture request");
    });
    render(setupView());
    await tick();
    applySavedOrSelected();
    await tick();
    const configureCalls = () =>
      native.mock.calls.filter(
        ([command]) => command === "configure_desktop_tool_v2",
      );
    expect(configureCalls()).toHaveLength(1);
    expect(configureCalls()[0][1]).not.toMatchObject({
      request: { restartRunningApp: true },
    });
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("请先保存正在编辑的内容");
    expect(dialog).toHaveTextContent("只剩后台进程");
    expect(dialog).toHaveTextContent("不会按名称结束其他程序");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "已保存，退出并继续" }),
    );
    await tick();
    expect(configureCalls()).toHaveLength(2);
    expect(configureCalls()[1][1]).toMatchObject({
      request: { restartRunningApp: true },
    });
    const statuses = screen
      .getAllByRole("status")
      .map((status) => status.textContent)
      .join(" ");
    expect(statuses).toContain("接入完成");
    expect(statuses).toContain("第一次真实请求的结果会显示");
  });

  it.each(["claude_code", "pi"] as const)(
    "ru056 keeps the current %s session alive and tells the user to start a new one",
    async (toolId) => {
      native.mockImplementation(async (command, args) => {
        const req = (args as { request: Record<string, unknown> }).request;
        if (command === "scan_activation_targets_v1")
          return scan(String(req.requestId));
        if (command === "configure_desktop_tool_v2")
          return {
            requestId: req.requestId,
            schemaVersion: 4,
            status: "ready",
            toolId: req.toolId,
            modelId: req.modelId,
            billingGroup: req.billingGroup,
            models: req.models,
            observedAtEpochMs: 2000,
            reasonCode: "configuration_ready",
          };
        throw Error("Unexpected fixture request");
      });
      render(setupView(session(), local(), "mainland_optimized", toolId));
      await tick();
      applySavedOrSelected();
      await tick();
      const statuses = screen
        .getAllByRole("status")
        .map((status) => status.textContent)
        .join(" ");
      expect(statuses).toContain("正在运行的命令行会话不会被中断");
      expect(statuses).toContain("请新开一个会话");
    },
  );

  it("ru042 keeps unavailable enrolled groups visible and supports removing one model", async () => {
    render(
      setupView(
        session(),
        local({
          models: [
            { modelId: "model-a", billingGroup: "优惠组" },
            { modelId: "model-b", billingGroup: "old-group" },
          ],
        }),
      ),
    );
    await tick();
    expect(screen.getByRole("region", { name: "常用模型" })).toHaveTextContent(
      "old-group · 当前不可用",
    );
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "检查常用模型与分组",
    );
    fireEvent.click(screen.getByRole("button", { name: "移除 model-b" }));
    expect(screen.getByTestId("configuration-apply-action")).toBeEnabled();
    expect(
      screen.getByRole("radio", { name: "默认模型 model-a" }),
    ).toBeChecked();
    expect(
      native.mock.calls.some(([c]) => c === "configure_desktop_tool_v2"),
    ).toBe(false);
  });

  it("ru042 shows recent request route and curated retry error without asserting model identity", async () => {
    render(
      setupView(
        session(),
        local({
          lastRequest: {
            modelId: "gpt-6-astra",
            billingGroup: "优惠组",
            lineId: "global_accelerated",
            outcome: "stream_interrupted",
            httpStatus: 200,
            observedAtEpochMs: 2000,
          },
        }),
      ),
    );
    await tick();
    const result = screen.getByRole("region", { name: "最近连接结果" });
    expect(result).toHaveTextContent("最近野菜中转记录");
    expect(result).toHaveTextContent("回复在完成前中断");
    expect(result).toHaveTextContent("gpt-6-astra");
    expect(result).toHaveTextContent("官方账号是登录身份");
    expect(result).toHaveTextContent("记录来自服务端用量日志");
    expect(result).toHaveTextContent("不依据应用缩写或 AI 的自我介绍判断");
    expect(result).not.toHaveTextContent("连接成功");
  });

  it("ru060 labels a completed bridge observation as confirmed relay use", async () => {
    render(
      setupView(
        session(),
        local({
          lastRequest: {
            modelId: "gpt-6-astra",
            billingGroup: "优惠组",
            lineId: "mainland_optimized",
            outcome: "ok",
            httpStatus: 200,
            observedAtEpochMs: 2000,
          },
        }),
      ),
    );
    await tick();
    const result = screen.getByRole("region", { name: "最近连接结果" });
    expect(result).toHaveTextContent("已确认经野菜中转完成 · HTTP 200");
    expect(result).toHaveTextContent("gpt-6-astra");
    expect(result).toHaveTextContent("Codex 显示的官方账号是登录身份");
  });
  it("never substitutes an unavailable saved group with the standard price", async () => {
    render(setupView(session(), local({ billingGroup: "旧特价组" })));
    await tick();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "之前的计费分组已不可用",
    );
    expect(
      screen
        .getAllByRole("radio")
        .every((radio) => !(radio as HTMLInputElement).checked),
    ).toBe(true);
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "先选择计费分组",
    );
    fireEvent.click(screen.getByRole("radio", { name: /标准方案/ }));
    expect(
      screen.getByRole("region", { name: "所选分组价格" }),
    ).toHaveTextContent("1 亿 Token");
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "保存并应用",
    );
    expect(
      native.mock.calls.some((c) => c[0] === "configure_desktop_tool_v2"),
    ).toBe(false);
  });
  it("keeps a removed saved model visible without silently choosing the first model", async () => {
    render(setupView(session(), local({ modelId: "removed-model" })));
    await tick();
    expect(
      screen.getByRole("combobox", { name: /选择模型/ }),
    ).toHaveTextContent("removed-model");
    expect(screen.getByRole("alert")).toHaveTextContent("不会自动替换");
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "先选择模型",
    );
  });
  it("does not overwrite a user selection when saved state arrives late", async () => {
    const account = session();
    const pending = { ...local(), loading: true };
    const view = render(setupView(account, pending));
    await tick();
    fireEvent.click(screen.getByRole("combobox", { name: /选择模型/ }));
    fireEvent.click(screen.getByRole("option", { name: "model-b" }));
    fireEvent.click(screen.getByRole("radio", { name: /标准方案/ }));
    view.rerender(setupView(account, local()));
    await tick();
    expect(
      screen.getByRole("combobox", { name: /选择模型/ }),
    ).toHaveTextContent("model-b");
    expect(screen.getByRole("radio", { name: /标准方案/ })).toBeChecked();
  });
  it("restores saved choices after the first local-state read failed", async () => {
    const account = session();
    const view = render(setupView(account, { ...local(), error: true }));
    await tick();
    view.rerender(setupView(account, local()));
    await tick();
    const savedGroup = screen.getByRole("radio", { name: /优惠组/ });
    expect(savedGroup).toHaveAttribute("value", "优惠组");
    expect(savedGroup).toBeChecked();
  });
  it("keeps one-click setup enabled when the account changes but its choices stay identical", async () => {
    const account = session();
    const disconnected = local({
      state: "not_connected",
      modelId: "",
      lineId: "",
      billingGroup: "",
      models: [],
    });
    const view = render(setupView(account, disconnected));
    await tick();
    expect(screen.getByTestId("configuration-apply-action")).toBeEnabled();

    view.rerender(
      setupView(
        {
          ...account,
          projection: {
            ...account.projection!,
            account: {
              ...account.projection!.account,
              username: "another-user",
            },
          },
        },
        disconnected,
      ),
    );
    await tick();
    expect(screen.getByTestId("configuration-apply-action")).toBeEnabled();
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "一键接入",
    );
  });
  it("turns a failed application scan into an explicit retry action", async () => {
    let scans = 0;
    native.mockImplementation(async (command) => {
      if (command === "scan_activation_targets_v1") {
        scans += 1;
        throw new Error("scan failed");
      }
      throw new Error("unexpected request");
    });
    render(
      setupView(
        session(),
        local({
          state: "not_connected",
          modelId: "",
          lineId: "",
          billingGroup: "",
          models: [],
        }),
      ),
    );
    await tick();
    const beforeRetry = scans;
    const retry = screen.getByRole("button", { name: "重新检查应用" });
    expect(retry).toBeEnabled();
    expect(screen.getByText(/这次没有完成本机应用检查/)).toBeInTheDocument();
    fireEvent.click(retry);
    await tick();
    expect(scans).toBeGreaterThan(beforeRetry);
  });
  it("turns a failed connection-state read into an explicit retry action", async () => {
    const failed = {
      ...local(),
      connections: [],
      error: true,
    };
    render(setupView(session(), failed));
    await tick();
    const retry = screen.getByRole("button", {
      name: "重新读取接入状态",
    });
    expect(retry).toBeEnabled();
    expect(screen.getByText(/没有读到上次的接入状态/)).toBeInTheDocument();
    fireEvent.click(retry);
    expect(failed.refresh).toHaveBeenCalledOnce();
  });
  it("shows an attempted setup failure even if an old matching connection still exists", async () => {
    native.mockImplementation(async (command, args) => {
      const req = (args as { request: Record<string, string> }).request;
      if (command === "scan_activation_targets_v1") return scan(req.requestId);
      return {
        requestId: req.requestId,
        schemaVersion: 3,
        status: "verification_failed",
        toolId: req.toolId,
        modelId: req.modelId,
        billingGroup: req.billingGroup,
        reasonCode: "endpoint_unavailable",
        observedAtEpochMs: 2000,
      };
    });
    render(setupView());
    await tick();
    applySavedOrSelected();
    await tick();
    expect(screen.getByRole("alert")).toHaveTextContent("暂时没有完成");
    expect(screen.queryByText("接入完成")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看其他线路" }));
    expect(screen.getByRole("button", { name: /全球加速/ })).toBeVisible();
  });
  it("keeps in-flight results bound to their submitted choice after external refresh and line change", async () => {
    let finish!: (value: unknown) => void;
    let request!: Record<string, string>;
    native.mockImplementation(async (command, args) => {
      const req = (args as { request: Record<string, string> }).request;
      if (command === "scan_activation_targets_v1") return scan(req.requestId);
      request = req;
      return await new Promise((resolve) => {
        finish = resolve;
      });
    });
    const account = session();
    const connections = local();
    const view = render(setupView(account, connections));
    await tick();
    applySavedOrSelected();
    await tick();
    applySavedOrSelected();
    expect(
      native.mock.calls.filter((c) => c[0] === "configure_desktop_tool_v2"),
    ).toHaveLength(1);
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "正在接入",
    );
    expect(screen.queryByRole("button", { name: "打开使用" })).toBeNull();
    expect(screen.getByRole("button", { name: "重新检查" })).toBeDisabled();
    const updated = {
      ...account,
      projection: { ...account.projection!, models: [model("other-model")] },
    };
    view.rerender(setupView(updated, connections, "global_accelerated"));
    await act(async () =>
      finish({
        requestId: request.requestId,
        toolId: request.toolId,
        modelId: request.modelId,
        billingGroup: request.billingGroup,
        schemaVersion: 3,
        status: "ready",
        reasonCode: "verified",
        observedAtEpochMs: 2000,
      }),
    );
    await tick();
    expect(screen.getByText("刚才的接入已完成")).toBeInTheDocument();
    expect(screen.getByText(/现在的选择尚未应用/)).toHaveTextContent("model-a");
    // 账号刷新后原选择已不可用：按钮保持可点，并直接说明下一步。
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "先选择模型",
    );
  });
  it("discloses stale prices and keeps refresh available instead of silently submitting", async () => {
    const account = session();
    const view = render(setupView(account));
    await tick();
    view.rerender(setupView({ ...account, lastError: "network_error" }));
    expect(screen.getByRole("alert")).toHaveTextContent("上次的模型与价格");
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "重新获取账户数据",
    );
    applySavedOrSelected();
    expect(account.refresh).toHaveBeenCalledOnce();
    expect(
      native.mock.calls.some((c) => c[0] === "configure_desktop_tool_v2"),
    ).toBe(false);
  });
  it("does not attribute a previous account's in-flight setup success to the new login", async () => {
    let finish!: (value: unknown) => void;
    let request!: Record<string, string>;
    native.mockImplementation(async (command, args) => {
      const req = (args as { request: Record<string, string> }).request;
      if (command === "scan_activation_targets_v1") return scan(req.requestId);
      request = req;
      return await new Promise((resolve) => {
        finish = resolve;
      });
    });
    const accountA = session();
    const connections = local();
    const view = render(setupView(accountA, connections));
    await tick();
    applySavedOrSelected();
    await tick();
    const accountB = {
      ...accountA,
      projection: {
        ...accountA.projection!,
        account: {
          ...accountA.projection!.account,
          username: "user999",
          displayName: "第二个账户",
        },
      },
    };
    view.rerender(setupView(accountB, connections));
    await act(async () =>
      finish({
        requestId: request.requestId,
        toolId: request.toolId,
        modelId: request.modelId,
        billingGroup: request.billingGroup,
        schemaVersion: 3,
        status: "ready",
        reasonCode: "verified",
        observedAtEpochMs: 2000,
      }),
    );
    await tick();
    expect(screen.getByText("刚才的接入已完成")).toBeInTheDocument();
    expect(screen.getByText(/现在的选择尚未应用/)).toHaveTextContent(
      "账户 user535",
    );
    expect(screen.queryByRole("button", { name: "打开使用" })).toBeNull();
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "保存并应用",
    );
  });
  it("opens locally without login or reconfiguration and rejects duplicate clicks", async () => {
    let finish!: (value: unknown) => void;
    let req!: Record<string, string>;
    native.mockImplementation(async (cmd, args) => {
      req = (args as { request: Record<string, string> }).request;
      if (cmd === "manage_tool_connections_v1")
        return {
          ...connectionsFixture(req.requestId),
          connections: local().connections,
        };
      return await new Promise((resolve) => {
        finish = resolve;
      });
    });
    function Connected() {
      const state = useToolConnections();
      return (
        <ConnectionProvider value={state}>
          <OpenConnection
            connection={
              local().connections.find((c) => c.toolId === "codex_desktop")!
            }
            name="Codex Desktop"
            onAdjust={callback}
          />
        </ConnectionProvider>
      );
    }
    render(<Connected />);
    await tick();
    native.mockClear();
    const button = screen.getByRole("button", { name: "打开使用" });
    fireEvent.click(button);
    fireEvent.click(button);
    await tick();
    expect(native).toHaveBeenCalledOnce();
    expect(native.mock.calls[0][0]).toBe("open_tool_connection_v1");
    await act(async () =>
      finish({
        requestId: req.requestId,
        toolId: req.toolId,
        schemaVersion: 1,
        status: "opened",
      }),
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "不会重新配置或发送测试消息",
    );
  });
  it("rejects extra secret-capable launch reply fields", async () => {
    native.mockImplementation(async (_cmd, args) => ({
      ...(args as { request: object }).request,
      schemaVersion: 1,
      status: "opened",
      apiKey: "must-not-render",
    }));
    await expect(openConnection("dsh_web")).rejects.toThrow(
      "invalid_open_reply",
    );
    await expect(openConnection("pi")).rejects.toThrow("invalid_open_reply");
    await expect(openConnection("terminal" as never)).rejects.toThrow(
      "invalid_open_target",
    );
  });
  it("does not show an old opening result on a changed app or connection", async () => {
    const state = local();
    let finish!: (status: "opened") => void;
    state.open.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const codex = state.connections.find((c) => c.toolId === "codex_desktop")!;
    const pi = {
      ...state.connections.find((c) => c.toolId === "pi")!,
      state: "connected" as const,
    };
    const view = render(
      <ConnectionProvider value={state}>
        <OpenConnection
          connection={codex}
          name="Codex Desktop"
          onAdjust={callback}
        />
      </ConnectionProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "打开使用" }));
    view.rerender(
      <ConnectionProvider value={state}>
        <OpenConnection connection={pi} name="Pi" onAdjust={callback} />
      </ConnectionProvider>,
    );
    await act(async () => finish("opened"));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "打开终端使用" }));
    await tick();
    expect(state.open).toHaveBeenLastCalledWith("pi");
    expect(screen.getByRole("status")).toHaveTextContent("已请求打开终端");
    view.rerender(
      <ConnectionProvider value={state}>
        <OpenConnection
          connection={{ ...pi, updatedAtEpochMs: pi.updatedAtEpochMs + 1 }}
          name="Pi"
          onAdjust={callback}
        />
      </ConnectionProvider>,
    );
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
  it.each([0, 2])(
    "prioritizes %s installed apps and makes the rest discoverable",
    async (count) => {
      native.mockImplementation(async (_cmd, args) => {
        const result = scan(
          (args as { request: { requestId: string } }).request.requestId,
        );
        result.targets.forEach((t, i) => {
          if (i >= count) {
            t.status = "not_found";
            t.installations = [];
          }
        });
        return result;
      });
      render(
        <ConnectionProvider
          value={{
            ...local(),
            connections: connectionsFixture("known-empty").connections,
          }}
        >
          <AppLibraryView
            accountSession={{ ...session(), projection: null }}
            onOpenAccount={callback}
            onOpenSetup={callback}
            onOpenCommunity={callback}
          />
        </ConnectionProvider>,
      );
      await tick();
      // count=0 时仅 Claude Desktop / Codex Desktop 可见。
      expect(screen.queryAllByRole("article")).toHaveLength(count || 2);
      if (count === 0)
        expect(
          screen.getAllByRole("button", { name: /安装并接入/ }),
        ).toHaveLength(2);
      fireEvent.click(
        screen.getByRole("button", {
          name: /查看其他/,
        }),
      );
      expect(screen.getAllByRole("article")).toHaveLength(6);
      expect(
        screen.queryByRole("region", { name: "用量账单" }),
      ).not.toBeInTheDocument();
    },
  );
  it("explains terminal usage without reporting a fake desktop launch", async () => {
    const connection = {
      ...local().connections.find((c) => c.toolId === "pi")!,
      state: "connected" as const,
    };
    render(
      <ConnectionProvider value={local()}>
        <OpenConnection connection={connection} name="Pi" onAdjust={callback} />
      </ConnectionProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "使用说明" }));
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "不会出现独立桌面窗口",
    );
    expect(screen.getByRole("dialog")).not.toHaveTextContent(
      "credential-helper",
    );
    expect(native).not.toHaveBeenCalled();
  });
  it.each(["claude_code", "pi"] as const)(
    "opens %s in a terminal without reconfiguration or a paid probe",
    async (toolId) => {
      const state = local();
      const connection = {
        ...state.connections.find((c) => c.toolId === toolId)!,
        state: "connected" as const,
      };
      render(
        <ConnectionProvider value={state}>
          <OpenConnection
            connection={connection}
            name={toolId}
            onAdjust={callback}
          />
        </ConnectionProvider>,
      );
      const button = screen.getByRole("button", { name: "打开终端使用" });
      fireEvent.click(button);
      fireEvent.click(button);
      await tick();
      expect(state.open).toHaveBeenCalledOnce();
      expect(state.open).toHaveBeenCalledWith(toolId);
      expect(state.restore).not.toHaveBeenCalled();
      expect(native).not.toHaveBeenCalled();
      expect(screen.getByRole("status")).toHaveTextContent("已请求打开终端");
      expect(screen.getByRole("status")).not.toHaveTextContent("模型验证成功");
    },
  );
  it.each(ACTIVATION_TOOL_IDS)(
    "launch IPC permits only the closed target %s",
    async (toolId) => {
      await expect(openConnection(toolId)).resolves.toBe("opened");
      expect(native).toHaveBeenCalledOnce();
      expect(native.mock.calls[0][0]).toBe("open_tool_connection_v1");
      const args = native.mock.calls[0][1] as { request: object };
      expect(Object.keys(args.request).sort()).toEqual(["requestId", "toolId"]);
    },
  );
  it.each([
    [
      "configuration_failed",
      "configuration_rollback_failed",
      "部分设置尚未恢复",
    ],
    ["configuration_failed", "credential_restore_failed", "密钥设置尚未恢复"],
    [
      "configuration_failed",
      "desktop_start_failed_restored",
      "已自动恢复上一次配置",
    ],
    [
      "configuration_failed",
      "desktop_change_failed_restored",
      "本次更新已取消或未能完成",
    ],
    [
      "configuration_failed",
      "previous_app_reopen_failed",
      "系统仍未能确认应用启动",
    ],
    [
      "configuration_failed",
      "desktop_recovery_waiting_for_exit",
      "尚未恢复设置",
    ],
    [
      "configuration_failed",
      "previous_connection_runtime_failed",
      "上一次本地连接尚未重新启动",
    ],
    ["configuration_failed", "recovery_receipt_failed", "不能确认操作已完成"],
    [
      "secure_storage_unavailable",
      "secure_storage_unavailable",
      "检查本机接入状态",
    ],
    ["configuration_failed", "unexpected_failure", "请检查接入状态"],
    ["launch_failed", "desktop_launch_target_changed", "安装位置刚刚发生变化"],
    ["verification_failed", "tool_start_failed", "应用没有启动成功"],
    ["verification_failed", "tool_request_timed_out", "连接测试超时"],
    [
      "verification_failed",
      "tool_output_read_failed",
      "未能读取应用的测试结果",
    ],
    ["external_override", "higher_precedence_override", "环境变量"],
  ])(
    "reports %s/%s without claiming an unproved restore",
    async (status, reasonCode, expected) => {
      native.mockImplementation(async (command, args) => {
        const req = (args as { request: Record<string, string> }).request;
        if (command === "scan_activation_targets_v1")
          return scan(req.requestId);
        if (command === "configure_desktop_tool_v2")
          return {
            requestId: req.requestId,
            toolId: req.toolId,
            modelId: req.modelId,
            billingGroup: req.billingGroup,
            schemaVersion: 3,
            status,
            reasonCode,
            observedAtEpochMs: 2000,
          };
        throw new Error("unexpected request");
      });
      render(setupView());
      await tick();
      applySavedOrSelected();
      await tick();
      const alerts = [
        ...screen.queryAllByRole("alert"),
        ...screen.queryAllByRole("status"),
      ]
        .map((notice) => notice.textContent)
        .join(" ");
      expect(alerts).toContain(expected);
      if (
        ["configuration_failed", "secure_storage_unavailable"].includes(status)
      ) {
        expect(alerts).not.toMatch(
          /原有设置已保留|没有改动应用设置|所有本机改动已恢复/,
        );
      }
      expect(screen.queryByText("接入完成")).not.toBeInTheDocument();
    },
  );
  it("names only connections affected by quitting", () => {
    const state = local({ requiresBackground: true });
    render(
      <ConnectionProvider value={state}>
        <ShutdownProvider>
          <QuitAssistant />
        </ShutdownProvider>
      </ConnectionProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "退出野菜助手" }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("list")).toHaveTextContent("Codex Desktop");
    expect(within(dialog).getByRole("list")).not.toHaveTextContent("Claude");
    expect(
      native.mock.calls.some(([cmd]) => cmd === "quit_desktop_assistant"),
    ).toBe(false);
  });
});
