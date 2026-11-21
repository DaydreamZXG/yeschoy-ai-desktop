import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountModel } from "../account/session";
import {
  activateDesktopTool,
  cancelDesktopToolActivation,
  scanActivationTargets,
  type ActivationTarget,
  type ToolActivationProjection,
} from "../configuration/activation";
import type { ToolConnection } from "../configuration/connections";
import { Join } from "./Join";
import { Reveal } from "./Reveal";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));
vi.mock("../configuration/activation", async (original) => ({
  ...(await original<typeof import("../configuration/activation")>()),
  scanActivationTargets: vi.fn(),
  activateDesktopTool: vi.fn(),
  cancelDesktopToolActivation: vi.fn(),
}));
// 关掉打字机和声音：这里只看流程。
vi.mock("./sound", async (original) => ({
  ...(await original<typeof import("./sound")>()),
  prefersCalm: () => true,
}));

const scan = vi.mocked(scanActivationTargets);
const activate = vi.mocked(activateDesktopTool);
const cancel = vi.mocked(cancelDesktopToolActivation);

const model = (id: string): AccountModel => ({
  id,
  description: "",
  billingMode: "ratio",
  pricingAvailable: false,
  officialInputCnyPerMillion: "",
  officialOutputCnyPerMillion: "",
  actualInputCnyPerMillion: "",
  actualOutputCnyPerMillion: "",
  billing: null,
});
const MODELS = [
  model("claude-sonnet-4-5-20250929"),
  model("deepseek-v4"),
  model("deepseek-v4-flash"),
  model("gpt-image-2"),
];

const target = (
  toolId: ActivationTarget["toolId"],
  patch: Partial<ActivationTarget> = {},
): ActivationTarget =>
  ({
    toolId,
    displayName: toolId,
    surface: "",
    manualLocation: "none",
    status: "available",
    installations: [
      {
        installationId: `${toolId}-1`,
        label: "安装 1",
        version: "1.0",
        supported: true,
        recommended: true,
      },
    ],
    ...patch,
  }) as ActivationTarget;

const found = (targets: ActivationTarget[]) =>
  scan.mockResolvedValue({
    requestId: "scan",
    schemaVersion: 1,
    platform: "macos",
    targets,
  } as Awaited<ReturnType<typeof scanActivationTargets>>);

const answer = (status: string, reasonCode: string) =>
  activate.mockImplementationOnce(
    async (input) =>
      ({
        requestId: "r",
        schemaVersion: 5,
        toolId: input.toolId,
        modelId: input.modelId,
        billingGroup: input.billingGroup,
        status,
        reasonCode,
        models: input.models,
        skipped: [],
      }) as unknown as ToolActivationProjection,
  );

function setup(patch: Partial<Parameters<typeof Join>[0]> = {}) {
  const props = {
    open: true,
    lineId: "mainland_optimized" as const,
    models: MODELS,
    current: "cla" as const,
    connections: [] as ToolConnection[],
    onClose: vi.fn(),
    onJoined: vi.fn(),
    onReconnect: vi.fn(),
    onClassic: vi.fn(),
    onSettled: vi.fn(),
    ...patch,
  };
  render(<Join {...props} />);
  return props;
}
const go = () => screen.getByRole("button", { name: /去接入/ });
const says = () => screen.getByTestId("kaifan-join-says").textContent;
const ready = () => waitFor(() => expect(go()).toBeEnabled());

beforeEach(() => {
  vi.clearAllMocks();
  found([target("claude_desktop"), target("codex_desktop")]);
});

describe("接入：选应用、选人、点一下", () => {
  it("默认让首页那位去；站上没有的人点不了；画图的型号不算", async () => {
    setup();
    await ready();
    expect(go()).toHaveTextContent("让橘子去接入");
    expect(screen.getByRole("button", { name: /小星/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /阿青/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /小鲸/ })).toBeEnabled();
  });

  it("点一下就把选好的应用、型号和位置交给后端，成了就登场", async () => {
    const props = setup();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: /Codex/ }));
    fireEvent.click(screen.getByRole("button", { name: /小鲸/ }));
    answer("ready", "configuration_ready");
    fireEvent.click(go());
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled());
    expect(activate.mock.calls[0][0]).toMatchObject({
      lineId: "mainland_optimized",
      toolId: "codex_desktop",
      modelId: "deepseek-v4-flash",
      installationId: "codex_desktop-1",
      billingGroup: "default",
      models: [{ modelId: "deepseek-v4-flash", billingGroup: "default" }],
    });
    expect(props.onJoined).toHaveBeenCalledWith({
      toolId: "codex_desktop",
      who: "ds",
      modelId: "deepseek-v4-flash",
      opened: true,
    });
    expect(props.onSettled).toHaveBeenCalled();
  });

  it("一个人有好几个型号时可以换", async () => {
    setup({ current: "ds" });
    await ready();
    fireEvent.change(screen.getByLabelText("型号"), {
      target: { value: "deepseek-v4" },
    });
    answer("ready", "configuration_ready");
    fireEvent.click(go());
    await waitFor(() => expect(activate).toHaveBeenCalled());
    expect(activate.mock.calls[0][0].modelId).toBe("deepseek-v4");
  });

  it("接过的应用：沿用原来的型号，名单里别的模型照旧留着", async () => {
    setup({
      current: "ds",
      initialApp: "codex_desktop",
      connections: [
        {
          toolId: "codex_desktop",
          state: "connected",
          modelId: "deepseek-v4",
          lineId: "mainland_optimized",
          billingGroup: "default",
          updatedAtEpochMs: 1,
          restoreMode: "original",
          requiresBackground: false,
          reasonCode: "",
          models: [
            { modelId: "deepseek-v4", billingGroup: "default" },
            { modelId: "claude-sonnet-4-5-20250929", billingGroup: "default" },
            { modelId: "gone-model", billingGroup: "default" },
          ],
        },
      ],
    });
    await ready();
    answer("ready", "configuration_ready");
    fireEvent.click(go());
    await waitFor(() => expect(activate).toHaveBeenCalled());
    expect(activate.mock.calls[0][0]).toMatchObject({
      toolId: "codex_desktop",
      modelId: "deepseek-v4",
      models: [
        { modelId: "deepseek-v4", billingGroup: "default" },
        { modelId: "claude-sonnet-4-5-20250929", billingGroup: "default" },
      ],
    });
  });

  it("存过的分组原样沿用：把名单里的另一个型号换成默认时，不替用户换分组", async () => {
    setup({
      current: "cla",
      initialApp: "codex_desktop",
      connections: [
        {
          toolId: "codex_desktop",
          state: "connected",
          modelId: "deepseek-v4",
          lineId: "mainland_optimized",
          billingGroup: "vip",
          updatedAtEpochMs: 1,
          restoreMode: "original",
          requiresBackground: false,
          reasonCode: "",
          models: [
            { modelId: "deepseek-v4", billingGroup: "vip" },
            { modelId: "claude-sonnet-4-5-20250929", billingGroup: "cheap" },
          ],
        },
      ],
    });
    await ready();
    answer("ready", "configuration_ready");
    fireEvent.click(go());
    await waitFor(() => expect(activate).toHaveBeenCalled());
    expect(activate.mock.calls[0][0]).toMatchObject({
      modelId: "claude-sonnet-4-5-20250929",
      billingGroup: "cheap",
      models: [
        { modelId: "claude-sonnet-4-5-20250929", billingGroup: "cheap" },
        { modelId: "deepseek-v4", billingGroup: "vip" },
      ],
    });
  });

  it("应用开着：先说清楚要重启，用户点了才重启", async () => {
    const props = setup();
    await ready();
    answer("application_running", "desktop_running");
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("正开着"));
    expect(activate.mock.calls[0][0].restartRunningApp).toBeUndefined();
    answer("ready", "configuration_ready");
    fireEvent.click(screen.getByRole("button", { name: /关掉并重新打开/ }));
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled());
    expect(activate.mock.calls[1][0].restartRunningApp).toBe(true);
  });

  it("Claude 登录着官方账号：问一句，答应了才收起来", async () => {
    setup();
    await ready();
    answer("application_running", "claude_login_conflict");
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("官方账号"));
    answer("ready", "configuration_ready");
    fireEvent.click(screen.getByRole("button", { name: /收起官方登录/ }));
    await waitFor(() => expect(activate).toHaveBeenCalledTimes(2));
    expect(activate.mock.calls[1][0].displaceClaudeLogin).toBe(true);
  });

  it("设置写好但没能打开：照样登场，但不说已经打开", async () => {
    const props = setup();
    await ready();
    answer("launch_failed", "desktop_launch_start_failed");
    fireEvent.click(go());
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled());
    expect(props.onJoined).toHaveBeenCalledWith(
      expect.objectContaining({ opened: false }),
    );
  });

  it("没接成但已改回原样：说清楚，给再试一次", async () => {
    const props = setup();
    await ready();
    answer("configuration_failed", "desktop_start_failed_restored");
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("已经改回原来的样子"));
    expect(props.onJoined).not.toHaveBeenCalled();
    answer("ready", "configuration_ready");
    fireEvent.click(screen.getByRole("button", { name: "再试一次" }));
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled());
  });

  it("登录不算数了：带去重新连站点", async () => {
    const props = setup();
    await ready();
    answer("signed_out", "signed_out");
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("重新连一下站点"));
    fireEvent.click(screen.getByRole("button", { name: /重新连站点/ }));
    expect(props.onReconnect).toHaveBeenCalled();
  });

  it("没接成时可以回去重选", async () => {
    setup();
    await ready();
    answer("unsupported_model", "unsupported_model");
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("换个人"));
    fireEvent.click(screen.getByRole("button", { name: "回去换人" }));
    expect(screen.getByTestId("kaifan-join").dataset.stage).toBe("pick");
  });

  it("正在接的时候可以停下", async () => {
    setup();
    await ready();
    let finish: (value: ToolActivationProjection) => void = () => undefined;
    activate.mockImplementationOnce((input) => {
      input.onRequestId?.("req-1");
      return new Promise((resolve) => (finish = resolve));
    });
    cancel.mockResolvedValue("cancel_requested");
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("正在帮你接入"));
    fireEvent.click(screen.getByRole("button", { name: "先停下" }));
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("req-1"));
    finish({
      requestId: "req-1",
      schemaVersion: 5,
      toolId: "claude_desktop",
      modelId: "claude-sonnet-4-5-20250929",
      billingGroup: "default",
      status: "configuration_failed",
      reasonCode: "activation_cancelled",
    } as ToolActivationProjection);
    await waitFor(() => expect(says()).toContain("没有被改动"));
  });

  it("没装的应用：标出来，按钮改成去安装，不去点后端", async () => {
    found([target("codex_desktop")]);
    const props = setup({ initialApp: "claude_desktop" });
    const install = await screen.findByRole("button", { name: /先去安装/ });
    expect(
      screen.getByRole("button", { name: /^Claude Desktop/ }),
    ).toHaveTextContent("还没装");
    fireEvent.click(install);
    expect(props.onClassic).toHaveBeenCalledWith("claude_desktop");
    expect(activate).not.toHaveBeenCalled();
  });

  it("没指定应用时，先选中电脑上已经装了的那个", async () => {
    found([target("codex_desktop")]);
    setup();
    await ready();
    expect(screen.getByRole("button", { name: /Codex/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("电脑上有好几处：默认用推荐的，可以改", async () => {
    found([
      target("claude_desktop", {
        status: "selection_required",
        installations: [
          {
            installationId: "a",
            label: "系统应用",
            version: "1",
            supported: true,
            recommended: false,
          },
          {
            installationId: "b",
            label: "用户目录",
            version: "2",
            supported: true,
            recommended: true,
          },
        ],
      } as Partial<ActivationTarget>),
    ]);
    setup();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: /系统应用/ }));
    answer("ready", "configuration_ready");
    fireEvent.click(go());
    await waitFor(() => expect(activate).toHaveBeenCalled());
    expect(activate.mock.calls[0][0].installationId).toBe("a");
  });

  it("没看清电脑上装了什么：不让点接入，给再看一次", async () => {
    scan.mockRejectedValueOnce(new Error("timeout"));
    setup();
    const again = await screen.findByRole("button", { name: "再看一次" });
    expect(go()).toBeDisabled();
    fireEvent.click(again);
    await ready();
  });
});

describe("角色登场", () => {
  const joined = {
    toolId: "claude_desktop" as const,
    who: "cla" as const,
    modelId: "claude-sonnet-4-5-20250929",
    opened: true,
  };
  const show = (patch: Partial<Parameters<typeof Reveal>[0]> = {}) => {
    const props = {
      joined,
      opening: false,
      openFailed: false,
      onOpen: vi.fn(),
      onHome: vi.fn(),
      ...patch,
    };
    render(<Reveal {...props} />);
    return props;
  };

  it("替用户打开了：说打开了，只留回首页", () => {
    const props = show();
    expect(screen.getByTestId("kaifan-reveal")).toHaveTextContent(
      "Claude Desktop 已经替你打开",
    );
    expect(screen.queryByRole("button", { name: /打开 Claude/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "回到首页" }));
    expect(props.onHome).toHaveBeenCalled();
  });

  it("只写好了设置：不说已经打开，给一个打开的按钮", () => {
    const props = show({ joined: { ...joined, opened: false } });
    const text = screen.getByTestId("kaifan-reveal").textContent ?? "";
    expect(text).toContain("设置写好了");
    expect(text).not.toContain("已经替你打开");
    fireEvent.click(
      screen.getByRole("button", { name: /打开 Claude Desktop/ }),
    );
    expect(props.onOpen).toHaveBeenCalled();
  });

  it("没打开成：照实说，设置还在", () => {
    show({ joined: { ...joined, opened: false }, openFailed: true });
    expect(screen.getByTestId("kaifan-reveal")).toHaveTextContent(
      "没能替你打开 Claude Desktop",
    );
  });

  it("终端里用的应用：说的是终端窗口", () => {
    show({ joined: { ...joined, toolId: "claude_code" } });
    expect(screen.getByTestId("kaifan-reveal")).toHaveTextContent("终端窗口");
  });

  it("没有刚接好的就不出现", () => {
    show({ joined: null });
    expect(screen.queryByTestId("kaifan-reveal")).toBeNull();
  });
});
