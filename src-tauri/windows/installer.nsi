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
!include "installer-hooks.nsh"

Name "野菜API"
OutFile "${OUTPUT_EXE}"
InstallDir "$LOCALAPPDATA\Programs\野菜API"
InstallDirRegKey HKCU "Software\野菜API" "InstallDir"
BrandingText "野菜API ${APP_VERSION}"
Icon "${APP_ICON}"
UninstallIcon "${APP_ICON}"

VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey /LANG=2052 "ProductName" "野菜API"
VIAddVersionKey /LANG=2052 "CompanyName" "野菜API"
VIAddVersionKey /LANG=2052 "FileDescription" "野菜API 桌面助手安装程序"
VIAddVersionKey /LANG=2052 "FileVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=2052 "ProductVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=2052 "LegalCopyright" "Copyright (c) 野菜API"

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

Section "安装野菜API" SEC_MAIN
  ; This must remain the first runtime instruction: no file, shortcut or
  ; registry mutation is allowed while an older executable is still mapped.
  Call YeschoyEnsureStopped
  SetShellVarContext current
  SetRegView 64
  SetOutPath "$INSTDIR"
  File /oname=野菜API.exe "${APP_EXE}"
  WriteUninstaller "$INSTDIR\卸载野菜API.exe"
  WriteRegStr HKCU "Software\野菜API" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayName" "野菜API"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "Publisher" "野菜API"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayIcon" "$INSTDIR\野菜API.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "UninstallString" '"$INSTDIR\卸载野菜API.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "NoRepair" 1
  CreateDirectory "$SMPROGRAMS\野菜API"
  CreateShortcut "$SMPROGRAMS\野菜API\野菜API.lnk" "$INSTDIR\野菜API.exe" "" "$INSTDIR\野菜API.exe" 0
  CreateShortcut "$SMPROGRAMS\野菜API\卸载野菜API.lnk" "$INSTDIR\卸载野菜API.exe"
  CreateShortcut "$DESKTOP\野菜API.lnk" "$INSTDIR\野菜API.exe" "" "$INSTDIR\野菜API.exe" 0
SectionEnd

; Every managed connection points at a loopback gateway hosted by 野菜API.
; Upgrades deliberately stop the old process before replacing its executable,
; so a successful install must restore that gateway without relying on an
; optional finish-page checkbox. This also covers silent/manual upgrades.
Function .onInstSuccess
  ClearErrors
  Exec '"$INSTDIR\野菜API.exe"'
  IfErrors 0 yeschoy_launch_done
  IfSilent yeschoy_launch_done
  MessageBox MB_ICONEXCLAMATION|MB_OK "野菜API 已安装完成，但未能自动启动。请从桌面或开始菜单打开野菜API，再继续使用 Codex、Claude 等已接入应用。"

yeschoy_launch_done:
FunctionEnd

Section "Uninstall"
  SetShellVarContext current
  SetRegView 64
  Delete "$DESKTOP\野菜API.lnk"
  Delete "$SMPROGRAMS\野菜API\野菜API.lnk"
  Delete "$SMPROGRAMS\野菜API\卸载野菜API.lnk"
  RMDir "$SMPROGRAMS\野菜API"
  Delete "$INSTDIR\野菜API.exe"
  Delete "$INSTDIR\卸载野菜API.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop"
  DeleteRegKey HKCU "Software\野菜API"
SectionEnd
