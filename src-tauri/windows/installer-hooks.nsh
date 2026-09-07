!include "WinMessages.nsh"

!define YESCHOY_INSTALLER_SHUTDOWN_EVENT "Local\YesChoyDesktopInstallerShutdown_v1"

; Stops the exact process owning the 野菜API window. New versions receive a
; named-event request and drain their local gateways before exiting. Versions
; published before this protocol are terminated only after a second, explicit
; confirmation; taskkill is scoped to the PID obtained from that exact window.
Function YeschoyEnsureStopped
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5

  StrCpy $5 0

yeschoy_detect_running:
  FindWindow $0 "" "野菜API"
  StrCmp $0 0 yeschoy_done

  StrCmp $5 0 yeschoy_first_notice yeschoy_request_safe_exit

yeschoy_first_notice:
  StrCpy $5 1
  MessageBox MB_ICONEXCLAMATION|MB_OKCANCEL "检测到野菜API正在运行。旧版本在任务管理器中可能显示为“CC Switch”，也可能位于“后台进程”。请先保存正在进行的操作。点击“确定”后，安装程序会先请求应用安全退出；如果旧版本不支持，安装程序会再次征求你的确认。" IDOK yeschoy_request_safe_exit IDCANCEL yeschoy_cancel

yeschoy_request_safe_exit:
  System::Call 'kernel32::OpenEventW(i 0x0002, i 0, w "${YESCHOY_INSTALLER_SHUTDOWN_EVENT}") p.r1'
  StrCmp $1 0 yeschoy_force_notice
  System::Call 'kernel32::SetEvent(p r1) i.r2'
  System::Call 'kernel32::CloseHandle(p r1)'

  ; Give the application up to 25 seconds to finish an in-flight configuration
  ; write and stop its helper-owned local gateways.
  StrCpy $3 100
yeschoy_wait_safe_exit:
  FindWindow $0 "" "野菜API"
  StrCmp $0 0 yeschoy_detect_running
  StrCmp $3 0 yeschoy_force_notice
  Sleep 250
  IntOp $3 $3 - 1
  Goto yeschoy_wait_safe_exit

yeschoy_force_notice:
  FindWindow $0 "" "野菜API"
  StrCmp $0 0 yeschoy_detect_running
  MessageBox MB_ICONEXCLAMATION|MB_YESNO "当前运行的是不支持安全退出协议的旧版本，或应用未能在限定时间内退出。是否由安装程序结束刚才检测到的这个野菜API进程并继续安装？只会结束该窗口对应的准确进程及其子进程。" IDYES yeschoy_force_exit IDNO yeschoy_cancel

yeschoy_force_exit:
  System::Call 'user32::GetWindowThreadProcessId(p r0, *i .r1) i.r2'
  StrCmp $1 0 yeschoy_failed
  ; Holding this process handle prevents PID reuse between window lookup and
  ; taskkill. The handle also gives us an exact wait target after child cleanup.
  System::Call 'kernel32::OpenProcess(i 0x00100001, i 0, i r1) p.r3'
  StrCmp $3 0 yeschoy_failed
  nsExec::ExecToStack '"$SYSDIR\taskkill.exe" /PID $1 /T /F'
  Pop $2
  Pop $4
  StrCmp $2 0 yeschoy_wait_forced_exit yeschoy_force_exit_failed

yeschoy_force_exit_failed:
  System::Call 'kernel32::CloseHandle(p r3)'
  Goto yeschoy_failed

yeschoy_wait_forced_exit:
  System::Call 'kernel32::WaitForSingleObject(p r3, i 5000) i.r2'
  System::Call 'kernel32::CloseHandle(p r3)'
  StrCmp $2 0 yeschoy_detect_running yeschoy_failed

yeschoy_failed:
  MessageBox MB_ICONSTOP|MB_OK "安装程序未能结束野菜API。请在任务管理器的“详细信息”页结束“野菜API.exe”；非常早期的版本也可能显示为“CC Switch”。随后重新运行安装程序。"
  Goto yeschoy_abort

yeschoy_cancel:
  MessageBox MB_ICONINFORMATION|MB_OK "安装已取消，正在运行的应用没有被结束。"

yeschoy_abort:
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
  Abort

yeschoy_done:
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

!macro NSIS_HOOK_PREINSTALL
  Call YeschoyEnsureStopped
!macroend
