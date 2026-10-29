import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import nativeFixtures from "../../tests/work_packages/ru041/fixtures/connectivity-native.json";
import { DiagnosticsView } from "./DiagnosticsView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const invokeMock = vi.mocked(invoke);

// 这几句刚从 `diagnostics/copy.ts` 搬进语言文件。断言跟着读同一个来源，
// 否则改文案时这里会红，而红的原因与被测行为无关。
const D = zh.yeschoyDiagnostics;

const clipboardWrite = vi.fn();

beforeAll(() => {
  i18n.addResourceBundle(
    "zh",
    "translation",
    {
      yeschoyDiagnostics: zh.yeschoyDiagnostics,
      yeschoyConfiguration: zh.yeschoyConfiguration,
    },
    true,
    true,
  );
  Object.assign(navigator, {
    clipboard: { writeText: clipboardWrite },
  });
});
beforeEach(() => {
  invokeMock.mockReset();
  clipboardWrite.mockReset();
  clipboardWrite.mockResolvedValue(undefined);
});

const healthySnapshot = {
  platform: "macos",
  arch: "aarch64",
  appVersion: "0.5.1",
  frontendMode: "bundled-custom-protocol",
  distribution: "community",
  logAvailable: true,
  logTail: ["1 INFO a::b stage=start", "2 INFO a::b stage=ready"],
  markers: [{ element: "desktop_shell", detail: "stage=ready" }],
};

const routes = {
  setup: vi.fn(),
  account: vi.fn(),
  home: vi.fn(),
  tools: vi.fn(),
};

// 一次性失败：只让线路检查失败，挂载时的健康快照读取仍正常（它是同一条
// invoke，不能跟着一起被拒）。
function failNextConnectivity(error: unknown) {
  let pending = true;
  const previous = invokeMock.getMockImplementation();
  invokeMock.mockImplementation((command, args) => {
    if (pending && command === "check_line_connectivity_read_only") {
      pending = false;
      return Promise.reject(error);
    }
    if (previous) return previous(command, args);
    return Promise.resolve(healthySnapshot);
  });
}

function showView() {
  for (const spy of Object.values(routes)) spy.mockReset();
  return render(
    <DiagnosticsView
      onOpenSetup={routes.setup}
      onOpenAccount={routes.account}
      onOpenHome={routes.home}
      onOpenTools={routes.tools}
    />,
  );
}

// 线路检查专用：健康检查助手挂载时的快照读取也计入 mock.calls，所以这里
// 只数 check_line_connectivity_read_only，下标语义与以前一致。
function connectivityCalls() {
  return invokeMock.mock.calls.filter(
    ([command]) => command === "check_line_connectivity_read_only",
  );
}

function currentRequestId(callIndex: number) {
  return (
    connectivityCalls()[callIndex][1] as { request: { requestId: string } }
  ).request.requestId;
}

function replyWith(
  fixture: typeof nativeFixtures.reachable | Record<string, unknown>,
) {
  // 健康检查助手的快照读取也走同一个 invoke。以前这里用 mockImplementationOnce，
  // 现在多了一次挂载时的读取，顺手回一份合法快照，其余命令仍按一次性回复。
  invokeMock.mockImplementation((command, args) => {
    if (command === "health_snapshot_read_only")
      return Promise.resolve(healthySnapshot);
    return Promise.resolve({
      ...structuredClone(fixture),
      requestId: (args as { request: { requestId: string } }).request.requestId,
    });
  });
}

// 健康检查助手在挂载时读一次本机快照；它不是被测动作，但会占掉一次 IPC
// 调用。攻击性用例（"出错前不调用任何命令"）必须把它排除在外。
function invokeWithoutSnapshot() {
  return invokeMock.mock.calls.filter(
    ([command]) => command !== "health_snapshot_read_only",
  );
}

function line(name: string) {
  return screen.getByRole("article", { name });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

describe("diagnostics native-to-renderer outcomes", () => {
  it("requires an explicit action and displays every layer of both successes", async () => {
    replyWith(nativeFixtures.reachable);
    showView();
    expect(invokeWithoutSnapshot()).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    expect(await screen.findAllByText("基础连接正常")).toHaveLength(2);
    expect(screen.getByTestId("diagnostics-view")).toHaveAttribute(
      "data-phase",
      "success",
    );
    const mainland = line("大陆优化");
    expect(within(mainland).getAllByText("通过")).toHaveLength(4);
    expect(within(mainland).getByText("DNS 解析")).toBeInTheDocument();
    expect(within(mainland).getByText("TLS 证书校验")).toBeInTheDocument();
    expect(within(mainland).getByText("API Key 有效性")).toBeInTheDocument();
    expect(within(mainland).getByText("11 ms")).toBeInTheDocument();
    expect(within(mainland).getByText("14 ms")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(invokeWithoutSnapshot()).toHaveLength(1);
    expect(invokeWithoutSnapshot()[0]).toEqual([
      "check_line_connectivity_read_only",
      {
        request: { requestId: currentRequestId(0) },
      },
    ]);
    expect(
      screen.getByText("连接正常，不代表模型一定可用"),
    ).toBeInTheDocument();
  });

  it("shows a mixed network outcome without treating DNS failure as an unreadable result", async () => {
    replyWith(nativeFixtures.mixed);
    showView();
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    expect(await screen.findByText("找不到线路地址")).toBeInTheDocument();
    expect(
      within(line("大陆优化")).getByText("基础连接正常"),
    ).toBeInTheDocument();
    // API key layer is skipped without a saved session, not shown as failed.
    expect(
      within(line("大陆优化")).getByText("未保存登录会话，本层跳过。"),
    ).toBeInTheDocument();
    expect(within(line("全球加速")).getAllByText("跳过")).toHaveLength(3);
    expect(screen.getByTestId("diagnostics-view")).toHaveAttribute(
      "data-phase",
      "partial",
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("keeps TCP failure and timeout as their own actual network outcomes", async () => {
    replyWith(nativeFixtures.failed);
    showView();
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    expect(await screen.findByText("无法连接线路")).toBeInTheDocument();
    expect(within(line("全球加速")).getByText("检查超时")).toBeInTheDocument();
    expect(screen.getByTestId("diagnostics-view")).toHaveAttribute(
      "data-phase",
      "empty",
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("retains a valid success beside a malformed peer without inventing a failed connection", async () => {
    replyWith({
      ...nativeFixtures.reachable,
      lines: [nativeFixtures.reachable.lines[0], null],
    });
    showView();
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      D.readError.partial.title,
    );
    expect(
      within(line("大陆优化")).getByText("基础连接正常"),
    ).toBeInTheDocument();
    expect(
      within(line("全球加速")).getByText(D.status.unavailable),
    ).toBeInTheDocument();
    expect(screen.queryByText("无法连接线路")).not.toBeInTheDocument();
    expect(screen.queryByText("找不到线路地址")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "重新检查两条线路" }),
    ).toBeEnabled();
  });

  it("separates invoke failure from unknown response data and never renders raw errors", async () => {
    showView();
    failNextConnectivity(
      new Error("synthetic-private error: invoke bridge failed"),
    );
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      D.readError.invoke.title,
    );
    expect(screen.queryByText(/synthetic-private/)).not.toBeInTheDocument();
    expect(screen.getAllByText(D.status.unavailable)).toHaveLength(2);
    invokeMock.mockResolvedValueOnce(null);
    fireEvent.click(screen.getByRole("button", { name: "重新检查两条线路" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      D.readError.invalid.title,
    );
    expect(screen.queryByText("无法连接线路")).not.toBeInTheDocument();
    replyWith(nativeFixtures.reachable);
    fireEvent.click(screen.getByRole("button", { name: "重新检查两条线路" }));
    expect(await screen.findAllByText("基础连接正常")).toHaveLength(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("retains older observations after invoke failure and replaces only the next valid line", async () => {
    replyWith(nativeFixtures.reachable);
    showView();
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    await screen.findAllByText("基础连接正常");
    failNextConnectivity(new Error("synthetic-invoke-error"));
    fireEvent.click(screen.getByRole("button", { name: "重新检查两条线路" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      D.readError.invoke.title,
    );
    expect(screen.getAllByText("基础连接正常")).toHaveLength(2);
    expect(screen.getAllByText(D.previousResult)).toHaveLength(2);
    replyWith({
      ...nativeFixtures.mixed,
      lines: [null, nativeFixtures.mixed.lines[1]],
    });
    fireEvent.click(screen.getByRole("button", { name: "重新检查两条线路" }));
    expect(await screen.findByText("找不到线路地址")).toBeInTheDocument();
    expect(
      within(line("大陆优化")).getByText("基础连接正常"),
    ).toBeInTheDocument();
    expect(
      within(line("大陆优化")).getByText(D.previousResult),
    ).toBeInTheDocument();
    expect(
      within(line("全球加速")).queryByText(D.previousResult),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      D.readError.partial.title,
    );
  });

  it("rejects a previous request's reply on retry and recovers with the current reply", async () => {
    replyWith(nativeFixtures.reachable);
    showView();
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    await screen.findAllByText("基础连接正常");
    const oldRequestId = currentRequestId(0);
    invokeMock.mockResolvedValueOnce({
      ...nativeFixtures.failed,
      requestId: oldRequestId,
    });
    fireEvent.click(screen.getByRole("button", { name: "重新检查两条线路" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      D.readError.invalid.title,
    );
    expect(screen.getAllByText("基础连接正常")).toHaveLength(2);
    expect(screen.queryByText("无法连接线路")).not.toBeInTheDocument();
    expect(currentRequestId(1)).not.toBe(oldRequestId);
    replyWith(nativeFixtures.failed);
    fireEvent.click(screen.getByRole("button", { name: "重新检查两条线路" }));
    expect(await screen.findByText("无法连接线路")).toBeInTheDocument();
    expect(screen.queryByText(D.previousResult)).not.toBeInTheDocument();
  });

  it.each(["resolve", "reject"] as const)(
    "does not let an older %s replace a newer result after rapid checks",
    async (outcome) => {
      const older = deferred<unknown>(),
        newer = deferred<unknown>();
      // 挂载时的快照读取排在前面；只给之后两次线路检查排队 deferred。
      showView();
      invokeMock
        .mockReturnValueOnce(older.promise)
        .mockReturnValueOnce(newer.promise);
      const action = screen.getByRole("button", { name: "检查两条线路" });
      act(() => {
        action.click();
        action.click();
      });
      expect(invokeWithoutSnapshot()).toHaveLength(2);
      await act(async () => {
        newer.resolve({
          ...nativeFixtures.reachable,
          requestId: currentRequestId(1),
        });
      });
      expect(screen.getAllByText("基础连接正常")).toHaveLength(2);
      await act(async () => {
        if (outcome === "resolve")
          older.resolve({
            ...nativeFixtures.failed,
            requestId: currentRequestId(0),
          });
        else older.reject(new Error("synthetic-old-error"));
      });
      expect(screen.getAllByText("基础连接正常")).toHaveLength(2);
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(screen.getByTestId("diagnostics-view")).toHaveAttribute(
        "data-phase",
        "success",
      );
    },
  );

  it("copies a sanitized report without hosts, tokens or account data", async () => {
    replyWith(nativeFixtures.reachable);
    showView();
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    await screen.findAllByText("基础连接正常");
    fireEvent.click(screen.getByTestId("copy-diagnostic-report"));
    await screen.findByText("已复制");
    expect(clipboardWrite).toHaveBeenCalledTimes(1);
    const report = clipboardWrite.mock.calls[0][0] as string;
    expect(report).toContain("request-id: ");
    expect(report).toContain("dns: passed (11 ms) [dns_resolved]");
    expect(report).toContain("api_key: passed (14 ms) [session_token_valid]");
    expect(report).toContain("[大陆优化]");
    expect(report).not.toContain("yeschoy.com");
    expect(report).not.toContain("https://");
    expect(report).not.toContain("Bearer");
    expect(report).not.toContain("accessToken");
  });

  it("surfaces a repair action when the saved session was rejected", async () => {
    const rejected = structuredClone(nativeFixtures.reachable);
    for (const line_ of rejected.lines) {
      const index = line_.layers.findIndex(
        (layer) => layer.layer === "api_key",
      );
      line_.layers[index] = {
        layer: "api_key",
        status: "failed",
        reasonCode: "session_token_rejected",
      } as (typeof line_.layers)[number];
    }
    replyWith(rejected);
    showView();
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    // api_key failure is not a network failure: lines stay reachable.
    expect(await screen.findAllByText("登录状态已失效")).toHaveLength(2);
    expect(screen.getAllByText("基础连接正常")).toHaveLength(2);
    expect(screen.getByTestId("diagnostics-view")).toHaveAttribute(
      "data-phase",
      "success",
    );
    const relogin = screen.getAllByRole("button", { name: "重新登录" });
    expect(relogin).toHaveLength(2);
    // What failed here is an account credential. This button used to open the
    // setup page, which has no way to log in — telling the user to re-login
    // and then sending them somewhere they cannot.
    fireEvent.click(relogin[0]);
    expect(routes.account).toHaveBeenCalledOnce();
    expect(routes.setup).not.toHaveBeenCalled();
  });

  it("offers a way back to the home view, not only sideways", async () => {
    showView();
    fireEvent.click(screen.getByRole("button", { name: "返回我的应用" }));
    expect(routes.home).toHaveBeenCalledOnce();
  });
});
