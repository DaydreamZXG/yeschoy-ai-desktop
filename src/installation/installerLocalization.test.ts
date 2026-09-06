import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Windows installer localization", () => {
  it("keeps both supported installer formats in Simplified Chinese", () => {
    const config = JSON.parse(
      readFileSync(
        resolve(process.cwd(), "src-tauri/tauri.conf.json"),
        "utf8",
      ),
    );

    expect(config.bundle.windows.nsis).toEqual({
      languages: ["SimpChinese"],
      displayLanguageSelector: false,
    });
    expect(config.bundle.windows.wix.language).toBe("zh-CN");
  });
});
