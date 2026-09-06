import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import nativeFixtures from "../../tests/work_packages/ru041/fixtures/connectivity-native.json";
import { DiagnosticsView } from "./DiagnosticsView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const invokeMock = vi.mocked(invoke);

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
});
beforeEach(() => {
  invokeMock.mockReset();
});

function showView() {
  return render(
    <DiagnosticsView onOpenSetup={vi.fn()} onOpenTools={vi.fn()} />,
  );
}

function currentRequestId(callIndex: number) {
  return (
    invokeMock.mock.calls[callIndex][1] as { request: { requestId: string } }
  ).request.requestId;
}

function replyWith(
  fixture: typeof nativeFixtures.reachable | Record<string, unknown>,
) {
  invokeMock.mockImplementationOnce((_command, args) =>
    Promise.resolve({
      ...structuredClone(fixture),
      requestId: (args as { request: { requestId: string } }).request.requestId,
    }),
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
  it("requires an explicit action and displays both serialized successes", async () => {
    replyWith(nativeFixtures.reachable);
    showView();
    expect(invokeMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    expect(await screen.findAllByText("基础连接正常")).toHaveLength(2);
    expect(screen.getByTestId("diagnostics-view")).toHaveAttribute(
      "data-phase",
      "success",
    );
    expect(within(line("大陆优化")).getByText("10 ms")).toBeInTheDocument();
    expect(within(line("全球加速")).getByText("20 ms")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith(
      "check_line_connectivity_read_only",
      {
        request: { requestId: currentRequestId(0) },
      },
    );
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
      "部分结果未能读取",
    );
    expect(
      within(line("大陆优化")).getByText("基础连接正常"),
    ).toBeInTheDocument();
    expect(
      within(line("全球加速")).getByText("本次结果不可用"),
    ).toBeInTheDocument();
    expect(screen.queryByText("无法连接线路")).not.toBeInTheDocument();
    expect(screen.queryByText("找不到线路地址")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "重新检查两条线路" }),
    ).toBeEnabled();
  });

  it("separates invoke failure from unknown response data and never renders raw errors", async () => {
    invokeMock.mockRejectedValueOnce(
      new Error("synthetic-private error: invoke bridge failed"),
    );
    showView();
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "暂时无法运行检查",
    );
    expect(screen.queryByText(/synthetic-private/)).not.toBeInTheDocument();
    expect(screen.getAllByText("本次结果不可用")).toHaveLength(2);
    invokeMock.mockResolvedValueOnce(null);
    fireEvent.click(screen.getByRole("button", { name: "重新检查两条线路" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "暂时无法读取结果",
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
    invokeMock.mockRejectedValueOnce(new Error("synthetic-invoke-error"));
    fireEvent.click(screen.getByRole("button", { name: "重新检查两条线路" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "暂时无法运行检查",
    );
    expect(screen.getAllByText("基础连接正常")).toHaveLength(2);
    expect(screen.getAllByText("上次结果，尚未更新")).toHaveLength(2);
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
      within(line("大陆优化")).getByText("上次结果，尚未更新"),
    ).toBeInTheDocument();
    expect(
      within(line("全球加速")).queryByText("上次结果，尚未更新"),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("部分结果未能读取");
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
      "暂时无法读取结果",
    );
    expect(screen.getAllByText("基础连接正常")).toHaveLength(2);
    expect(screen.queryByText("无法连接线路")).not.toBeInTheDocument();
    expect(currentRequestId(1)).not.toBe(oldRequestId);
    replyWith(nativeFixtures.failed);
    fireEvent.click(screen.getByRole("button", { name: "重新检查两条线路" }));
    expect(await screen.findByText("无法连接线路")).toBeInTheDocument();
    expect(screen.queryByText("上次结果，尚未更新")).not.toBeInTheDocument();
  });

  it.each(["resolve", "reject"] as const)(
    "does not let an older %s replace a newer result after rapid checks",
    async (outcome) => {
      const older = deferred<unknown>(),
        newer = deferred<unknown>();
      invokeMock
        .mockReturnValueOnce(older.promise)
        .mockReturnValueOnce(newer.promise);
      showView();
      const action = screen.getByRole("button", { name: "检查两条线路" });
      act(() => {
        action.click();
        action.click();
      });
      expect(invokeMock).toHaveBeenCalledTimes(2);
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
});
