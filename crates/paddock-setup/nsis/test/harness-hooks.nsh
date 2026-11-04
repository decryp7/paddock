; The hooks file of the test harness: the same shape as an app's installer-hooks.nsh.
!include "${__FILEDIR__}\paddock-per-machine.generated.nsh"
!include "${__FILEDIR__}\paddock-per-machine.nsh"

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
  !insertmacro PADDOCK_STOP_ALL
  !insertmacro PADDOCK_CLEAN_RUN_VALUES
  !insertmacro PADDOCK_REMOVE_PROGRAM_FILES
!macroend
