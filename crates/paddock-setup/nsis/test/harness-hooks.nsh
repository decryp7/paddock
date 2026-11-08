; The hooks file of the test harness: the same shape as an app's installer-hooks.nsh.
!include "${__FILEDIR__}\paddock-per-machine.generated.nsh"
!include "${__FILEDIR__}\paddock-per-machine.nsh"

; The shape of an app's service step in an uninstaller (Pitwall's _PITWALL_UNREGISTER_SERVICE): ask the include whether the folder
; is accepted, and only then run a program of it.
!macro _HARNESS_SERVICE_STEP
  !insertmacro PADDOCK_DIR_ACCEPTED
  ${If} $0 == 0
    ${If} ${FileExists} "$INSTDIR\harness-server.exe"
      nsExec::ExecToLog '"$INSTDIR\harness-server.exe" uninstall'
      Pop $0
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro PADDOCK_INSTALLING
  !insertmacro PADDOCK_CHECK_DIR
  !insertmacro PADDOCK_GUARD_PER_USER
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
  !insertmacro _HARNESS_SERVICE_STEP
  !insertmacro PADDOCK_STOP_ALL
  !insertmacro PADDOCK_CLEAN_RUN_VALUES
  !insertmacro PADDOCK_REMOVE_PROGRAM_FILES
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro PADDOCK_FINISH_UNINSTALL
!macroend
