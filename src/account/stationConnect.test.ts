import { describe, expect, it } from "vitest";
import { decodeStationConnectProgress } from "./stationConnect";

describe("decodeStationConnectProgress", () => {
  it("accepts signed_in payloads from the native event", () => {
    expect(
      decodeStationConnectProgress({
        phase: "signed_in",
        origin: "https://yeschoy.com",
        lineId: "mainland_optimized",
        recoverable: false,
      }),
    ).toEqual({
      phase: "signed_in",
      origin: "https://yeschoy.com",
      lineId: "mainland_optimized",
      recoverable: false,
    });
  });

  it("accepts recoverable failures with reason codes", () => {
    expect(
      decodeStationConnectProgress({
        phase: "failed",
        origin: "https://yeschoy.com",
        reasonCode: "connect_code_expired",
        recoverable: true,
      }),
    ).toEqual({
      phase: "failed",
      origin: "https://yeschoy.com",
      reasonCode: "connect_code_expired",
      recoverable: true,
    });
  });

  it("rejects malformed events", () => {
    expect(decodeStationConnectProgress(null)).toBeNull();
    expect(
      decodeStationConnectProgress({ phase: "unknown", recoverable: true }),
    ).toBeNull();
  });
});
