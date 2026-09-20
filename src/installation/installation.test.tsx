import { StrictMode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import {
  InstallationContext,
  InstallationProvider,
  useInstallation,
  type InstallationController,
} from "./InstallationProvider";
import {
  InstallationPanel,
  InstallationNotice,
  installationSource,
} from "./InstallationPanel";
import {
  decodeInstallation,
  SelectionRevision,
  type InstallationProgress,
} from "./api";
import { ConfigurationPreviewView } from "../configuration/ConfigurationPreviewView";
import { AppLibraryView } from "../workbench/AppLibraryView";
import {
  ACTIVATION_TOOL_IDS,
  scanActivationTargets,
  activateDesktopTool,
  type ActivationTargetScan,
  type ActivationToolId,
} from "../configuration/activation";
import { ConnectionProvider } from "../configuration/connections";
import { connectionsFixture } from "../configuration/connection-test-fixtures";
import type { AccountSessionController } from "../account/useAccountSession";
import type { AccountProjection } from "../account/session";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../configuration/activation", async (load) => ({
  ...(await load<typeof import("../configuration/activation")>()),
  scanActivationTargets: vi.fn(),
  activateDesktopTool: vi.fn(),
}));
const native = vi.mocked(invoke);
const scan = vi.mocked(scanActivationTargets);
const activate = vi.mocked(activateDesktopTool);
const tick = async () => {
  await act(async () => {});
};
const noop = vi.fn();
function progress(
  patch: Partial<InstallationProgress> = {},
): InstallationProgress {
  return {
    schemaVersion: 2,
    requestId: "fixture",
    toolId: "codex_desktop",
    jobId: "",
    mode: "automatic",
    phase: "idle",
    downloadedBytes: 0,
    totalBytes: 0,
    canCancel: false,
    reasonCode: "none",
    source: "none",
    platform: "macos",
    architecture: "arm64",
    disposition: "none",
    installationId: "",
    ...patch,
  };
}
function targets(installed = 0): ActivationTargetScan {
  return {
    schemaVersion: 1,
    requestId: "test",
    platform: "macos",
    targets: ACTIVATION_TOOL_IDS.map((toolId) => ({
      toolId,
      displayName: toolId,
      surface: "桌面应用",
      status:
        toolId === "codex_desktop" && installed
          ? installed > 1
            ? "selection_required"
            : "available"
          : "not_found",
      installations:
        toolId === "codex_desktop"
          ? Array.from({ length: installed }, (_, n) => ({
              installationId:
                n === 0 ? "i0000000000000001" : "i0000000000000002",
              supported: true,
              recommended: n === 0,
              label: "本机应用",
              version: "1",
            }))
          : [],
    })),
  };
}
const projection: AccountProjection = {
  schemaVersion: 3,
  requestId: "test",
  status: "signed_in",
  userCode: "",
  pollAfterSeconds: 0,
  expiresAtEpochMs: 0,
  observedAtEpochMs: 1,
  account: {
    available: true,
    displayName: "测试用户",
    username: "fixture-user",
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
  comparisonFx: "1",
  reasonCode: "none",
  models: ["model-a", "model-b"].map((id) => ({
    id,
    description: "",
    billingMode: "ratio",
    supportedEndpointTypes: ["openai-response"],
    pricingAvailable: false,
    officialInputCnyPerMillion: "",
    officialOutputCnyPerMillion: "",
    actualInputCnyPerMillion: "",
    actualOutputCnyPerMillion: "",
    billing: {
      groups: [{ id: "default", description: "标准", ratio: 0.7 }],
      baseInputUsd: 1,
      baseOutputUsd: 2,
      cacheReadUsd: 0.1,
      requestUsd: null,
      expression: "",
    },
  })),
};
const account: AccountSessionController = {
  projection,
  loading: false,
  refresh: vi.fn(async () => projection),
  beginAuthorization: vi.fn(async () => null),
  openAuthorization: vi.fn(async () => null),
  cancelAuthorization: vi.fn(async () => null),
  logout: vi.fn(async () => null),
  openWallet: vi.fn(async () => true),
};
const connections = {
  connections: connectionsFixture("test").connections,
  loading: false,
  error: false,
  restoring: null,
  opening: null,
  refresh: vi.fn(async () => {}),
  restore: vi.fn(async () => connectionsFixture("test")),
  open: vi.fn(async () => ({
    status: "opened" as const,
    reasonCode: "opened",
  })),
};

describe("ru052 truthful download source", () => {
  it("accepts only the bundled source enum and does not claim a mirror before selection", () => {
    expect(
      decodeInstallation(progress({ source: "mirror" }), "fixture")?.source,
    ).toBe("mirror");
    expect(
      decodeInstallation(
        { ...progress(), source: "unknown-provider" },
        "fixture",
      ),
    ).toBeNull();
    expect(installationSource("none")).toBe(
      "优先使用野菜国内加速，不可用时自动改走厂商官网。",
    );
    expect(installationSource("official")).toContain("厂商官网");
    expect(installationSource("mirror")).toContain("野菜国内加速");
  });
  it("changes the visible source when a failed mirror falls back to official bytes", () => {
    const panel = (source: InstallationProgress["source"]) => (
      <InstallationContext.Provider
        value={{
          progress: progress({
            source,
            phase: "downloading",
            jobId: "download-job",
            canCancel: true,
          }),
          working: false,
          error: false,
          run: vi.fn(),
        }}
      >
        <InstallationPanel
          tool="codex_desktop"
          name="Codex"
          canConnect={false}
          onStart={vi.fn()}
          onConfirm={vi.fn()}
          onRefresh={vi.fn()}
        />
      </InstallationContext.Provider>
    );
    const view = render(panel("mirror"));
    expect(
      screen.getByText("下载来源：野菜国内加速 · 安装前校验厂商签名"),
    ).toBeInTheDocument();
    view.rerender(panel("official"));
    expect(
      screen.getByText("下载来源：厂商官网 · 安装前校验厂商签名"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("下载来源：野菜国内加速 · 安装前校验厂商签名"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/国内备用下载源尚未启用/),
    ).not.toBeInTheDocument();
  });
});
const run = vi.fn<InstallationController["run"]>();
function setup(
  p: InstallationProgress,
  line: "mainland_optimized" | "global_accelerated" = "mainland_optimized",
  active = true,
) {
  return (
    <StrictMode>
      <ConnectionProvider value={connections}>
        <InstallationContext.Provider
          value={{ progress: p, working: false, error: false, run }}
        >
          <ConfigurationPreviewView
            active={active}
            initialDesktopAppId="codex_desktop"
            enableLocalActivation
            lineId={line}
            onLineChange={noop}
            session={account}
            onOpenAccount={noop}
            onOpenTools={noop}
          />
        </InstallationContext.Provider>
      </ConnectionProvider>
    </StrictMode>
  );
}
beforeEach(async () => {
  await i18n.init({
    lng: "zh",
    fallbackLng: "zh",
    resources: { zh: { translation: zh } },
  });
  vi.clearAllMocks();
  localStorage.clear();
  scan.mockResolvedValue(targets());
  run.mockResolvedValue(
    progress({ jobId: "native-job", phase: "downloading", canCancel: true }),
  );
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

// Real provider + real configuration view: static context snapshots cannot
// expose job polling, cross-app ownership or effect ordering regressions.
function liveSetup(tool: ActivationToolId, session = account) {
  return (
    <ConnectionProvider value={connections}>
      <InstallationProvider>
        <ConfigurationPreviewView
          initialDesktopAppId={tool}
          enableLocalActivation
          lineId="mainland_optimized"
          onLineChange={noop}
          session={session}
          onOpenAccount={noop}
          onOpenTools={noop}
        />
      </InstallationProvider>
    </ConnectionProvider>
  );
}

describe("ru051 installer recovery with real provider", () => {
  it("continues a matching signed-in Windows confirmation exactly once", async () => {
    let job = progress({ platform: "windows", mode: "system_assisted" });
    native.mockImplementation(async (command, args) => {
      if (command !== "manage_app_installation_v2") return;
      const r = (
        args as {
          request: {
            requestId: string;
            toolId: ActivationToolId;
            action: string;
            intent?: unknown;
          };
        }
      ).request;
      if (r.action === "start" || r.action === "confirm") {
        expect(r.intent).toMatchObject({
          modelId: "model-a",
          billingGroup: "default",
          lineId: "mainland_optimized",
        });
        job = {
          ...job,
          toolId: r.toolId,
          jobId: "windows-exact",
          phase: "awaiting_system_confirmation",
        };
      }
      if (r.action === "confirm") {
        scan.mockResolvedValue(targets(1));
        job = {
          ...job,
          phase: "installed",
          disposition: "confirmed",
          installationId: "i0000000000000001",
        };
      }
      return { ...job, toolId: r.toolId, requestId: r.requestId };
    });
    activate.mockResolvedValueOnce({
      schemaVersion: 3,
      requestId: "activation-fixture",
      status: "ready",
      toolId: "codex_desktop",
      modelId: "model-a",
      billingGroup: "default",
      observedAtEpochMs: 1,
      reasonCode: "connection_verified",
    });
    const view = render(liveSetup("codex_desktop"));
    await tick();
    fireEvent.click(screen.getAllByRole("button", { name: "安装并接入" })[0]);
    await tick();
    fireEvent.click(screen.getByRole("button", { name: "检查安装并继续" }));
    await tick();
    await tick();
    expect(activate).toHaveBeenCalledTimes(1);
    expect(activate).toHaveBeenCalledWith(
      expect.objectContaining({
        installationJobId: "windows-exact",
        installationId: "i0000000000000001",
        modelId: "model-a",
        billingGroup: "default",
        lineId: "mainland_optimized",
      }),
    );
    view.rerender(liveSetup("codex_desktop"));
    await tick();
    expect(activate).toHaveBeenCalledTimes(1);
  });

  it.each(["installed", "failed", "cancelled"] as const)(
    "enables the selected missing app after the other job becomes %s",
    async (terminal) => {
      vi.useFakeTimers();
      let job: InstallationProgress | null = null;
      native.mockImplementation(async (command, args) => {
        if (command !== "manage_app_installation_v2") return;
        const r = (
          args as {
            request: {
              requestId: string;
              toolId: ActivationToolId;
              action: string;
            };
          }
        ).request;
        if (r.action === "start")
          job = progress({
            toolId: r.toolId,
            jobId: "other-job",
            phase: "downloading",
            canCancel: true,
          });
        const snapshot =
          job &&
          (job.toolId === r.toolId ||
            !["installed", "failed", "cancelled"].includes(job.phase))
            ? job
            : progress({ toolId: r.toolId });
        return { ...snapshot, requestId: r.requestId };
      });
      const view = render(liveSetup("claude_desktop"));
      await tick();
      fireEvent.click(
        screen.getAllByRole("button", { name: /^(先安装应用|安装并接入)$/ })[0],
      );
      await tick();
      view.rerender(liveSetup("codex_desktop"));
      await tick();
      expect(screen.getByText("另一个应用正在安装")).toBeInTheDocument();
      job = progress({
        toolId: "claude_desktop",
        jobId: "other-job",
        phase: terminal,
        disposition: terminal === "installed" ? "created" : "none",
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1200);
      });
      await tick();
      expect(screen.queryByText("另一个应用正在安装")).not.toBeInTheDocument();
      expect(
        screen.getAllByRole("button", { name: "安装并接入" })[0],
      ).toBeEnabled();
      const requests = native.mock.calls
        .filter(([c]) => c === "manage_app_installation_v2")
        .map(
          ([, a]) =>
            (a as { request: { action: string; toolId: string } }).request,
        );
      expect(requests.filter((r) => r.action === "start")).toHaveLength(1);
      expect(requests.at(-1)).toMatchObject({
        action: "inspect",
        toolId: "codex_desktop",
      });
      expect(activate).not.toHaveBeenCalled();
      fireEvent.click(
        screen.getByRole("button", { name: "我已安装，重新检查" }),
      );
      await tick();
      expect(native.mock.calls.at(-1)?.[0]).toBe("manage_app_installation_v2");
      expect(activate).not.toHaveBeenCalled();
    },
  );

  it.each([false, true])(
    "checks Windows installation without login, including a retired earlier consent (initial login %s)",
    async (initialLogin) => {
      const signedOut = {
        ...account,
        projection: {
          ...projection,
          status: "signed_out" as const,
          models: [],
        },
      };
      let job = progress({ platform: "windows", mode: "system_assisted" });
      let confirmations = 0;
      native.mockImplementation(async (command, args) => {
        if (command !== "manage_app_installation_v2") return;
        const r = (
          args as {
            request: {
              requestId: string;
              toolId: ActivationToolId;
              action: string;
              intent?: unknown;
            };
          }
        ).request;
        if (r.action === "start") {
          if (initialLogin)
            expect(r.intent).toMatchObject({ modelId: "model-a" });
          else expect(r.intent).toBeUndefined();
          job = {
            ...job,
            toolId: r.toolId,
            jobId: "windows-job",
            phase: "awaiting_system_confirmation",
          };
        }
        if (r.action === "confirm") {
          expect(r.intent).toBeUndefined();
          confirmations++;
          if (confirmations === 1)
            job = { ...job, reasonCode: "installation_not_detected" };
          else {
            scan.mockResolvedValue(targets(1));
            job = {
              ...job,
              phase: "installed",
              disposition: "confirmed",
              installationId: "i0000000000000001",
              reasonCode: "none",
            };
          }
        }
        return { ...job, toolId: r.toolId, requestId: r.requestId };
      });
      const view = render(
        liveSetup("codex_desktop", initialLogin ? account : signedOut),
      );
      await tick();
      fireEvent.click(
        screen.getAllByRole("button", { name: /^(先安装应用|安装并接入)$/ })[0],
      );
      await tick();
      view.rerender(liveSetup("codex_desktop", signedOut));
      await tick();
      expect(
        screen.getByRole("button", { name: "检查安装结果" }),
      ).toBeEnabled();
      fireEvent.click(screen.getByRole("button", { name: "检查安装结果" }));
      await tick();
      expect(screen.getByRole("alert")).toHaveTextContent("还没找到装好的应用");
      fireEvent.click(screen.getByRole("button", { name: "检查安装结果" }));
      await tick();
      await tick();
      expect(confirmations).toBe(2);
      expect(activate).not.toHaveBeenCalled();
      view.rerender(liveSetup("codex_desktop", account));
      await tick();
      expect(activate).not.toHaveBeenCalled();
    },
  );

  it("ignores an older inspection reply after the selected app is ready", async () => {
    let resolveOld: (value: unknown) => void = () => {};
    let oldId = "";
    native.mockImplementation(async (_command, args) => {
      const r = (
        args as {
          request: {
            toolId: ActivationToolId;
            requestId: string;
            action: string;
          };
        }
      ).request;
      expect(r.action).toBe("inspect");
      if (r.toolId === "claude_desktop") {
        oldId = r.requestId;
        return new Promise((resolve) => {
          resolveOld = resolve;
        });
      }
      return progress({ toolId: r.toolId, requestId: r.requestId });
    });
    const hook = renderHook(() => useInstallation(), {
      wrapper: InstallationProvider,
    });
    await tick();
    await act(async () => {
      await hook.result.current!.run("codex_desktop", "inspect");
    });
    await act(async () => {
      resolveOld(progress({ toolId: "claude_desktop", requestId: oldId }));
    });
    expect(hook.result.current?.progress?.toolId).toBe("codex_desktop");
    expect(hook.result.current?.working).toBe(false);
  });
});

describe("ru048 novice installer", () => {
  it("offers desktop installation on an empty computer without hiding it behind filters", async () => {
    render(
      <ConnectionProvider value={connections}>
        <AppLibraryView
          accountSession={account}
          onOpenAccount={noop}
          onOpenSetup={noop}
          onOpenCommunity={noop}
        />
      </ConnectionProvider>,
    );
    await tick();
    expect(screen.getAllByRole("button", { name: /安装并接入/ })).toHaveLength(
      2,
    );
    expect(screen.getByText("Claude Desktop")).toBeInTheDocument();
    expect(screen.getByText("Codex Desktop")).toBeInTheDocument();
  });
  it("automatically continues once only for the exact newly installed target", async () => {
    const view = render(setup(progress()));
    await tick();
    fireEvent.click(screen.getAllByRole("button", { name: "安装并接入" })[0]);
    await tick();
    expect(run).toHaveBeenCalledWith(
      "codex_desktop",
      "start",
      expect.objectContaining({ modelId: "model-a", billingGroup: "default" }),
    );
    expect(activate).not.toHaveBeenCalled();
    scan.mockResolvedValue(targets(1));
    const completed = progress({
      jobId: "native-job",
      phase: "installed",
      disposition: "created",
      installationId: "i0000000000000001",
    });
    view.rerender(setup(completed));
    await tick();
    await tick();
    expect(activate).toHaveBeenCalledTimes(1);
    expect(activate).toHaveBeenCalledWith(
      expect.objectContaining({
        installationJobId: "native-job",
        installationId: "i0000000000000001",
        modelId: "model-a",
        billingGroup: "default",
        lineId: "mainland_optimized",
      }),
    );
    view.rerender(setup({ ...completed }));
    await tick();
    expect(activate).toHaveBeenCalledTimes(1);
  });
  it.each(["line ABA", "navigation", "existing", "ambiguous"])(
    "does not silently continue after %s",
    async (reason) => {
      const view = render(setup(progress()));
      await tick();
      fireEvent.click(screen.getAllByRole("button", { name: "安装并接入" })[0]);
      await tick();
      const downloading = progress({
        jobId: "native-job",
        phase: "downloading",
        canCancel: true,
      });
      if (reason === "line ABA") {
        view.rerender(setup(downloading, "global_accelerated"));
        await tick();
        view.rerender(setup(downloading));
        await tick();
      }
      if (reason === "navigation") {
        view.rerender(setup(downloading, "mainland_optimized", false));
        await tick();
        view.rerender(setup(downloading));
        await tick();
      }
      scan.mockResolvedValue(targets(reason === "ambiguous" ? 2 : 1));
      view.rerender(
        setup(
          progress({
            jobId: "native-job",
            phase: "installed",
            disposition: reason === "existing" ? "existing" : "created",
            installationId: reason === "existing" ? "" : "i0000000000000001",
          }),
        ),
      );
      await tick();
      await tick();
      expect(activate).not.toHaveBeenCalled();
    },
  );
  it("explains Windows system confirmation, cancellation and unsigned-package failures", () => {
    const onConfirm = vi.fn();
    const onStart = vi.fn();
    const view = render(
      <InstallationContext.Provider
        value={{
          progress: progress({
            mode: "system_assisted",
            platform: "windows",
            phase: "awaiting_system_confirmation",
            jobId: "job",
          }),
          working: false,
          error: false,
          run,
        }}
      >
        <InstallationPanel
          tool="codex_desktop"
          name="Codex"
          canConnect
          onConfirm={onConfirm}
          onStart={onStart}
          onRefresh={noop}
        />
      </InstallationContext.Provider>,
    );
    expect(
      screen.getByText(/如果出现“重新安装”或“替换”，请取消/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/^接入成功$/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "检查安装并继续" }));
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onStart).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /结束本次引导/ }));
    expect(run).toHaveBeenCalledWith("codex_desktop", "cancel");
    view.rerender(
      <InstallationContext.Provider
        value={{
          progress: progress({
            phase: "failed",
            jobId: "job",
            reasonCode: "signature_invalid",
          }),
          working: false,
          error: false,
          run,
        }}
      >
        <InstallationPanel
          tool="codex_desktop"
          name="Codex"
          canConnect
          onConfirm={onConfirm}
          onStart={onStart}
          onRefresh={noop}
        />
      </InstallationContext.Provider>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("安装包未通过检查");
  });
  it("keeps an accessible native progress notice on other pages", () => {
    render(
      <InstallationContext.Provider
        value={{
          progress: progress({
            phase: "downloading",
            jobId: "job",
            downloadedBytes: 40,
            totalBytes: 100,
            canCancel: true,
          }),
          working: false,
          error: false,
          run,
        }}
      >
        <InstallationNotice onOpen={noop} />
      </InstallationContext.Provider>,
    );
    fireEvent.click(screen.getByRole("button", { name: /查看进度/ }));
    expect(noop).toHaveBeenCalledWith("codex_desktop");
  });
  it("rejects untrusted projection fields and tracks A to B to A as new consent", () => {
    expect(decodeInstallation(progress(), "fixture")).not.toBeNull();
    for (const patch of [
      { token: "secret" },
      { downloadedBytes: 200, totalBytes: 100 },
      { phase: "installed", canCancel: true },
      { installationId: "../../path" },
    ]) {
      expect(
        decodeInstallation({ ...progress(), ...patch }, "fixture"),
      ).toBeNull();
    }
    const revision = new SelectionRevision();
    const a = revision.observe("A");
    expect(revision.observe("A")).toBe(a);
    revision.observe("B");
    expect(revision.observe("A")).toBeGreaterThan(a);
  });
  it("recovers a lost start response by inspecting without a second start", async () => {
    native.mockImplementation(async (_command, args) => {
      const request = (
        args as { request: { requestId: string; action: string } }
      ).request;
      if (request.action === "start") throw new Error("lost reply");
      return progress({
        requestId: request.requestId,
        jobId: "owned-job",
        phase: "downloading",
        canCancel: true,
      });
    });
    const hook = renderHook(() => useInstallation(), {
      wrapper: InstallationProvider,
    });
    await tick();
    await act(async () => {
      await hook.result.current!.run("codex_desktop", "start");
    });
    expect(
      native.mock.calls.filter(
        ([, args]) =>
          (args as { request: { action: string } }).request.action === "start",
      ),
    ).toHaveLength(1);
    expect(hook.result.current?.progress?.jobId).toBe("owned-job");
    expect(hook.result.current?.error).toBe(false);
  });
  it("continues observing after repeated polling errors and never relaunches", async () => {
    vi.useFakeTimers();
    let calls = 0;
    native.mockImplementation(async (_command, args) => {
      const request = (
        args as { request: { requestId: string; action: string } }
      ).request;
      expect(request.action).toBe("inspect");
      calls++;
      if (calls === 2 || calls === 3) throw new Error("offline");
      return progress({
        requestId: request.requestId,
        jobId: "owned-job",
        phase: calls > 3 ? "awaiting_system_confirmation" : "downloading",
        canCancel: calls <= 3,
      });
    });
    const hook = renderHook(() => useInstallation(), {
      wrapper: InstallationProvider,
    });
    await tick();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(calls).toBeGreaterThanOrEqual(4);
    expect(hook.result.current?.error).toBe(false);
    expect(hook.result.current?.progress?.phase).toBe(
      "awaiting_system_confirmation",
    );
  });
});
