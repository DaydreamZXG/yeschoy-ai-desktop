import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { openStationConnectPage } from "../account/openStationConnectPage";
import { connectWithKey } from "../account/session";
import { Connect } from "./Connect";
import { resetProviderPresetsForTest } from "./providers";
import { lookAtStation } from "./station";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
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
const ipc = vi.mocked(invoke);
const preset = (patch: Record<string, unknown>) => ({
  region: null,
  billing: "plan",
  keyCheck: "models",
  onlyTools: [],
  keyPage: "https://example.com/keys",
  common: true,
  ...patch,
});
const PRESETS = [
  preset({
    id: "kimi_code_cn",
    family: "kimi_code",
    region: "cn",
    name: "Kimi Code",
    origin: "https://api.kimi.com",
  }),
  preset({
    id: "kimi_code_global",
    family: "kimi_code",
    region: "global",
    name: "Kimi Code",
    origin: "https://api.kimi.ai",
  }),
  preset({
    id: "deepseek",
    family: "deepseek",
    name: "DeepSeek",
    origin: "https://api.deepseek.com",
    billing: "pay_as_you_go",
  }),
  preset({
    id: "bailian_coding",
    family: "bailian_coding",
    name: "阿里云百炼 Coding Plan",
    origin: "https://coding.dashscope.aliyuncs.com",
    keyCheck: "unverifiable",
    common: false,
  }),
  preset({
    id: "opencode_go",
    family: "opencode_go",
    name: "OpenCode Go",
    origin: "https://opencode.ai",
    onlyTools: ["opencode"],
    common: false,
  }),
];
/** 原生侧给了厂商表。 */
const withVendors = () =>
  ipc.mockImplementation(async (command) =>
    command === "provider_presets_v1" ? PRESETS : null,
  );
const vendor = (name: RegExp) => screen.findByRole("button", { name });
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
    onNotice: vi.fn(),
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
    // 默认读不到厂商表：填网址那条路要照旧能用。
    ipc.mockReset();
    ipc.mockRejectedValue(new Error("unavailable"));
    resetProviderPresetsForTest();
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

  it("网页那边发起的连接成了：不管面板停在哪一步，都回首页", async () => {
    const { onConnected, onNotice, rerender, ...props } = setup();
    expect(step()).toBe("url");
    rerender(
      <Connect
        {...props}
        onConnected={onConnected}
        onNotice={onNotice}
        linkEvent={{
          phase: "signed_in",
          lineId: "station_connect:https://web.example.com",
          at: 1,
        }}
      />,
    );
    await waitFor(() =>
      expect(onConnected).toHaveBeenCalledWith({
        host: "web.example.com",
        viaKey: false,
      }),
    );
  });

  it("网页那边发起的连接没成：面板开着就在面板里说", async () => {
    const { onNotice, rerender, ...props } = setup();
    rerender(
      <Connect
        {...props}
        onNotice={onNotice}
        linkEvent={{ phase: "failed", reasonCode: "whatever", at: 1 }}
      />,
    );
    await waitFor(() => expect(says()).toContain("网页那边发起的连接没成"));
    expect(step()).toBe("url");
    expect(onNotice).not.toHaveBeenCalled();
  });

  it("网页那边发起的连接没成：面板没开就交给首页的角色说", async () => {
    const { onNotice, rerender, ...props } = setup();
    rerender(
      <Connect
        {...props}
        open={false}
        onNotice={onNotice}
        linkEvent={{ phase: "failed", reasonCode: "whatever", at: 1 }}
      />,
    );
    await waitFor(() =>
      expect(onNotice).toHaveBeenCalledWith(
        expect.stringContaining("网页那边发起的连接没成"),
        "panic",
      ),
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
        onNotice={vi.fn()}
        onClose={onClose}
      />,
    );
    fireEvent.keyDown(screen.getByTestId("kaifan-connect"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  describe("云厂商", () => {
    it("读不到厂商表：只少了云厂商那一组，填网址照旧", async () => {
      setup();
      await waitFor(() => expect(ipc).toHaveBeenCalled());
      expect(screen.queryByTestId("kaifan-connect-vendors")).toBeNull();
      expect(screen.getByLabelText("站点网址")).toBeEnabled();
    });

    it("不常用的默认收着，点一下才露出来", async () => {
      withVendors();
      setup();
      await vendor(/Kimi Code/);
      expect(screen.getByRole("button", { name: /DeepSeek/ })).toBeVisible();
      expect(screen.queryByRole("button", { name: /百炼/ })).toBeNull();
      const fold = screen.getByRole("button", { name: /还有 2 家/ });
      expect(fold).toHaveAttribute("aria-expanded", "false");
      fireEvent.click(fold);
      expect(screen.getByRole("button", { name: /百炼/ })).toBeVisible();
      expect(screen.getByRole("button", { name: /OpenCode Go/ })).toBeVisible();
      fireEvent.click(screen.getByRole("button", { name: /收起/ }));
      expect(screen.queryByRole("button", { name: /百炼/ })).toBeNull();
    });

    it("套餐：粘贴密钥之前先说清有被封的风险；不去探站点，直接连", async () => {
      withVendors();
      connect.mockResolvedValue({ status: "signed_in" } as never);
      const { onLineChange, onConnected, container } = setup();
      fireEvent.click(await vendor(/Kimi Code/));
      expect(step()).toBe("vendor");
      expect(screen.getByTestId("kaifan-connect-risk")).toHaveTextContent(
        /Kimi Code.*停掉你的套餐/,
      );
      typeKey(` ${KEY} `);
      await waitFor(() => expect(onConnected).toHaveBeenCalled());
      expect(look).not.toHaveBeenCalled();
      expect(connect).toHaveBeenCalledWith("https://api.kimi.com", KEY);
      expect(onLineChange).toHaveBeenCalledWith(
        "station_key:https://api.kimi.com",
      );
      expect(onConnected).toHaveBeenCalledWith({
        host: "Kimi Code",
        viaKey: true,
      });
      expect(container.innerHTML).not.toContain(KEY);
    });

    it("有国内站和海外站的：换一下，连的就是另一个地址", async () => {
      withVendors();
      connect.mockResolvedValue({ status: "signed_in" } as never);
      const { onConnected } = setup();
      fireEvent.click(await vendor(/Kimi Code/));
      const abroad = screen.getByRole("button", { name: "海外站" });
      expect(abroad).toHaveAttribute("aria-pressed", "false");
      fireEvent.click(abroad);
      expect(abroad).toHaveAttribute("aria-pressed", "true");
      typeKey(KEY);
      await waitFor(() => expect(onConnected).toHaveBeenCalled());
      expect(connect).toHaveBeenCalledWith("https://api.kimi.ai", KEY);
    });

    it("按量付费的不吓人；没有地区可选的不出地区", async () => {
      withVendors();
      setup();
      fireEvent.click(await vendor(/DeepSeek/));
      expect(screen.queryByTestId("kaifan-connect-risk")).toBeNull();
      expect(screen.queryByRole("button", { name: "海外站" })).toBeNull();
      expect(screen.queryByTestId("kaifan-connect-limits")).toBeNull();
    });

    it("没法提前核对密钥的、只认某个应用的，连之前就说", async () => {
      withVendors();
      setup();
      fireEvent.click(await vendor(/还有 2 家/));
      fireEvent.click(screen.getByRole("button", { name: /百炼/ }));
      expect(screen.getByTestId("kaifan-connect-limits")).toHaveTextContent(
        "没法提前核对密钥",
      );
      fireEvent.click(screen.getByRole("button", { name: "上一步" }));
      expect(step()).toBe("url");
      // 回来时刚才展开的还展开着，不用再点一次。
      fireEvent.click(screen.getByRole("button", { name: /OpenCode Go/ }));
      expect(screen.getByTestId("kaifan-connect-limits")).toHaveTextContent(
        "只认OpenCode",
      );
    });

    it("去拿密钥：只告诉原生侧是哪一家；浏览器没开就说怎么办", async () => {
      withVendors();
      setup();
      fireEvent.click(await vendor(/Kimi Code/));
      fireEvent.click(screen.getByRole("button", { name: /去拿密钥/ }));
      await waitFor(() =>
        expect(ipc).toHaveBeenCalledWith("provider_open_key_page_v1", {
          id: "kimi_code_cn",
        }),
      );
      ipc.mockRejectedValue("browser_unavailable");
      fireEvent.click(screen.getByRole("button", { name: /去拿密钥/ }));
      await waitFor(() => expect(says()).toContain("浏览器没打开"));
    });

    it("密钥被拒：留在这一屏，说清楚，可以重新粘", async () => {
      withVendors();
      connect.mockResolvedValue({
        status: "signed_out",
        reasonCode: "key_rejected",
      } as never);
      const { onConnected } = setup();
      fireEvent.click(await vendor(/DeepSeek/));
      typeKey(KEY);
      await waitFor(() =>
        expect(screen.getByRole("button", { name: /^连接/ })).toBeEnabled(),
      );
      expect(step()).toBe("vendor");
      expect(onConnected).not.toHaveBeenCalled();
      expect(screen.getByLabelText("密钥")).toBeEnabled();
    });
  });
});
