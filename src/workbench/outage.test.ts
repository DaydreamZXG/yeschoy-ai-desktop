import { describe, expect, it } from "vitest";
import { outageIsTotal } from "./AppLibraryView";

describe("converging an outage notice", () => {
  it("converges only when every source is down", () => {
    expect(outageIsTotal(true, true, true)).toBe(true);
  });

  // Each of these still has something specific and actionable to say, and
  // collapsing them into one "cannot reach the service" would throw that away.
  it.each([
    ["account only", true, false, false],
    ["local scan only", false, true, false],
    ["connection state only", false, false, true],
    ["account and scan", true, true, false],
    ["account and connections", true, false, true],
    ["scan and connections", false, true, true],
    ["nothing failing", false, false, false],
  ])("keeps the specific message for %s", (_name, a, b, c) => {
    expect(outageIsTotal(a, b, c)).toBe(false);
  });
});
