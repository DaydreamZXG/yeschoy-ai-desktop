import { act, render, screen, waitFor } from "@testing-library/react";
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

  it("没有新版本：不提这件事", () => {
    render(<KaifanApp />);
    expect(screen.queryByTitle("有新版本")).toBeNull();
  });
});
