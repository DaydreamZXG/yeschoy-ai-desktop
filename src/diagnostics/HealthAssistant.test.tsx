import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import zh from "../i18n/locales/zh.json";
import nativeFixtures from "../../tests/work_packages/ru041/fixtures/connectivity-native.json";
import { DiagnosticsView } from "./DiagnosticsView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const invokeMock = vi.mocked(invoke);
const H = zh.yeschoyHealth;
const clipboardWrite = vi.fn();

Object.assign(navigator, { clipboard: { writeText: clipboardWrite } });

const snapshot = {
  platform: "macos",
  arch: "aarch64",
  appVersion: "0.5.1",
  frontendMode: "bundled-custom-protocol",
  distribution: "community",
  logAvailable: true,
  logTail: [
    "1793171656.160 INFO yeschoy_desktop_lib::tool_discovery tool_discovery executable=claude path_directories=4 candidates=2",
    "1793176958.155 ERROR tauri_plugin_updater stage=check_failed",
  ],
  markers: [{ element: "desktop_shell", detail: "stage=start version=0.5.1" }],
};

beforeEach(() => {
  invokeMock.mockReset();
  clipboardWrite.mockReset();
  clipboardWrite.mockResolvedValue(undefined);
});

function route(snapshotReply: unknown, connectivityReply?: unknown) {
  invokeMock.mockImplementation((command, args) => {
    if (command === "health_snapshot_read_only")
      return typeof snapshotReply === "string"
        ? Promise.reject(new Error(snapshotReply))
        : Promise.resolve(snapshotReply);
    if (command === "check_line_connectivity_read_only")
      return Promise.resolve({
        ...structuredClone(connectivityReply ?? nativeFixtures.reachable),
        requestId: (args as { request: { requestId: string } }).request
          .requestId,
      });
    throw Error(`unexpected:${command}`);
  });
}

function showView(onReportBug = vi.fn()) {
  render(
    <DiagnosticsView
      onOpenSetup={vi.fn()}
      onOpenAccount={vi.fn()}
      onOpenHome={vi.fn()}
      onOpenTools={vi.fn()}
      onReportBug={onReportBug}
    />,
  );
  return onReportBug;
}

describe("health assistant", () => {
  it("reads the environment once and shows it without any user action", async () => {
    route(snapshot);
    showView();
    const env = await screen.findByTestId("health-environment");
    expect(env).toHaveTextContent(
      "macos · 0.5.1 · bundled-custom-protocol · community",
    );
    expect(env).toHaveTextContent("已读取最近 2 行（已去掉路径与密钥）");
    // Never uploads: the only IPC is the read itself.
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      "health_snapshot_read_only",
    ]);
  });

  it("names the first failing layer per line after a check", async () => {
    route(snapshot, nativeFixtures.mixed);
    showView();
    await screen.findByTestId("health-environment");
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    const findings = await screen.findByTestId("health-findings");
    // mixed 里失败的是「全球加速」的 DNS 层。
    expect(findings).toHaveTextContent("全球加速");
    expect(findings).toHaveTextContent(
      zh.yeschoyDiagnostics.reason.dns_resolution_failed,
    );
    // The healthy line is not reported as a problem.
    expect(findings.querySelectorAll("li")).toHaveLength(1);
    expect(findings).not.toHaveTextContent("大陆优化");
  });

  it("says nothing is wrong when both lines pass", async () => {
    route(snapshot);
    showView();
    await screen.findByTestId("health-environment");
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    await screen.findByText(H.noFindings);
    expect(screen.queryByTestId("health-findings")).not.toBeInTheDocument();
  });

  it("copies one pasteable report with environment, findings and stage markers", async () => {
    route(snapshot, nativeFixtures.mixed);
    showView();
    await screen.findByTestId("health-environment");
    fireEvent.click(screen.getByRole("button", { name: "检查两条线路" }));
    await screen.findByTestId("health-findings");
    fireEvent.click(screen.getByTestId("copy-health-report"));
    await screen.findByText(H.reportCopied);
    const report = clipboardWrite.mock.calls[0][0] as string;
    expect(report).toContain("Yeschoy health report (schema v1)");
    expect(report).toContain("env: platform=macos arch=aarch64 app=0.5.1");
    expect(report).toContain("dns_resolution_failed");
    expect(report).toContain("desktop_shell stage=start version=0.5.1");
    expect(report).toContain("tool_discovery executable=claude");
    expect(report).toContain("[what-happened]");
  });

  it("still copies a report when the native snapshot cannot be read", async () => {
    route("ipc down", nativeFixtures.reachable);
    showView();
    await screen.findByText(H.envUnavailable);
    fireEvent.click(screen.getByTestId("copy-health-report"));
    await screen.findByText(H.reportCopied);
    const report = clipboardWrite.mock.calls[0][0] as string;
    expect(report).toContain("env: unavailable");
    expect(report).toContain("[log-tail]\nunavailable");
    expect(report).toContain("[connectivity]\nnot-checked");
  });

  it("drops a snapshot that failed the renderer's leak re-check", async () => {
    route({
      ...snapshot,
      logTail: ["opened /Users/someone/.codex/config.toml"],
    });
    showView();
    const env = await screen.findByTestId("health-environment");
    await waitFor(() => expect(env).toHaveTextContent(H.envUnavailable));
    expect(screen.queryByTestId("health-log-preview")).not.toBeInTheDocument();
  });

  it("reports a copy failure instead of pretending success", async () => {
    clipboardWrite.mockRejectedValue(new Error("denied"));
    route(snapshot);
    showView();
    await screen.findByTestId("health-environment");
    fireEvent.click(screen.getByTestId("copy-health-report"));
    await screen.findByText(zh.yeschoyDiagnostics.copyFailed);
  });

  it("hands bug feedback over when the host offers it", async () => {
    route(snapshot);
    const onReportBug = showView();
    await screen.findByTestId("health-environment");
    fireEvent.click(screen.getByTestId("report-bug-from-health"));
    expect(onReportBug).toHaveBeenCalledOnce();
  });

  it("hides the bug entry when the host does not wire one", async () => {
    route(snapshot);
    render(
      <DiagnosticsView
        onOpenSetup={vi.fn()}
        onOpenAccount={vi.fn()}
        onOpenHome={vi.fn()}
        onOpenTools={vi.fn()}
      />,
    );
    await screen.findByTestId("health-environment");
    expect(
      screen.queryByTestId("report-bug-from-health"),
    ).not.toBeInTheDocument();
  });
});
