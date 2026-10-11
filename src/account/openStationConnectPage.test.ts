import { describe, expect, it, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { openStationConnectPage } from "./openStationConnectPage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

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
    expect(invoke).toHaveBeenCalledWith("station_connect_open_connect_page_v1", {
      origin: "https://ai.example.com",
    });
  });

  it("rejects invalid origins without invoking", async () => {
    const result = await openStationConnectPage("http://bad.test");

    expect(result).toEqual({ ok: false, reason: "invalid_origin" });
    expect(invoke).not.toHaveBeenCalled();
  });
});
