import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../i18n";
import type { Notice } from "../workbench/announcements";
import { News } from "./News";
import type { Notices } from "./useNotices";

const notice = (id: string, extra: Partial<Notice> = {}): Notice => ({
  id,
  title: `公告 ${id}`,
  body: "正文",
  severity: "info",
  publishedAtEpochMs: new Date(2026, 10, 20).getTime(),
  expiresAtEpochMs: 0,
  banner: false,
  actionLabel: "",
  ...extra,
});
const board = (extra: Partial<Notices> = {}): Notices => ({
  available: true,
  notices: [notice("a"), notice("b")],
  loading: false,
  failed: false,
  unread: new Set(["a"]),
  reload: vi.fn(),
  markSeen: vi.fn(),
  follow: vi.fn(async () => true),
  ...extra,
});
const show = (
  notices: Notices,
  props: Partial<Parameters<typeof News>[0]> = {},
) =>
  render(
    <News
      open
      who="gpt"
      reach="ok"
      notices={notices}
      onConnect={vi.fn()}
      onClose={vi.fn()}
      {...props}
    />,
  );
const says = () => screen.getByTestId("kaifan-news-says");

describe("News", () => {
  it("站点连不上时没摆出来的公告不算看过，连回来还是新的", () => {
    const notices = board();
    const { rerender } = show(notices, { reach: "off" });
    expect(screen.queryByTestId("kaifan-notice")).toBeNull();
    expect(notices.markSeen).not.toHaveBeenCalled();
    rerender(
      <News
        open
        who="gpt"
        reach="ok"
        notices={notices}
        onConnect={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(notices.markSeen).toHaveBeenCalledWith(["a"]);
    expect(screen.getAllByTestId("kaifan-notice")[0]).toHaveClass("new");
  });

  it("打开就算看过，但这次新到的一直标着「新」", async () => {
    const notices = board();
    const { rerender } = show(notices);
    expect(notices.markSeen).toHaveBeenCalledWith(["a"]);
    const cards = screen.getAllByTestId("kaifan-notice");
    expect(cards[0]).toHaveClass("new");
    expect(cards[1]).not.toHaveClass("new");
    await waitFor(() => expect(says()).toHaveTextContent("有 1 条新公告。"));

    // 记成看过之后，开着的这一次仍然标着。
    rerender(
      <News
        open
        who="gpt"
        reach="ok"
        notices={{ ...notices, unread: new Set() }}
        onConnect={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getAllByTestId("kaifan-notice")[0]).toHaveClass("new");
  });

  it("正文里的 **加粗** 显示成粗体，不露出星号；别的写法原样显示", () => {
    show(
      board({
        notices: [
          notice("a", {
            body: "今晚 **22:00** 维护。<b>不是标签</b> 2 * 3 * 4",
          }),
        ],
      }),
    );
    const body = screen.getByTestId("kaifan-notice").querySelector("p")!;
    expect(body.querySelector("strong")).toHaveTextContent("22:00");
    expect(body).toHaveTextContent(
      "今晚 22:00 维护。<b>不是标签</b> 2 * 3 * 4",
    );
    expect(body.querySelectorAll("strong")).toHaveLength(1);
    expect(body.querySelector("b")).toBeNull();
  });

  it("关着的时候不把公告记成看过", () => {
    const notices = board();
    show(notices, { open: false });
    expect(notices.markSeen).not.toHaveBeenCalled();
  });

  it("公告上的按钮：没打开成由角色说，不装作打开了", async () => {
    const follow = vi.fn(async () => false);
    show(
      board({
        notices: [notice("a", { actionLabel: "去充值" })],
        follow,
      }),
    );
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "去充值" }));
    });
    expect(follow).toHaveBeenCalled();
    await waitFor(() =>
      expect(says()).toHaveTextContent("这个我没打开成，等下再点一次试试。"),
    );
  });

  it("这个站没有公告栏：直说，不说「没有新公告」", async () => {
    show(board({ available: false, notices: [], unread: new Set() }));
    expect(screen.queryByTestId("kaifan-notice")).toBeNull();
    await waitFor(() =>
      expect(says()).toHaveTextContent("这个站没有公告栏，有消息我也收不到。"),
    );
  });

  it("这次没读到：留着上次的公告，给「再读一次」", () => {
    const notices = board({ failed: true });
    show(notices);
    expect(screen.getAllByTestId("kaifan-notice")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "再读一次" }));
    expect(notices.reload).toHaveBeenCalled();
  });

  it("没连站点：给「连接站点」", () => {
    const onConnect = vi.fn();
    show(board(), { reach: "out", onConnect });
    expect(screen.queryByTestId("kaifan-notice")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "连接站点" }));
    expect(onConnect).toHaveBeenCalled();
  });

  it("截止日期和警示公告照实标出来", () => {
    show(
      board({
        notices: [
          notice("a", {
            severity: "warning",
            expiresAtEpochMs: new Date(2026, 10, 30).getTime(),
          }),
        ],
      }),
    );
    const card = screen.getByTestId("kaifan-notice");
    expect(card).toHaveClass("warn");
    expect(card).toHaveTextContent("11/30 截止");
  });
});
