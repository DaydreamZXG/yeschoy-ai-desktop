import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  followAnnouncement,
  readAnnouncements,
  type Notice,
} from "../workbench/announcements";
import { useNotices } from "./useNotices";

vi.mock("../workbench/announcements", async (load) => ({
  ...(await load<typeof import("../workbench/announcements")>()),
  readAnnouncements: vi.fn(),
  followAnnouncement: vi.fn(),
}));
const read = vi.mocked(readAnnouncements);
const follow = vi.mocked(followAnnouncement);

const notice = (id: string): Notice => ({
  id,
  title: id,
  body: "",
  severity: "info",
  publishedAtEpochMs: 1,
  expiresAtEpochMs: 0,
  banner: false,
  actionLabel: "去看看",
});

beforeEach(() => {
  localStorage.clear();
  read.mockReset();
  follow.mockReset().mockResolvedValue(true);
});

describe("useNotices", () => {
  it("没连站点不去读", () => {
    const { result } = renderHook(() => useNotices("line", false));
    expect(read).not.toHaveBeenCalled();
    expect(result.current.available).toBe(false);
  });

  it("读到的公告里没看过的算未读；看过就记在这台电脑上", async () => {
    read.mockResolvedValue({
      available: true,
      notices: [notice("a"), notice("b")],
    });
    const first = renderHook(() => useNotices("line", true));
    await waitFor(() => expect(first.result.current.unread.size).toBe(2));
    act(() => first.result.current.markSeen(["a"]));
    expect([...first.result.current.unread]).toEqual(["b"]);
    first.unmount();

    const again = renderHook(() => useNotices("line", true));
    await waitFor(() => expect(again.result.current.notices).toHaveLength(2));
    expect([...again.result.current.unread]).toEqual(["b"]);
  });

  it("读到过之后一次没读到：留着上次的，标成这次没读到，不说这个站没有公告栏", async () => {
    read.mockResolvedValueOnce({ available: true, notices: [notice("a")] });
    const { result } = renderHook(() => useNotices("line", true));
    await waitFor(() => expect(result.current.notices).toHaveLength(1));

    read.mockResolvedValueOnce({ available: false, notices: [] });
    act(() => result.current.reload());
    await waitFor(() => expect(result.current.failed).toBe(true));
    expect(result.current.available).toBe(true);
    expect(result.current.notices).toHaveLength(1);
  });

  it("换了站点：上一个站的公告不留，晚到的旧结果也不要", async () => {
    let late: (
      value: Awaited<ReturnType<typeof readAnnouncements>>,
    ) => void = () => undefined;
    read.mockImplementation((lineId) =>
      lineId === "old"
        ? new Promise((resolve) => {
            late = resolve;
          })
        : Promise.resolve({ available: true, notices: [notice("new-1")] }),
    );
    const { result, rerender } = renderHook(
      ({ line }) => useNotices(line, true),
      { initialProps: { line: "old" } },
    );
    rerender({ line: "new" });
    await waitFor(() =>
      expect(result.current.notices.map((item) => item.id)).toEqual(["new-1"]),
    );
    await act(async () => {
      late({ available: true, notices: [notice("old-1")] });
    });
    expect(result.current.notices.map((item) => item.id)).toEqual(["new-1"]);
  });

  it("点公告上的按钮：记成看过，结果照实返回", async () => {
    read.mockResolvedValue({ available: true, notices: [notice("a")] });
    follow.mockResolvedValue(false);
    const { result } = renderHook(() => useNotices("line", true));
    await waitFor(() => expect(result.current.unread.size).toBe(1));
    let opened = true;
    await act(async () => {
      opened = await result.current.follow(result.current.notices[0]);
    });
    expect(opened).toBe(false);
    expect(follow).toHaveBeenCalledWith("line", "a");
    expect(result.current.unread.size).toBe(0);
  });
});
