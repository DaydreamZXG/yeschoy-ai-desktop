import { describe, expect, it, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  openStationConnectPage,
  readStationOriginDraftInitial,
  STATION_ORIGIN_DRAFT_KEY,
} from "./openStationConnectPage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("readStationOriginDraftInitial", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.unstubAllEnvs();
  });

  it("prefers a saved draft over distribution defaults", () => {
    localStorage.setItem(
      STATION_ORIGIN_DRAFT_KEY,
      "https://partner.example.com",
    );
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    expect(readStationOriginDraftInitial()).toBe("https://partner.example.com");
  });

  it("defaults to the reference YesChoy origin on official builds", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "official");
    expect(readStationOriginDraftInitial()).toBe("https://yeschoy.com");
  });

  it("leaves the origin empty on OSS builds (no silent official default)", () => {
    vi.stubEnv("VITE_YESCHOY_DISTRIBUTION", "oss");
    expect(readStationOriginDraftInitial()).toBe("");
  });
});

describe("openStationConnectPage", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("opens the browser via Tauri with canonical origin", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    const result = await openStationConnectPage("ai.Example.com");

    expect(result).toEqual({
      ok: true,
      origin: "https://ai.example.com",
      lineId: "station_connect:https://ai.example.com",
      pageUrl: "https://ai.example.com/desktop/connect",
    });
    expect(invoke).toHaveBeenCalledWith(
      "station_connect_open_connect_page_v1",
      {
        origin: "https://ai.example.com",
      },
    );
  });

  it("rejects invalid origins without invoking", async () => {
    const result = await openStationConnectPage("http://bad.test");

    expect(result).toEqual({ ok: false, reason: "invalid_origin" });
    expect(invoke).not.toHaveBeenCalled();
  });
});
