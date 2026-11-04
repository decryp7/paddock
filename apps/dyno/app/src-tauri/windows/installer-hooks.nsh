; Dyno's NSIS hooks (bundle.windows.nsis.installerHooks), on the family's shared per-machine include (ADR 0053): Dyno installs
; once for every user of the PC, in Program Files. The include stops every user's Dyno before files are replaced or removed,
; refuses a folder outside Program Files and a fresh install over a per-user copy, and treats a manual upgrade's in-place
; uninstall as an update. Dyno's own steps are the helper's (Dyno ADR 0027): the user switches the helper on in Settings, so the
; installer never registers it; an uninstall removes it, and an update leaves it as it is.
; paddock-per-machine.generated.nsh (the stop list) and paddock-per-machine.nsh are generated and synced by
; `PADDOCK_WRITE_TS=1 cargo test -p paddock-service --lib per_machine`; paddock-setup.exe is staged next to this file by the bundle step.

!include "${__FILEDIR__}\paddock-per-machine.generated.nsh"
!include "${__FILEDIR__}\paddock-per-machine.nsh"

; Graceful first where Dyno has a way: the agent of the account running the installer is asked to stop (it flushes its history)
; before the include ends every user's programs by handle. Other users' agents have no such door across sessions and are
; ended by handle; they survive that (windows/per-machine.json, "survivesForcedEnd").
; Two limits, both accepted: `dyno-agent stop` runs with no timeout of its own here (a wedged agent would hold the installer
; until the installer is closed by hand; the agent's stop is a local request that answers at once or fails), and when a standard
; user types an administrator's credentials, this elevated process belongs to the administrator, so the agent it reaches is the
; administrator's, not the one of the user at the keyboard; that one, like everyone else's, is ended by handle in stop-all.
!macro _DYNO_GRACEFUL_STOP
  IfFileExists "$INSTDIR\dyno-agent.exe" 0 +3
    nsExec::ExecToLog '"$INSTDIR\dyno-agent.exe" stop'
    Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro PADDOCK_INSTALLING
  !insertmacro PADDOCK_CHECK_DIR
  !insertmacro PADDOCK_GUARD_PER_USER
  !insertmacro _DYNO_GRACEFUL_STOP
  !insertmacro PADDOCK_STOP_ALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro PADDOCK_INSTALL_SETUP
  !insertmacro PADDOCK_CLEAN_RUN_VALUES
  !insertmacro PADDOCK_FINISH_INSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro PADDOCK_INSTALLING
  !insertmacro PADDOCK_REINSTALL_AS_UPDATE
  !insertmacro _DYNO_GRACEFUL_STOP
  !insertmacro PADDOCK_STOP_ALL
  ; The administrator readings (Dyno ADR 0027): when the helper's service is there, the bundled helper's `uninstall` removes
  ; the service, the protected copy in Program Files and the helper's data. The bundled copy does it, not the installed one:
  ; Windows cannot delete a program that is running. This uninstaller is elevated already, so there is no second prompt. Only
  ; on a real uninstall: a manual upgrade (an in-place uninstall treated as an update) keeps the helper, its owner and its data.
  ${If} $UpdateMode <> 1
    ReadRegStr $0 HKLM "SYSTEM\CurrentControlSet\Services\DynoHelper" "ImagePath"
    ${If} $0 != ""
      nsExec::ExecToLog '"$INSTDIR\dyno-helper.exe" uninstall'
      Pop $0
      DetailPrint "dyno-helper uninstall: exit code $0"
    ${EndIf}
  ${EndIf}
  !insertmacro PADDOCK_CLEAN_RUN_VALUES
  !insertmacro PADDOCK_REMOVE_PROGRAM_FILES
!macroend
