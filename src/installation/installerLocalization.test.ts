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

  it("stops every supported installed executable before an upgrade writes files", () => {
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
    expect(hook).toContain('!include "Win\\RestartManager.nsh"');
    expect(hook).toContain("RestartManager_ShutdownFile");
    expect(hook).toContain("CreateFileW");
    expect(hook).toContain("$LOCALAPPDATA\\Programs\\野菜API");
    expect(hook).toContain("$LOCALAPPDATA\\野菜API");
    expect(hook).toContain("野菜API.exe");
    expect(hook).toContain("yeschoy-desktop.exe");
    expect(hook).toContain("CC Switch.exe");
    expect(hook).toContain("cc-switch.exe");
    expect(hook).toContain("hidden windows and multiple copies");
    expect(hook).toContain("尚未修改任何程序文件");
    expect(hook).toContain(
      "!define MUI_CUSTOMFUNCTION_GUIINIT YeschoyInstallerGuiInit",
    );
    expect(hook).toContain("Tauri's reinstall page");
    expect(hook).not.toContain("FindWindow");
    expect(hook).not.toMatch(/taskkill\.exe/i);
    expect(standalone).toContain('!include "installer-hooks.nsh"');
    const guard = standalone.indexOf("Call YeschoyEnsureStopped");
    const firstWrite = standalone.indexOf(
      'File /oname=野菜API.exe "${APP_EXE}"',
    );
    expect(guard).toBeGreaterThan(-1);
    expect(firstWrite).toBeGreaterThan(guard);
    expect(standalone).not.toContain("Function .onInit");
  });

  it("always restarts the assistant after a successful standalone upgrade", () => {
    const standalone = readFileSync(
      resolve(process.cwd(), "src-tauri/windows/installer.nsi"),
      "utf8",
    );

    expect(standalone).toContain("Function .onInstSuccess");
    expect(standalone).toContain('Exec \'"$INSTDIR\\野菜API.exe"\'');
    expect(standalone).toContain("This also covers silent/manual upgrades");
    expect(standalone).not.toContain("MUI_FINISHPAGE_RUN");
  });
});
