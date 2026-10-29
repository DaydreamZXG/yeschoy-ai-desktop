import { afterEach, describe, expect, it } from "vitest";
import {
  CODEX_PROBE_KEY,
  probeRequestedFor,
  readCodexProbeEnabled,
  writeCodexProbeEnabled,
} from "./codexProbe";

afterEach(() => {
  localStorage.clear();
});

describe("codex transport probe preference", () => {
  it("is off unless explicitly turned on", () => {
    expect(readCodexProbeEnabled()).toBe(false);
    localStorage.setItem(CODEX_PROBE_KEY, "something-else");
    expect(readCodexProbeEnabled()).toBe(false);
  });

  it("round-trips and removing the flag restores the default", () => {
    writeCodexProbeEnabled(true);
    expect(readCodexProbeEnabled()).toBe(true);
    writeCodexProbeEnabled(false);
    expect(localStorage.getItem(CODEX_PROBE_KEY)).toBeNull();
    expect(readCodexProbeEnabled()).toBe(false);
  });

  it("survives storage that throws without blocking the UI", () => {
    const getItem = Storage.prototype.getItem;
    const setItem = Storage.prototype.setItem;
    const removeItem = Storage.prototype.removeItem;
    Storage.prototype.getItem = () => {
      throw new Error("denied");
    };
    Storage.prototype.setItem = () => {
      throw new Error("denied");
    };
    Storage.prototype.removeItem = () => {
      throw new Error("denied");
    };
    try {
      expect(readCodexProbeEnabled()).toBe(false);
      expect(() => writeCodexProbeEnabled(true)).not.toThrow();
    } finally {
      Storage.prototype.getItem = getItem;
      Storage.prototype.setItem = setItem;
      Storage.prototype.removeItem = removeItem;
    }
  });

  it("only asks for a probe on Codex", () => {
    expect(probeRequestedFor("codex_desktop", true)).toBe(true);
    expect(probeRequestedFor("codex_desktop", false)).toBe(false);
    for (const tool of ["claude_code", "claude_desktop", "opencode", "pi"]) {
      expect(probeRequestedFor(tool, true)).toBe(false);
    }
  });
});
