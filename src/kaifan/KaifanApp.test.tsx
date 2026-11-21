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
import { useDesktopUpdate } from "../update/UpdateProvider";
import { KaifanApp } from "./KaifanApp";

vi.mock("../account/useAccountSession", () => ({
  useAccountSession: vi.fn(),
}));
vi.mock("../configuration/connections", async (load) => ({
  ...(await load<typeof import("../configuration/connections")>()),
  useToolConnections: () => ({
    connections: [],
    error: null,
    refresh: vi.fn(),
  }),
}));
vi.mock("../update/UpdateProvider", () => ({ useDesktopUpdate: vi.fn() }));
vi.mock("../uiChoice", () => ({ RETURNING: true, openClassic: vi.fn() }));
vi.mock("../model-profiles/remoteCatalog", () => ({
  refreshModelCatalog: () => Promise.resolve(),
}));
vi.mock("../settings/QuitAssistant", () => ({ ShutdownHost: () => null }));
vi.mock("./useStationLink", () => ({
  useStationLink: () => ({ trustOrigin: null, answerTrust: vi.fn() }),
}));
vi.mock("./Fx", async () => {
  const { forwardRef } = await import("react");
  return { Fx: forwardRef(() => null) };
});

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
  refresh.mockReset().mockResolvedValue(signedIn);
  update.mockReturnValue(idle);
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
      JSON.parse(localStorage.getItem("kaifan-card-picks")!).model.gpt,
    ).toBe("gpt-5-mini");

    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    await waitFor(() => expect(card).toHaveAttribute("inert"));
    expect(screen.getByTestId("kaifan-model")).toHaveTextContent("gpt-5-mini");

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

  it("没有新版本：不提这件事", () => {
    render(<KaifanApp />);
    expect(screen.queryByTitle("有新版本")).toBeNull();
  });
});
