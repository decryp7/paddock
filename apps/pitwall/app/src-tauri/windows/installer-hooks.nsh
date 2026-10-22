; Pitwall's NSIS hooks (bundle.windows.nsis.installerHooks). The installer runs elevated (perMachine: Windows asks
; for permission once), so it registers the Pitwall service here. Upgrades stop the running service first so its
; binary can be replaced; the new service starts the server in the logged-on user's session again.
; (Adapted from Pitcrew's hooks.)

!macro NSIS_HOOK_PREINSTALL
  IfFileExists "$INSTDIR\pitwall-server.exe" 0 +3
    nsExec::ExecToLog '"$INSTDIR\pitwall-server.exe" uninstall'
    Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog '"$INSTDIR\pitwall-server.exe" install'
  Pop $0
  StrCmp $0 "0" +3
    IfSilent +2
      MessageBox MB_ICONEXCLAMATION|MB_OK "The Pitwall service could not be registered (code $0).$\r$\nOpen Pitwall, Settings, Service to try again."
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$INSTDIR\pitwall-server.exe" uninstall'
  Pop $0
!macroend
