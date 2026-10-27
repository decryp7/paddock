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
!macroend
