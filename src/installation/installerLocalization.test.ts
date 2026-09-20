import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

/**
 * 读安装器源文件，并把换行归一成 LF。
 *
 * Windows 上 git 默认 `core.autocrlf=true`，检出来的 `.nsi` / `.nsh` 是 CRLF，
 * 于是下面那些写死 `\n` 的正则一条都匹配不上 —— 这个测试就是这么只在 Windows
 * CI 上红的。它要验的是安装器逻辑，不是换行符政策，所以归一化之后两个平台
 * 问的才是同一个问题。
 */
const readSource = (relative: string) =>
  readFileSync(resolve(process.cwd(), relative), "utf8").replace(/\r\n/g, "\n");

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
    const hook = readSource("src-tauri/windows/installer-hooks.nsh");
    const standalone = readSource("src-tauri/windows/installer.nsi");
    const native = readSource("src-tauri/src/lib.rs");

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
    const initialization = standalone.match(
      /^Function \.onInit\n([\s\S]*?)^FunctionEnd/m,
    )?.[1];
    expect(initialization).toBeDefined();
    // Reading the install directory and updater flags is safe before the
    // running-app guard. Initialization must not replace files or launch apps.
    expect(initialization).not.toMatch(
      /^\s*(?:File|WriteReg\w*|Delete\w*|RMDir|SetOutPath|CreateDirectory|CreateShortcut|Exec\w*)\b/m,
    );
  });

  it("handles Tauri updater launches without an unattended directory wizard", () => {
    const standalone = readSource("src-tauri/windows/installer.nsi");
    const initialization =
      standalone.match(/^Function \.onInit\n([\s\S]*?)^FunctionEnd/m)?.[1] ??
      "";
    expect(initialization.indexOf("SetRegView 64")).toBeLessThan(
      initialization.indexOf("ReadRegStr"),
    );
    expect(initialization).toContain(
      'ReadRegStr $R0 HKCU "Software\\野菜API" "InstallDir"',
    );
    expect(initialization).toContain('${GetOptions} $R0 "/UPDATE" $R1');
    expect(initialization).toContain(
      "IfErrors yeschoy_manual_install\n  SetSilent silent",
    );
  });

  it("always restarts the assistant after a successful standalone upgrade", () => {
    const standalone = readSource("src-tauri/windows/installer.nsi");

    expect(standalone).toContain("Function .onInstSuccess");
    expect(standalone).toContain(
      'ExecShell "open" "$INSTDIR\\野菜API.exe" "" SW_SHOWNORMAL',
    );
    expect(standalone).toContain("This also covers silent/manual upgrades");
    expect(standalone).not.toContain("Exec '\"$INSTDIR\\野菜API.exe\"'");
    expect(standalone).not.toContain("MUI_FINISHPAGE_RUN");
  });
});
