; Pitwall's NSIS hooks (bundle.windows.nsis.installerHooks). The installer runs elevated (perMachine: Windows asks
; for permission once), so it registers the Pitwall service here. Upgrades stop the running service first so its
; binary can be replaced; the new service starts the server in the logged-on user's session again.
; (Adapted from Pitcrew's hooks.)

!macro NSIS_HOOK_PREINSTALL
  IfFileExists "$INSTDIR\pitwall-server.exe" 0 +3
    nsExec::ExecToLog '"$INSTDIR\pitwall-server.exe" uninstall'
    Pop $0
!macroend

; `install` refuses a folder outside Program Files, a folder or program other accounts can change, and another copy's
; registration (plan B6, ADR 0053); its own words say what to do, so they are shown, and logged in the details.
!macro NSIS_HOOK_POSTINSTALL
  Push $1 ; kept for the installer's own use
  nsExec::ExecToStack '"$INSTDIR\pitwall-server.exe" install'
  Pop $0
  Pop $1
  DetailPrint "pitwall-server install: exit code $0: $1"
  StrCmp $0 "0" pitwall_install_done
    ; The words for the user come from check-install, which writes nothing but its answer (no OS-log lines in front).
    nsExec::ExecToStack '"$INSTDIR\pitwall-server.exe" check-install'
    Pop $0
    Pop $1
    IfSilent pitwall_install_done
      MessageBox MB_ICONEXCLAMATION|MB_OK "The Pitwall service could not be registered.$\r$\n$1"
  pitwall_install_done:
  Pop $1
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$INSTDIR\pitwall-server.exe" uninstall'
  Pop $0
!macroend
