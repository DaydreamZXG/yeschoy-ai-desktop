import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  SKIN_KEY,
  applySkinToDocument,
  readSkinPreference,
  writeSkinPreference,
} from "./skin";

beforeEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.skin;
  delete document.documentElement.dataset.skinCharacter;
});

afterEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.skin;
  delete document.documentElement.dataset.skinCharacter;
});

describe("skin preference", () => {
  it("defaults to anime + dafeiyu", () => {
    expect(readSkinPreference()).toEqual({
      mode: "anime",
      character: "dafeiyu",
    });
  });

  it("round-trips through localStorage", () => {
    writeSkinPreference({ mode: "standard", character: "gpt" });
    expect(localStorage.getItem(SKIN_KEY)).toContain("standard");
    expect(readSkinPreference()).toEqual({
      mode: "standard",
      character: "gpt",
    });
  });

  it("applies data-skin attributes on the document element", () => {
    applySkinToDocument({ mode: "anime", character: "claude" });
    expect(document.documentElement.dataset.skin).toBe("anime");
    expect(document.documentElement.dataset.skinCharacter).toBe("claude");

    applySkinToDocument({ mode: "standard", character: "claude" });
    expect(document.documentElement.dataset.skin).toBe("standard");
    expect(document.documentElement.dataset.skinCharacter).toBeUndefined();
  });

  it("ignores corrupt storage and falls back to defaults", () => {
    localStorage.setItem(SKIN_KEY, "{not-json");
    expect(readSkinPreference()).toEqual({
      mode: "anime",
      character: "dafeiyu",
    });
  });
});
