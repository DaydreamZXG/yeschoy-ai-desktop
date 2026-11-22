Unicode true
ManifestSupportedOS all
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 32

!ifndef APP_EXE
  !error "APP_EXE is required"
!endif
!ifndef OUTPUT_EXE
  !error "OUTPUT_EXE is required"
!endif
!ifndef APP_ICON
  !error "APP_ICON is required"
!endif
!ifndef APP_VERSION
  !error "APP_VERSION is required"
!endif

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "installer-hooks.nsh"

Name "YesChoy"
OutFile "${OUTPUT_EXE}"
InstallDir "$LOCALAPPDATA\Programs\YesChoy"
; Tauri 生成的安装包把它的安装位置记在 Software\yeschoy\YesChoy 的默认值里，
; 这个安装包另用一个键，两种包都装着时卸载一个不会抹掉另一个的记录。
InstallDirRegKey HKCU "Software\yeschoy\YesChoyStandalone" "InstallDir"
BrandingText "YesChoy ${APP_VERSION}"
Icon "${APP_ICON}"
UninstallIcon "${APP_ICON}"

VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey /LANG=2052 "ProductName" "YesChoy"
VIAddVersionKey /LANG=2052 "CompanyName" "YesChoy contributors"
VIAddVersionKey /LANG=2052 "FileDescription" "YesChoy 安装程序"
VIAddVersionKey /LANG=2052 "FileVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=2052 "ProductVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=2052 "LegalCopyright" "Copyright (c) YesChoy contributors"

!define MUI_ABORTWARNING
!define MUI_ICON "${APP_ICON}"
!define MUI_UNICON "${APP_ICON}"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

; Tauri v2 passes /UPDATE for updater launches. Reuse the actual 64-bit
; registration and run without an unattended welcome/directory wizard.
Function .onInit
  SetRegView 64
  ReadRegStr $R0 HKCU "Software\yeschoy\YesChoyStandalone" "InstallDir"
  StrCmp $R0 "" +2
  StrCpy $INSTDIR $R0
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/UPDATE" $R1
  IfErrors kaifan_manual_install
  SetSilent silent
kaifan_manual_install:
FunctionEnd

; 以前用过两个名字：发布过的“野菜API”，和只出过测试版的“开饭”。新文件都写好
; 之后，把这两个名字留下的程序、快捷方式和登记清掉，免得用户电脑上出现两个
; 客户端。只删我们自己放的那几个文件；目录里还有别的东西就留着（RMDir 不带
; /r，只删空目录）。卸载列表里 com.yeschoy.desktop 那一条现在是我们自己的，不删。
; 登录、密钥和接入记录不在这里，由客户端第一次启动时搬。
Function KaifanRetireOldName
  Push $R0
  ReadRegStr $R0 HKCU "Software\野菜API" "InstallDir"
  StrCmp $R0 "" 0 +2
  StrCpy $R0 "$LOCALAPPDATA\Programs\野菜API"
  Delete "$R0\野菜API.exe"
  Delete "$R0\卸载野菜API.exe"
  RMDir "$R0"
  Delete "$DESKTOP\野菜API.lnk"
  Delete "$SMPROGRAMS\野菜API\野菜API.lnk"
  Delete "$SMPROGRAMS\野菜API\卸载野菜API.lnk"
  RMDir "$SMPROGRAMS\野菜API"
  DeleteRegKey HKCU "Software\野菜API"
  ; “开饭”有两种装法：内测包（Tauri 生成的安装器）和独立安装包，都清。
  Call KaifanRetireTestBuild
  ReadRegStr $R0 HKCU "Software\Kaifan" "InstallDir"
  StrCmp $R0 "" 0 +2
  StrCpy $R0 "$LOCALAPPDATA\Programs\开饭"
  Delete "$R0\开饭.exe"
  Delete "$R0\卸载开饭.exe"
  RMDir "$R0"
  Delete "$DESKTOP\开饭.lnk"
  Delete "$SMPROGRAMS\开饭\开饭.lnk"
  Delete "$SMPROGRAMS\开饭\卸载开饭.lnk"
  RMDir "$SMPROGRAMS\开饭"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop"
  DeleteRegKey HKCU "Software\Classes\kaifan"
  DeleteRegKey HKCU "Software\Kaifan"
  Pop $R0
FunctionEnd

Section "安装 YesChoy" SEC_MAIN
  ; This must remain the first runtime instruction: no file, shortcut or
  ; registry mutation is allowed while an older executable is still mapped.
  Call KaifanEnsureStopped
  SetShellVarContext current
  SetRegView 64
  SetOutPath "$INSTDIR"
  File /oname=YesChoy.exe "${APP_EXE}"
  WriteUninstaller "$INSTDIR\卸载 YesChoy.exe"
  WriteRegStr HKCU "Software\yeschoy\YesChoyStandalone" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayName" "YesChoy"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "Publisher" "YesChoy contributors"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayIcon" "$INSTDIR\YesChoy.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "UninstallString" '"$INSTDIR\卸载 YesChoy.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "NoRepair" 1
  ; Station connect deep link (mirrors WiX deep_link_protocols registration).
  WriteRegStr HKCU "Software\Classes\yeschoy" "" "URL:yeschoy protocol"
  WriteRegStr HKCU "Software\Classes\yeschoy" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\yeschoy\DefaultIcon" "" "$INSTDIR\YesChoy.exe,0"
  WriteRegStr HKCU "Software\Classes\yeschoy\shell\open\command" "" '"$INSTDIR\YesChoy.exe" "%1"'
  CreateDirectory "$SMPROGRAMS\YesChoy"
  CreateShortcut "$SMPROGRAMS\YesChoy\YesChoy.lnk" "$INSTDIR\YesChoy.exe" "" "$INSTDIR\YesChoy.exe" 0
  CreateShortcut "$SMPROGRAMS\YesChoy\卸载 YesChoy.lnk" "$INSTDIR\卸载 YesChoy.exe"
  CreateShortcut "$DESKTOP\YesChoy.lnk" "$INSTDIR\YesChoy.exe" "" "$INSTDIR\YesChoy.exe" 0
  Call KaifanRetireOldName
SectionEnd

; Every managed connection points at a loopback gateway hosted by YesChoy.
; Upgrades deliberately stop the old process before replacing its executable,
; so a successful install must restore that gateway without relying on an
; optional finish-page checkbox. This also covers silent/manual upgrades.
Function .onInstSuccess
  ClearErrors
  ; A silent installer itself runs hidden. `Exec` inherits that startup state,
  ; which leaves the freshly restarted desktop assistant invisible after an
  ; automatic or /S upgrade. Ask Explorer to launch it explicitly in the
  ; normal shown state so the user gets an immediate, observable result.
  ExecShell "open" "$INSTDIR\YesChoy.exe" "" SW_SHOWNORMAL
  IfErrors 0 kaifan_launch_done
  IfSilent kaifan_launch_done
  MessageBox MB_ICONEXCLAMATION|MB_OK "YesChoy 已安装完成，但未能自动启动。请从桌面或开始菜单打开 YesChoy，再继续使用 Codex、Claude 等已接入应用。"

kaifan_launch_done:
FunctionEnd

Section "Uninstall"
  SetShellVarContext current
  SetRegView 64
  Delete "$DESKTOP\YesChoy.lnk"
  Delete "$SMPROGRAMS\YesChoy\YesChoy.lnk"
  Delete "$SMPROGRAMS\YesChoy\卸载 YesChoy.lnk"
  RMDir "$SMPROGRAMS\YesChoy"
  Delete "$INSTDIR\YesChoy.exe"
  Delete "$INSTDIR\卸载 YesChoy.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop"
  DeleteRegKey HKCU "Software\yeschoy\YesChoyStandalone"
  DeleteRegKey /ifempty HKCU "Software\yeschoy"
  DeleteRegKey HKCU "Software\Classes\kaifan"
  DeleteRegKey HKCU "Software\Classes\yeschoy"
SectionEnd
