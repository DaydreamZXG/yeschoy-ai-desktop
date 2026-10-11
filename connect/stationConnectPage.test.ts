import { describe, expect, it } from "vitest";
import {
  canonicalStationHttpsOrigin,
  lineIdForStationConnectOrigin,
  stationConnectPageUrl,
  STATION_CONNECT_PAGE_PATH,
} from "./stationConnectPage";

describe("STATION_CONNECT_PAGE_PATH", () => {
  it("is the documented fixed path", () => {
    expect(STATION_CONNECT_PAGE_PATH).toBe("/desktop/connect");
  });
});

describe("stationConnectPageUrl", () => {
  it("appends the connect page path to the origin", () => {
    expect(stationConnectPageUrl("https://example.com")).toBe(
      "https://example.com/desktop/connect",
    );
  });
});

describe("lineIdForStationConnectOrigin", () => {
  it("prefixes the canonical origin", () => {
    expect(lineIdForStationConnectOrigin("https://ai.example.com")).toBe(
      "station_connect:https://ai.example.com",
    );
  });
});

describe("canonicalStationHttpsOrigin", () => {
  it("accepts bare domains", () => {
    expect(canonicalStationHttpsOrigin("Example.COM")).toBe("https://example.com");
  });

  it("accepts https URLs and strips paths for convenience", () => {
    expect(canonicalStationHttpsOrigin("https://station.test/login")).toBe(
      "https://station.test",
    );
    expect(
      canonicalStationHttpsOrigin("https://station.test/desktop/connect"),
    ).toBe("https://station.test");
  });

  it("rejects http and literal IPs", () => {
    expect(canonicalStationHttpsOrigin("http://yeschoy.com")).toBeNull();
    expect(canonicalStationHttpsOrigin("https://127.0.0.1")).toBeNull();
    expect(canonicalStationHttpsOrigin("192.168.0.2")).toBeNull();
  });

  it("rejects non-default ports", () => {
    expect(canonicalStationHttpsOrigin("https://host:8443")).toBeNull();
  });
});
