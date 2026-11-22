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

Name "野菜"
OutFile "${OUTPUT_EXE}"
InstallDir "$LOCALAPPDATA\Programs\野菜"
InstallDirRegKey HKCU "Software\野菜" "InstallDir"
BrandingText "野菜 ${APP_VERSION}"
Icon "${APP_ICON}"
UninstallIcon "${APP_ICON}"

VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey /LANG=2052 "ProductName" "野菜"
VIAddVersionKey /LANG=2052 "CompanyName" "YesChoy contributors"
VIAddVersionKey /LANG=2052 "FileDescription" "野菜安装程序"
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
  ReadRegStr $R0 HKCU "Software\野菜" "InstallDir"
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

Section "安装野菜" SEC_MAIN
  ; This must remain the first runtime instruction: no file, shortcut or
  ; registry mutation is allowed while an older executable is still mapped.
  Call KaifanEnsureStopped
  SetShellVarContext current
  SetRegView 64
  SetOutPath "$INSTDIR"
  File /oname=野菜.exe "${APP_EXE}"
  WriteUninstaller "$INSTDIR\卸载野菜.exe"
  WriteRegStr HKCU "Software\野菜" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayName" "野菜"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "Publisher" "YesChoy contributors"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayIcon" "$INSTDIR\野菜.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "UninstallString" '"$INSTDIR\卸载野菜.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "NoRepair" 1
  ; Station connect deep link (mirrors WiX deep_link_protocols registration).
  WriteRegStr HKCU "Software\Classes\yeschoy" "" "URL:yeschoy protocol"
  WriteRegStr HKCU "Software\Classes\yeschoy" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\yeschoy\DefaultIcon" "" "$INSTDIR\野菜.exe,0"
  WriteRegStr HKCU "Software\Classes\yeschoy\shell\open\command" "" '"$INSTDIR\野菜.exe" "%1"'
  CreateDirectory "$SMPROGRAMS\野菜"
  CreateShortcut "$SMPROGRAMS\野菜\野菜.lnk" "$INSTDIR\野菜.exe" "" "$INSTDIR\野菜.exe" 0
  CreateShortcut "$SMPROGRAMS\野菜\卸载野菜.lnk" "$INSTDIR\卸载野菜.exe"
  CreateShortcut "$DESKTOP\野菜.lnk" "$INSTDIR\野菜.exe" "" "$INSTDIR\野菜.exe" 0
  Call KaifanRetireOldName
SectionEnd

; Every managed connection points at a loopback gateway hosted by 野菜.
; Upgrades deliberately stop the old process before replacing its executable,
; so a successful install must restore that gateway without relying on an
; optional finish-page checkbox. This also covers silent/manual upgrades.
Function .onInstSuccess
  ClearErrors
  ; A silent installer itself runs hidden. `Exec` inherits that startup state,
  ; which leaves the freshly restarted desktop assistant invisible after an
  ; automatic or /S upgrade. Ask Explorer to launch it explicitly in the
  ; normal shown state so the user gets an immediate, observable result.
  ExecShell "open" "$INSTDIR\野菜.exe" "" SW_SHOWNORMAL
  IfErrors 0 kaifan_launch_done
  IfSilent kaifan_launch_done
  MessageBox MB_ICONEXCLAMATION|MB_OK "野菜 已安装完成，但未能自动启动。请从桌面或开始菜单打开野菜，再继续使用 Codex、Claude 等已接入应用。"

kaifan_launch_done:
FunctionEnd

Section "Uninstall"
  SetShellVarContext current
  SetRegView 64
  Delete "$DESKTOP\野菜.lnk"
  Delete "$SMPROGRAMS\野菜\野菜.lnk"
  Delete "$SMPROGRAMS\野菜\卸载野菜.lnk"
  RMDir "$SMPROGRAMS\野菜"
  Delete "$INSTDIR\野菜.exe"
  Delete "$INSTDIR\卸载野菜.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop"
  DeleteRegKey HKCU "Software\野菜"
  DeleteRegKey HKCU "Software\Classes\kaifan"
  DeleteRegKey HKCU "Software\Classes\yeschoy"
SectionEnd
