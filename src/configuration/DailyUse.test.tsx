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
import zh from "../i18n/locales/zh.json";
import { ConfigurationPreviewView } from "./ConfigurationPreviewView";
import { AppLibraryView } from "../workbench/AppLibraryView";
import { OpenConnection } from "./OpenConnection";
import { QuitAssistant } from "../settings/QuitAssistant";
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

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
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
const callback = vi.fn();
function setupView(
  account = session(),
  connections = local(),
  line: "mainland_optimized" | "global_accelerated" = "mainland_optimized",
) {
  return (
    <StrictMode>
      <ConnectionProvider value={connections}>
        <ConfigurationPreviewView
          initialDesktopAppId="codex_desktop"
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
    expect(screen.getByTestId("configuration-apply-action")).toBeDisabled();
    fireEvent.click(screen.getByRole("radio", { name: /标准分组/ }));
    expect(
      screen.getByRole("region", { name: "所选分组价格" }),
    ).toHaveTextContent("1 亿 Token");
    expect(screen.getByTestId("configuration-apply-action")).toBeEnabled();
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
    expect(screen.getByTestId("configuration-apply-action")).toBeDisabled();
  });
  it("does not overwrite a user selection when saved state arrives late", async () => {
    const account = session();
    const pending = { ...local(), loading: true };
    const view = render(setupView(account, pending));
    await tick();
    fireEvent.click(screen.getByRole("combobox", { name: /选择模型/ }));
    fireEvent.click(screen.getByRole("option", { name: "model-b" }));
    fireEvent.click(screen.getByRole("radio", { name: /标准分组/ }));
    view.rerender(setupView(account, local()));
    await tick();
    expect(
      screen.getByRole("combobox", { name: /选择模型/ }),
    ).toHaveTextContent("model-b");
    expect(screen.getByRole("radio", { name: /标准分组/ })).toBeChecked();
  });
  it("restores saved choices after the first local-state read failed", async () => {
    const account = session();
    const view = render(setupView(account, { ...local(), error: true }));
    await tick();
    view.rerender(setupView(account, local()));
    await tick();
    expect(screen.getByRole("radio", { name: /优惠组/ })).toBeChecked();
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
    fireEvent.click(screen.getByTestId("configuration-apply-action"));
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
    fireEvent.click(screen.getByTestId("configuration-apply-action"));
    await tick();
    fireEvent.click(screen.getByTestId("configuration-apply-action"));
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
    expect(screen.getByText("刚才的接入已完成")).toBeInTheDocument();
    expect(screen.getByText(/现在的选择尚未应用/)).toHaveTextContent("model-a");
    expect(screen.getByTestId("configuration-apply-action")).toBeDisabled();
  });
  it("discloses stale prices and keeps refresh available instead of silently submitting", async () => {
    const account = session();
    const view = render(setupView(account));
    await tick();
    view.rerender(setupView({ ...account, lastError: "network_error" }));
    expect(screen.getByRole("alert")).toHaveTextContent("上次的模型与价格");
    expect(screen.getByTestId("configuration-apply-action")).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "刷新账户数据" }));
    expect(account.refresh).toHaveBeenCalledOnce();
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
    fireEvent.click(screen.getByTestId("configuration-apply-action"));
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
    expect(screen.getByText("刚才的接入已完成")).toBeInTheDocument();
    expect(screen.getByText(/现在的选择尚未应用/)).toHaveTextContent(
      "账户 user535",
    );
    expect(screen.queryByRole("button", { name: "打开使用" })).toBeNull();
    expect(screen.getByTestId("configuration-apply-action")).toHaveTextContent(
      "一键接入",
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
    await expect(openConnection("pi")).rejects.toThrow("invalid_open_target");
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
        <AppLibraryView
          accountSession={{ ...session(), projection: null }}
          onOpenAccount={callback}
          onOpenSetup={callback}
          onOpenDiagnostics={callback}
        />,
      );
      await tick();
      expect(screen.queryAllByRole("article")).toHaveLength(count);
      fireEvent.click(
        screen.getByRole("button", {
          name: count ? /查看其他/ : "查看支持的应用",
        }),
      );
      expect(screen.getAllByRole("article")).toHaveLength(7);
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
    fireEvent.click(screen.getByRole("button", { name: "使用方式" }));
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "不会出现独立桌面窗口",
    );
    expect(screen.getByRole("dialog")).not.toHaveTextContent(
      "credential-helper",
    );
    expect(native).not.toHaveBeenCalled();
  });
  it("names only connections affected by quitting", () => {
    const state = local({ requiresBackground: true });
    render(
      <ConnectionProvider value={state}>
        <QuitAssistant />
      </ConnectionProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "退出野菜助手" }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("list")).toHaveTextContent("Codex Desktop");
    expect(within(dialog).getByRole("list")).not.toHaveTextContent("Claude");
    expect(native).not.toHaveBeenCalled();
  });
});
