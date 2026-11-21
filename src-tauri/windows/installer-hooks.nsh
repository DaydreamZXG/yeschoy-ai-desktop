!include "Win\RestartManager.nsh"

!define KAIFAN_INSTALLER_SHUTDOWN_EVENT "Local\KaifanDesktopInstallerShutdown_v1"
; 改名前的客户端等的是这个名字；两个都发，新旧客户端都能自己安全退出。
!define KAIFAN_OLD_INSTALLER_SHUTDOWN_EVENT "Local\YesChoyDesktopInstallerShutdown_v1"

; Current standalone packages and the earlier Tauri packages used different
; install directories and executable names. Treat every known combination as
; one upgrade family, but never terminate an unrelated process by image name.
Var KaifanLockState
Var KaifanProbeRoot
Var KaifanShutdownPath
Var KaifanShutdownError
Var KaifanRegisteredRoot
Var KaifanOldRegisteredRoot
Var KaifanTauriRegisteredRoot
Var KaifanWaitTicks

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

  ; 改名后的安装位置（独立安装包 / Tauri 生成的安装包）。
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\Programs\开饭"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\开饭"
  Call KaifanProbeRootForLocks
  StrCpy $KaifanProbeRoot "$KaifanOldRegisteredRoot"
  Call KaifanProbeRootForLocks

  ; 改名前：0.4.10+ standalone packages.
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

  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\Programs\开饭"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$LOCALAPPDATA\开饭"
  Call KaifanShutdownRoot
  StrCpy $KaifanProbeRoot "$KaifanOldRegisteredRoot"
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
  ReadRegStr $KaifanRegisteredRoot HKCU "Software\Kaifan" "InstallDir"
  ; 改名前的两种登记方式；用户当时选了别的目录也能找到。
  ReadRegStr $KaifanOldRegisteredRoot HKCU "Software\野菜API" "InstallDir"
  ReadRegStr $KaifanTauriRegisteredRoot HKCU "Software\yeschoy\野菜API" ""

kaifan_check_running:
  Call KaifanRefreshLockState
  StrCmp $KaifanLockState 0 kaifan_stopped

  IfSilent kaifan_request_safe_exit
  MessageBox MB_ICONEXCLAMATION|MB_OKCANCEL "检测到开饭（或它改名前的旧版“野菜API”）仍在运行。请先保存正在进行的操作。点击“确定”后，安装程序会先等待应用安全退出；如果旧版不支持安全退出，会自动结束对应的已安装程序，然后继续升级。" IDOK kaifan_request_safe_exit IDCANCEL kaifan_cancel

kaifan_request_safe_exit:
  System::Call 'kernel32::OpenEventW(i 0x0002, i 0, w "${KAIFAN_INSTALLER_SHUTDOWN_EVENT}") p.r0'
  StrCmp $0 0 0 kaifan_signal_safe_exit
  System::Call 'kernel32::OpenEventW(i 0x0002, i 0, w "${KAIFAN_OLD_INSTALLER_SHUTDOWN_EVENT}") p.r0'
  StrCmp $0 0 kaifan_force_exit

kaifan_signal_safe_exit:
  System::Call 'kernel32::SetEvent(p r0) i.r1'
  System::Call 'kernel32::CloseHandle(p r0)'
  StrCmp $1 0 kaifan_force_exit

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
  MessageBox MB_ICONSTOP|MB_RETRYCANCEL "安装程序无法结束正在运行的开饭，因此尚未修改任何程序文件。请在任务管理器的“详细信息”中结束“开饭.exe”“kaifan-desktop.exe”“野菜API.exe”或“yeschoy-desktop.exe”；若列表里仍是早期版本遗留的可执行名，一并结束后点击“重试”。错误代码：$KaifanShutdownError" IDRETRY kaifan_check_running IDCANCEL kaifan_abort

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
