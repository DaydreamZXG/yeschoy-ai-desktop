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

Name "开饭"
OutFile "${OUTPUT_EXE}"
InstallDir "$LOCALAPPDATA\Programs\开饭"
InstallDirRegKey HKCU "Software\Kaifan" "InstallDir"
BrandingText "开饭 ${APP_VERSION}"
Icon "${APP_ICON}"
UninstallIcon "${APP_ICON}"

VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey /LANG=2052 "ProductName" "开饭"
VIAddVersionKey /LANG=2052 "CompanyName" "Kaifan contributors"
VIAddVersionKey /LANG=2052 "FileDescription" "开饭安装程序"
VIAddVersionKey /LANG=2052 "FileVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=2052 "ProductVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=2052 "LegalCopyright" "Copyright (c) Kaifan contributors"

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
  ReadRegStr $R0 HKCU "Software\Kaifan" "InstallDir"
  StrCmp $R0 "" +2
  StrCpy $INSTDIR $R0
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/UPDATE" $R1
  IfErrors kaifan_manual_install
  SetSilent silent
kaifan_manual_install:
FunctionEnd

; 改名前叫“野菜API”。新文件都写好之后，把旧名字留下的程序、快捷方式和登记
; 清掉，免得用户电脑上出现两个客户端。只删我们自己放的那几个文件；目录里
; 还有别的东西就留着（RMDir 不带 /r，只删空目录）。登录、密钥和接入记录不
; 在这里，由客户端第一次启动时搬。
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
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop"
  DeleteRegKey HKCU "Software\野菜API"
  Pop $R0
FunctionEnd

Section "安装开饭" SEC_MAIN
  ; This must remain the first runtime instruction: no file, shortcut or
  ; registry mutation is allowed while an older executable is still mapped.
  Call KaifanEnsureStopped
  SetShellVarContext current
  SetRegView 64
  SetOutPath "$INSTDIR"
  File /oname=开饭.exe "${APP_EXE}"
  WriteUninstaller "$INSTDIR\卸载开饭.exe"
  WriteRegStr HKCU "Software\Kaifan" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop" "DisplayName" "开饭"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop" "Publisher" "Kaifan contributors"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop" "DisplayIcon" "$INSTDIR\开饭.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop" "UninstallString" '"$INSTDIR\卸载开饭.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop" "NoRepair" 1
  ; Station connect deep link (mirrors WiX deep_link_protocols registration).
  WriteRegStr HKCU "Software\Classes\kaifan" "" "URL:kaifan protocol"
  WriteRegStr HKCU "Software\Classes\kaifan" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\kaifan\DefaultIcon" "" "$INSTDIR\开饭.exe,0"
  WriteRegStr HKCU "Software\Classes\kaifan\shell\open\command" "" '"$INSTDIR\开饭.exe" "%1"'
  ; 站点网页上“打开客户端”的按钮发的还是改名前的 yeschoy://，所以它也指到这里。
  WriteRegStr HKCU "Software\Classes\yeschoy" "" "URL:yeschoy protocol"
  WriteRegStr HKCU "Software\Classes\yeschoy" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\yeschoy\DefaultIcon" "" "$INSTDIR\开饭.exe,0"
  WriteRegStr HKCU "Software\Classes\yeschoy\shell\open\command" "" '"$INSTDIR\开饭.exe" "%1"'
  CreateDirectory "$SMPROGRAMS\开饭"
  CreateShortcut "$SMPROGRAMS\开饭\开饭.lnk" "$INSTDIR\开饭.exe" "" "$INSTDIR\开饭.exe" 0
  CreateShortcut "$SMPROGRAMS\开饭\卸载开饭.lnk" "$INSTDIR\卸载开饭.exe"
  CreateShortcut "$DESKTOP\开饭.lnk" "$INSTDIR\开饭.exe" "" "$INSTDIR\开饭.exe" 0
  Call KaifanRetireOldName
SectionEnd

; Every managed connection points at a loopback gateway hosted by 开饭.
; Upgrades deliberately stop the old process before replacing its executable,
; so a successful install must restore that gateway without relying on an
; optional finish-page checkbox. This also covers silent/manual upgrades.
Function .onInstSuccess
  ClearErrors
  ; A silent installer itself runs hidden. `Exec` inherits that startup state,
  ; which leaves the freshly restarted desktop assistant invisible after an
  ; automatic or /S upgrade. Ask Explorer to launch it explicitly in the
  ; normal shown state so the user gets an immediate, observable result.
  ExecShell "open" "$INSTDIR\开饭.exe" "" SW_SHOWNORMAL
  IfErrors 0 kaifan_launch_done
  IfSilent kaifan_launch_done
  MessageBox MB_ICONEXCLAMATION|MB_OK "开饭 已安装完成，但未能自动启动。请从桌面或开始菜单打开开饭，再继续使用 Codex、Claude 等已接入应用。"

kaifan_launch_done:
FunctionEnd

Section "Uninstall"
  SetShellVarContext current
  SetRegView 64
  Delete "$DESKTOP\开饭.lnk"
  Delete "$SMPROGRAMS\开饭\开饭.lnk"
  Delete "$SMPROGRAMS\开饭\卸载开饭.lnk"
  RMDir "$SMPROGRAMS\开饭"
  Delete "$INSTDIR\开饭.exe"
  Delete "$INSTDIR\卸载开饭.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.kaifan.desktop"
  DeleteRegKey HKCU "Software\Kaifan"
  DeleteRegKey HKCU "Software\Classes\kaifan"
  DeleteRegKey HKCU "Software\Classes\yeschoy"
SectionEnd
