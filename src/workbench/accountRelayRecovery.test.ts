import { describe, expect, it } from "vitest";
import {
  accountStatusSuggestsRelayLineRetry,
  alternateRelayLine,
  isRelayCatalogLine,
} from "./accountRelayRecovery";

describe("accountRelayRecovery", () => {
  it.each([
    "backend_unavailable",
    "network_error",
    "incompatible_server",
  ] as const)("suggests relay retry for %s", (status) => {
    expect(accountStatusSuggestsRelayLineRetry(status)).toBe(true);
  });

  it.each(["signed_out", "signed_in", "invalid_response"] as const)(
    "does not suggest relay retry for %s",
    (status) => {
      expect(accountStatusSuggestsRelayLineRetry(status)).toBe(false);
    },
  );

  it("alternates catalog relay lines", () => {
    expect(alternateRelayLine("mainland_optimized")).toBe("global_accelerated");
    expect(alternateRelayLine("global_accelerated")).toBe("mainland_optimized");
  });

  it("recognizes built-in relay line ids", () => {
    expect(isRelayCatalogLine("mainland_optimized")).toBe(true);
    expect(isRelayCatalogLine("station_connect:https://ai.example.com")).toBe(
      false,
    );
  });
});
