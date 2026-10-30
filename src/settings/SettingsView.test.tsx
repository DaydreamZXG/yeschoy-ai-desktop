import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import { invoke } from "@tauri-apps/api/core";
import { SettingsView } from "./SettingsView";
import { ConnectionProvider } from "../configuration/connections";
import { connectionsFixture } from "../configuration/connection-test-fixtures";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./QuitAssistant", () => ({
  QuitAssistant: () => null,
}));
const native = vi.mocked(invoke);

vi.mock("../update/UpdateSettingsCard", () => ({
  UpdateSettingsCard: () => null,
}));

beforeEach(async () => {
  localStorage.clear();
  vi.unstubAllEnvs();
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  await i18n.changeLanguage("zh");
});

afterEach(() => {
  localStorage.clear();
  vi.unstubAllEnvs();
});

describe("SettingsView", () => {
  it("does not render the obsolete account-unavailable boundary", () => {
    const { container } = render(
      <SettingsView
        onOpenAccount={vi.fn()}
        onOpenDiagnostics={vi.fn()}
        onOpenModels={vi.fn()}
      />,
    );

    expect(container.querySelector(".settings-boundary")).toBeNull();
    expect(container).toHaveTextContent("桌面版");
    expect(zh.yeschoySettings.candidateStage).toBe("桌面版");
    expect(zh.yeschoySettings.candidateStage).not.toContain("测试");
  });

  it("#11 renders advanced read-only connection details for connected tools", async () => {
    await i18n.changeLanguage("zh");
    const connections = {
      ...connectionsFixture("settings-1"),
      loading: false,
      error: false,
      restoring: null,
      opening: null,
      requestId: "settings-1",
      refresh: vi.fn(async () => {}),
      restore: vi.fn(async () => connectionsFixture("restore")),
      open: vi.fn(async () => ({
        status: "opened" as const,
        reasonCode: "opened",
      })),
    };
    Object.assign(
      connections.connections.find((c) => c.toolId === "claude_code")!,
      {
        state: "connected",
        modelId: "glm-5.3",
        lineId: "mainland_optimized",
        billingGroup: "default",
        updatedAtEpochMs: 1_700_000_000_000,
      },
    );
    const clipboardWrite = vi.fn(async (_text: string) => {});
    Object.assign(navigator, { clipboard: { writeText: clipboardWrite } });

    render(
      <ConnectionProvider value={connections}>
        <SettingsView
          onOpenAccount={vi.fn()}
          onOpenDiagnostics={vi.fn()}
          onOpenModels={vi.fn()}
        />
      </ConnectionProvider>,
    );

    const details = document.querySelector(
      "details.advanced-connection-details",
    )!;
    expect(details).not.toHaveAttribute("open");
    fireEvent.click(screen.getByText("高级信息：已接入应用的连接详情"));
    expect(details).toHaveAttribute("open");

    const card = screen
      .getByRole("heading", { name: "Claude Code" })
      .closest("article")!;
    expect(card).toHaveTextContent("https://yeschoy.com");
    expect(card).toHaveTextContent("~/.claude/settings.json");
    expect(card).toHaveTextContent("glm-5.3");
    expect(card).toHaveTextContent("保存在系统安全存储，界面不显示密钥内容");
    // dsh_web 未接入：不出现 DSH 卡片
    expect(
      screen.queryByRole("heading", { name: "DSH web" }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "在 Finder 中查看" }));
    await waitFor(() =>
      expect(native).toHaveBeenCalledWith("open_config_folder", {
        app: "claude",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "复制诊断信息" }));
    await waitFor(() => expect(clipboardWrite).toHaveBeenCalled());
    const text = clipboardWrite.mock.calls[0][0] as string;
    expect(text).toContain("claude_code");
    expect(text).toContain("https://yeschoy.com");
    expect(text).toContain("读取编号: settings-1");
  });

  it("#11 shows the empty note when nothing is connected", async () => {
    await i18n.changeLanguage("zh");
    const connections = {
      ...connectionsFixture("settings-2"),
      loading: false,
      error: false,
      restoring: null,
      opening: null,
      requestId: "settings-2",
      refresh: vi.fn(async () => {}),
      restore: vi.fn(async () => connectionsFixture("restore")),
      open: vi.fn(async () => ({
        status: "opened" as const,
        reasonCode: "opened",
      })),
    };
    render(
      <ConnectionProvider value={connections}>
        <SettingsView
          onOpenAccount={vi.fn()}
          onOpenDiagnostics={vi.fn()}
          onOpenModels={vi.fn()}
        />
      </ConnectionProvider>,
    );
    fireEvent.click(screen.getByText("高级信息：已接入应用的连接详情"));
    expect(
      screen.getByText("尚未接入任何应用，接入后这里会显示实际连接参数。"),
    ).toBeInTheDocument();
  });

  it("shows dismissible official site promo only on official builds", async () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    const { rerender } = render(
      <SettingsView
        onOpenAccount={vi.fn()}
        onOpenDiagnostics={vi.fn()}
        onOpenModels={vi.fn()}
      />,
    );
    expect(screen.getByTestId("official-site-promo")).toHaveTextContent(
      zh.workbench.officialSitePromo,
    );
    expect(screen.queryByTestId("oss-reference-attribution")).toBeNull();

    fireEvent.click(screen.getByTestId("official-site-promo-dismiss"));
    await waitFor(() =>
      expect(screen.queryByTestId("official-site-promo")).toBeNull(),
    );

    rerender(
      <SettingsView
        onOpenAccount={vi.fn()}
        onOpenDiagnostics={vi.fn()}
        onOpenModels={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("official-site-promo")).toBeNull();
  });

  it("keeps OSS attribution without official site promo", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "oss");
    render(
      <SettingsView
        onOpenAccount={vi.fn()}
        onOpenDiagnostics={vi.fn()}
        onOpenModels={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("official-site-promo")).toBeNull();
    expect(screen.getByTestId("oss-reference-attribution")).toBeTruthy();
  });
});
