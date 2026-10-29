import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import zh from "../i18n/locales/zh.json";
import { FeedbackView } from "./FeedbackView";
import { redactSecrets } from "./feedback";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const invokeMock = vi.mocked(invoke);
const clipboardWrite = vi.fn();
const F = zh.yeschoyFeedback;

const snapshot = {
  platform: "windows",
  arch: "x86_64",
  appVersion: "0.5.1",
  frontendMode: "bundled-custom-protocol",
  distribution: "community",
  logAvailable: true,
  logTail: ["1 ERROR mod stage=check_failed"],
  markers: [{ element: "desktop_shell", detail: "stage=start" }],
};

beforeEach(() => {
  invokeMock.mockReset();
  clipboardWrite.mockReset();
  clipboardWrite.mockResolvedValue(undefined);
  Object.assign(navigator, { clipboard: { writeText: clipboardWrite } });
});
afterEach(() => {
  vi.unstubAllEnvs();
  vi.resetModules();
});

function showView() {
  const routes = { diagnostics: vi.fn(), home: vi.fn() };
  render(
    <FeedbackView
      onOpenDiagnostics={routes.diagnostics}
      onOpenHome={routes.home}
    />,
  );
  return routes;
}

describe("feedback view", () => {
  it("prefills an editable report and copies it", async () => {
    invokeMock.mockResolvedValueOnce(snapshot);
    showView();
    const box = (await screen.findByTestId(
      "feedback-report",
    )) as HTMLTextAreaElement;
    await vi.waitFor(() => expect(box).not.toHaveAttribute("readonly"));
    expect(box.value).toContain("env: platform=windows");
    expect(box.value).toContain("stage=check_failed");

    fireEvent.change(box, { target: { value: "点了接入没反应" } });
    fireEvent.click(screen.getByTestId("copy-feedback-report"));
    await screen.findByText(F.copied);
    expect(clipboardWrite).toHaveBeenCalledWith("点了接入没反应");
  });

  it("redacts a key the user pasted into the report on copy only", async () => {
    invokeMock.mockResolvedValueOnce(snapshot);
    showView();
    const box = (await screen.findByTestId(
      "feedback-report",
    )) as HTMLTextAreaElement;
    fireEvent.change(box, {
      target: { value: "我填的 key: sk-live-abcdef1234567890 报错" },
    });
    fireEvent.click(screen.getByTestId("copy-feedback-report"));
    await screen.findByText(F.copied);
    // `sk-` 保留为前缀，后面的内容被打掉，足够让人认出这里原本是个密钥。
    expect(clipboardWrite.mock.calls[0][0]).toBe(
      "我填的 key: sk-[redacted] 报错",
    );
    // What the user typed stays visible so they notice the mistake.
    expect(box.value).toContain("sk-live-abcdef1234567890");
  });

  it("links the community build to the public issue tracker, not the official site", async () => {
    invokeMock.mockResolvedValueOnce(snapshot);
    showView();
    const link = await screen.findByTestId("open-feedback-channel");
    expect(link).toHaveAttribute("data-channel", "community");
    expect(link.getAttribute("href")).not.toContain("yeschoy.app");
    expect(link.getAttribute("href")).toMatch(/^https:\/\/github\.com\//);
    expect(link).toHaveAttribute("target", "_blank");
  });

  it("links official builds to the official feedback page", async () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    vi.resetModules();
    invokeMock.mockResolvedValueOnce(snapshot);
    const { FeedbackView: OfficialFeedbackView } = await import(
      "./FeedbackView"
    );
    render(
      <OfficialFeedbackView onOpenDiagnostics={vi.fn()} onOpenHome={vi.fn()} />,
    );
    const link = await screen.findByTestId("open-feedback-channel");
    expect(link).toHaveAttribute("data-channel", "official");
    expect(link.getAttribute("href")).toMatch(/^https:\/\/yeschoy\.app\//);
  });

  it("still offers a report when the native snapshot fails", async () => {
    invokeMock.mockRejectedValueOnce(new Error("ipc"));
    showView();
    const box = (await screen.findByTestId(
      "feedback-report",
    )) as HTMLTextAreaElement;
    await vi.waitFor(() => expect(box).not.toHaveAttribute("readonly"));
    expect(box.value).toContain("env: unavailable");
  });

  it("reports a clipboard failure", async () => {
    clipboardWrite.mockRejectedValueOnce(new Error("denied"));
    invokeMock.mockResolvedValueOnce(snapshot);
    showView();
    const box = await screen.findByTestId("feedback-report");
    await vi.waitFor(() => expect(box).not.toHaveAttribute("readonly"));
    fireEvent.click(screen.getByTestId("copy-feedback-report"));
    await screen.findByText(zh.yeschoyDiagnostics.copyFailed);
  });

  it("routes to the health assistant and home", async () => {
    invokeMock.mockResolvedValueOnce(snapshot);
    const routes = showView();
    fireEvent.click(
      await screen.findByRole("button", { name: F.openDiagnostics }),
    );
    fireEvent.click(screen.getByRole("button", { name: F.openHome }));
    expect(routes.diagnostics).toHaveBeenCalledOnce();
    expect(routes.home).toHaveBeenCalledOnce();
  });
});

describe("redactSecrets", () => {
  it("masks keys, JWTs, bearer tokens and key=value secrets", () => {
    const text = [
      "key sk-abcdefgh12345678",
      "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.c2lnbmF0dXJl",
      "Authorization: Bearer abcdefghijklmnopqrstuvwxyz",
      "api_key = 'hello-world-123'",
      'password:"hunter2hunter2"',
    ].join("\n");
    const masked = redactSecrets(text);
    for (const secret of [
      "sk-abcdefgh12345678",
      "eyJhbGciOiJIUzI1NiJ9",
      "abcdefghijklmnopqrstuvwxyz",
      "hello-world-123",
      "hunter2hunter2",
    ]) {
      expect(masked).not.toContain(secret);
    }
    expect(masked).toContain("[redacted]");
  });

  it("leaves ordinary prose alone", () => {
    const text =
      "安装后点接入，Claude Desktop 没有反应。stage=ready 不是密钥。";
    expect(redactSecrets(text)).toBe(text);
  });
});
