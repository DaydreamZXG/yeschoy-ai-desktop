!include "Win\RestartManager.nsh"

!define YESCHOY_INSTALLER_SHUTDOWN_EVENT "Local\YesChoyDesktopInstallerShutdown_v1"

; Current standalone packages and the earlier Tauri packages used different
; install directories and executable names. Treat every known combination as
; one upgrade family, but never terminate an unrelated process by image name.
Var YeschoyLockState
Var YeschoyProbeRoot
Var YeschoyShutdownPath
Var YeschoyShutdownError
Var YeschoyRegisteredRoot
Var YeschoyTauriRegisteredRoot
Var YeschoyWaitTicks

; Returns 0 when a missing/existing file can be replaced, and 1 when Windows
; still has the executable mapped or the installer cannot obtain write access.
Function YeschoyCanReplaceFile
  Exch $0
  Push $1

  IfFileExists "$0" yeschoy_probe_file yeschoy_probe_clear

yeschoy_probe_file:
  System::Call 'kernel32::CreateFileW(w r0, i 0x40000000, i 0, p 0, i 3, i 0x80, p 0) p.r1'
  StrCmp $1 -1 yeschoy_probe_locked
  System::Call 'kernel32::CloseHandle(p r1)'

yeschoy_probe_clear:
  StrCpy $0 0
  Goto yeschoy_probe_done

yeschoy_probe_locked:
  StrCpy $0 1

yeschoy_probe_done:
  Pop $1
  Exch $0
FunctionEnd

; Accumulates the lock state for every binary name shipped by a supported old
; or current package. Task Manager may show the PE description "CC Switch"
; even when the on-disk binary is yeschoy-desktop.exe, so path ownership rather
; than a window caption or display name is the source of truth.
Function YeschoyProbeRootForLocks
  Push $0
  StrCmp $YeschoyProbeRoot "" yeschoy_probe_root_done

  Push "$YeschoyProbeRoot\野菜API.exe"
  Call YeschoyCanReplaceFile
  Pop $0
  StrCmp $0 1 yeschoy_probe_root_locked

  Push "$YeschoyProbeRoot\yeschoy-desktop.exe"
  Call YeschoyCanReplaceFile
  Pop $0
  StrCmp $0 1 yeschoy_probe_root_locked

  Push "$YeschoyProbeRoot\CC Switch.exe"
  Call YeschoyCanReplaceFile
  Pop $0
  StrCmp $0 1 yeschoy_probe_root_locked

  Push "$YeschoyProbeRoot\cc-switch.exe"
  Call YeschoyCanReplaceFile
  Pop $0
  StrCmp $0 1 yeschoy_probe_root_locked yeschoy_probe_root_done

yeschoy_probe_root_locked:
  StrCpy $YeschoyLockState 1

yeschoy_probe_root_done:
  Pop $0
FunctionEnd

Function YeschoyRefreshLockState
  StrCpy $YeschoyLockState 0

  ; 0.4.10+ standalone packages.
  StrCpy $YeschoyProbeRoot "$LOCALAPPDATA\Programs\野菜API"
  Call YeschoyProbeRootForLocks

  ; 0.4.7/0.4.8 and future Tauri-generated current-user packages.
  StrCpy $YeschoyProbeRoot "$LOCALAPPDATA\野菜API"
  Call YeschoyProbeRootForLocks

  ; The directory selected by this installer, plus both historic registry
  ; layouts in case a user chose a non-default directory.
  StrCpy $YeschoyProbeRoot "$INSTDIR"
  Call YeschoyProbeRootForLocks
  StrCpy $YeschoyProbeRoot "$YeschoyRegisteredRoot"
  Call YeschoyProbeRootForLocks
  StrCpy $YeschoyProbeRoot "$YeschoyTauriRegisteredRoot"
  Call YeschoyProbeRootForLocks
FunctionEnd

Function YeschoyShutdownOneFile
  Push $0
  IfFileExists "$YeschoyShutdownPath" 0 yeschoy_shutdown_one_done
  !insertmacro RestartManager_ShutdownFile "$YeschoyShutdownPath" $0
  StrCmp $0 0 yeschoy_shutdown_one_done
  StrCmp $YeschoyShutdownError 0 0 yeschoy_shutdown_one_done
  StrCpy $YeschoyShutdownError $0

yeschoy_shutdown_one_done:
  Pop $0
FunctionEnd

Function YeschoyShutdownRoot
  StrCmp $YeschoyProbeRoot "" yeschoy_shutdown_root_done

  StrCpy $YeschoyShutdownPath "$YeschoyProbeRoot\野菜API.exe"
  Call YeschoyShutdownOneFile
  StrCpy $YeschoyShutdownPath "$YeschoyProbeRoot\yeschoy-desktop.exe"
  Call YeschoyShutdownOneFile
  StrCpy $YeschoyShutdownPath "$YeschoyProbeRoot\CC Switch.exe"
  Call YeschoyShutdownOneFile
  StrCpy $YeschoyShutdownPath "$YeschoyProbeRoot\cc-switch.exe"
  Call YeschoyShutdownOneFile

yeschoy_shutdown_root_done:
FunctionEnd

Function YeschoyShutdownKnownInstallations
  StrCpy $YeschoyShutdownError 0

  StrCpy $YeschoyProbeRoot "$LOCALAPPDATA\Programs\野菜API"
  Call YeschoyShutdownRoot
  StrCpy $YeschoyProbeRoot "$LOCALAPPDATA\野菜API"
  Call YeschoyShutdownRoot
  StrCpy $YeschoyProbeRoot "$INSTDIR"
  Call YeschoyShutdownRoot
  StrCpy $YeschoyProbeRoot "$YeschoyRegisteredRoot"
  Call YeschoyShutdownRoot
  StrCpy $YeschoyProbeRoot "$YeschoyTauriRegisteredRoot"
  Call YeschoyShutdownRoot
FunctionEnd

; Waits for the executable mappings themselves to disappear. A window can be
; hidden or destroyed while its process is still draining, so it is not a safe
; upgrade boundary.
Function YeschoyWaitForUnlock
  Push $0
  StrCpy $0 $YeschoyWaitTicks

yeschoy_wait_for_unlock:
  Call YeschoyRefreshLockState
  StrCmp $YeschoyLockState 0 yeschoy_wait_for_unlock_done
  StrCmp $0 0 yeschoy_wait_for_unlock_done
  Sleep 250
  IntOp $0 $0 - 1
  Goto yeschoy_wait_for_unlock

yeschoy_wait_for_unlock_done:
  Pop $0
FunctionEnd

; Stops every supported installed copy before the caller writes a single byte.
; New versions receive a cooperative named-event request first. Older versions
; are closed through Windows Restart Manager, which operates on the exact
; executable file resource and also covers hidden windows and multiple copies.
Function YeschoyEnsureStopped
  Push $0
  Push $1

  SetRegView 64
  ReadRegStr $YeschoyRegisteredRoot HKCU "Software\野菜API" "InstallDir"
  ReadRegStr $YeschoyTauriRegisteredRoot HKCU "Software\yeschoy\野菜API" ""

yeschoy_check_running:
  Call YeschoyRefreshLockState
  StrCmp $YeschoyLockState 0 yeschoy_stopped

  IfSilent yeschoy_request_safe_exit
  MessageBox MB_ICONEXCLAMATION|MB_OKCANCEL "检测到旧版或后台中的野菜API仍在运行。请先保存正在进行的操作。点击“确定”后，安装程序会先等待应用安全退出；如果旧版不支持安全退出，会自动结束对应的已安装程序，然后继续升级。" IDOK yeschoy_request_safe_exit IDCANCEL yeschoy_cancel

yeschoy_request_safe_exit:
  System::Call 'kernel32::OpenEventW(i 0x0002, i 0, w "${YESCHOY_INSTALLER_SHUTDOWN_EVENT}") p.r0'
  StrCmp $0 0 yeschoy_force_exit
  System::Call 'kernel32::SetEvent(p r0) i.r1'
  System::Call 'kernel32::CloseHandle(p r0)'
  StrCmp $1 0 yeschoy_force_exit

  ; Current versions may be finishing an atomic configuration write. Allow up
  ; to 25 seconds for their cooperative shutdown before using the OS fallback.
  StrCpy $YeschoyWaitTicks 100
  Call YeschoyWaitForUnlock
  StrCmp $YeschoyLockState 0 yeschoy_stopped

yeschoy_force_exit:
  Call YeschoyShutdownKnownInstallations
  StrCpy $YeschoyWaitTicks 40
  Call YeschoyWaitForUnlock
  StrCmp $YeschoyLockState 0 yeschoy_stopped

  IfSilent yeschoy_abort
  MessageBox MB_ICONSTOP|MB_RETRYCANCEL "安装程序无法结束正在运行的野菜API，因此尚未修改任何程序文件。请在任务管理器的“详细信息”中结束“野菜API.exe”或“yeschoy-desktop.exe”；早期版本的应用名称可能显示为“CC Switch”。完成后点击“重试”。错误代码：$YeschoyShutdownError" IDRETRY yeschoy_check_running IDCANCEL yeschoy_abort

yeschoy_cancel:
  MessageBox MB_ICONINFORMATION|MB_OK "安装已取消，正在运行的应用和现有程序文件均未修改。"

yeschoy_abort:
  Pop $1
  Pop $0
  Abort

yeschoy_stopped:
  ; Let Windows finish releasing the image section before the first File
  ; instruction attempts to replace an older executable.
  Sleep 300
  Pop $1
  Pop $0
FunctionEnd

; Tauri's reinstall page can invoke an older uninstaller before the Install
; section begins. Stop the app before that page is shown, then run the same
; idempotent guard again immediately before file copying for race protection.
Function YeschoyInstallerGuiInit
  Call YeschoyEnsureStopped
FunctionEnd
!define MUI_CUSTOMFUNCTION_GUIINIT YeschoyInstallerGuiInit

!macro NSIS_HOOK_PREINSTALL
  Call YeschoyEnsureStopped
!macroend
