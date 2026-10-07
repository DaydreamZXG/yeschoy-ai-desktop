import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  readAnnouncements,
  readSeenAnnouncements,
  rememberSeenAnnouncements,
} from "./announcements";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);
const KEY = "yeschoy.announcements.seen.v1";

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("announcement projection", () => {
  it("defaults the optional fields an older client or server leaves out", async () => {
    native.mockResolvedValue({
      available: true,
      notices: [
        { id: "a", title: "旧格式", body: "", severity: "info" },
        {
          id: "b",
          title: "新格式",
          body: "",
          severity: "warning",
          publishedAtEpochMs: 1,
          expiresAtEpochMs: 2,
          banner: true,
          actionLabel: "去充值去充值去充值去充值去充值",
        },
        { id: "c", title: "", body: "没有标题的丢掉" },
      ],
    });
    const result = await readAnnouncements("mainland_optimized");
    expect(result.notices).toEqual([
      {
        id: "a",
        title: "旧格式",
        body: "",
        severity: "info",
        publishedAtEpochMs: 0,
        expiresAtEpochMs: 0,
        banner: false,
        actionLabel: "",
      },
      {
        id: "b",
        title: "新格式",
        body: "",
        severity: "warning",
        publishedAtEpochMs: 1,
        expiresAtEpochMs: 2,
        banner: true,
        actionLabel: "去充值去充值去充值去充值去充值".slice(0, 16),
      },
    ]);
  });
});

describe("read announcements", () => {
  it("survives storage that is broken or holds something else", () => {
    localStorage.setItem(KEY, "{not json");
    expect(readSeenAnnouncements().size).toBe(0);
    localStorage.setItem(KEY, JSON.stringify({ a: 1 }));
    expect(readSeenAnnouncements().size).toBe(0);
    localStorage.setItem(KEY, JSON.stringify(["a", 2, "b"]));
    expect([...readSeenAnnouncements()]).toEqual(["a", "b"]);
  });

  it("keeps only the most recent 200 ids", () => {
    let seen = new Set<string>();
    for (let i = 0; i < 250; i += 1)
      seen = rememberSeenAnnouncements(seen, [`n${i}`]);
    expect(seen.size).toBe(200);
    expect(seen.has("n49")).toBe(false);
    expect(seen.has("n249")).toBe(true);
    expect(readSeenAnnouncements()).toEqual(seen);
  });
});
