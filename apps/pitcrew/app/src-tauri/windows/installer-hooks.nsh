; Pitcrew's NSIS hooks (bundle.windows.nsis.installerHooks). The installer runs elevated (perMachine: Windows asks
; for permission once), so it registers the Pitcrew service here. Upgrades stop the running service first so its
; binary can be replaced: runners are asked to stop and finish a running job; one still busy keeps running and is
; adopted by the new service (ADR 0003).

!macro NSIS_HOOK_PREINSTALL
  IfFileExists "$INSTDIR\pitcrew-service.exe" 0 +3
    nsExec::ExecToLog '"$INSTDIR\pitcrew-service.exe" uninstall'
    Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog '"$INSTDIR\pitcrew-service.exe" install'
  Pop $0
  StrCmp $0 "0" +3
    IfSilent +2
      MessageBox MB_ICONEXCLAMATION|MB_OK "The Pitcrew service could not be registered (code $0).$\r$\nOpen Pitcrew, Settings, Service to try again."
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$INSTDIR\pitcrew-service.exe" uninstall'
  Pop $0
!macroend
