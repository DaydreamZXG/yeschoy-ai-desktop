import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { openStationConnectPage } from "../account/openStationConnectPage";
import { connectWithKey } from "../account/session";
import { Connect } from "./Connect";
import { lookAtStation } from "./station";

vi.mock("../account/openStationConnectPage", () => ({
  openStationConnectPage: vi.fn(),
}));
vi.mock("../account/session", async (original) => ({
  ...(await original<typeof import("../account/session")>()),
  connectWithKey: vi.fn(),
}));
vi.mock("./station", async (original) => ({
  ...(await original<typeof import("./station")>()),
  lookAtStation: vi.fn(),
}));
// 关掉打字机和声音：这里只看流程。
vi.mock("./sound", async (original) => ({
  ...(await original<typeof import("./sound")>()),
  prefersCalm: () => true,
}));

const look = vi.mocked(lookAtStation);
const connect = vi.mocked(connectWithKey);
const openPage = vi.mocked(openStationConnectPage);
const KEY = "sk-test-0000000000000000";

const station = (patch: Record<string, unknown> = {}) =>
  ({
    ok: true,
    probe: {
      status: "ok",
      desktopConnect: false,
      tokenStatus: "not_provided",
      ...patch,
    },
  }) as Awaited<ReturnType<typeof lookAtStation>>;

function setup(linkEvent: Parameters<typeof Connect>[0]["linkEvent"] = null) {
  const props = {
    open: true,
    dismissible: false,
    linkEvent,
    onLineChange: vi.fn(),
    onConnected: vi.fn(),
    onClose: vi.fn(),
  };
  const view = render(<Connect {...props} />);
  return { ...props, ...view };
}
const step = () => screen.getByTestId("kaifan-connect").dataset.step;
const says = () => screen.getByTestId("kaifan-connect-says").textContent;
const typeUrl = async (value: string) => {
  fireEvent.change(screen.getByLabelText("站点网址"), { target: { value } });
  fireEvent.click(screen.getByRole("button", { name: /下一步/ }));
};
const typeKey = (value: string) => {
  fireEvent.change(screen.getByLabelText("密钥"), { target: { value } });
  fireEvent.click(screen.getByRole("button", { name: /^连接/ }));
};

describe("连接站点", () => {
  beforeEach(() => {
    look.mockReset();
    connect.mockReset();
    openPage.mockReset();
  });

  it("网址空着或看不懂：说清楚，不去探测", async () => {
    setup();
    await typeUrl("");
    expect(says()).toContain("网址还是空的");
    await typeUrl("http://example.com");
    expect(says()).toContain("看不懂");
    expect(look).not.toHaveBeenCalled();
    expect(step()).toBe("url");
  });

  it("带路径贴进来也行；站点不支持一键登录就直接换到密钥，不让用户选", async () => {
    look.mockResolvedValue(station());
    setup();
    await typeUrl("my.example.com/console/token");
    await waitFor(() => expect(step()).toBe("key"));
    expect(look).toHaveBeenCalledWith("https://my.example.com");
    expect(says()).toContain("不支持一键登录");
    expect(openPage).not.toHaveBeenCalled();
  });

  it("连不上：回到填网址，说人话", async () => {
    look.mockResolvedValue({ ok: false, reason: "unreachable" });
    setup();
    await typeUrl("down.example.com");
    await waitFor(() => expect(says()).toContain("连不上这个站"));
    expect(step()).toBe("url");
  });

  it("密钥连上：切到这个站的线路，回首页，输入框里不留密钥", async () => {
    look
      .mockResolvedValueOnce(station())
      .mockResolvedValueOnce(station({ tokenStatus: "valid" }));
    connect.mockResolvedValue({ status: "signed_in" } as never);
    const { onLineChange, onConnected, container } = setup();
    await typeUrl("my.example.com");
    await waitFor(() => expect(step()).toBe("key"));
    typeKey(`  ${KEY} `);
    await waitFor(() => expect(onConnected).toHaveBeenCalled());
    expect(connect).toHaveBeenCalledWith("https://my.example.com", KEY);
    expect(onLineChange).toHaveBeenCalledWith(
      "station_key:https://my.example.com",
    );
    expect(onConnected).toHaveBeenCalledWith({
      host: "my.example.com",
      viaKey: true,
    });
    expect(screen.getByLabelText("密钥")).toHaveValue("");
    expect(container.innerHTML).not.toContain(KEY);
    expect(
      JSON.stringify({ ...localStorage, ...sessionStorage }),
    ).not.toContain(KEY);
  });

  it.each([
    ["invalid", "这个密钥不对"],
    ["no_quota", "余额或额度用完了"],
  ])("密钥 %s：不保存，告诉用户怎么办", async (tokenStatus, words) => {
    look
      .mockResolvedValueOnce(station())
      .mockResolvedValueOnce(station({ tokenStatus }));
    const { onConnected } = setup();
    await typeUrl("my.example.com");
    await waitFor(() => expect(step()).toBe("key"));
    typeKey(KEY);
    await waitFor(() => expect(says()).toContain(words));
    expect(connect).not.toHaveBeenCalled();
    expect(onConnected).not.toHaveBeenCalled();
    expect(step()).toBe("key");
  });

  it("系统密码库用不了：照实说没连", async () => {
    look
      .mockResolvedValueOnce(station())
      .mockResolvedValueOnce(station({ tokenStatus: "valid" }));
    connect.mockResolvedValue({
      status: "secure_storage_unavailable",
    } as never);
    const { onConnected } = setup();
    await typeUrl("my.example.com");
    await waitFor(() => expect(step()).toBe("key"));
    typeKey(KEY);
    await waitFor(() => expect(says()).toContain("系统密码库"));
    expect(onConnected).not.toHaveBeenCalled();
  });

  it("支持一键登录：先问是不是自己的站，焦点停在「上一步」，不自己开浏览器", async () => {
    look.mockResolvedValue(station({ desktopConnect: true }));
    setup();
    await typeUrl("auth.example.com");
    await waitFor(() => expect(step()).toBe("trust"));
    expect(openPage).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "上一步" })).toHaveFocus();
  });

  it("确认后去浏览器；浏览器那边登好了就回首页", async () => {
    look.mockResolvedValue(station({ desktopConnect: true }));
    openPage.mockResolvedValue({
      ok: true,
      origin: "https://auth.example.com",
      lineId: "station_connect:https://auth.example.com",
      pageUrl: "",
    });
    const { onLineChange, onConnected, rerender, ...props } = setup();
    await typeUrl("auth.example.com");
    await waitFor(() => expect(step()).toBe("trust"));
    fireEvent.click(screen.getByRole("button", { name: /是我的站/ }));
    await waitFor(() => expect(step()).toBe("browser"));
    expect(onLineChange).toHaveBeenCalledWith(
      "station_connect:https://auth.example.com",
    );
    rerender(
      <Connect
        {...props}
        onLineChange={onLineChange}
        onConnected={onConnected}
        linkEvent={{ phase: "signed_in", at: 1 }}
      />,
    );
    await waitFor(() =>
      expect(onConnected).toHaveBeenCalledWith({
        host: "auth.example.com",
        viaKey: false,
      }),
    );
  });

  it("浏览器打不开：不卡住，换到密钥", async () => {
    look.mockResolvedValue(station({ desktopConnect: true }));
    openPage.mockResolvedValue({ ok: false, reason: "browser_open_failed" });
    setup();
    await typeUrl("auth.example.com");
    await waitFor(() => expect(step()).toBe("trust"));
    fireEvent.click(screen.getByRole("button", { name: /是我的站/ }));
    await waitFor(() => expect(step()).toBe("key"));
    expect(says()).toContain("浏览器我没能打开");
  });

  it("一个站点都没连时关不掉；已经连着时 Esc 能关", () => {
    const first = setup();
    expect(screen.queryByRole("button", { name: "关闭" })).toBeNull();
    fireEvent.keyDown(screen.getByTestId("kaifan-connect"), { key: "Escape" });
    expect(first.onClose).not.toHaveBeenCalled();
    first.unmount();

    const onClose = vi.fn();
    render(
      <Connect
        open
        dismissible
        linkEvent={null}
        onLineChange={vi.fn()}
        onConnected={vi.fn()}
        onClose={onClose}
      />,
    );
    fireEvent.keyDown(screen.getByTestId("kaifan-connect"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
