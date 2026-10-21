import { describe, expect, it, vi } from "vitest";
import {
  detectDownloadPlatform,
  detectDownloadVariant,
  resolveDesktopDownloads,
  ERGOU_PERMANENT_INSTALLERS,
} from "./yeschoyDownload";

describe("detectDownloadPlatform", () => {
  it("detects Windows and macOS from user agent", () => {
    expect(detectDownloadPlatform("Mozilla/5.0 Windows NT 10.0")).toBe(
      "windows",
    );
    expect(
      detectDownloadPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)"),
    ).toBe("macos");
    expect(detectDownloadPlatform("Linux")).toBe("other");
  });
});

describe("detectDownloadVariant", () => {
  it("reads partner from query or host", () => {
    expect(
      detectDownloadVariant({
        hostname: "yeschoy.com",
        search: "?variant=partner",
      }),
    ).toBe("partner");
    expect(
      detectDownloadVariant({ hostname: "ai.yeschoy.io", search: "" }),
    ).toBe("partner");
    expect(detectDownloadVariant({ hostname: "yeschoy.com", search: "" })).toBe(
      "official",
    );
  });
});

describe("resolveDesktopDownloads", () => {
  it("prefers GitHub when a published release exists", async () => {
    const fetchImpl = vi.fn(async (url: string) => {
      if (url.includes("api.github.com")) {
        return {
          ok: true,
          json: async () => [
            {
              tag_name: "v0.5.1",
              draft: true,
              published_at: null,
              assets: [],
            },
            {
              tag_name: "v0.4.29",
              draft: false,
              published_at: "2026-10-01T00:00:00Z",
              assets: [
                {
                  name: "yeschoy-0.4.29-official-windows-x86_64-installer.exe",
                  browser_download_url:
                    "https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases/download/v0.4.29/yeschoy-0.4.29-official-windows-x86_64-installer.exe",
                },
                {
                  name: "yeschoy-0.4.29-official-macos-universal-installer.dmg",
                  browser_download_url:
                    "https://github.com/DaydreamZXG/yeschoy-ai-desktop/releases/download/v0.4.29/yeschoy-0.4.29-official-macos-universal-installer.dmg",
                },
              ],
            },
          ],
        };
      }
      if (url.includes("yeschoy-official.json")) {
        return { ok: true, json: async () => ({ version: "0.5.1", platforms: {} }) };
      }
      throw new Error(`unexpected ${url}`);
    }) as typeof fetch;

    const result = await resolveDesktopDownloads({
      variant: "official",
      userAgent: "Windows NT 10.0",
      fetchImpl,
    });
    expect(result.primary?.source).toBe("github");
    expect(result.primary?.version).toBe("0.4.29");
    expect(result.githubUnpublished).toBe(false);
  });

  it("falls back to ergou manifest when GitHub has no published release", async () => {
    const fetchImpl = vi.fn(async (url: string) => {
      if (url.includes("api.github.com")) {
        return {
          ok: true,
          json: async () => [
            { tag_name: "v0.5.1", draft: true, published_at: null, assets: [] },
          ],
        };
      }
      if (url.includes("yeschoy-official.json")) {
        return {
          ok: true,
          json: async () => ({
            version: "0.5.1",
            platforms: {
              "windows-x86_64": {
                url:
                  "https://ergou.qzz.io/updates/releases/official/0.5.1/yeschoy-0.5.1-official-windows-x86_64-installer.exe",
              },
              "macos-universal": {
                url:
                  "https://ergou.qzz.io/updates/releases/official/0.5.1/yeschoy-0.5.1-official-macos-universal-installer.dmg",
              },
            },
          }),
        };
      }
      throw new Error(`unexpected ${url}`);
    }) as typeof fetch;

    const result = await resolveDesktopDownloads({
      variant: "official",
      userAgent: "Macintosh",
      fetchImpl,
    });
    expect(result.githubUnpublished).toBe(true);
    expect(result.primary?.source).toBe("ergou");
    expect(result.primary?.version).toBe("0.5.1");
    expect(result.primary?.url).toContain("ergou.qzz.io");
  });

  it("uses permanent ergou URLs when manifest fetch fails", async () => {
    const fetchImpl = vi.fn(async (url: string) => {
      if (url.includes("api.github.com")) {
        return { ok: false, json: async () => null };
      }
      if (url.includes("yeschoy-official.json")) {
        return { ok: false, json: async () => null };
      }
      throw new Error(`unexpected ${url}`);
    }) as typeof fetch;

    const result = await resolveDesktopDownloads({
      variant: "official",
      userAgent: "Windows NT",
      fetchImpl,
    });
    expect(result.primary?.url).toBe(
      ERGOU_PERMANENT_INSTALLERS.official.windows,
    );
    expect(result.githubUnpublished).toBe(true);
  });
});
