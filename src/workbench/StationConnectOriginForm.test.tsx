import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import { StationConnectOriginForm } from "./StationConnectOriginForm";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

beforeAll(async () => {
  await i18n.init({
    lng: "zh",
    resources: { zh: { translation: zh } },
  });
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.unstubAllEnvs();
  invoke.mockReset();
});

describe("StationConnectOriginForm", () => {
  it("prefills the reference origin on official builds", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    render(<StationConnectOriginForm onLineChange={vi.fn()} />);
    const input = screen.getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("https://yeschoy.com");
    expect(screen.getByText(zh.stationConnectPage.originHint)).toBeTruthy();
  });

  it("does not prefill an official station on OSS builds", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "oss");
    render(<StationConnectOriginForm onLineChange={vi.fn()} />);
    const input = screen.getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("");
    expect(screen.getByText(zh.stationConnectPage.originHint)).toBeTruthy();
  });

  it("keeps continue enabled when draft is valid", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    render(<StationConnectOriginForm onLineChange={vi.fn()} />);
    expect(
      screen.getByRole("button", { name: zh.stationConnectPage.continue }),
    ).toBeEnabled();
    expect(
      screen.queryByText(zh.stationConnectPage.accountChecking),
    ).not.toBeInTheDocument();
  });

  it("shows progress then failure with retry when probe is unreachable", async () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    const user = userEvent.setup();
    let release!: (v: unknown) => void;
    invoke.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    render(<StationConnectOriginForm onLineChange={vi.fn()} />);
    const input = screen.getByRole("textbox");
    await user.clear(input);
    await user.type(input, "https://relay.example.com");
    await user.click(
      screen.getByRole("button", { name: zh.stationConnectPage.continue }),
    );
    expect(await screen.findByTestId("station-probing")).toBeTruthy();
    expect(screen.getByText(zh.stationOnboarding.probingTitle)).toBeTruthy();
    expect(
      screen.getByRole("button", { name: zh.stationOnboarding.cancelProbe }),
    ).toBeEnabled();

    release({
      origin: "https://relay.example.com",
      status: "unreachable",
      kind: "unknown",
      systemName: "",
      version: "",
      desktopConnect: false,
      tokenStatus: "not_provided",
      models: [],
    });
    expect(await screen.findByTestId("station-probe-failed")).toBeTruthy();
    expect(
      screen.getByText(zh.stationOnboarding.failed.unreachable.title),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: zh.stationOnboarding.retryProbe }),
    ).toBeEnabled();
  });

  it("cancel during probe clears progress without applying a late result", async () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    const user = userEvent.setup();
    let release!: (v: unknown) => void;
    invoke.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    render(<StationConnectOriginForm onLineChange={vi.fn()} />);
    const input = screen.getByRole("textbox");
    await user.clear(input);
    await user.type(input, "https://relay.example.com");
    await user.click(
      screen.getByRole("button", { name: zh.stationConnectPage.continue }),
    );
    expect(await screen.findByTestId("station-probing")).toBeTruthy();
    await user.click(
      screen.getByRole("button", { name: zh.stationOnboarding.cancelProbe }),
    );
    expect(screen.queryByTestId("station-probing")).not.toBeInTheDocument();
    release({
      origin: "https://relay.example.com",
      status: "ok",
      kind: "new_api",
      systemName: "晚到的结果",
      version: "1",
      desktopConnect: true,
      tokenStatus: "not_provided",
      models: [],
    });
    await waitFor(() => {
      expect(screen.queryByText(/晚到的结果/)).not.toBeInTheDocument();
      expect(screen.queryByTestId("station-probe-failed")).not.toBeInTheDocument();
    });
  });

  it("lists friendly model names on successful token check", async () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    const user = userEvent.setup();
    invoke.mockResolvedValue({
      origin: "https://relay.example.com",
      status: "ok",
      kind: "new_api",
      systemName: "小明的中转",
      version: "v1",
      desktopConnect: false,
      tokenStatus: "not_provided",
      models: [],
    });
    render(<StationConnectOriginForm onLineChange={vi.fn()} />);
    const input = screen.getByRole("textbox");
    await user.clear(input);
    await user.type(input, "https://relay.example.com");
    await user.click(
      screen.getByRole("button", { name: zh.stationConnectPage.continue }),
    );
    expect(await screen.findByText(/找到站点/)).toBeTruthy();

    invoke.mockResolvedValue({
      origin: "https://relay.example.com",
      status: "ok",
      kind: "new_api",
      systemName: "小明的中转",
      version: "v1",
      desktopConnect: false,
      tokenStatus: "valid",
      models: ["claude-sonnet-4-5-20250929", "gpt-6.1-sol-2026-10-preview-long-model-identifier"],
    });
    await user.type(screen.getByPlaceholderText("sk-..."), "sk-test-token");
    await user.click(
      screen.getByRole("button", { name: zh.stationOnboarding.tokenCheck }),
    );
    expect(await screen.findByText("Claude Sonnet 4.5")).toBeTruthy();
    const chips = screen.getByLabelText(zh.stationOnboarding.modelsLabel);
    expect(chips.querySelectorAll("li").length).toBeGreaterThanOrEqual(2);
    for (const li of chips.querySelectorAll("li")) {
      expect(li.getAttribute("title")).toBeTruthy();
    }
  });
});
