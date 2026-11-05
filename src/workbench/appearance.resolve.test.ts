import { describe, expect, it } from "vitest";
import { resolveTheme } from "./appearance";

describe("resolveTheme", () => {
  it("honors explicit light and dark", () => {
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("follows system preference", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("locks warm light when requested", () => {
    expect(resolveTheme("dark", true, { lockLight: true })).toBe("light");
    expect(resolveTheme("system", true, { lockLight: true })).toBe("light");
  });
});
