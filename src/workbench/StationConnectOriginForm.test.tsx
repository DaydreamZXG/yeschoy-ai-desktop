import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "../i18n/locales/zh.json";
import { StationConnectOriginForm } from "./StationConnectOriginForm";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

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
});

describe("StationConnectOriginForm", () => {
  it("prefills the reference origin on official builds", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    render(<StationConnectOriginForm onLineChange={vi.fn()} />);
    const input = screen.getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("https://yeschoy.com");
    expect(screen.getByText(zh.stationConnectPage.originHint)).toBeTruthy();
  });

  it("prefills the reference origin on OSS builds", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "oss");
    render(<StationConnectOriginForm onLineChange={vi.fn()} />);
    const input = screen.getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("https://yeschoy.com");
    expect(screen.getByText(zh.stationConnectPage.originHint)).toBeTruthy();
  });
});
