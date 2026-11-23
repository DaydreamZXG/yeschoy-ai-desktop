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
      installerIcon: "icons/icon.ico",
      sidebarImage: "windows/installer-sidebar.bmp",
      headerImage: "windows/installer-header.bmp",
      customLanguageFiles: { SimpChinese: "windows/SimpChinese.nsh" },
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
    // “开饭”测试版等的是另一个名字；安装器两个都发。
    expect(hook).toContain("Local\\KaifanDesktopInstallerShutdown_v1");
    // 两个都开着时，各自都要收到通知，不能通知了一个就不管另一个。
    const request = hook.slice(
      hook.indexOf("kaifan_request_safe_exit:"),
      hook.indexOf("kaifan_safe_exit_requested:"),
    );
    expect(request.match(/kernel32::SetEvent/g)).toHaveLength(2);
    expect(request.indexOf("${KAIFAN_INSTALLER_SHUTDOWN_EVENT}")).toBeLessThan(
      request.indexOf("${KAIFAN_OLD_INSTALLER_SHUTDOWN_EVENT}"),
    );
    expect(request).not.toContain("kaifan_force_exit");
    expect(hook).toContain("$LOCALAPPDATA\\Programs\\YesChoy");
    expect(hook).toContain("YesChoy.exe");
    expect(hook).toContain("$LOCALAPPDATA\\Programs\\开饭");
    expect(hook).toContain('HKCU "Software\\Kaifan" "InstallDir"');
    expect(hook).toContain("开饭.exe");
    expect(hook).toContain("kaifan-desktop.exe");
    // 改名 YesChoy 之前叫“野菜”的内测版还开着时，也要先让它退出。
    expect(hook).toContain('"$LOCALAPPDATA\\Programs\\野菜"');
    expect(hook).toContain('"$LOCALAPPDATA\\野菜"');
    expect(hook).toContain('"$KaifanProbeRoot\\野菜.exe"');
    expect(hook).toContain('HKCU "Software\\yeschoy\\野菜" ""');
    expect(hook).toContain("$LOCALAPPDATA\\Programs\\野菜API");
    expect(hook).toContain("$LOCALAPPDATA\\野菜API");
    expect(hook).toContain("野菜API.exe");
    expect(hook).toContain("yeschoy-desktop.exe");
    expect(hook).toContain("CC Switch.exe");
    expect(hook).toContain("cc-switch.exe");
    // User-visible copy must stay neutral; keep legacy exe probe paths only.
    expect(hook).not.toMatch(/显示为[“"]CC Switch[”"]/);
    expect(hook).toContain("早期版本遗留的可执行名");
    expect(hook).toContain("hidden windows and multiple copies");
    expect(hook).toContain("尚未修改任何程序文件");
    expect(hook).toContain(
      "!define MUI_CUSTOMFUNCTION_GUIINIT KaifanInstallerGuiInit",
    );
    expect(hook).toContain("Tauri's reinstall page");
    expect(hook).not.toContain("FindWindow");
    expect(hook).not.toMatch(/taskkill\.exe/i);
    expect(standalone).toContain('!include "installer-hooks.nsh"');
    const guard = standalone.indexOf("Call KaifanEnsureStopped");
    const firstWrite = standalone.indexOf(
      'File /oname=YesChoy.exe "${APP_EXE}"',
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

  it("retires installs under earlier names only after the new files are written", () => {
    const standalone = readSource("src-tauri/windows/installer.nsi");
    const retire = standalone.match(
      /^Function KaifanRetireOldName\n([\s\S]*?)^FunctionEnd/m,
    )?.[1];
    expect(retire).toBeDefined();
    expect(retire).toContain('Delete "$R0\\野菜API.exe"');
    expect(retire).toContain('Delete "$R0\\开饭.exe"');
    expect(retire).toContain("Uninstall\\com.kaifan.desktop");
    expect(retire).toContain('DeleteRegKey HKCU "Software\\Classes\\kaifan"');
    // 只删我们自己放的文件：不递归删目录；卸载列表里现在这一条和 yeschoy:// 的
    // 登记是我们自己的，不能跟着删掉。
    expect(retire).not.toMatch(/RMDir\s+\/r/);
    expect(retire).not.toContain("com.yeschoy.desktop");
    expect(retire).not.toContain("Software\\Classes\\yeschoy");
    const main = standalone.match(
      /^Section "安装 YesChoy" SEC_MAIN\n([\s\S]*?)^SectionEnd/m,
    )?.[1];
    expect(main).toBeDefined();
    expect(main!.trimEnd().endsWith("Call KaifanRetireOldName")).toBe(true);
    expect(main).toContain(
      `"Software\\Classes\\yeschoy\\shell\\open\\command" "" '"$INSTDIR\\YesChoy.exe" "%1"'`,
    );
    expect(main).not.toContain("Software\\Classes\\kaifan");
  });

  it("clears the Tauri-installed 开饭 test build from both installers", () => {
    const hook = readSource("src-tauri/windows/installer-hooks.nsh");
    const retire = hook.match(
      /^Function KaifanRetireTestBuild\n([\s\S]*?)^FunctionEnd/m,
    )?.[1];
    expect(retire).toBeDefined();
    // 它装在哪由它自己的登记说了算，没登记才按默认位置。
    expect(retire).toContain('ReadRegStr $R0 HKCU "Software\\kaifan\\开饭" ""');
    expect(retire).toContain('StrCpy $R0 "$LOCALAPPDATA\\开饭"');
    expect(retire).toContain('Delete "$R0\\kaifan-desktop.exe"');
    expect(retire).toContain('Delete "$R0\\uninstall.exe"');
    expect(retire).toContain('Delete "$SMPROGRAMS\\开饭.lnk"');
    expect(retire).toContain('Delete "$DESKTOP\\开饭.lnk"');
    expect(retire).toContain("Uninstall\\开饭");
    expect(retire).toContain('DeleteRegKey HKCU "Software\\Classes\\kaifan"');
    // 和现在装的是同一个目录时，uninstall.exe 是刚写的那个，不能删。
    expect(retire!.indexOf('StrCmp $R0 "$INSTDIR"')).toBeLessThan(
      retire!.indexOf('Delete "$R0\\uninstall.exe"'),
    );
    expect(retire).not.toMatch(/RMDir\s+\/r/);
    expect(retire).not.toContain("YesChoy");
    expect(retire).not.toContain("yeschoy");

    // 内测包（Tauri 生成的安装器）在新文件写好之后清；独立安装包跟着别的旧名字一起清。
    expect(hook).toMatch(
      /^!macro NSIS_HOOK_POSTINSTALL\n\s+Call KaifanRetireTestBuild\n!macroend/m,
    );
    const standalone = readSource("src-tauri/windows/installer.nsi");
    expect(
      standalone.match(
        /^Function KaifanRetireOldName\n([\s\S]*?)^FunctionEnd/m,
      )?.[1],
    ).toContain("Call KaifanRetireTestBuild");
    // 它还开着就删不掉：装之前要先按它登记的位置找到它并让它退出。
    expect(hook).toContain(
      'ReadRegStr $KaifanTestTauriRegisteredRoot HKCU "Software\\kaifan\\开饭" ""',
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
      'ReadRegStr $R0 HKCU "Software\\yeschoy\\YesChoyStandalone" "InstallDir"',
    );
    expect(initialization).toContain('${GetOptions} $R0 "/UPDATE" $R1');
    expect(initialization).toContain(
      "IfErrors kaifan_manual_install\n  SetSilent silent",
    );
  });

  it("always restarts the assistant after a successful standalone upgrade", () => {
    const standalone = readSource("src-tauri/windows/installer.nsi");

    expect(standalone).toContain("Function .onInstSuccess");
    expect(standalone).toContain(
      'ExecShell "open" "$INSTDIR\\YesChoy.exe" "" SW_SHOWNORMAL',
    );
    expect(standalone).toContain("This also covers silent/manual upgrades");
    expect(standalone).not.toContain("Exec '\"$INSTDIR\\YesChoy.exe\"'");
    expect(standalone).not.toContain("MUI_FINISHPAGE_RUN");
  });
});
