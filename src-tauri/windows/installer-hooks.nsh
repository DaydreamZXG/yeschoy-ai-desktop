!include "Win\RestartManager.nsh"

; Tauri 的模板只给安装程序设了图标，卸载程序（uninstall.exe）还是 NSIS 默认的。
!define MUI_UNICON "${__FILEDIR__}\..\icons\icon.ico"

!define KAIFAN_INSTALLER_SHUTDOWN_EVENT "Local\YesChoyDesktopInstallerShutdown_v1"
; 只出过测试版的“开饭”等的是这个名字；两个都发，哪一个开着都能自己安全退出。
!define KAIFAN_OLD_INSTALLER_SHUTDOWN_EVENT "Local\KaifanDesktopInstallerShutdown_v1"

; Current standalone packages and the earlier Tauri packages used different
; install directories and executable names. Treat every known combination as
; one upgrade family, but never terminate an unrelated process by image name.
Var KaifanLockState
Var KaifanProbeRoot
Var KaifanShutdownPath
Var KaifanShutdownError
Var KaifanRegisteredRoot
Var KaifanOldRegisteredRoot
Var KaifanPriorRegisteredRoot
Var KaifanPriorTauriRegisteredRoot
Var KaifanTestRegisteredRoot
Var KaifanTestTauriRegisteredRoot
Var KaifanTauriRegisteredRoot
Var KaifanWaitTicks
Var KaifanSafeExitRequested

; Returns 0 when a missing/existing file can be replaced, and 1 when Windows
; still has the executable mapped or the installer cannot obtain write access.
Function KaifanCanReplaceFile
  Exch $0
  Push $1

  IfFileExists "$0" kaifan_probe_file kaifan_probe_clear

kaifan_probe_file:
  System::Call 'kernel32::CreateFileW(w r0, i 0x40000000, i 0, p 0, i 3, i 0x80, p 0) p.r1'
  StrCmp $1 -1 kaifan_probe_locked
  System::Call 'kernel32::CloseHandle(p r1)'

kaifan_probe_clear:
  StrCpy $0 0
  Goto kaifan_probe_done

kaifan_probe_locked:
  StrCpy $0 1

kaifan_probe_done:
  Pop $1
  Exch $0
FunctionEnd

; Accumulates the lock state for every binary name shipped by a supported old
; or current package. Early builds may still use legacy executable names on disk,
; and Task Manager may show an older PE description, so path ownership rather
; than a window caption or display name is the source of truth.
Function KaifanProbeRootForLocks
  Push $0
  StrCmp $KaifanProbeRoot "" kaifan_probe_root_done

  Push "$KaifanProbeRoot\YesChoy.exe"
  Call KaifanCanReplaceFile
  Pop $0
  StrCmp $0 1 kaifan_probe_root_locked

  Push "$KaifanProbeRoot\野菜.exe"
  Call KaifanCanReplaceFile
  Pop $0
  StrCmp $0 1 kaifan_probe_root_locked

  Push "$KaifanProbeRoot\开饭.exe"
  Call KaifanCanReplaceFile
  Pop $0
  StrCmp $0 1 kaifan_probe_root_locked

  Push "$KaifanProbeRoot\kaifan-desktop.exe"
  Call KaifanCanReplaceFile
  Pop $0
  StrCmp $0 1 kaifan_probe_root_locked

  Push "$KaifanProbeRoot\野菜API.exe"
  Call KaifanCanReplaceFile
  Pop $0
  StrCmp $0 1 kaifan_probe_root_locked

  Push "$KaifanProbeRoot\yeschoy-desktop.exe"
  Call KaifanCanReplaceFile
  Pop $0
  StrCmp $0 1 kaifan_probe_root_locked

  Push "$KaifanProbeRoot\CC Switch.exe"
  Call KaifanCanReplaceFile
  Pop $0
  StrCmp $0 1 kaifan_probe_root_locked

  Push "$KaifanProbeRoot\cc-switch.exe"
  Call KaifanCanReplaceFile
  Pop $0
  StrCmp $0 1 kaifan_probe_root_locked kaifan_probe_root_done

kaifan_probe_root_locked:
  StrCpy $KaifanLockState 1

kaifan_probe_root_done:
  Pop $0
FunctionEnd

Function KaifanRefreshLockState
  StrCpy $KaifanLockState 0

  ; 现在的安装位置（独立安装包 / Tauri 生成的安装包）。
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\Programs\YesChoy"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\YesChoy"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$KaifanOldRegisteredRoot"
  Call KaifanProbeRootForLocks

  ; 改名 YesChoy 之前叫“野菜”的内测版。
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\Programs\野菜"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\野菜"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$KaifanPriorRegisteredRoot"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$KaifanPriorTauriRegisteredRoot"
  Call KaifanProbeRootForLocks

  ; 只出过测试版的“开饭”。
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\Programs\开饭"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\开饭"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$KaifanTestRegisteredRoot"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$KaifanTestTauriRegisteredRoot"
  Call KaifanProbeRootForLocks

  ; “野菜API”：0.4.10+ standalone packages.
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\Programs\野菜API"
  Call KaifanProbeRootForLocks

  ; 0.4.7/0.4.8 and future Tauri-generated current-user packages.
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\野菜API"
  Call KaifanProbeRootForLocks

  ; The directory selected by this installer, plus both historic registry
  ; layouts in case a user chose a non-default directory.
  StrCpy $KaifanProbeRoot "$INSTDIR"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$KaifanRegisteredRoot"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$KaifanTauriRegisteredRoot"
  Call KaifanProbeRootForLocks
FunctionEnd

Function KaifanShutdownOneFile
  Push $0
  IfFileExists "$KaifanShutdownPath" 0 kaifan_shutdown_one_done
  !insertmacro RestartManager_ShutdownFile "$KaifanShutdownPath" $0
  StrCmp $0 0 kaifan_shutdown_one_done
  StrCmp $KaifanShutdownError 0 0 kaifan_shutdown_one_done
  StrCpy $KaifanShutdownError $0

kaifan_shutdown_one_done:
  Pop $0
FunctionEnd

Function KaifanShutdownRoot
  StrCmp $KaifanProbeRoot "" kaifan_shutdown_root_done

  StrCpy $KaifanShutdownPath "$KaifanProbeRoot\YesChoy.exe"
  Call KaifanShutdownOneFile
  StrCpy $KaifanShutdownPath "$KaifanProbeRoot\野菜.exe"
  Call KaifanShutdownOneFile
  StrCpy $KaifanShutdownPath "$KaifanProbeRoot\开饭.exe"
  Call KaifanShutdownOneFile
  StrCpy $KaifanShutdownPath "$KaifanProbeRoot\kaifan-desktop.exe"
  Call KaifanShutdownOneFile
  StrCpy $KaifanShutdownPath "$KaifanProbeRoot\野菜API.exe"
  Call KaifanShutdownOneFile
  StrCpy $KaifanShutdownPath "$KaifanProbeRoot\yeschoy-desktop.exe"
  Call KaifanShutdownOneFile
  StrCpy $KaifanShutdownPath "$KaifanProbeRoot\CC Switch.exe"
  Call KaifanShutdownOneFile
  StrCpy $KaifanShutdownPath "$KaifanProbeRoot\cc-switch.exe"
  Call KaifanShutdownOneFile

kaifan_shutdown_root_done:
FunctionEnd

Function KaifanShutdownKnownInstallations
  StrCpy $KaifanShutdownError 0

  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\Programs\YesChoy"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\YesChoy"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$KaifanOldRegisteredRoot"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\Programs\野菜"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\野菜"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$KaifanPriorRegisteredRoot"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$KaifanPriorTauriRegisteredRoot"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\Programs\开饭"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\开饭"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$KaifanTestRegisteredRoot"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$KaifanTestTauriRegisteredRoot"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\Programs\野菜API"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\野菜API"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$INSTDIR"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$KaifanRegisteredRoot"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$KaifanTauriRegisteredRoot"
  Call KaifanShutdownRoot
FunctionEnd

; Waits for the executable mappings themselves to disappear. A window can be
; hidden or destroyed while its process is still draining, so it is not a safe
; upgrade boundary.
Function KaifanWaitForUnlock
  Push $0
  StrCpy $0 $KaifanWaitTicks

kaifan_wait_for_unlock:
  Call KaifanRefreshLockState
  StrCmp $KaifanLockState 0 kaifan_wait_for_unlock_done
  StrCmp $0 0 kaifan_wait_for_unlock_done
  Sleep 250
  IntOp $0 $0 - 1
  Goto kaifan_wait_for_unlock

kaifan_wait_for_unlock_done:
  Pop $0
FunctionEnd

; Stops every supported installed copy before the caller writes a single byte.
; New versions receive a cooperative named-event request first. Older versions
; are closed through Windows Restart Manager, which operates on the exact
; executable file resource and also covers hidden windows and multiple copies.
Function KaifanEnsureStopped
  Push $0
  Push $1

  SetRegView 64
  ReadRegStr $KaifanRegisteredRoot HKCU "Software\yeschoy\YesChoyStandalone" "InstallDir"
  ; 以前的名字登记过的位置；用户当时选了别的目录也能找到。
  ReadRegStr $KaifanPriorRegisteredRoot HKCU "Software\野菜" "InstallDir"
  ReadRegStr $KaifanPriorTauriRegisteredRoot HKCU "Software\yeschoy\野菜" ""
  ReadRegStr $KaifanTestRegisteredRoot HKCU "Software\Kaifan" "InstallDir"
  ReadRegStr $KaifanTestTauriRegisteredRoot HKCU "Software\kaifan\开饭" ""
  ReadRegStr $KaifanOldRegisteredRoot HKCU "Software\野菜API" "InstallDir"
  ReadRegStr $KaifanTauriRegisteredRoot HKCU "Software\yeschoy\野菜API" ""

kaifan_check_running:
  Call KaifanRefreshLockState
  StrCmp $KaifanLockState 0 kaifan_stopped

  IfSilent kaifan_request_safe_exit
  MessageBox MB_ICONEXCLAMATION|MB_OKCANCEL "检测到 YesChoy（或旧版“野菜API”）仍在运行。请先保存正在进行的操作。点击“确定”后，安装程序会先等待应用安全退出；如果旧版不支持安全退出，会自动结束对应的已安装程序，然后继续升级。" IDOK kaifan_request_safe_exit IDCANCEL kaifan_cancel

kaifan_request_safe_exit:
  ; YesChoy 和“开饭”测试版可能同时开着，各听各的事件：两个都要通知到。
  StrCpy $KaifanSafeExitRequested 0
  System::Call 'kernel32::OpenEventW(i 0x0002, i 0, w "${KAIFAN_INSTALLER_SHUTDOWN_EVENT}") p.r0'
  StrCmp $0 0 kaifan_request_old_safe_exit
  System::Call 'kernel32::SetEvent(p r0) i.r1'
  System::Call 'kernel32::CloseHandle(p r0)'
  StrCmp $1 0 kaifan_request_old_safe_exit
  StrCpy $KaifanSafeExitRequested 1

kaifan_request_old_safe_exit:
  System::Call 'kernel32::OpenEventW(i 0x0002, i 0, w "${KAIFAN_OLD_INSTALLER_SHUTDOWN_EVENT}") p.r0'
  StrCmp $0 0 kaifan_safe_exit_requested
  System::Call 'kernel32::SetEvent(p r0) i.r1'
  System::Call 'kernel32::CloseHandle(p r0)'
  StrCmp $1 0 kaifan_safe_exit_requested
  StrCpy $KaifanSafeExitRequested 1

kaifan_safe_exit_requested:
  StrCmp $KaifanSafeExitRequested 0 kaifan_force_exit

  ; Current versions may be finishing an atomic configuration write. Allow up
  ; to 25 seconds for their cooperative shutdown before using the OS fallback.
  StrCpy $KaifanWaitTicks 100
  Call KaifanWaitForUnlock
  StrCmp $KaifanLockState 0 kaifan_stopped

kaifan_force_exit:
  Call KaifanShutdownKnownInstallations
  StrCpy $KaifanWaitTicks 40
  Call KaifanWaitForUnlock
  StrCmp $KaifanLockState 0 kaifan_stopped

  IfSilent kaifan_abort
  MessageBox MB_ICONSTOP|MB_RETRYCANCEL "安装程序无法结束正在运行的 YesChoy，因此尚未修改任何程序文件。请在任务管理器的“详细信息”中结束“YesChoy.exe”“kaifan-desktop.exe”“野菜API.exe”或“yeschoy-desktop.exe”；若列表里仍是早期版本遗留的可执行名，一并结束后点击“重试”。错误代码：$KaifanShutdownError" IDRETRY kaifan_check_running IDCANCEL kaifan_abort

kaifan_cancel:
  MessageBox MB_ICONINFORMATION|MB_OK "安装已取消，正在运行的应用和现有程序文件均未修改。"

kaifan_abort:
  Pop $1
  Pop $0
  Abort

kaifan_stopped:
  ; Let Windows finish releasing the image section before the first File
  ; instruction attempts to replace an older executable.
  Sleep 300
  Pop $1
  Pop $0
FunctionEnd

; Tauri's reinstall page can invoke an older uninstaller before the Install
; section begins. Stop the app before that page is shown, then run the same
; idempotent guard again immediately before file copying for race protection.
Function KaifanInstallerGuiInit
  Call KaifanEnsureStopped
FunctionEnd
!define MUI_CUSTOMFUNCTION_GUIINIT KaifanInstallerGuiInit

!macro NSIS_HOOK_PREINSTALL
  Call KaifanEnsureStopped
!macroend

; “开饭”内测版是 Tauri 生成的安装器装的：程序在 $LOCALAPPDATA\开饭（用户当时
; 选了别的目录，就在登记的那个位置），快捷方式直接放在开始菜单和桌面上。新文件
; 都写好之后把它清掉，免得电脑上留着两个客户端。只删它自己放的那两个文件；目录
; 里还有别的东西就留着（RMDir 不带 /r）。登录、密钥和接入记录不在这里，由客户端
; 第一次启动时搬。
Function KaifanRetireTestBuild
  Push $R0
  ReadRegStr $R0 HKCU "Software\kaifan\开饭" ""
  StrCmp $R0 "" 0 kaifan_retire_test_root_known
  StrCpy $R0 "$LOCALAPPDATA\开饭"

kaifan_retire_test_root_known:
  Delete "$R0\kaifan-desktop.exe"
  ; 装在同一个目录里时，uninstall.exe 是现在这个安装器刚写的，不能删。
  StrCmp $R0 "$INSTDIR" kaifan_retire_test_files_done
  Delete "$R0\uninstall.exe"
  RMDir "$R0"

kaifan_retire_test_files_done:
  Delete "$DESKTOP\开饭.lnk"
  Delete "$SMPROGRAMS\开饭.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\开饭"
  DeleteRegKey HKCU "Software\Classes\kaifan"
  DeleteRegKey HKCU "Software\kaifan\开饭"
  DeleteRegKey /ifempty HKCU "Software\kaifan"
  Pop $R0
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  Call KaifanRetireTestBuild
!macroend

; 卸载时勾了「删除应用数据」：Tauri 自己只删 $APPDATA / $LOCALAPPDATA 下按 bundle id
; 命名的目录。日志、~/.yeschoy 和系统凭据管理器里的密钥都在别处，这里补上。
; ~/.yeschoy/cli-workspace 是命令行应用默认的工作文件夹，里面可能有用户自己的东西，不删。
; Claude Code 原登录的备份（displaced-login）是用户自己的登录，也不删。
; 还有应用没恢复（connection-recovery 里有 .sealed）时，~/.yeschoy 和凭据都不动，只删日志：
; 那些应用的设置还是改过的样子，重装后要靠这些备份恢复，并收回站点发的密钥。
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$LOCALAPPDATA\YesChoy\logs"
    RmDir "$LOCALAPPDATA\YesChoy"
    nsExec::Exec `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$$d = Join-Path $$env:USERPROFILE '.yeschoy'; $$k = [bool](Get-ChildItem -LiteralPath (Join-Path $$d 'connection-recovery') -Filter '*.sealed' -Force -ErrorAction SilentlyContinue); if (-not $$k -and (Test-Path -LiteralPath $$d)) { Get-ChildItem -LiteralPath $$d -Force | Where-Object { $$_.Name -ne 'cli-workspace' } | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue; if (-not (Get-ChildItem -LiteralPath $$d -Force)) { Remove-Item -LiteralPath $$d -Force -ErrorAction SilentlyContinue } }; if (-not $$k) { cmdkey /list | ForEach-Object { if ($$_ -match 'target=(\S*com\.(yeschoy|kaifan)\.desktop\S*)' -and $$_ -notmatch 'displaced-login') { cmdkey /delete:$$($$Matches[1]) | Out-Null } } }"`
    Pop $0
  ${EndIf}
!macroend
