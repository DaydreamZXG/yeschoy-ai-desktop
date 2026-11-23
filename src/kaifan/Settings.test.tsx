import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import type {
  ConnectionResponse,
  ToolConnection,
} from "../configuration/connections";
import { idleUpdateProjection } from "../update/contract";
import { useDesktopUpdate } from "../update/UpdateProvider";
import { reportOf } from "./checkup";
import { Settings, type Checkup } from "./Settings";
import { isMuted, setMuted } from "./sound";

vi.mock("../update/UpdateProvider", () => ({ useDesktopUpdate: vi.fn() }));
const update = vi.mocked(useDesktopUpdate);
const updater = (projection = {}) => ({
  projection: { ...idleUpdateProjection(), ...projection },
  attention: false,
  working: false,
  check: vi.fn(),
  install: vi.fn(),
});

const app = (
  toolId: ToolConnection["toolId"],
  extra: Partial<ToolConnection> = {},
): ToolConnection => ({
  toolId,
  state: "connected",
  modelId: "m",
  lineId: "",
  billingGroup: "",
  updatedAtEpochMs: 0,
  restoreMode: "original",
  requiresBackground: false,
  reasonCode: "",
  ...extra,
});
const reply = (
  extra: Partial<ConnectionResponse> = {},
): ConnectionResponse => ({
  requestId: "r",
  schemaVersion: 2,
  status: "restored",
  connections: [],
  reasonCode: "",
  ...extra,
});
const idleCheck: Checkup = { running: false, report: null, run: vi.fn() };

type Props = Parameters<typeof Settings>[0];
const show = (props: Partial<Props> = {}) => {
  const all: Props = {
    open: true,
    who: "gpt",
    reach: "ok",
    host: "a.example",
    viaKey: false,
    connections: [],
    restoring: null,
    appName: (id) => `应用:${id}`,
    checkup: idleCheck,
    onConnect: vi.fn(),
    onLogout: vi.fn(async () => true),
    onRestore: vi.fn(async () => reply()),
    onRepair: vi.fn(),
    onJoin: vi.fn(),
    onClose: vi.fn(),
    ...props,
  };
  return { ...render(<Settings {...all} />), props: all };
};
const says = () => screen.getByTestId("kaifan-settings-says");
/** 她是一个字一个字说的，等她说到这一句。 */
const said = (text: string) =>
  waitFor(() => expect(says()).toHaveTextContent(text), { timeout: 8000 });

beforeEach(() => {
  localStorage.clear();
  update.mockReturnValue(
    updater({ phase: "current", currentVersion: "0.6.0" }),
  );
});

describe("Settings", () => {
  it("说清连的是哪个站、怎么连的，能换站点", () => {
    const { props } = show({ viaKey: true });
    const row = screen.getByTestId("kaifan-settings-station");
    expect(row).toHaveTextContent("a.example");
    expect(row).toHaveTextContent("粘贴密钥连的");
    fireEvent.click(screen.getByText("换站点"));
    expect(props.onConnect).toHaveBeenCalled();
  });

  it("没连站点：只给连接站点，不给退出和体检", () => {
    show({ reach: "out", host: null });
    expect(screen.getByTestId("kaifan-settings-station")).toHaveTextContent(
      "还没连站点",
    );
    expect(screen.queryByTestId("kaifan-settings-logout")).toBeNull();
    expect(screen.queryByTestId("kaifan-settings-check")).toBeNull();
  });

  it("退出登录先问一句，说清后果，点了确定才退", async () => {
    const { props } = show();
    fireEvent.click(screen.getByTestId("kaifan-settings-logout"));
    expect(props.onLogout).not.toHaveBeenCalled();
    await said("接好的应用我不会去动");
    await act(async () => {
      fireEvent.click(screen.getByTestId("kaifan-settings-logout-yes"));
    });
    expect(props.onLogout).toHaveBeenCalledTimes(1);
    await said("退好啦");
  });

  it("退出问了之后点算了，什么都不做", () => {
    const { props } = show();
    fireEvent.click(screen.getByTestId("kaifan-settings-logout"));
    fireEvent.click(screen.getByText("算了"));
    expect(props.onLogout).not.toHaveBeenCalled();
    expect(screen.getByTestId("kaifan-settings-logout")).toBeInTheDocument();
  });

  it("没退出成会说出来", async () => {
    show({ onLogout: vi.fn(async () => false) });
    fireEvent.click(screen.getByTestId("kaifan-settings-logout"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("kaifan-settings-logout-yes"));
    });
    await said("刚才没退成");
  });

  it("声音开关记在本机", () => {
    show();
    const toggle = screen.getByTestId("kaifan-settings-sound");
    expect(toggle).toHaveAttribute("aria-checked", "true");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(isMuted()).toBe(true);
    fireEvent.click(toggle);
    expect(isMuted()).toBe(false);
    setMuted(false);
  });

  it("把应用改回原样：先问，再做，做完说结果", async () => {
    const onRestore = vi.fn(async () => reply());
    show({ connections: [app("claude_code")], onRestore });
    const row = screen.getByTestId("kaifan-settings-app-claude_code");
    fireEvent.click(row.querySelector("button")!);
    expect(onRestore).not.toHaveBeenCalled();
    await said("删掉给它的那把密钥");
    await act(async () => {
      fireEvent.click(screen.getByText("确定改回"));
    });
    expect(onRestore).toHaveBeenCalledWith("claude_code");
    await said("改回接入之前的样子了");
  });

  it("粘贴密钥接的应用：改回时不说会删密钥，说清要作废得去站点后台", async () => {
    show({
      connections: [
        app("claude_code", { lineId: "station_key:https://a.example" }),
      ],
    });
    const row = screen.getByTestId("kaifan-settings-app-claude_code");
    fireEvent.click(row.querySelector("button")!);
    await said("我不会去删它");
    expect(screen.getByTestId("kaifan-settings-says")).not.toHaveTextContent(
      "并删掉给它的那把密钥",
    );
  });

  it("体检：应用用的型号没有了，说是哪个应用，按钮带去换人", async () => {
    const gone = show({
      checkup: {
        ...idleCheck,
        report: reportOf({
          online: true,
          probe: { ok: true as const, probe: {} as never },
          status: "signed_in" as const,
          connections: [app("pi")],
          connectionsFailed: false,
          models: [{ id: "other" }],
        }),
      },
    });
    await said("应用:pi 用的型号这个站现在没有了");
    expect(screen.getByTestId("kaifan-settings-report")).toHaveTextContent(
      "1 个应用用的型号没有了",
    );
    fireEvent.click(screen.getByTestId("kaifan-settings-check-act"));
    expect(gone.props.onRepair).toHaveBeenCalledWith("pi");
  });

  it("改回的几种结果各说各的，不都说成功", async () => {
    const cases: [Partial<ConnectionResponse> | Error, string][] = [
      [{ status: "recovery_failed" }, "没改回成"],
      [
        { reasonCode: "local_settings_restored_token_cleanup_pending" },
        "密钥还没删掉",
      ],
      [{ status: "restored_with_changes" }, "你后来自己改的那些我没动"],
      [new Error("busy"), "这次没改成"],
    ];
    for (const [result, text] of cases) {
      const onRestore = vi.fn(async () => {
        if (result instanceof Error) throw result;
        return reply(result);
      });
      const view = show({ connections: [app("pi")], onRestore });
      fireEvent.click(
        screen.getByTestId("kaifan-settings-app-pi").querySelector("button")!,
      );
      await act(async () => {
        fireEvent.click(screen.getByText("确定改回"));
      });
      await said(text);
      view.unmount();
    }
  });

  it("旧版本接的应用说的是撤掉，并说明原设置没存下来", async () => {
    show({
      connections: [app("codex_desktop", { restoreMode: "remove_yeschoy" })],
    });
    fireEvent.click(screen.getByText("撤掉接入"));
    await said("原来的设置没存下来");
  });

  it("没接过、或者没东西可改回的应用不列出来", () => {
    show({
      connections: [
        app("pi", { state: "not_connected" }),
        app("opencode", { restoreMode: "none" }),
      ],
    });
    expect(screen.queryByTestId("kaifan-settings-app-pi")).toBeNull();
    expect(screen.queryByTestId("kaifan-settings-app-opencode")).toBeNull();
  });

  it("体检：点一下开始，查的时候按钮不能再点", () => {
    const run = vi.fn();
    const view = show({ checkup: { running: false, report: null, run } });
    fireEvent.click(screen.getByTestId("kaifan-settings-check"));
    expect(run).toHaveBeenCalledTimes(1);
    view.unmount();
    show({ checkup: { running: true, report: null, run } });
    expect(screen.getByTestId("kaifan-settings-check")).toBeDisabled();
    expect(screen.queryByTestId("kaifan-settings-report")).toBeNull();
  });

  it("体检说出是哪一环的问题，并给对应的按钮", async () => {
    const base = {
      online: true,
      probe: { ok: true as const, probe: {} as never },
      status: "signed_in" as const,
      connections: [app("claude_code")],
      connectionsFailed: false,
    };
    const expired = show({
      checkup: {
        ...idleCheck,
        report: reportOf({ ...base, status: "session_expired" }),
      },
    });
    await said("登录失效了");
    fireEvent.click(screen.getByTestId("kaifan-settings-check-act"));
    expect(expired.props.onConnect).toHaveBeenCalled();
    expired.unmount();

    const broken = show({
      checkup: {
        ...idleCheck,
        report: reportOf({
          ...base,
          connections: [app("pi", { state: "changed" })],
        }),
      },
    });
    await said("应用:pi 的设置要修一下");
    expect(screen.getByTestId("kaifan-settings-check-act")).toHaveTextContent(
      "去修 应用:pi",
    );
    fireEvent.click(screen.getByTestId("kaifan-settings-check-act"));
    expect(broken.props.onRepair).toHaveBeenCalledWith("pi");
    broken.unmount();

    const down = show({
      checkup: {
        ...idleCheck,
        report: reportOf({
          ...base,
          probe: { ok: false, reason: "timeout" },
        }),
      },
    });
    await said("a.example 连不上");
    expect(screen.getByTestId("kaifan-settings-report")).toHaveTextContent(
      "站点通了才能看",
    );
    fireEvent.click(screen.getByTestId("kaifan-settings-check-act"));
    expect(down.props.checkup.run).toHaveBeenCalled();
    down.unmount();

    show({ checkup: { ...idleCheck, report: reportOf(base) } });
    await said("都妥妥的");
    expect(screen.queryByTestId("kaifan-settings-check-act")).toBeNull();
  });

  it("有的应用没读到：不说都没问题", async () => {
    show({
      checkup: {
        ...idleCheck,
        report: reportOf({
          online: true,
          probe: { ok: true, probe: {} as never },
          status: "signed_in",
          connections: [
            app("claude_code"),
            app("pi", { state: "unavailable" }),
          ],
          connectionsFailed: false,
        }),
      },
    });
    await said("有应用的情况这次没读到");
    expect(screen.getByTestId("kaifan-settings-report")).toHaveTextContent(
      "1 个应用接好了，还有的这次没读到",
    );
  });

  it("体检查出登录失效、首页变成没连站点：结果和重新连接还在", async () => {
    const { props } = show({
      reach: "out",
      host: null,
      checkup: {
        ...idleCheck,
        report: reportOf({
          online: true,
          probe: { ok: true, probe: {} as never },
          status: "session_expired",
          connections: [],
          connectionsFailed: false,
        }),
      },
    });
    await said("登录失效了");
    expect(screen.getByTestId("kaifan-settings-report")).toHaveTextContent(
      "登录失效了",
    );
    fireEvent.click(screen.getByTestId("kaifan-settings-check-act"));
    expect(props.onConnect).toHaveBeenCalled();
  });

  it("体检结果能复制给帮忙的人：只有状态词和应用代号，没有地址和账户", async () => {
    const writeText = vi.fn(async (_text: string) => undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    show({
      host: "secret.example",
      connections: [app("claude_code", { state: "changed" }), app("pi")],
      checkup: {
        ...idleCheck,
        report: reportOf({
          online: true,
          probe: { ok: true as const, probe: {} as never },
          status: "signed_in",
          connections: [app("claude_code", { state: "changed" }), app("pi")],
          connectionsFailed: false,
        }),
      },
    });
    fireEvent.click(screen.getByTestId("kaifan-settings-check-copy"));
    await waitFor(() => expect(writeText).toHaveBeenCalled());
    const text = writeText.mock.calls[0][0];
    expect(text).toContain("version: 0.6.0");
    expect(text).toContain("claude_code: changed");
    expect(text).toContain("verdict: repair");
    expect(text).not.toContain("secret.example");
    expect(await screen.findByText(/复制好了/)).toBeInTheDocument();
    writeText.mockRejectedValueOnce(new Error("denied"));
    fireEvent.click(screen.getByTestId("kaifan-settings-check-copy"));
    expect(await screen.findByText(/没复制成/)).toBeInTheDocument();
  });

  it("没体检过就没有结果可复制", () => {
    show();
    expect(screen.queryByTestId("kaifan-settings-check-copy")).toBeNull();
  });

  it("说清数据放在哪、会不会上传；退出客户端交给那一问去办", () => {
    const asked = vi.fn();
    window.addEventListener("kaifan:open-exit-choice", asked);
    show();
    expect(screen.getByTestId("kaifan-settings-privacy")).toHaveTextContent(
      "密码库",
    );
    fireEvent.click(
      within(screen.getByTestId("kaifan-settings-quit")).getByRole("button"),
    );
    expect(asked).toHaveBeenCalledOnce();
    window.removeEventListener("kaifan:open-exit-choice", asked);
  });

  it("更新：有新版本给安装，查不到给再试和官网下载", () => {
    const ready = updater({
      phase: "available",
      currentVersion: "0.6.0",
      availableVersion: "0.7.0",
    });
    update.mockReturnValue(ready);
    const view = show();
    const row = screen.getByTestId("kaifan-settings-update");
    expect(row).toHaveTextContent("版本 0.6.0");
    expect(row).toHaveTextContent("有新版本 0.7.0");
    fireEvent.click(screen.getByText("安装并重启"));
    expect(ready.install).toHaveBeenCalled();
    view.unmount();

    const stuck = updater({ phase: "unavailable" });
    update.mockReturnValue(stuck);
    show();
    expect(screen.getByTestId("kaifan-settings-update")).toHaveTextContent(
      "版本看不到",
    );
    fireEvent.click(screen.getByText("再试一次"));
    expect(stuck.check).toHaveBeenCalled();
    expect(screen.getByText("去官网下载最新版")).toHaveAttribute("href");
  });

  it("下载时显示进度", () => {
    update.mockReturnValue(
      updater({ phase: "downloading", downloadedBytes: 30, totalBytes: 120 }),
    );
    show();
    expect(screen.getByTestId("kaifan-settings-update")).toHaveTextContent(
      "25%",
    );
  });
});
