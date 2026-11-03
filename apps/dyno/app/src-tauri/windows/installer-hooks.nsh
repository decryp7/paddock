; Dyno's NSIS hooks (bundle.windows.nsis.installerHooks). The installer runs as the user (currentUser), because the
; agent's start-at-login entry lives in the user's own registry hive. It never registers that entry itself (the user
; opts in under Settings); it stops a running agent before its binary is replaced or removed, and removes the entry
; on uninstall.

!macro NSIS_HOOK_PREINSTALL
  IfFileExists "$INSTDIR\dyno-agent.exe" 0 +3
    nsExec::ExecToLog '"$INSTDIR\dyno-agent.exe" stop'
    Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  IfFileExists "$INSTDIR\dyno-agent.exe" 0 +5
    nsExec::ExecToLog '"$INSTDIR\dyno-agent.exe" service uninstall'
    Pop $0
    nsExec::ExecToLog '"$INSTDIR\dyno-agent.exe" stop'
    Pop $0
  ; The administrator readings (Dyno ADR 0027): when the helper's service is there, one UAC prompt runs the bundled helper's
  ; `uninstall`, which removes the service, the protected copy in Program Files and the helper's data. The bundled copy does it,
  ; not the installed one: Windows cannot delete a program that is running. Declining leaves the helper as it was.
  ReadRegStr $0 HKLM "SYSTEM\CurrentControlSet\Services\DynoHelper" "ImagePath"
  StrCmp $0 "" +2 0
    ExecShellWait "runas" "$INSTDIR\dyno-helper.exe" "uninstall" SW_HIDE
!macroend
