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
import type { AccountProjection } from "../account/session";
import { useAccountSession } from "../account/useAccountSession";
import { idleUpdateProjection } from "../update/contract";
import { readAnnouncements } from "../workbench/announcements";
import { useDesktopUpdate } from "../update/UpdateProvider";
import { lookAtStation } from "./station";
import { KaifanApp } from "./KaifanApp";

vi.mock("../account/useAccountSession", () => ({
  useAccountSession: vi.fn(),
}));
const tools = vi.hoisted(() => ({
  now: {} as Record<string, unknown>,
}));
vi.mock("../configuration/connections", async (load) => ({
  ...(await load<typeof import("../configuration/connections")>()),
  useToolConnections: () => tools.now,
}));
vi.mock("../update/UpdateProvider", () => ({ useDesktopUpdate: vi.fn() }));
// 开机那一幕有自己的测试；这里直接到首页。
vi.mock("./Boot", async () => {
  const { useEffect } = await import("react");
  return {
    Boot: ({ onDone }: { onDone: () => void }) => {
      useEffect(() => onDone(), [onDone]);
      return null;
    },
  };
});
vi.mock("../model-profiles/remoteCatalog", () => ({
  refreshModelCatalog: () => Promise.resolve(),
}));
vi.mock("./Shutdown", () => ({
  ShutdownHost: () => null,
  askToQuit: vi.fn(),
}));
vi.mock("./useStationLink", () => ({
  useStationLink: () => ({ trustOrigin: null, answerTrust: vi.fn() }),
}));
vi.mock("../workbench/announcements", async (load) => ({
  ...(await load<typeof import("../workbench/announcements")>()),
  readAnnouncements: vi.fn(),
  followAnnouncement: vi.fn(),
}));
vi.mock("./station", async (load) => ({
  ...(await load<typeof import("./station")>()),
  lookAtStation: vi.fn(),
}));
vi.mock("./Fx", async () => {
  const { forwardRef } = await import("react");
  return { Fx: forwardRef(() => null) };
});

const board = vi.mocked(readAnnouncements);
const look = vi.mocked(lookAtStation);
const session = vi.mocked(useAccountSession);
const update = vi.mocked(useDesktopUpdate);
const refresh = vi.fn();

const signedIn = {
  status: "signed_in",
  models: [{ id: "gpt-5" }],
  money: {
    currency: "CNY",
    balanceAmount: "86.40",
    consumedAmount: "0",
    displayRate: "1",
  },
} as unknown as AccountProjection;

const idle = {
  projection: idleUpdateProjection(),
  attention: false,
  working: false,
  check: vi.fn(),
  install: vi.fn(),
};

beforeEach(() => {
  vi.useRealTimers();
  tools.now = { connections: [], error: null, refresh: vi.fn() };
  refresh.mockReset().mockResolvedValue(signedIn);
  update.mockReturnValue(idle);
  board.mockReset().mockResolvedValue({ available: false, notices: [] });
  session.mockReturnValue({
    projection: signedIn,
    loading: false,
    stationCheck: "ok",
    lastError: null,
    refresh,
  } as unknown as ReturnType<typeof useAccountSession>);
});

describe("开饭首页自己会做的事", () => {
  it("从别的窗口回来：自己再看一眼账户，但不连着看", () => {
    vi.useFakeTimers();
    render(<KaifanApp />);
    act(() => void window.dispatchEvent(new Event("focus")));
    expect(refresh).not.toHaveBeenCalled();

    act(() => void vi.advanceTimersByTime(6_000));
    act(() => void window.dispatchEvent(new Event("focus")));
    expect(refresh).toHaveBeenCalledTimes(1);

    act(() => void window.dispatchEvent(new Event("focus")));
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("没连站点时，回到窗口不去查账户", () => {
    vi.useFakeTimers();
    session.mockReturnValue({
      projection: { status: "signed_out" },
      loading: false,
      stationCheck: "idle",
      lastError: null,
      refresh,
    } as unknown as ReturnType<typeof useAccountSession>);
    render(<KaifanApp />);
    act(() => void vi.advanceTimersByTime(6_000));
    act(() => void window.dispatchEvent(new Event("focus")));
    expect(refresh).not.toHaveBeenCalled();
  });

  it("有新版本：角色说一声，设置上亮个点", async () => {
    update.mockReturnValue({
      ...idle,
      attention: true,
      projection: {
        ...idleUpdateProjection(),
        phase: "available",
        availableVersion: "9.9.9",
      },
    });
    render(<KaifanApp />);
    await waitFor(() =>
      expect(screen.getByTestId("kaifan-says").textContent).toContain("9.9.9"),
    );
    expect(screen.getByTitle("有新版本")).toBeInTheDocument();
  });

  it("打开接入：焦点进面板，下面的首页不再接键盘；按 Esc 关掉后焦点回到原来的按钮", async () => {
    const { container } = render(<KaifanApp />);
    const home = container.querySelector(".ui")!;
    await waitFor(() => expect(home).not.toHaveAttribute("inert"));
    const opener = screen.getByRole("button", { name: /接入新应用/ });
    opener.focus();
    fireEvent.click(opener);
    const join = screen.getByTestId("kaifan-join");
    await waitFor(() =>
      expect(join).toContainElement(document.activeElement as HTMLElement),
    );
    expect(home).toHaveAttribute("inert");
    expect(container.querySelector(".roster")).toHaveAttribute("inert");
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    await waitFor(() => expect(home).not.toHaveAttribute("inert"));
    expect(opener).toHaveFocus();
  });

  it("再点一下当前这位：打开她的名片；挑的型号记住，首页和接入都用它", async () => {
    localStorage.clear();
    session.mockReturnValue({
      projection: {
        ...signedIn,
        models: [{ id: "gpt-5" }, { id: "gpt-5-mini" }],
      },
      loading: false,
      stationCheck: "ok",
      lastError: null,
      refresh,
    } as unknown as ReturnType<typeof useAccountSession>);
    const { container } = render(<KaifanApp />);
    const home = container.querySelector(".ui")!;
    await waitFor(() => expect(home).not.toHaveAttribute("inert"));
    const card = screen.getByTestId("kaifan-card");
    expect(card).toHaveAttribute("inert");

    fireEvent.click(container.querySelector(".roster [aria-pressed='true']")!);
    await waitFor(() => expect(card).not.toHaveAttribute("inert"));
    expect(home).toHaveAttribute("inert");
    fireEvent.click(
      within(card).getByRole("button", { name: "GPT-5 Mini", pressed: false }),
    );
    expect(
      JSON.parse(localStorage.getItem("kaifan-card-picks")!)[""].model.gpt,
    ).toBe("gpt-5-mini");

    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    await waitFor(() => expect(card).toHaveAttribute("inert"));
    // 首页写的名字和名片上一样；原始的型号 ID 悬停时看得到。
    expect(screen.getByTestId("kaifan-model")).toHaveTextContent("GPT-5 Mini");
    expect(screen.getByTestId("kaifan-model")).toHaveAttribute(
      "title",
      "gpt-5-mini",
    );

    // 点型号也能打开名片；从名片去接入，带的是刚挑的型号。
    fireEvent.click(screen.getByTestId("kaifan-model"));
    await waitFor(() => expect(card).not.toHaveAttribute("inert"));
    fireEvent.click(screen.getByTestId("kaifan-card-join"));
    await waitFor(() => expect(card).toHaveAttribute("inert"));
    expect(screen.getByTestId("kaifan-join")).not.toHaveAttribute("inert");
    expect(screen.getByLabelText("型号")).toHaveValue("gpt-5-mini");
  });

  it("开着名片时站点掉线：名片收起，首页还能点，不会被一直盖着", async () => {
    const online = {
      projection: { ...signedIn, models: [{ id: "gpt-5" }] },
      loading: false,
      stationCheck: "ok",
      lastError: null,
      refresh,
    } as unknown as ReturnType<typeof useAccountSession>;
    session.mockReturnValue(online);
    const { container, rerender } = render(<KaifanApp />);
    const home = container.querySelector(".ui")!;
    await waitFor(() => expect(home).not.toHaveAttribute("inert"));
    fireEvent.click(screen.getByTestId("kaifan-model"));
    const card = screen.getByTestId("kaifan-card");
    await waitFor(() => expect(card).not.toHaveAttribute("inert"));

    session.mockReturnValue({
      projection: null,
      loading: false,
      stationCheck: "unreachable",
      lastError: "account_http_failed",
      refresh,
    } as unknown as ReturnType<typeof useAccountSession>);
    rerender(<KaifanApp />);
    await waitFor(() => expect(card).toHaveAttribute("inert"));
    expect(home).not.toHaveAttribute("inert");

    // 站点回来了：名片不会自己又弹出来。
    session.mockReturnValue(online);
    rerender(<KaifanApp />);
    await waitFor(() =>
      expect(screen.getByTestId("kaifan-model")).toBeInTheDocument(),
    );
    expect(card).toHaveAttribute("inert");
    expect(home).not.toHaveAttribute("inert");
  });

  it("点「账本」：在新界面里滑出来，读一次最新的数，不去旧页面", async () => {
    const { container } = render(<KaifanApp />);
    const home = container.querySelector(".ui")!;
    await waitFor(() => expect(home).not.toHaveAttribute("inert"));
    const ledger = screen.getByTestId("kaifan-ledger");
    expect(ledger).toHaveAttribute("inert");
    refresh.mockClear();
    let done = () => {};
    refresh.mockReturnValueOnce(new Promise<void>((ok) => (done = ok)));

    fireEvent.click(screen.getByTestId("kaifan-nav-ledger"));
    await waitFor(() => expect(ledger).not.toHaveAttribute("inert"));
    expect(home).toHaveAttribute("inert");
    expect(refresh).toHaveBeenCalledTimes(1);
    // 最新的数还没读回来：手里上一次的余额不摆出来。
    expect(screen.queryByTestId("kaifan-ledger-balance")).toBeNull();
    await act(async () => done());
    expect(screen.getByTestId("kaifan-ledger-balance")).toHaveTextContent(
      "¥86.40",
    );

    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    await waitFor(() => expect(ledger).toHaveAttribute("inert"));
    expect(home).not.toHaveAttribute("inert");
  });

  it("点「设置」：在新界面里滑出来，不去旧页面；体检当场去探站点", async () => {
    look.mockResolvedValue({ ok: false, reason: "timeout" });
    render(<KaifanApp />);
    const settings = screen.getByTestId("kaifan-settings");
    expect(settings).toHaveAttribute("inert");
    fireEvent.click(screen.getByTestId("kaifan-nav-settings"));
    await waitFor(() => expect(settings).not.toHaveAttribute("inert"));

    refresh.mockClear();
    fireEvent.click(screen.getByTestId("kaifan-settings-check"));
    const report = await screen.findByTestId("kaifan-settings-report");
    expect(look).toHaveBeenCalledWith("https://yeschoy.com");
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(report).toHaveTextContent("站点连不上");
    expect(report).toHaveTextContent("站点通了才能看");
  });

  it("充值页没打开成：收起账本，让她在首页说怎么办", async () => {
    session.mockReturnValue({
      projection: signedIn,
      loading: false,
      stationCheck: "ok",
      lastError: null,
      refresh,
      openWallet: vi.fn(async () => false),
    } as unknown as ReturnType<typeof useAccountSession>);
    const { container } = render(<KaifanApp />);
    const home = container.querySelector(".ui")!;
    await waitFor(() => expect(home).not.toHaveAttribute("inert"));
    fireEvent.click(screen.getByTestId("kaifan-nav-ledger"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("kaifan-ledger-recharge"));
    });
    await waitFor(() =>
      expect(screen.getByTestId("kaifan-ledger")).toHaveAttribute("inert"),
    );
    expect(home).not.toHaveAttribute("inert");
  });

  it("有没看过的公告：「公告」上写几条；要提醒的那条由角色说一次；打开就算看过", async () => {
    localStorage.clear();
    board.mockResolvedValue({
      available: true,
      notices: [
        {
          id: "n1",
          title: "充值加赠",
          body: "充 100 送 20",
          severity: "info",
          publishedAtEpochMs: 1,
          expiresAtEpochMs: 0,
          banner: true,
          actionLabel: "",
        },
        {
          id: "n2",
          title: "线路维护",
          body: "",
          severity: "warning",
          publishedAtEpochMs: 1,
          expiresAtEpochMs: 0,
          banner: false,
          actionLabel: "",
        },
      ],
    });
    render(<KaifanApp />);
    const nav = screen.getByTestId("kaifan-nav-news");
    await waitFor(() => expect(nav).toHaveTextContent("2 条没看"));
    await waitFor(() =>
      expect(screen.getByTestId("kaifan-says")).toHaveTextContent(
        "站点有新公告：充值加赠。",
      ),
    );

    fireEvent.click(nav);
    const news = screen.getByTestId("kaifan-news");
    await waitFor(() => expect(news).not.toHaveAttribute("inert"));
    expect(within(news).getAllByTestId("kaifan-notice")).toHaveLength(2);
    await waitFor(() => expect(nav).not.toHaveTextContent("没看"));
  });

  it("没连站点：再点当前这位只是逗她一下，没有名片可看", async () => {
    session.mockReturnValue({
      projection: { status: "signed_out" },
      loading: false,
      stationCheck: "idle",
      lastError: null,
      refresh,
    } as unknown as ReturnType<typeof useAccountSession>);
    render(<KaifanApp />);
    expect(screen.queryByTestId("kaifan-model")).toBeNull();
    expect(screen.getByTestId("kaifan-card")).toHaveAttribute("inert");
  });

  it("站点连不上：网络一恢复就马上再试，不等那二十秒", async () => {
    session.mockReturnValue({
      projection: { status: "network_error" },
      loading: false,
      stationCheck: "unreachable",
      lastError: null,
      refresh,
    } as unknown as ReturnType<typeof useAccountSession>);
    render(<KaifanApp />);
    expect(refresh).not.toHaveBeenCalled();
    act(() => void window.dispatchEvent(new Event("online")));
    expect(refresh).toHaveBeenCalledTimes(1);
    // 这一次还没回来，再来一个事件也不叠着试。
    act(() => void window.dispatchEvent(new Event("online")));
    expect(refresh).toHaveBeenCalledTimes(1);
    await act(async () => undefined);
  });

  it("站点好好的时候，网络事件不去打扰它", () => {
    render(<KaifanApp />);
    act(() => void window.dispatchEvent(new Event("online")));
    expect(refresh).not.toHaveBeenCalled();
  });

  it("应用的情况没读到：自己再读，最多三次，不没完没了", () => {
    vi.useFakeTimers();
    const again = vi.fn();
    tools.now = { connections: [], error: true, refresh: again };
    render(<KaifanApp />);
    expect(again).not.toHaveBeenCalled();
    act(() => void vi.advanceTimersByTime(4_000));
    expect(again).toHaveBeenCalledTimes(1);
    act(() => void vi.advanceTimersByTime(60_000));
    expect(again).toHaveBeenCalledTimes(1);
  });

  it("重读的那一下错会先清掉：不当成读到了，三次的数不重新算", () => {
    vi.useFakeTimers();
    const again = vi.fn();
    const failed = { connections: [], error: true, refresh: again };
    tools.now = failed;
    const { rerender } = render(<KaifanApp />);
    for (const _ of [1, 2, 3, 4, 5]) {
      act(() => void vi.advanceTimersByTime(20_000));
      tools.now = {
        connections: [],
        error: null,
        refreshing: true,
        refresh: again,
      };
      rerender(<KaifanApp />);
      tools.now = { ...failed };
      rerender(<KaifanApp />);
    }
    act(() => void vi.advanceTimersByTime(60_000));
    expect(again).toHaveBeenCalledTimes(3);
  });

  it("在设置里自己点退出：留在设置里听那句「已经退出」，不弹出连接站点", async () => {
    const out = { status: "signed_out" } as unknown as AccountProjection;
    const logout = vi.fn(async () => {
      session.mockReturnValue({
        projection: out,
        loading: false,
        stationCheck: "idle",
        lastError: null,
        refresh,
        logout,
      } as unknown as ReturnType<typeof useAccountSession>);
      return out;
    });
    session.mockReturnValue({
      projection: signedIn,
      loading: false,
      stationCheck: "ok",
      lastError: null,
      refresh,
      logout,
    } as unknown as ReturnType<typeof useAccountSession>);
    const { rerender } = render(<KaifanApp />);
    fireEvent.click(screen.getByTestId("kaifan-nav-settings"));
    fireEvent.click(screen.getByTestId("kaifan-settings-logout"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("kaifan-settings-logout-yes"));
    });
    rerender(<KaifanApp />);
    expect(logout).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("kaifan-connect")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    expect(screen.getByTestId("kaifan-settings")).not.toHaveAttribute("inert");
  });

  it("要修的应用：点「去修好」在新界面里修，不去旧页面", async () => {
    tools.now = {
      connections: [
        {
          toolId: "pi",
          state: "recovery_pending",
          modelId: "gpt-5",
          lineId: "mainland_optimized",
          billingGroup: "default",
          updatedAtEpochMs: 1,
          restoreMode: "original",
          requiresBackground: false,
          reasonCode: "",
        },
      ],
      error: null,
      refresh: vi.fn(),
      restore: vi.fn(),
    };
    render(<KaifanApp />);
    fireEvent.click(await screen.findByTestId("kaifan-go"));
    const join = screen.getByTestId("kaifan-join");
    await waitFor(() => expect(join).not.toHaveAttribute("inert"));
    expect(within(join).getByTestId("kaifan-join-mend")).toHaveTextContent(
      "上次接到一半停了",
    );
  });

  it("没有新版本：不提这件事", () => {
    render(<KaifanApp />);
    expect(screen.queryByTitle("有新版本")).toBeNull();
  });
});
