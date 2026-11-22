import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderPreset } from "./providers";
import {
  clearManualLocation,
  pickManualLocation,
} from "../installation/manualLocationApi";
import type { AccountModel } from "../account/session";
import {
  activateDesktopTool,
  cancelDesktopToolActivation,
  scanActivationTargets,
  type ActivationTarget,
  type ActivationTargetScan,
  type ToolActivationProjection,
} from "../configuration/activation";
import type { ToolConnection } from "../configuration/connections";
import {
  installationAction,
  type InstallationProgress,
} from "../installation/api";
import type { CardPicks } from "./cardInfo";
import { Join } from "./Join";
import { installOutcome } from "./joinOutcome";
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
vi.mock("../installation/api", async (original) => ({
  ...(await original<typeof import("../installation/api")>()),
  installationAction: vi.fn(),
}));
// 关掉打字机和声音：这里只看流程。
vi.mock("../installation/manualLocationApi", () => ({
  pickManualLocation: vi.fn(),
  clearManualLocation: vi.fn(),
}));
vi.mock("./sound", async (original) => ({
  ...(await original<typeof import("./sound")>()),
  prefersCalm: () => true,
}));

const scan = vi.mocked(scanActivationTargets);
const activate = vi.mocked(activateDesktopTool);
const cancel = vi.mocked(cancelDesktopToolActivation);
const installer = vi.mocked(installationAction);
const pointOut = vi.mocked(pickManualLocation);

/** 后端报回来的安装进度。 */
const installing = (
  patch: Partial<InstallationProgress> = {},
): InstallationProgress => ({
  schemaVersion: 2,
  requestId: "i",
  toolId: "claude_code",
  jobId: "job-1",
  mode: "automatic",
  phase: "downloading",
  downloadedBytes: 0,
  totalBytes: 0,
  canCancel: true,
  reasonCode: "none",
  source: "official",
  platform: "macos",
  architecture: "arm64",
  disposition: "none",
  installationId: "",
  ...patch,
});
/** 电脑上没有 Claude Code；`mode` 是后端说它能不能由客户端自己装。 */
const absentClaudeCode = (
  onStart: () => InstallationProgress,
  mode: InstallationProgress["mode"] = "automatic",
) =>
  installer.mockImplementation(async (toolId, action) => {
    if (action === "start") return onStart();
    return installing({ toolId, jobId: "", phase: "idle", mode });
  });

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
    picks: { model: {}, plan: {} } as CardPicks,
    onClose: vi.fn(),
    onJoined: vi.fn(),
    onReconnect: vi.fn(),
    onUndo: vi.fn(),
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
  // 默认问不到安装能力：没装的应用只能带用户去装。
  installer.mockRejectedValue(new Error("unavailable"));
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
      terminal: false,
      skipped: [],
    });
    expect(props.onSettled).toHaveBeenCalled();
  });

  it("挑中的那一份是命令行版（Codex 只装了命令行）：登场时知道开的是终端", async () => {
    found([
      target("codex_desktop", {
        installations: [
          {
            installationId: "cli",
            label: "命令行",
            version: "",
            supported: true,
            recommended: true,
            terminal: true,
          },
        ],
      }),
    ]);
    const props = setup({ initialApp: "codex_desktop" });
    await ready();
    answer("ready", "configuration_ready");
    fireEvent.click(go());
    await waitFor(() =>
      expect(props.onJoined).toHaveBeenCalledWith(
        expect.objectContaining({ toolId: "codex_desktop", terminal: true }),
      ),
    );
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

  const priced = (id: string, groups: string[]): AccountModel => ({
    ...model(id),
    billing: {
      groups: groups.map((group) => ({
        id: group,
        description: "",
        ratio: null,
      })),
      baseInputUsd: null,
      baseOutputUsd: null,
    } as AccountModel["billing"],
  });
  const savedOn = (billingGroup: string): ToolConnection[] => [
    {
      toolId: "codex_desktop",
      state: "connected",
      modelId: "claude-sonnet-4-5-20250929",
      lineId: "mainland_optimized",
      billingGroup,
      updatedAtEpochMs: 1,
      restoreMode: "original",
      requiresBackground: false,
      reasonCode: "",
    },
  ];

  it("存过的方案站点不提供了：不发出去也不自己换，先问；点了头才用新方案", async () => {
    setup({
      initialApp: "codex_desktop",
      models: [priced("claude-sonnet-4-5-20250929", ["default", "promo"])],
      connections: savedOn("vip"),
    });
    await ready();
    fireEvent.click(go());
    await waitFor(() =>
      expect(says()).toMatch(/「vip」.*不提供了.*「标准分组」/),
    );
    expect(activate).not.toHaveBeenCalled();
    answer("ready", "configuration_ready");
    fireEvent.click(
      screen.getByRole("button", { name: "换成「标准分组」接入" }),
    );
    await waitFor(() => expect(activate).toHaveBeenCalled());
    expect(activate.mock.calls[0][0]).toMatchObject({
      billingGroup: "default",
      models: [
        { modelId: "claude-sonnet-4-5-20250929", billingGroup: "default" },
      ],
    });
  });

  it("站点没给存过的方案开通：能再试，也能点头换成另一个方案，不会只剩再试", async () => {
    setup({
      initialApp: "codex_desktop",
      models: [priced("claude-sonnet-4-5-20250929", ["vip", "promo"])],
      connections: savedOn("vip"),
    });
    await ready();
    activate.mockImplementationOnce(
      async (input) =>
        ({
          requestId: "r",
          schemaVersion: 5,
          toolId: input.toolId,
          modelId: input.modelId,
          billingGroup: input.billingGroup,
          status: "server_unavailable",
          reasonCode: "server_unavailable",
          models: input.models,
          skipped: [
            {
              modelId: input.modelId,
              billingGroup: input.billingGroup,
              reasonCode: "server_unavailable",
            },
          ],
        }) as unknown as ToolActivationProjection,
    );
    fireEvent.click(go());
    await waitFor(() => expect(says()).toMatch(/没给.*「vip」开通/));
    expect(activate.mock.calls[0][0]).toMatchObject({ billingGroup: "vip" });
    expect(screen.getByRole("button", { name: "再试一次" })).toBeEnabled();
    answer("ready", "configuration_ready");
    fireEvent.click(screen.getByRole("button", { name: "换成「promo」接入" }));
    await waitFor(() => expect(activate).toHaveBeenCalledTimes(2));
    expect(activate.mock.calls[1][0]).toMatchObject({ billingGroup: "promo" });
  });

  it("名片上挑的型号和方案排在存过的前面：那是用户自己定的", async () => {
    setup({
      current: "cla",
      initialApp: "codex_desktop",
      models: [
        priced("claude-sonnet-4-5-20250929", ["default", "vip", "promo"]),
        priced("claude-opus-4-1-20250805", ["default", "promo"]),
      ],
      connections: savedOn("vip"),
      picks: {
        model: { cla: "claude-opus-4-1-20250805" },
        plan: { "claude-opus-4-1-20250805": "promo" },
      },
    });
    await ready();
    expect(screen.getByLabelText("型号")).toHaveValue(
      "claude-opus-4-1-20250805",
    );
    expect(screen.getByTestId("kaifan-join-plan")).toHaveTextContent("promo");
    answer("ready", "configuration_ready");
    fireEvent.click(go());
    await waitFor(() => expect(activate).toHaveBeenCalled());
    expect(activate.mock.calls[0][0]).toMatchObject({
      modelId: "claude-opus-4-1-20250805",
      billingGroup: "promo",
    });
  });

  it("名片上挑的方案站点不提供了：不算数，存过的原样沿用", async () => {
    setup({
      current: "cla",
      initialApp: "codex_desktop",
      models: [priced("claude-sonnet-4-5-20250929", ["default", "vip"])],
      connections: savedOn("vip"),
      picks: {
        model: { cla: "gone-model" },
        plan: { "claude-sonnet-4-5-20250929": "停售" },
      },
    });
    await ready();
    answer("ready", "configuration_ready");
    fireEvent.click(go());
    await waitFor(() => expect(activate).toHaveBeenCalled());
    expect(activate.mock.calls[0][0]).toMatchObject({
      modelId: "claude-sonnet-4-5-20250929",
      billingGroup: "vip",
    });
  });

  it("在一个应用上点头换的方案，不会带到另一个应用上", async () => {
    setup({
      initialApp: "codex_desktop",
      models: [priced("claude-sonnet-4-5-20250929", ["vip", "promo"])],
      connections: [
        { ...savedOn("gone")[0] },
        { ...savedOn("promo")[0], toolId: "claude_desktop" },
      ],
    });
    await ready();
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("不提供了"));
    // 在 Codex 上点头换成 vip，结果它正开着，用户退回去改选 Claude。
    answer("application_running", "desktop_running");
    fireEvent.click(screen.getByRole("button", { name: "换成「vip」接入" }));
    await waitFor(() => expect(says()).toContain("正开着"));
    fireEvent.click(screen.getByRole("button", { name: "回去重选" }));
    fireEvent.click(screen.getByRole("button", { name: /Claude Desktop/ }));
    answer("ready", "configuration_ready");
    fireEvent.click(go());
    await waitFor(() => expect(activate).toHaveBeenCalledTimes(2));
    expect(activate.mock.calls[1][0]).toMatchObject({
      toolId: "claude_desktop",
      billingGroup: "promo",
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

  it("默认的接上了、名单里另一个被站点拒了：把被拒的带给登场那一屏", async () => {
    const props = setup();
    await ready();
    activate.mockImplementationOnce(
      async (input) =>
        ({
          requestId: "r",
          schemaVersion: 5,
          toolId: input.toolId,
          modelId: input.modelId,
          billingGroup: input.billingGroup,
          status: "ready",
          reasonCode: "configuration_ready",
          models: input.models,
          skipped: [
            {
              modelId: "deepseek-v4",
              billingGroup: "vip",
              reasonCode: "server_unavailable",
            },
          ],
        }) as unknown as ToolActivationProjection,
    );
    fireEvent.click(go());
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled());
    expect(props.onJoined).toHaveBeenCalledWith(
      expect.objectContaining({ opened: true, skipped: ["deepseek-v4"] }),
    );
  });

  it("设置写好但没能打开：照样登场，但不说已经打开", async () => {
    const props = setup();
    await ready();
    answer("launch_failed", "desktop_launch_start_failed");
    fireEvent.click(go());
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled());
    expect(props.onJoined).toHaveBeenCalledWith(
      expect.objectContaining({
        opened: false,
        why: "desktop_launch_start_failed",
      }),
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

  it("后端说得出为什么没接成：说原因和出路，不只说再试一次", async () => {
    setup();
    await ready();
    answer("configuration_failed", "configuration_readback_failed");
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("杀毒软件或云同步盘"));
    expect(says()).not.toContain("这次没接成");
    expect(screen.getByRole("button", { name: "再试一次" })).toBeEnabled();
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

  it("没装的应用：标出来，按钮打开它的官网，不去接入", async () => {
    found([target("codex_desktop")]);
    setup({ initialApp: "claude_desktop" });
    const install = await screen.findByRole("button", { name: /去官网下载/ });
    expect(
      screen.getByRole("button", { name: /^Claude Desktop/ }),
    ).toHaveTextContent("还没装");
    fireEvent.click(install);
    expect(installer).toHaveBeenCalledWith("claude_desktop", "help");
    expect(activate).not.toHaveBeenCalled();
  });

  it("装了但缺东西：说「没装完整」，按钮是重新装，不说成没装", async () => {
    found([
      target("codex_desktop"),
      target("claude_desktop", {
        status: "missing_runtime",
        installations: [
          {
            installationId: "a",
            label: "系统应用",
            version: "1",
            supported: false,
            recommended: false,
          },
        ],
      }),
    ]);
    setup({ initialApp: "claude_desktop" });
    await screen.findByRole("button", { name: /去官网重新装/ });
    const chip = screen.getByRole("button", { name: /^Claude Desktop/ });
    expect(chip).toHaveTextContent("没装完整");
    expect(chip).not.toHaveTextContent("还没装");
    expect(screen.queryByRole("button", { name: /去官网下载/ })).toBeNull();
  });

  it("装了好几份的也算装了：默认选中它，不跳过去选后面的", async () => {
    const two = ["a", "b"].map((installationId) => ({
      installationId,
      label: installationId,
      version: "1",
      supported: true,
      recommended: installationId === "a",
    }));
    found([
      target("claude_desktop", {
        status: "selection_required",
        installations: two,
      }),
      target("codex_desktop"),
    ]);
    setup();
    await ready();
    expect(
      screen.getByRole("button", { name: /^Claude Desktop/ }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("这个应用接不了（和型号无关）：带回去换应用，不让人换型号", async () => {
    setup();
    await ready();
    answer("unsupported_profile", "profile_not_supported");
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("换一个应用"));
    expect(says()).not.toContain("换个型号");
    fireEvent.click(screen.getByRole("button", { name: "回去换应用" }));
    expect(screen.getByTestId("kaifan-join").dataset.stage).toBe("pick");
  });

  it("去官网装完回到窗口：自己再找一遍，找到了就能直接接", async () => {
    found([target("codex_desktop")]);
    setup({ initialApp: "claude_desktop" });
    await screen.findByRole("button", { name: /去官网下载/ });
    found([target("codex_desktop"), target("claude_desktop")]);
    fireEvent(window, new Event("focus"));
    await waitFor(() => expect(go()).toBeEnabled());
    expect(screen.queryByTestId("kaifan-join-absent")).toBeNull();
  });

  it("装在别处、自动找不到：让用户指一下，记住了就重新找", async () => {
    found([target("codex_desktop")]);
    pointOut.mockResolvedValueOnce("wrong_file").mockResolvedValueOnce("saved");
    setup({ initialApp: "claude_desktop" });
    const tell = await screen.findByRole("button", { name: "告诉我它在哪" });
    fireEvent.click(tell);
    expect(await screen.findByRole("alert")).toHaveTextContent("选中的不是");
    found([target("codex_desktop"), target("claude_desktop")]);
    fireEvent.click(tell);
    await waitFor(() => expect(go()).toBeEnabled());
    expect(pointOut).toHaveBeenCalledWith("claude_desktop");
  });

  it("用的是用户指的那份：能换一个位置，也能改回自己找", async () => {
    found([
      target("codex_desktop"),
      target("claude_desktop", { manualLocation: "in_use" }),
    ]);
    vi.mocked(clearManualLocation).mockResolvedValueOnce("cleared");
    setup({ initialApp: "claude_desktop" });
    expect(await screen.findByTestId("kaifan-join-pinned")).toHaveTextContent(
      "你指给我的那份",
    );
    expect(screen.getByRole("button", { name: "换一个位置" })).toBeEnabled();
    found([target("codex_desktop"), target("claude_desktop")]);
    fireEvent.click(screen.getByRole("button", { name: "改回自己找" }));
    await waitFor(() =>
      expect(screen.queryByTestId("kaifan-join-pinned")).toBeNull(),
    );
    expect(clearManualLocation).toHaveBeenCalledWith("claude_desktop");
  });

  it("安装包交给了系统的安装窗口：等用户点完，回来说一声就核对并接着接入", async () => {
    const waitingAt = installing({
      toolId: "claude_desktop",
      mode: "system_assisted",
      phase: "awaiting_system_confirmation",
      canCancel: false,
    });
    let confirmed = 0;
    installer.mockImplementation(async (toolId, action) => {
      if (action === "start") return waitingAt;
      if (action === "confirm") {
        confirmed++;
        return { ...waitingAt, phase: "checking_install" };
      }
      if (action === "inspect" && confirmed === 1)
        return { ...waitingAt, reasonCode: "installation_not_detected" };
      if (action === "inspect" && confirmed > 1)
        return {
          ...waitingAt,
          phase: "installed",
          disposition: "confirmed",
          installationId: "claude_desktop-1",
        };
      return installing({
        toolId,
        jobId: "",
        phase: "idle",
        mode: "system_assisted",
      });
    });
    found([target("codex_desktop")]);
    answer("ready", "ready");
    const props = setup({ initialApp: "claude_desktop" });
    fireEvent.click(await screen.findByRole("button", { name: /安装并接入/ }));
    await waitFor(() => expect(says()).toContain("系统弹出了安装窗口"));
    expect(activate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "重新打开安装窗口" }));
    expect(installer).toHaveBeenCalledWith("claude_desktop", "reopen", "job-1");
    // 第一次回来说装好了，其实还没装上：照实说，留在这一步。
    fireEvent.click(screen.getByRole("button", { name: /装好了/ }));
    await waitFor(() => expect(says()).toContain("还没看到"), {
      timeout: 4000,
    });
    expect(activate).not.toHaveBeenCalled();
    found([target("codex_desktop"), target("claude_desktop")]);
    fireEvent.click(screen.getByRole("button", { name: /装好了/ }));
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled(), {
      timeout: 4000,
    });
    expect(activate.mock.calls[0][0]).toMatchObject({
      installationId: "claude_desktop-1",
      installationJobId: "job-1",
    });
  });

  it("等系统安装窗口时不想装了：回到选的那一步，不去接入", async () => {
    const waitingAt = installing({
      toolId: "claude_desktop",
      mode: "system_assisted",
      phase: "awaiting_system_confirmation",
      canCancel: false,
    });
    // 上次停在这一步的：一打开就接着这一步，不用重新下载。
    installer.mockImplementation(async (_tool, action) =>
      action === "cancel" ? { ...waitingAt, phase: "cancelled" } : waitingAt,
    );
    found([target("codex_desktop")]);
    setup({ initialApp: "claude_desktop" });
    await waitFor(() => expect(says()).toContain("系统弹出了安装窗口"));
    fireEvent.click(screen.getByRole("button", { name: "先不装了" }));
    await waitFor(() =>
      expect(installer).toHaveBeenCalledWith(
        "claude_desktop",
        "cancel",
        "job-1",
      ),
    );
    expect(screen.queryByTestId("kaifan-join-says")).toBeNull();
    expect(activate).not.toHaveBeenCalled();
  });

  it("没装、客户端能自己装的应用：点一下就装好并接着接入，不用再点", async () => {
    absentClaudeCode(() => {
      // 装好之后，电脑上就能找到它了。
      found([target("codex_desktop"), target("claude_code")]);
      return installing({
        phase: "installed",
        canCancel: false,
        disposition: "created",
        installationId: "fresh-1",
      });
    });
    found([target("codex_desktop")]);
    const props = setup({ initialApp: "claude_code" });
    answer("ready", "configuration_ready");
    fireEvent.click(
      await screen.findByRole("button", { name: /安装并接入 Claude Code/ }),
    );
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled());
    const start = installer.mock.calls.find((call) => call[1] === "start");
    expect(start?.[3]).toEqual({
      lineId: "mainland_optimized",
      modelId: "claude-sonnet-4-5-20250929",
      billingGroup: "default",
      models: [
        { modelId: "claude-sonnet-4-5-20250929", billingGroup: "default" },
      ],
    });
    // 接入用的是安装报回来的那一处，并带着那次安装的凭证；内容和安装时说好的一样。
    expect(activate.mock.calls[0][0]).toMatchObject({
      toolId: "claude_code",
      installationId: "fresh-1",
      installationJobId: "job-1",
      ...start?.[3],
    });
  });

  it("安装时一直看着进度，装完才去接入", async () => {
    let asked = 0;
    installer.mockImplementation(async (toolId, action) => {
      if (action === "start")
        return installing({ downloadedBytes: 50 << 20, totalBytes: 200 << 20 });
      if (action === "inspect" && asked++ > 0)
        return installing({
          phase: "installed",
          canCancel: false,
          disposition: "created",
          installationId: "fresh-1",
        });
      return installing({ toolId, jobId: "", phase: "idle" });
    });
    found([target("codex_desktop")]);
    const props = setup({ initialApp: "claude_code" });
    answer("ready", "configuration_ready");
    fireEvent.click(await screen.findByRole("button", { name: /安装并接入/ }));
    await waitFor(() => expect(says()).toContain("正在下载 Claude Code"));
    expect(screen.getByText("50 MB / 200 MB")).toBeInTheDocument();
    expect(activate).not.toHaveBeenCalled();
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled(), {
      timeout: 4000,
    });
    expect(installer).toHaveBeenCalledWith("claude_code", "inspect", "job-1");
  });

  it("官方下载地址连不上：说清楚，没装也没接；能再试，也能去官网自己装", async () => {
    absentClaudeCode(() =>
      installing({
        phase: "failed",
        canCancel: false,
        reasonCode: "network_unavailable",
      }),
    );
    found([target("codex_desktop")]);
    setup({ initialApp: "claude_code" });
    fireEvent.click(await screen.findByRole("button", { name: /安装并接入/ }));
    await waitFor(() => expect(says()).toContain("没连上"));
    expect(activate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "去官网自己装" }));
    expect(installer).toHaveBeenCalledWith("claude_code", "help");
    fireEvent.click(screen.getByRole("button", { name: "再试一次" }));
    await waitFor(() =>
      expect(
        installer.mock.calls.filter((call) => call[1] === "start"),
      ).toHaveLength(2),
    );
  });

  it("下载到的和官方校验值对不上：不装，也不接", async () => {
    absentClaudeCode(() =>
      installing({
        phase: "failed",
        canCancel: false,
        reasonCode: "invalid_download",
      }),
    );
    found([target("codex_desktop")]);
    const props = setup({ initialApp: "claude_code" });
    fireEvent.click(await screen.findByRole("button", { name: /安装并接入/ }));
    await waitFor(() => expect(says()).toContain("对不上"));
    expect(screen.getByRole("button", { name: "重新下载" })).toBeVisible();
    expect(activate).not.toHaveBeenCalled();
    expect(props.onJoined).not.toHaveBeenCalled();
  });

  it("装到一半说停：去停掉那次安装，回到选择，不接入", async () => {
    let stopped = false;
    installer.mockImplementation(async (toolId, action) => {
      if (action === "cancel") stopped = true;
      if (action === "start" || action === "cancel") return installing();
      if (action === "inspect" && stopped)
        return installing({
          phase: "cancelled",
          canCancel: false,
          reasonCode: "cancelled",
        });
      return installing({ toolId, jobId: "", phase: "idle" });
    });
    found([target("codex_desktop")]);
    setup({ initialApp: "claude_code" });
    fireEvent.click(await screen.findByRole("button", { name: /安装并接入/ }));
    fireEvent.click(await screen.findByRole("button", { name: "先停下" }));
    await waitFor(() =>
      expect(installer).toHaveBeenCalledWith("claude_code", "cancel", "job-1"),
    );
    await screen.findByRole(
      "button",
      { name: /安装并接入/ },
      { timeout: 4000 },
    );
    expect(activate).not.toHaveBeenCalled();
  });

  it("客户端装不了的应用（官方没有可核对的安装包）：打开官网，不开始安装", async () => {
    absentClaudeCode(() => installing(), "guided");
    found([target("codex_desktop")]);
    setup({ initialApp: "claude_code" });
    fireEvent.click(await screen.findByRole("button", { name: /去官网下载/ }));
    expect(installer).toHaveBeenCalledWith("claude_code", "help");
    expect(
      installer.mock.calls.filter((call) => call[1] === "start"),
    ).toHaveLength(0);
  });

  it("安装的结果只在真的装好并认出位置时才往下接", () => {
    const at = (patch: Partial<InstallationProgress>) =>
      installOutcome(installing({ canCancel: false, ...patch }));
    expect(
      at({ phase: "installed", disposition: "created", installationId: "a" }),
    ).toBeNull();
    expect(
      at({ phase: "installed", disposition: "unconfirmed" }),
    ).toMatchObject({ kind: "installUnseen", action: "look" });
    expect(at({ phase: "installed", disposition: "existing" })).toMatchObject({
      kind: "installFound",
    });
    expect(at({ phase: "cancelled" })).toBe("stopped");
    expect(at({ phase: "awaiting_system_confirmation" })).toBe("waiting");
    for (const [reasonCode, kind] of [
      ["source_unavailable", "installOffline"],
      ["download_changed", "installBad"],
      ["disk_full", "installFull"],
      ["permission_denied", "installFailed"],
    ])
      expect(at({ phase: "failed", reasonCode })).toMatchObject({
        kind,
        action: "install",
      });
    // 连进度都问不到：当作没装成，不往下接。
    expect(installOutcome(null)).toMatchObject({ kind: "installFailed" });
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

/** 一个要修的应用：`state` 是哪种坏法。 */
const broken = (
  state: ToolConnection["state"],
  patch: Partial<ToolConnection> = {},
): ToolConnection => ({
  toolId: "codex_desktop",
  state,
  modelId: "deepseek-v4-flash",
  lineId: "mainland_optimized",
  billingGroup: "default",
  updatedAtEpochMs: 1,
  restoreMode: "original",
  requiresBackground: false,
  reasonCode: "",
  ...patch,
});
const undone = (
  status: "restored" | "restored_with_changes" | "recovery_failed",
) => ({
  requestId: "u",
  schemaVersion: 2 as const,
  status,
  connections: [],
  reasonCode: "",
});

describe("接入：应用多了不堆在一起", () => {
  const chip = (name: RegExp) => screen.queryByRole("button", { name });

  it("不常用的收着，点一下才露出来；接过的一直露着", async () => {
    found([target("claude_desktop"), target("pi")]);
    setup({
      // 后端给的名单里每个应用都有一行，没接过的写着 not_connected。
      connections: [
        { toolId: "dsh_desktop", state: "connected" } as ToolConnection,
        { toolId: "workbuddy", state: "not_connected" } as ToolConnection,
        { toolId: "pi", state: "not_connected" } as ToolConnection,
        { toolId: "dsh_web", state: "not_connected" } as ToolConnection,
      ],
    });
    await ready();
    for (const name of [
      /^Claude Desktop/,
      /^Codex/,
      /^OpenCode/,
      /^Claude Code/,
      /^WorkBuddy/,
      /^Pi/,
      /^DeepSeek Harness/,
    ])
      expect(chip(name)).not.toBeNull();
    for (const name of [/^DSH web/, /^Cline/, /^Hermes Agent/, /^OpenClaw/])
      expect(chip(name)).toBeNull();

    const fold = screen.getByRole("button", { name: /还有 4 个/ });
    expect(fold).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(fold);
    for (const name of [/^DSH web/, /^Cline/, /^Hermes Agent/, /^OpenClaw/])
      expect(chip(name)).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /收起/ }));
    expect(chip(/^DSH web/)).toBeNull();
  });

  it("展开过不常用的，关掉再打开：又是收着的", async () => {
    found([target("claude_desktop")]);
    const props = setup();
    cleanup();
    const view = render(<Join {...props} />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: /还有 \d 个/ }));
    view.rerender(<Join {...props} open={false} />);
    view.rerender(<Join {...props} open />);
    await ready();
    expect(chip(/^Cline/)).toBeNull();
    expect(screen.getByRole("button", { name: /还有 \d 个/ })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("还在找应用时点了「别的应用」：找完也不把要填的型号换掉", async () => {
    const OPUS = "claude-opus-4-1-20250805";
    let done: (scan: ActivationTargetScan) => void = () => {};
    scan.mockReturnValue(
      new Promise<ActivationTargetScan>((ok) => (done = ok)),
    );
    setup({
      lineId: "station_key:https://relay.example.com" as never,
      models: [...MODELS, model(OPUS)],
      connections: [
        {
          toolId: "codex_desktop",
          state: "connected",
          modelId: OPUS,
          lineId: "",
          billingGroup: "default",
          updatedAtEpochMs: 1,
          restoreMode: "original",
          requiresBackground: false,
          reasonCode: "",
        },
      ],
    });
    fireEvent.click(screen.getByRole("button", { name: "别的应用" }));
    const panel = screen.getByTestId("kaifan-join-other");
    expect(panel).toHaveTextContent("claude-sonnet-4-5-20250929");
    await act(async () =>
      done({
        requestId: "scan",
        schemaVersion: 1,
        platform: "macos",
        targets: [target("codex_desktop")],
      }),
    );
    expect(panel).toHaveTextContent("claude-sonnet-4-5-20250929");
    expect(panel).not.toHaveTextContent(OPUS);
  });

  it("别的应用：站点这次没列出型号，说清型号给不出来，不装作都齐了", async () => {
    found([target("claude_desktop")]);
    setup({
      lineId: "station_key:https://relay.example.com" as never,
      models: [],
    });
    fireEvent.click(await screen.findByRole("button", { name: "别的应用" }));
    expect(screen.getByTestId("kaifan-join-other-nomodel")).toHaveTextContent(
      "型号这一栏现在给不出来",
    );
  });

  it("从首页点进来的不常用应用：选着它的时候不收它", async () => {
    setup({ initialApp: "dsh_web" });
    await waitFor(() =>
      expect(chip(/^DSH web/)).toHaveAttribute("aria-pressed", "true"),
    );
    expect(screen.getByRole("button", { name: /还有 4 个/ })).toBeTruthy();
  });

  it("别的应用：给出要填的地址和型号，能复制；不拿密钥出来，也不去接入", async () => {
    const writeText = vi.fn(async (_text: string) => undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    setup({ lineId: "station_key:https://relay.example.com" as never });
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "别的应用" }));
    const panel = screen.getByTestId("kaifan-join-other");
    expect(panel).toHaveTextContent("https://relay.example.com/v1");
    expect(panel).toHaveTextContent("claude-sonnet-4-5-20250929");
    expect(panel).toHaveTextContent(/你连接这个站时粘贴的那把密钥/);
    expect(screen.queryByRole("button", { name: /去接入/ })).toBeNull();
    expect(screen.queryByTestId("kaifan-join-other-risk")).toBeNull();

    const copies = screen.getAllByRole("button", { name: "复制" });
    expect(copies).toHaveLength(3);
    fireEvent.click(copies[0]);
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("https://relay.example.com/v1"),
    );
    expect(await screen.findByRole("button", { name: "已复制" })).toBeTruthy();
    fireEvent.click(screen.getAllByRole("button", { name: "复制" })[0]);
    await waitFor(() =>
      expect(writeText).toHaveBeenLastCalledWith("https://relay.example.com"),
    );
    expect(activate).not.toHaveBeenCalled();

    // 点回名单里的应用，就回到接入。
    fireEvent.click(screen.getByRole("button", { name: /^Codex/ }));
    expect(screen.queryByTestId("kaifan-join-other")).toBeNull();
    expect(go()).toBeTruthy();
  });

  it("Cline 只装在编辑器里：不去接入，给出要手填的地址和型号", async () => {
    found([
      target("claude_desktop"),
      target("cline", {
        status: "manual_only",
        installations: [
          {
            installationId: "cline-ext",
            label: "编辑器扩展",
            version: "",
            supported: false,
            recommended: false,
          },
        ],
      }),
    ]);
    setup({
      initialApp: "cline",
      lineId: "station_key:https://relay.example.com" as never,
    });
    await waitFor(() =>
      expect(screen.getByTestId("kaifan-join-other")).toBeTruthy(),
    );
    const panel = screen.getByTestId("kaifan-join-other");
    expect(panel).toHaveTextContent(/Cline 装在你的编辑器里/);
    expect(panel).toHaveTextContent("OpenAI Compatible");
    expect(panel).toHaveTextContent("https://relay.example.com/v1");
    expect(panel).toHaveTextContent("claude-sonnet-4-5-20250929");
    // Cline 只用得上 OpenAI 格式的地址：地址和型号各一个复制按钮。
    expect(screen.getAllByRole("button", { name: "复制" })).toHaveLength(2);
    expect(chip(/^Cline/)).toHaveAttribute("aria-pressed", "true");
    expect(chip(/^Cline/)).not.toHaveTextContent("还没装");
    expect(screen.queryByRole("button", { name: /去接入/ })).toBeNull();
    expect(activate).not.toHaveBeenCalled();

    // 命令行版装在没找的地方：指给客户端，重新找一遍，就能一键接。
    pointOut.mockResolvedValue("saved");
    found([target("claude_desktop"), target("cline")]);
    fireEvent.click(
      within(screen.getByTestId("kaifan-join-byhand-cli")).getByRole("button", {
        name: "告诉我它在哪",
      }),
    );
    await waitFor(() => expect(pointOut).toHaveBeenCalledWith("cline"));
    await waitFor(() =>
      expect(screen.queryByTestId("kaifan-join-other")).toBeNull(),
    );
    expect(go()).toBeTruthy();
    expect(chip(/^Cline/)).toHaveAttribute("aria-pressed", "true");

    // 换成别的应用，照常接入。
    fireEvent.click(screen.getByRole("button", { name: /^Claude Desktop/ }));
    expect(screen.queryByTestId("kaifan-join-other")).toBeNull();
    expect(go()).toBeTruthy();
  });

  it("别的应用：浏览器授权连的站，说去站点网页新建密钥；剪贴板用不了就说怎么办", async () => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    setup();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "别的应用" }));
    const panel = screen.getByTestId("kaifan-join-other");
    expect(panel).toHaveTextContent("https://yeschoy.com/v1");
    expect(panel).toHaveTextContent(/去站点网页的「令牌」里新建一把/);
    fireEvent.click(screen.getAllByRole("button", { name: "复制" })[0]);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      /没复制上.*自己复制/,
    );
  });
});

describe("接入：连的是云厂商", () => {
  const vendor = (patch: Partial<ProviderPreset> = {}): ProviderPreset => ({
    id: "kimi_code_cn",
    family: "kimi_code",
    region: "cn",
    name: "Kimi Code",
    origin: "https://api.kimi.com",
    billing: "plan",
    unverifiable: false,
    onlyTools: [],
    keyPage: "https://example.com/keys",
    common: true,
    openaiBase: "https://api.kimi.com/coding/v1",
    anthropicBase: "https://api.kimi.com/coding",
    sessionHeader: null,
    ...patch,
  });

  it("套餐：接入之前再说一次有被封的风险，照样能接", async () => {
    setup({ preset: vendor() });
    await ready();
    expect(screen.getByTestId("kaifan-join-plan-risk")).toHaveTextContent(
      /Kimi Code.*停掉套餐/,
    );
  });

  it("按量付费的、中转站：不提套餐的事", async () => {
    setup({ preset: vendor({ billing: "pay_as_you_go" }) });
    await ready();
    expect(screen.queryByTestId("kaifan-join-plan-risk")).toBeNull();
  });

  it("别的应用：地址按厂商表里的来；套餐再提醒一次别填进不认的应用", async () => {
    setup({
      lineId: "station_key:https://api.kimi.com" as never,
      preset: vendor(),
    });
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "别的应用" }));
    const panel = screen.getByTestId("kaifan-join-other");
    expect(panel).toHaveTextContent("https://api.kimi.com/coding/v1");
    expect(panel).toHaveTextContent("https://api.kimi.com/coding");
    expect(screen.getByTestId("kaifan-join-other-risk")).toHaveTextContent(
      /Kimi Code 是包月套餐.*停掉套餐/,
    );
  });

  it("别的应用：这家没有 Claude 格式的地址就不列那一行", async () => {
    setup({
      lineId: "station_key:https://api.openai.com" as never,
      preset: vendor({
        id: "openai",
        family: "openai",
        name: "OpenAI",
        billing: "pay_as_you_go",
        openaiBase: "https://api.openai.com/v1",
        anthropicBase: null,
      }),
    });
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "别的应用" }));
    expect(screen.getByTestId("kaifan-join-other")).not.toHaveTextContent(
      "Claude 格式",
    );
    expect(screen.getAllByRole("button", { name: "复制" })).toHaveLength(2);
    expect(screen.queryByTestId("kaifan-join-other-header")).toBeNull();
  });

  it("别的应用：这家还要一个请求头，说清楚叫什么、没处填就接不了", async () => {
    setup({
      preset: vendor({
        id: "opencode_go",
        family: "opencode_go",
        name: "OpenCode Go",
        sessionHeader: "x-opencode-session",
      }),
    });
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "别的应用" }));
    expect(screen.getByTestId("kaifan-join-other-header")).toHaveTextContent(
      /OpenCode Go 还要求每个请求带一个请求头 x-opencode-session.*接不了这家/,
    );
  });

  it("这家不认这个应用：说清楚，不让点，也不发给后端", async () => {
    found([target("claude_desktop"), target("workbuddy")]);
    setup({
      preset: vendor({
        id: "opencode_go",
        family: "opencode_go",
        name: "OpenCode Go",
        onlyTools: [
          "opencode",
          "codex_desktop",
          "claude_desktop",
          "claude_code",
          "pi",
        ],
      }),
      initialApp: "workbuddy",
    });
    await waitFor(() =>
      expect(screen.getByTestId("kaifan-join-barred")).toHaveTextContent(
        /WorkBuddy 暂时接不了 OpenCode Go.*改选 OpenCode、Codex.*、Claude Desktop、Claude Code、Pi/,
      ),
    );
    expect(go()).toBeDisabled();
    expect(screen.queryByTestId("kaifan-join-plan-risk")).toBeNull();
    expect(activate).not.toHaveBeenCalled();

    // 开饭能替它带上标记的应用：照常能接。
    fireEvent.click(screen.getByRole("button", { name: /^Claude Desktop/ }));
    expect(screen.queryByTestId("kaifan-join-barred")).toBeNull();
    await ready();
    expect(screen.getByTestId("kaifan-join-plan-risk")).toBeTruthy();
  });
});

describe("接入：要修的应用就在这里修", () => {
  it("上次没做完：选回原来的人和型号，说清是怎么回事，点一下接着接好", async () => {
    answer("ready", "ready");
    const props = setup({
      initialApp: "codex_desktop",
      connections: [broken("recovery_pending")],
    });
    const mend = await screen.findByRole("button", { name: /接着接好/ });
    await waitFor(() => expect(mend).toBeEnabled());
    expect(mend).toHaveTextContent("让小鲸接着接好");
    expect(screen.getByTestId("kaifan-join-mend")).toHaveTextContent(
      "上次接到一半停了",
    );
    fireEvent.click(mend);
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled());
    expect(activate.mock.calls[0][0]).toMatchObject({
      toolId: "codex_desktop",
      modelId: "deepseek-v4-flash",
      billingGroup: "default",
    });
  });

  it("被别的工具改过：说重新接入会盖掉那边的；改回原样点一下就做，做完说结果", async () => {
    const props = setup({
      initialApp: "codex_desktop",
      connections: [broken("changed")],
      onUndo: vi.fn().mockResolvedValue(undone("restored_with_changes")),
    });
    expect(
      await screen.findByRole("button", { name: /重新接入/ }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("kaifan-join-mend")).toHaveTextContent(
      "会盖掉那边改的",
    );
    fireEvent.click(screen.getByTestId("kaifan-join-undo"));
    await waitFor(() => expect(says()).toContain("你后来自己改的那些我没动"));
    expect(props.onUndo).toHaveBeenCalledWith("codex_desktop");
    expect(activate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "好" }));
    expect(props.onClose).toHaveBeenCalled();
  });

  it("没改回成：照实说，能再试一次，也能回去", async () => {
    const onUndo = vi
      .fn()
      .mockRejectedValueOnce(new Error("busy"))
      .mockResolvedValueOnce(undone("restored"));
    setup({
      initialApp: "codex_desktop",
      connections: [broken("recovery_pending")],
      onUndo,
    });
    fireEvent.click(await screen.findByTestId("kaifan-join-undo"));
    await waitFor(() => expect(says()).toContain("这次没改成"));
    fireEvent.click(screen.getByRole("button", { name: "再试一次" }));
    await waitFor(() => expect(says()).toContain("改回接入之前的样子了"));
    expect(onUndo).toHaveBeenCalledTimes(2);
  });

  it("旧版本接的、没存下原设置的：按钮叫撤掉接入；好好的应用没有这个按钮", async () => {
    setup({
      initialApp: "codex_desktop",
      connections: [
        broken("changed", { restoreMode: "remove_yeschoy" }),
        broken("connected", { toolId: "claude_desktop" }),
      ],
    });
    expect(await screen.findByTestId("kaifan-join-undo")).toHaveTextContent(
      "撤掉接入",
    );
    fireEvent.click(screen.getByRole("button", { name: /^Claude Desktop/ }));
    expect(screen.queryByTestId("kaifan-join-undo")).not.toBeInTheDocument();
    expect(screen.queryByTestId("kaifan-join-mend")).not.toBeInTheDocument();
  });

  it("没接成、也没改回去：再试一次就在这里做，不去旧页面", async () => {
    answer("configuration_failed", "configuration_rollback_failed");
    answer("ready", "ready");
    const props = setup({ initialApp: "codex_desktop" });
    await ready();
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("还没完全改回原样"));
    fireEvent.click(screen.getByRole("button", { name: "再试一次" }));
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled());
    expect(activate).toHaveBeenCalledTimes(2);
  });
});

describe("接入：自己能解决的不问用户", () => {
  it("应用挪了地方：自己重新找一遍，找到了接着接", async () => {
    answer("tool_not_found", "tool_not_found");
    answer("ready", "ready");
    const props = setup({ initialApp: "codex_desktop" });
    await ready();
    const moved = target("codex_desktop");
    moved.installations[0].installationId = "codex_desktop-2";
    found([moved]);
    fireEvent.click(go());
    await waitFor(() => expect(props.onJoined).toHaveBeenCalled());
    expect(activate.mock.calls.map((call) => call[0].installationId)).toEqual([
      "codex_desktop-1",
      "codex_desktop-2",
    ]);
  });

  it("重新找了还是老地方：只找一次，照实说没找到", async () => {
    answer("tool_not_found", "tool_not_found");
    setup({ initialApp: "codex_desktop" });
    await ready();
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("没找到"));
    expect(activate).toHaveBeenCalledTimes(1);
    expect(scan).toHaveBeenCalledTimes(2);
  });

  it("好几份里用户点名的那份不见了：不替他换成另一份", async () => {
    const copy = (
      installationId: string,
      label: string,
      recommended: boolean,
    ) => ({
      installationId,
      label,
      version: "1",
      supported: true,
      recommended,
    });
    found([
      target("claude_desktop", {
        status: "selection_required",
        installations: [
          copy("a", "系统应用", false),
          copy("b", "用户目录", true),
        ],
      } as Partial<ActivationTarget>),
    ]);
    answer("tool_not_found", "tool_not_found");
    const props = setup();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: /系统应用/ }));
    found([
      target("claude_desktop", {
        installations: [copy("b", "用户目录", true)],
      } as Partial<ActivationTarget>),
    ]);
    fireEvent.click(go());
    await waitFor(() => expect(says()).toContain("没找到"));
    expect(activate.mock.calls.map((call) => call[0].installationId)).toEqual([
      "a",
    ]);
    expect(props.onJoined).not.toHaveBeenCalled();
  });

  it("断网没下成：网络一恢复自己再下，不用回来再点", async () => {
    absentClaudeCode(() =>
      installing({
        phase: "failed",
        canCancel: false,
        reasonCode: "network_unavailable",
      }),
    );
    found([target("codex_desktop")]);
    setup({ initialApp: "claude_code" });
    fireEvent.click(await screen.findByRole("button", { name: /安装并接入/ }));
    await waitFor(() => expect(says()).toContain("没连上"));
    const starts = () =>
      installer.mock.calls.filter((call) => call[1] === "start");
    expect(starts()).toHaveLength(1);
    act(() => void window.dispatchEvent(new Event("online")));
    await waitFor(() => expect(starts()).toHaveLength(2));
  });
});

describe("角色登场", () => {
  const joined = {
    toolId: "claude_desktop" as const,
    who: "cla" as const,
    modelId: "claude-sonnet-4-5-20250929",
    opened: true,
    terminal: false,
    skipped: [] as string[],
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

  it("没打开成、后端说得出原因：说那个原因和出路，不说笼统的那句", () => {
    show({
      joined: { ...joined, opened: false, why: "desktop_launch_access_denied" },
      openFailed: true,
    });
    const text = screen.getByTestId("kaifan-reveal").textContent ?? "";
    expect(text).toContain("系统拦住了这次启动");
    expect(text).not.toContain("没能替你打开");
  });

  it("没打开成：照实说，设置还在", () => {
    show({ joined: { ...joined, opened: false }, openFailed: true });
    expect(screen.getByTestId("kaifan-reveal")).toHaveTextContent(
      "没能替你打开 Claude Desktop",
    );
  });

  it("终端里用的应用：说的是终端窗口", () => {
    show({ joined: { ...joined, toolId: "claude_code", terminal: true } });
    expect(screen.getByTestId("kaifan-reveal")).toHaveTextContent("终端窗口");
  });

  it("Codex 只装了命令行版：说的也是终端窗口，不说「Codex 已经打开」", () => {
    show({ joined: { ...joined, toolId: "codex_desktop", terminal: true } });
    const reveal = screen.getByTestId("kaifan-reveal");
    expect(reveal).toHaveTextContent("终端窗口");
    expect(reveal).not.toHaveTextContent("已经替你打开，现在可以直接用了");
  });

  it("名单里有型号这次没接上：要说出来，不当作全成了", () => {
    show({ joined: { ...joined, skipped: ["deepseek-v4-flash"] } });
    expect(screen.getByTestId("kaifan-reveal")).toHaveTextContent(
      /DeepSeek.*这次没接上/,
    );
  });

  it("没有刚接好的就不出现", () => {
    show({ joined: null });
    expect(screen.queryByTestId("kaifan-reveal")).toBeNull();
  });
});
