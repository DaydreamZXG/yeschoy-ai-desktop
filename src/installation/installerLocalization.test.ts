import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Windows installer localization", () => {
  it("keeps both supported installer formats in Simplified Chinese", () => {
    const config = JSON.parse(
      readFileSync(resolve(process.cwd(), "src-tauri/tauri.conf.json"), "utf8"),
    );

    expect(config.bundle.windows.nsis).toEqual({
      languages: ["SimpChinese"],
      displayLanguageSelector: false,
      installerHooks: "windows/installer-hooks.nsh",
    });
    expect(config.bundle.windows.wix.language).toBe("zh-CN");
  });

  it("lets installers close the exact running assistant without making beginners find a hidden process", () => {
    const hook = readFileSync(
      resolve(process.cwd(), "src-tauri/windows/installer-hooks.nsh"),
      "utf8",
    );
    const standalone = readFileSync(
      resolve(process.cwd(), "src-tauri/windows/installer.nsi"),
      "utf8",
    );
    const native = readFileSync(
      resolve(process.cwd(), "src-tauri/src/lib.rs"),
      "utf8",
    );

    const eventName = "Local\\YesChoyDesktopInstallerShutdown_v1";
    expect(hook).toContain(eventName);
    expect(native).toContain(eventName.replace("\\", "\\\\"));
    expect(hook).toContain("GetWindowThreadProcessId");
    expect(hook).toContain("OpenProcess");
    expect(hook).toContain("WaitForSingleObject");
    expect(hook).toContain('taskkill.exe\" /PID $1 /T /F');
    expect(hook).not.toMatch(/taskkill\.exe[^\r\n]*\/IM/i);
    expect(hook).toContain("旧版本在任务管理器中可能显示为“CC Switch”");
    expect(hook).toContain("只会结束该窗口对应的准确进程及其子进程");
    expect(standalone).toContain('!include "installer-hooks.nsh"');
    expect(standalone).toContain("Call YeschoyEnsureStopped");
  });
});
