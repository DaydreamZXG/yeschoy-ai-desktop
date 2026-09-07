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

!include "MUI2.nsh"
!include "WinMessages.nsh"

Name "野菜API"
OutFile "${OUTPUT_EXE}"
InstallDir "$LOCALAPPDATA\Programs\野菜API"
InstallDirRegKey HKCU "Software\野菜API" "InstallDir"
BrandingText "野菜API 0.4.10 内测版"
Icon "${APP_ICON}"
UninstallIcon "${APP_ICON}"

VIProductVersion "0.4.10.0"
VIAddVersionKey /LANG=2052 "ProductName" "野菜API"
VIAddVersionKey /LANG=2052 "CompanyName" "野菜API"
VIAddVersionKey /LANG=2052 "FileDescription" "野菜API 桌面助手安装程序"
VIAddVersionKey /LANG=2052 "FileVersion" "0.4.10"
VIAddVersionKey /LANG=2052 "ProductVersion" "0.4.10"
VIAddVersionKey /LANG=2052 "LegalCopyright" "Copyright (c) 野菜API"

!define MUI_ABORTWARNING
!define MUI_ICON "${APP_ICON}"
!define MUI_UNICON "${APP_ICON}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\野菜API.exe"
!define MUI_FINISHPAGE_RUN_TEXT "启动野菜API"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

Function .onInit
  FindWindow $0 "" "野菜API"
  StrCmp $0 0 done
  MessageBox MB_ICONEXCLAMATION|MB_OKCANCEL "检测到野菜API正在运行。请先保存正在进行的操作；点击“确定”后安装程序会请求应用正常退出。" IDOK request_close IDCANCEL cancel
request_close:
  SendMessage $0 ${WM_CLOSE} 0 0 /TIMEOUT=5000
  Sleep 1200
  Goto done
cancel:
  Abort
done:
FunctionEnd

Section "安装野菜API" SEC_MAIN
  SetShellVarContext current
  SetRegView 64
  SetOutPath "$INSTDIR"
  File /oname=野菜API.exe "${APP_EXE}"
  WriteUninstaller "$INSTDIR\卸载野菜API.exe"
  WriteRegStr HKCU "Software\野菜API" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayName" "野菜API"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.yeschoy.desktop" "DisplayVersion" "0.4.10"
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
