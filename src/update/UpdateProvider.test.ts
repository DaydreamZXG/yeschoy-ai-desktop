import { describe, expect, it } from "vitest";
import { canStartInstall, shouldCheckAfterReturning } from "./scheduling";

describe("foreground update checks", () => {
  it("rechecks a visible app only after the bounded cooldown", () => {
    const fiveMinutes = 5 * 60_000;
    expect(shouldCheckAfterReturning("hidden", fiveMinutes, 0)).toBe(false);
    expect(shouldCheckAfterReturning("visible", fiveMinutes - 1, 0)).toBe(
      false,
    );
    expect(shouldCheckAfterReturning("visible", fiveMinutes, 0)).toBe(true);
    expect(
      shouldCheckAfterReturning("visible", fiveMinutes * 2, fiveMinutes),
    ).toBe(true);
  });

  it("stops a second install admission after the synchronous phase change", () => {
    let phase = "available" as const;
    expect(canStartInstall(phase, "0.4.16")).toBe(true);
    const changedPhase: "downloading" = "downloading";
    expect(canStartInstall(changedPhase, "0.4.16")).toBe(false);
    expect(canStartInstall(phase, "")).toBe(false);
  });
});
