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
  toolId: (typeof ACTIVATION_TOOL_IDS)[number] = "codex_desktop",
) {
  return (
    <StrictMode>
      <ConnectionProvider value={connections}>
        <ConfigurationPreviewView
          initialDesktopAppId={toolId}
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
    fireEvent.click(screen.getByRole("radio", { name: /标准分组/ }));
    expect(screen.getByTestId("configuration-apply-action")).toBeDisabled();
    expect(
      native.mock.calls.some(([c]) => c === "configure_desktop_tool_v2"),
    ).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "加入常用模型" }));
    fireEvent.click(screen.getByRole("radio", { name: "默认模型 model-b" }));
    fireEvent.click(screen.getByTestId("configuration-apply-action"));
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
    fireEvent.click(screen.getByTestId("configuration-apply-action"));
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
    expect(dialog).toHaveTextContent("不会强制结束进程");
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

  it.each(["claude_code", "pi", "hermes", "openclaw"] as const)(
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
      fireEvent.click(screen.getByTestId("configuration-apply-action"));
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
    expect(screen.getByTestId("configuration-apply-action")).toBeDisabled();
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
    expect(result).toHaveTextContent("回复在完成前中断");
    expect(result).toHaveTextContent("gpt-6-astra");
    expect(result).toHaveTextContent("不依据 AI 的自我介绍判断");
    expect(result).not.toHaveTextContent("连接成功");
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
        <AppLibraryView
          accountSession={{ ...session(), projection: null }}
          onOpenAccount={callback}
          onOpenSetup={callback}
          onOpenDiagnostics={callback}
        />,
      );
      await tick();
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
    fireEvent.click(screen.getByRole("button", { name: "使用说明" }));
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "不会出现独立桌面窗口",
    );
    expect(screen.getByRole("dialog")).not.toHaveTextContent(
      "credential-helper",
    );
    expect(native).not.toHaveBeenCalled();
  });
  it.each(["claude_code", "pi", "hermes", "openclaw"] as const)(
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
      fireEvent.click(screen.getByTestId("configuration-apply-action"));
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
