import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountProjection } from "../account/session";
import { useAccountSession } from "../account/useAccountSession";
import { idleUpdateProjection } from "../update/contract";
import { useDesktopUpdate } from "../update/UpdateProvider";
import { KaifanApp } from "./KaifanApp";

// 连接、接入、登场这三屏各有自己的测试；这里只看它们和首页之间怎么交接。
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
vi.mock("./Shutdown", () => ({ ShutdownHost: () => null, askToQuit: vi.fn() }));
vi.mock("./useStationLink", () => ({
  trustAnswerPending: () => false,
  useStationLink: () => ({ trustOrigin: null, answerTrust: vi.fn() }),
}));
vi.mock("../workbench/announcements", async (load) => ({
  ...(await load<typeof import("../workbench/announcements")>()),
  readAnnouncements: () => Promise.resolve({ available: false, notices: [] }),
}));
vi.mock("./Fx", async () => {
  const { forwardRef } = await import("react");
  return { Fx: forwardRef(() => null) };
});
vi.mock("./sound", async (original) => ({
  ...(await original<typeof import("./sound")>()),
  prefersCalm: () => true,
}));
vi.mock("./Connect", () => ({
  Connect: (props: {
    onLineChange: (lineId: string) => void;
    onConnected: (result: { host: string; viaKey: boolean }) => void;
  }) => (
    <button
      type="button"
      onClick={() => {
        props.onLineChange("station_key:https://other.example.com");
        props.onConnected({ host: "other.example.com", viaKey: true });
      }}
    >
      test-connect
    </button>
  ),
}));
vi.mock("./Join", () => ({
  Join: (props: { onJoined: (result: unknown) => void }) => (
    <button
      type="button"
      onClick={() =>
        props.onJoined({
          toolId: "pi",
          who: "new",
          modelId: "mystery-1",
          opened: false,
          skipped: [],
        })
      }
    >
      test-join
    </button>
  ),
}));
vi.mock("./Reveal", () => ({
  Reveal: (props: {
    joined: unknown;
    onOpen: () => void;
    onHome: () => void;
  }) => (
    <div data-testid="reveal" data-on={props.joined ? "yes" : "no"}>
      <button type="button" onClick={props.onOpen}>
        test-open
      </button>
      <button type="button" onClick={props.onHome}>
        test-home
      </button>
    </div>
  ),
}));

const session = vi.mocked(useAccountSession);
const refresh = vi.fn();
const account = (models: string[]) =>
  ({
    status: "signed_in",
    models: models.map((id) => ({ id })),
    money: {
      currency: "CNY",
      balanceAmount: "86.40",
      consumedAmount: "0",
      displayRate: "1",
    },
  }) as unknown as AccountProjection;
const use = (models: string[], stationCheck: string) =>
  session.mockReturnValue({
    projection: account(models),
    loading: false,
    stationCheck,
    lastError: null,
    refresh,
  } as unknown as ReturnType<typeof useAccountSession>);

beforeEach(() => {
  localStorage.clear();
  tools.now = { connections: [], error: null, refresh: vi.fn() };
  refresh.mockReset().mockResolvedValue(null);
  vi.mocked(useDesktopUpdate).mockReturnValue({
    projection: idleUpdateProjection(),
    attention: false,
    working: false,
    check: vi.fn(),
    install: vi.fn(),
  });
});

describe("首页和别的几屏怎么交接", () => {
  it("刚换了站：等新站的型号读回来再报到了几位，不拿上一个站的数报", async () => {
    use(["gpt-5", "claude-sonnet-4-5"], "ok");
    // 真的那个 refresh 一叫就把站点标成「正在读」；手里还是上一个站的两位。
    refresh.mockImplementation(() => {
      use(["gpt-5", "claude-sonnet-4-5"], "checking");
      return Promise.resolve(null);
    });
    const view = render(<KaifanApp />);
    fireEvent.click(screen.getByText("test-connect"));
    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(1));
    view.rerender(<KaifanApp />);
    expect(screen.getByTestId("kaifan-says")).not.toHaveTextContent("连上");

    use(["gpt-5"], "ok");
    view.rerender(<KaifanApp />);
    await waitFor(() =>
      expect(screen.getByTestId("kaifan-says")).toHaveTextContent(
        "连上 other.example.com 啦。这个站有 1 位同学在岗",
      ),
    );
  });

  it("派出去的是转校生：回首页陪着的就是她", async () => {
    use(["gpt-5", "mystery-1"], "ok");
    render(<KaifanApp />);
    expect(screen.getByTestId("kaifan-strip-gpt")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.click(screen.getByText("test-join"));
    await waitFor(() =>
      expect(screen.getByTestId("kaifan-strip-new")).toHaveAttribute(
        "aria-pressed",
        "true",
      ),
    );
  });

  it("点了打开又回了首页：应用开起来以后不把登场那一屏再拉回来", async () => {
    use(["gpt-5", "mystery-1"], "ok");
    let opened: (value: unknown) => void = () => undefined;
    tools.now = {
      connections: [],
      error: null,
      refresh: vi.fn(),
      open: () => new Promise((resolve) => (opened = resolve)),
    };
    render(<KaifanApp />);
    fireEvent.click(screen.getByText("test-join"));
    await waitFor(() =>
      expect(screen.getByTestId("reveal")).toHaveAttribute("data-on", "yes"),
    );
    fireEvent.click(screen.getByText("test-open"));
    fireEvent.click(screen.getByText("test-home"));
    expect(screen.getByTestId("reveal")).toHaveAttribute("data-on", "no");
    await act(async () => opened({ status: "opened", reasonCode: "" }));
    expect(screen.getByTestId("reveal")).toHaveAttribute("data-on", "no");
  });
});
