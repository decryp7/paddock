; Pitwall's NSIS hooks (bundle.windows.nsis.installerHooks), on the family's shared per-machine include (ADR 0053): Pitwall installs
; once for every user of the PC, in Program Files. The include refuses a folder outside Program Files before anything of the
; installed copy is run, refuses a fresh install over a per-user copy, stops every user's Pitwall before files are replaced or
; removed, and treats a manual upgrade's in-place uninstall as an update. Pitwall's own step is its LocalSystem service: the
; installer (elevated: Windows asks for permission once) removes the registration before the files change and registers it again
; afterwards, so the new service starts the server in the logged-on user's session again.
; paddock-per-machine.generated.nsh (the stop list) and paddock-per-machine.nsh are generated and synced by
; `PADDOCK_WRITE_TS=1 cargo test -p paddock-service --lib per_machine`; paddock-setup.exe is staged next to this file by the bundle step.

!include "${__FILEDIR__}\paddock-per-machine.generated.nsh"
!include "${__FILEDIR__}\paddock-per-machine.nsh"

; Takes the service out before its program is replaced. The installer's PREINSTALL has passed PADDOCK_CHECK_DIR (Program Files,
; owned and protected by administrators), so the program is run from a checked folder and a failed removal aborts the install
; (level PADDOCK_LEVEL_TOOL_FAILED) only while the service is still registered (`sc query`; 1060 means gone): a service that is still registered must not be left to restart the server between this step
; and the programs being ended (VM row PW-1). An uninstaller has no abort (it must stay usable), and its folder is not checked
; by anyone yet: the include decides in NSIS alone whether it trusts $INSTDIR (the folder the installer registered, directly
; under Program Files; `$PaddockTrusted`) before it copies or runs anything of it. Then PADDOCK_DIR_ACCEPTED asks `check-dir`, and
; only an accepted folder runs the program's `uninstall`. When the include trusts the folder but paddock-setup.exe is missing, no
; program of the folder is run and the service is removed with sc.exe from System32, and the user is told. An untrusted folder
; touches nothing (plan B6b, the security review of B6 P2-4 and of B6b P1-1).
!macro _PITWALL_UNREGISTER_SERVICE
  !ifdef __UNINSTALL__
    !insertmacro PADDOCK_DIR_ACCEPTED
    ${If} $0 == 0
      ${If} ${FileExists} "$INSTDIR\pitwall-server.exe"
        nsExec::ExecToLog '"$INSTDIR\pitwall-server.exe" uninstall'
        Pop $0
        DetailPrint "pitwall-server uninstall: exit code $0"
      ${EndIf}
    ${ElseIf} $PaddockTrusted == 1
    ${AndIf} $PaddockHaveSetup == 0
      nsExec::ExecToLog '"$SYSDIR\sc.exe" stop Pitwall'
      Pop $0
      nsExec::ExecToLog '"$SYSDIR\sc.exe" delete Pitwall'
      Pop $0
      DetailPrint "NOTE paddock-setup.exe is missing: the Pitwall service was removed with sc.exe (delete: exit code $0)"
      ${IfNot} ${Silent}
        MessageBox MB_ICONINFORMATION|MB_OK "The Pitwall service was removed without the installer's checking tool (it was missing). If Pitwall's service is still listed in Windows, remove it from Services."
      ${EndIf}
    ${Else}
      DetailPrint "NOTE the service was not removed: $INSTDIR is not an accepted install folder (check-dir: $0)"
    ${EndIf}
  !else
    ${If} ${FileExists} "$INSTDIR\pitwall-server.exe"
      nsExec::ExecToLog '"$INSTDIR\pitwall-server.exe" uninstall'
      Pop $0
      DetailPrint "pitwall-server uninstall: exit code $0"
      ${If} $0 != 0
        ; The verb failed (a corrupt or older program, error 1072 "marked for delete"): only a service that is still registered stops
        ; the install. 1060 from `sc query` is "does not exist": nothing is left to restart the server, so the install goes on.
        nsExec::ExecToLog '"$SYSDIR\sc.exe" query Pitwall'
        Pop $1
        DetailPrint "sc query Pitwall: exit code $1"
        ${If} $1 != 1060
          !insertmacro PADDOCK_ABORT "The Pitwall service could not be removed (code $0), so Pitwall cannot be updated safely. Remove the service in Windows Services, restart the PC, or uninstall Pitwall first, then run this installer again." ${PADDOCK_LEVEL_TOOL_FAILED} ""
        ${EndIf}
      ${EndIf}
    ${EndIf}
  !endif
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro PADDOCK_INSTALLING
  !insertmacro PADDOCK_CHECK_DIR
  !insertmacro PADDOCK_GUARD_PER_USER
  !insertmacro _PITWALL_UNREGISTER_SERVICE
  !insertmacro PADDOCK_STOP_ALL
!macroend

; `install` refuses a folder outside Program Files, a folder or program other accounts can change, and another copy's
; registration (plan B6, ADR 0053); its own words say what to do, so they are shown, and logged in the details.
!macro _PITWALL_REGISTER_SERVICE
  Push $1 ; kept for the installer's own use
  Push $2
  nsExec::ExecToStack '"$INSTDIR\pitwall-server.exe" install'
  Pop $0
  Pop $1
  DetailPrint "pitwall-server install: exit code $0: $1"
  StrCmp $0 "0" pitwall_install_done
    StrCpy $2 $1
    ; The words for the user come from check-install, which writes nothing but its answer (no OS-log lines in front).
    nsExec::ExecToStack '"$INSTDIR\pitwall-server.exe" check-install'
    Pop $0
    Pop $1
    ; The check passes (the install failed for another reason): the install's own words say what went wrong.
    StrCmp $0 "0" 0 +2
      StrCpy $1 $2
    IfSilent pitwall_install_done
      MessageBox MB_ICONEXCLAMATION|MB_OK "The Pitwall service could not be registered.$\r$\n$1"
  pitwall_install_done:
  Pop $2
  Pop $1
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro PADDOCK_INSTALL_SETUP
  !insertmacro PADDOCK_CLEAN_RUN_VALUES
  !insertmacro _PITWALL_REGISTER_SERVICE
  !insertmacro PADDOCK_FINISH_INSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro PADDOCK_INSTALLING
  !insertmacro PADDOCK_REINSTALL_AS_UPDATE
  !insertmacro _PITWALL_UNREGISTER_SERVICE
  !insertmacro PADDOCK_STOP_ALL
  !insertmacro PADDOCK_CLEAN_RUN_VALUES
  !insertmacro PADDOCK_REMOVE_PROGRAM_FILES
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro PADDOCK_FINISH_UNINSTALL
!macroend
