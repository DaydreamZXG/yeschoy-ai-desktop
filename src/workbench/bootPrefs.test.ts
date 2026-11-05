import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  BOOT_KEY,
  BOOT_SESSION_KEY,
  markBootPlayed,
  readBootPreference,
  shouldPlayBoot,
  todayKey,
  writeBootPreference,
} from "./bootPrefs";

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});

afterEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});

describe("boot preference", () => {
  it("defaults to enabled + session frequency", () => {
    expect(readBootPreference()).toMatchObject({
      enabled: true,
      frequency: "session",
    });
  });

  it("shouldPlayBoot respects disabled", () => {
    writeBootPreference({
      enabled: false,
      frequency: "always",
      lastPlayedDay: null,
    });
    expect(shouldPlayBoot()).toBe(false);
  });

  it("session frequency plays once per session", () => {
    writeBootPreference({
      enabled: true,
      frequency: "session",
      lastPlayedDay: null,
    });
    expect(shouldPlayBoot()).toBe(true);
    markBootPlayed(readBootPreference());
    expect(sessionStorage.getItem(BOOT_SESSION_KEY)).toBe("1");
    expect(shouldPlayBoot()).toBe(false);
  });

  it("daily frequency uses local calendar day", () => {
    const now = new Date("2026-11-05T10:00:00");
    writeBootPreference({
      enabled: true,
      frequency: "daily",
      lastPlayedDay: todayKey(now),
    });
    expect(shouldPlayBoot(readBootPreference(), now)).toBe(false);
    expect(
      shouldPlayBoot(readBootPreference(), new Date("2026-11-06T01:00:00")),
    ).toBe(true);
  });

  it("always frequency always returns true when enabled", () => {
    writeBootPreference({
      enabled: true,
      frequency: "always",
      lastPlayedDay: todayKey(),
    });
    sessionStorage.setItem(BOOT_SESSION_KEY, "1");
    expect(shouldPlayBoot()).toBe(true);
    expect(localStorage.getItem(BOOT_KEY)).toContain("always");
  });
});
