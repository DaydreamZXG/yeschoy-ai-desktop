import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountProjection } from "../account/session";
import { STATION_CONNECT_PROGRESS_EVENT } from "../account/stationConnect";
import type { AccountLineId } from "../configuration/preview";
import { useStationLink } from "./useStationLink";

const bus = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    bus.handlers.set(name, handler);
    return Promise.resolve(() => bus.handlers.delete(name));
  },
  emit: () => Promise.resolve(),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => bus.invoke(...args),
}));

const NEW = "station_connect:https://new.example.com" as AccountLineId;
const signedIn = { status: "signed_in" } as AccountProjection;

const send = async (payload: Record<string, unknown>) => {
  await waitFor(() =>
    expect(bus.handlers.has(STATION_CONNECT_PROGRESS_EVENT)).toBe(true),
  );
  act(() => bus.handlers.get(STATION_CONNECT_PROGRESS_EVENT)?.({ payload }));
};

beforeEach(() => {
  bus.handlers.clear();
  bus.invoke.mockReset();
});

describe("浏览器那边授权完了", () => {
  it("先把线路换过去，换好了才读账户、才报连上了", async () => {
    const onEvent = vi.fn();
    const onLineChange = vi.fn();
    const oldRefresh = vi.fn().mockResolvedValue(signedIn);
    const newRefresh = vi.fn().mockResolvedValue(signedIn);
    const { rerender } = renderHook(
      (props: { lineId: AccountLineId; refresh: typeof oldRefresh }) =>
        useStationLink({ ...props, onLineChange, onEvent }),
      {
        initialProps: {
          lineId: "mainland_optimized" as AccountLineId,
          refresh: oldRefresh,
        },
      },
    );
    await send({ phase: "signed_in", lineId: NEW, recoverable: false });
    expect(onLineChange).toHaveBeenCalledWith(NEW);
    // 线路还没真换过去：不拿原来那个站的账户来报。
    expect(oldRefresh).not.toHaveBeenCalled();
    expect(onEvent).not.toHaveBeenCalled();

    rerender({ lineId: NEW, refresh: newRefresh });
    await waitFor(() =>
      expect(onEvent).toHaveBeenCalledWith({
        phase: "signed_in",
        lineId: NEW,
      }),
    );
    expect(newRefresh).toHaveBeenCalledTimes(1);
    expect(oldRefresh).not.toHaveBeenCalled();
  });

  it("本来就在这个站：直接读一次", async () => {
    const onEvent = vi.fn();
    const refresh = vi.fn().mockResolvedValue({ status: "signed_out" });
    renderHook(() =>
      useStationLink({ lineId: NEW, refresh, onLineChange: vi.fn(), onEvent }),
    );
    await send({ phase: "signed_in", lineId: NEW, recoverable: false });
    await waitFor(() =>
      expect(onEvent).toHaveBeenCalledWith({
        phase: "failed",
        reasonCode: "signed_out",
      }),
    );
  });
});

describe("第一次见的站点", () => {
  const ask = async () => {
    const view = renderHook(() =>
      useStationLink({
        lineId: "mainland_optimized",
        refresh: vi.fn(),
        onLineChange: vi.fn(),
        onEvent: vi.fn(),
      }),
    );
    await send({
      phase: "confirm_required",
      origin: "https://new.example.com",
      recoverable: true,
    });
    expect(view.result.current.trustOrigin).toBe("https://new.example.com");
    return view;
  };

  it("本机后台没回话：问话留着，可以再答一次", async () => {
    bus.invoke.mockRejectedValueOnce(new Error("ipc down"));
    const { result } = await ask();
    await act(async () => {
      await result.current.answerTrust(true);
    });
    expect(result.current.trustOrigin).toBe("https://new.example.com");
    bus.invoke.mockResolvedValue(undefined);
    await act(async () => {
      await result.current.answerTrust(true);
    });
    expect(result.current.trustOrigin).toBeNull();
  });

  it("后台明说这次作废了：问话收起", async () => {
    bus.invoke.mockRejectedValueOnce("connect_code_expired");
    const { result } = await ask();
    let failure: string | null = null;
    await act(async () => {
      failure = await result.current.answerTrust(true);
    });
    expect(failure).toBe("connect_code_expired");
    expect(result.current.trustOrigin).toBeNull();
  });
});
