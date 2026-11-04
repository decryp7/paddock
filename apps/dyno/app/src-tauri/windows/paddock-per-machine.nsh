; paddock-per-machine.nsh: the shared installer include of every per-machine Windows app (family ADR 0053, section 2).
; Synced into each app's src-tauri/windows/ by paddock_commands::testing::sync_generated; do not edit the copies.
;
; The app's hooks file (bundle.windows.nsis.installerHooks) includes, in this order,
;     !include "${__FILEDIR__}\paddock-per-machine.generated.nsh"   ; !define PADDOCK_STOP_IMAGES "a.exe,b.exe" (ADR section 2.1)
;     !include "${__FILEDIR__}\paddock-per-machine.nsh"
; and calls the macros below from Tauri's four hooks. The app's own steps (service, helper) go between them.
;
;   NSIS_HOOK_PREINSTALL      PADDOCK_INSTALLING, PADDOCK_CHECK_DIR, PADDOCK_GUARD_PER_USER, <app steps>, PADDOCK_STOP_ALL
;   NSIS_HOOK_POSTINSTALL     <app steps>, PADDOCK_INSTALL_SETUP, PADDOCK_CLEAN_RUN_VALUES, PADDOCK_FINISH_INSTALL
;   NSIS_HOOK_PREUNINSTALL    PADDOCK_INSTALLING, PADDOCK_REINSTALL_AS_UPDATE, PADDOCK_STOP_ALL, <app steps>,
;                             PADDOCK_CLEAN_RUN_VALUES, PADDOCK_REMOVE_PROGRAM_FILES
;
; Every macro uses only $0 to $5 and $R8, $R9 (the template's pages use $R0 to $R7 and do not run in a section).
; The installer and the uninstaller both run paddock-setup.exe from $PLUGINSDIR, never from $INSTDIR: Restart Manager and the
; fallback in stop-all then cannot end the running stop-all, and the uninstaller can delete $INSTDIR\paddock-setup.exe.
;
; Command line contract with paddock-setup.exe (exit 0 = done unless stated):
;   check-dir --dir <dir>                                       3 = refused
;   guard --product <name> --main <exe> --dir <dir> [--update] [--restart]
;                                                               2 = blocked: stdout is the message on the first line and,
;                                                               when the old copy is to be started again, a last line
;                                                               RESTART:<full path of the per-user program>
;   stop-all --dir <dir> --images <a.exe,b.exe,...>             11 = a listed program still runs
;   same-folder --a <dir> --b <dir>                             0 = same, 1 = not the same
;   clean-run --dir <dir> --scope <machine|all>
;   clean-staging --dir <dir>
;   hold-installing                                             stays running until the installer exits
; Any other non-zero code is a failure of the tool itself (the details and $INSTDIR\updates\setup.log say why).
; stop-all ends every session's listed programs itself, by handle (Restart Manager's shutdown does nothing while another
; session holds a file, measured in S0); the template's CheckIfAppIsRunning, which uses RmShutdown and aborts on any
; non-zero code, therefore finds nothing running. The app that .onInstSuccess starts with RunAsUser gets the desktop
; user's own TEMP and TMP, so nothing is restored here.

!ifndef PADDOCK_PER_MACHINE_INCLUDED
!define PADDOCK_PER_MACHINE_INCLUDED
!define PADDOCK_INCLUDE_VERSION "1"
; makensis resolves a relative !include from its working folder, not from the including file: name everything by this file's folder.
!define /redef PADDOCK_SETUP_DIR "${__FILEDIR__}"

!include LogicLib.nsh
!include FileFunc.nsh
!include "Win\RestartManager.nsh"
!ifndef PADDOCK_STOP_IMAGES
  !include /NONFATAL "${PADDOCK_SETUP_DIR}\paddock-per-machine.generated.nsh"
!endif
!ifndef PADDOCK_STOP_IMAGES
  !error "paddock-per-machine.generated.nsh must !define PADDOCK_STOP_IMAGES (comma-separated file names); run paddock_commands::testing::sync_generated"
!endif

; Error levels of a refusal ($INSTDIR\updates\setup.log has the detail).
!define PADDOCK_LEVEL_PER_USER 2
!define PADDOCK_LEVEL_BAD_DIR 3
!define PADDOCK_LEVEL_STILL_RUNNING 4
!define PADDOCK_LEVEL_TOOL_FAILED 5

!define PADDOCK_SETUP "$PLUGINSDIR\paddock-setup.exe"

; The page's folder check (ADR 2.2): the answer of check-dir, extracting the embedded copy first.
Function .onVerifyInstDir
  InitPluginsDir
  IfFileExists "$PLUGINSDIR\paddock-setup.exe" +2 0
    File "/oname=$PLUGINSDIR\paddock-setup.exe" "${PADDOCK_SETUP_DIR}\paddock-setup.exe"
  nsExec::Exec '"$PLUGINSDIR\paddock-setup.exe" check-dir --dir "$INSTDIR"'
  Pop $0
  StrCmp $0 "0" +2 0
    Abort
FunctionEnd

; Runs paddock-setup.exe with the arguments and leaves its exit code in $0 (the details show the line and the code).
!macro _PADDOCK_RUN _ARGS
  DetailPrint 'paddock-setup ${_ARGS}'
  nsExec::ExecToLog '"${PADDOCK_SETUP}" ${_ARGS}'
  Pop $0
  DetailPrint "paddock-setup exit code: $0"
!macroend

; Registers "$INSTDIR\<image>" for every comma-separated name (at compile time) with the Restart Manager session $R8.
!macro _PADDOCK_REGISTER_STEP
  !searchparse /noerrors "${_PADDOCK_LIST}" "" _PADDOCK_HEAD "," _PADDOCK_TAIL
  !ifdef _PADDOCK_HEAD
    !if "${_PADDOCK_HEAD}" != ""
      !insertmacro RestartManager_RegisterFile $R8 "$INSTDIR\${_PADDOCK_HEAD}"
    !endif
    !ifdef _PADDOCK_TAIL
      !define _PADDOCK_NEXT "${_PADDOCK_TAIL}"
      !undef _PADDOCK_TAIL
    !else
      !define _PADDOCK_NEXT ""
    !endif
    !undef _PADDOCK_HEAD
    !undef _PADDOCK_LIST
    !define _PADDOCK_LIST "${_PADDOCK_NEXT}"
    !undef _PADDOCK_NEXT
    !if "${_PADDOCK_LIST}" != ""
      !insertmacro _PADDOCK_REGISTER_STEP
    !endif
  !endif
!macroend

; The created mutex is held until the process exits (ADR 4.1 step 4). Installer: extract the embedded paddock-setup.exe.
; Uninstaller: copy the installed one. Both run it from $PLUGINSDIR. Must be the first macro of PREINSTALL / PREUNINSTALL.
!macro PADDOCK_INSTALLING
  InitPluginsDir
  !ifndef __UNINSTALL__
    File "/oname=$PLUGINSDIR\paddock-setup.exe" "${PADDOCK_SETUP_DIR}\paddock-setup.exe"
  !else
    ClearErrors
    CopyFiles /SILENT "$INSTDIR\paddock-setup.exe" "$PLUGINSDIR\paddock-setup.exe"
    ${If} ${Errors}
      DetailPrint "paddock-setup.exe could not be copied from $INSTDIR; its steps are skipped"
    ${EndIf}
  !endif
  Exec '"${PADDOCK_SETUP}" hold-installing'
!macroend

; Refuses a folder outside Program Files (ADR 2.2). First check of PREINSTALL, after PADDOCK_INSTALLING.
!macro PADDOCK_CHECK_DIR
  !insertmacro _PADDOCK_RUN 'check-dir --dir "$INSTDIR"'
  ${If} $0 != 0
    !insertmacro PADDOCK_ABORT "Install ${PRODUCTNAME} in Program Files. The folder $INSTDIR is not allowed." ${PADDOCK_LEVEL_BAD_DIR} ""
  ${EndIf}
!macroend

; The per-user guard (ADR 5): a fresh install only, i.e. no HKLM uninstall key.
!macro PADDOCK_GUARD_PER_USER
  !ifdef __UNINSTALL__
    !error "PADDOCK_GUARD_PER_USER belongs in NSIS_HOOK_PREINSTALL"
  !endif
  ReadRegStr $0 HKLM "${UNINSTKEY}" "UninstallString"
  ${If} $0 == ""
    StrCpy $2 ""
    ${If} $UpdateMode = 1
      StrCpy $2 " --update"
      ClearErrors
      ${GetOptions} $CMDLINE "/R" $1
      ${IfNot} ${Errors}
        StrCpy $2 "$2 --restart"
      ${EndIf}
    ${EndIf}
    ClearErrors
    DetailPrint "paddock-setup guard"
    nsExec::ExecToStack '"${PADDOCK_SETUP}" guard --product "${PRODUCTNAME}" --main "${MAINBINARYNAME}.exe" --dir "$INSTDIR"$2'
    Pop $0
    Pop $1
    DetailPrint "paddock-setup exit code: $0"
    ${If} $0 == 2
      ; $1 is the message, then optionally a last line RESTART:<program>.
      StrCpy $2 ""
      ${StrLoc} $3 $1 "RESTART:" ">"
      ${If} $3 != ""
        StrCpy $2 $1 "" $3
        StrCpy $2 $2 "" 8
        StrCpy $1 $1 $3
      ${EndIf}
      !insertmacro _PADDOCK_TRIM_EOL $1
      !insertmacro _PADDOCK_TRIM_EOL $2
      !insertmacro PADDOCK_ABORT "$1" ${PADDOCK_LEVEL_PER_USER} "$2"
    ${ElseIf} $0 != 0
      !insertmacro PADDOCK_ABORT "${PRODUCTNAME} could not be installed: the check for an older copy failed (code $0). The reason is in $INSTDIR\updates\setup.log" ${PADDOCK_LEVEL_TOOL_FAILED} ""
    ${EndIf}
  ${Else}
    DetailPrint "per-user guard: not a fresh install, skipped"
  ${EndIf}
!macroend

; Removes trailing CR and LF from the register.
!macro _PADDOCK_TRIM_EOL _VAR
  ${Do}
    StrCpy $R9 ${_VAR} 1 -1
    ${If} $R9 == "$\r"
    ${OrIf} $R9 == "$\n"
      StrCpy ${_VAR} ${_VAR} -1
    ${Else}
      ${ExitDo}
    ${EndIf}
  ${Loop}
!macroend

; Stops every listed program in every session by handle (ADR 3), then a last Restart Manager listing over the same files
; here in the include, so a program that is still there is found before the template's own CheckIfAppIsRunning (which
; would call RmShutdown and abort) and its first File.
!macro PADDOCK_STOP_ALL
  !insertmacro _PADDOCK_RUN 'stop-all --dir "$INSTDIR" --images "${PADDOCK_STOP_IMAGES}"'
  ${If} $0 == 11
    !insertmacro PADDOCK_ABORT "${PRODUCTNAME} could not be closed for every signed-in user. Ask them to quit ${PRODUCTNAME}, then run this installer again." ${PADDOCK_LEVEL_STILL_RUNNING} "$INSTDIR\${MAINBINARYNAME}.exe"
  ${ElseIf} $0 != 0
    !insertmacro PADDOCK_ABORT "${PRODUCTNAME} could not be closed (code $0). The reason is in $INSTDIR\updates\setup.log" ${PADDOCK_LEVEL_TOOL_FAILED} "$INSTDIR\${MAINBINARYNAME}.exe"
  ${EndIf}
  ; The last RmGetList.
  !insertmacro RestartManager_StartSession $R8
  ${If} $R8 != ""
    !define _PADDOCK_LIST "${PADDOCK_STOP_IMAGES},"
    !insertmacro _PADDOCK_REGISTER_STEP
    !ifdef _PADDOCK_LIST
      !undef _PADDOCK_LIST
    !endif
    System::Call 'RSTRTMGR::RmGetList(i $R8, *i .r1, *i .r2, p 0, *i .r3) i .r0'
    !insertmacro RestartManager_EndSession $R8
    DetailPrint "last Restart Manager listing: code $0, processes $1"
    ; ERROR_SUCCESS: nothing uses the files; ERROR_MORE_DATA (234): something does.
    ${If} $0 == 234
      !insertmacro PADDOCK_ABORT "${PRODUCTNAME} could not be closed for every signed-in user. Ask them to quit ${PRODUCTNAME}, then run this installer again." ${PADDOCK_LEVEL_STILL_RUNNING} "$INSTDIR\${MAINBINARYNAME}.exe"
    ${EndIf}
  ${Else}
    DetailPrint "last Restart Manager listing: no session (code), skipped"
  ${EndIf}
!macroend

; A manual upgrade or a same-version "Uninstall" runs the old uninstaller in place (_?=): that is a reinstall, not an
; uninstall (ADR 2.4). First check of PREUNINSTALL after PADDOCK_INSTALLING; the template reads $UpdateMode after the hook.
!macro PADDOCK_REINSTALL_AS_UPDATE
  !insertmacro _PADDOCK_RUN 'same-folder --a "$EXEDIR" --b "$INSTDIR"'
  ${If} $0 == 0
    StrCpy $UpdateMode 1
    DetailPrint "reinstall: treated as an update"
  ${EndIf}
!macroend

; Run values naming a program in $INSTDIR (ADR 6.3): HKLM on install and uninstall; every loaded user's value on a real uninstall.
!macro PADDOCK_CLEAN_RUN_VALUES
  !ifndef __UNINSTALL__
    !insertmacro _PADDOCK_RUN 'clean-run --dir "$INSTDIR" --scope machine'
  !else
    ${If} $UpdateMode = 1
      !insertmacro _PADDOCK_RUN 'clean-run --dir "$INSTDIR" --scope machine'
    ${Else}
      !insertmacro _PADDOCK_RUN 'clean-run --dir "$INSTDIR" --scope all'
    ${EndIf}
  !endif
  ${If} $0 != 0
    DetailPrint "start-at-login values could not be cleaned (code $0); the install continues"
  ${EndIf}
!macroend

; Test harness only: appends a line to %PADDOCK_STUB_LOG% (the silent installer has no other output).
!macro _PADDOCK_TEST_LOG _LINE
  !ifdef PADDOCK_TEST_HARNESS
    ReadEnvStr $R7 PADDOCK_STUB_LOG
    ${If} $R7 != ""
      FileOpen $R6 $R7 a
      FileSeek $R6 0 END
      FileWrite $R6 "${_LINE}$\r$\n"
      FileClose $R6
    ${EndIf}
  !endif
!macroend

; Starts a program unelevated as the desktop user (the template's own call). The test harness has no Tauri plug-in.
!macro _PADDOCK_START_AS_USER _EXE
  !ifdef PADDOCK_TEST_HARNESS
    !insertmacro _PADDOCK_TEST_LOG "RunAsUser ${_EXE}"
  !else
    nsis_tauri_utils::RunAsUser "${_EXE}" ""
  !endif
!macroend

; Every refusal: the message, the clean-up of ADR 2.3, the restart of 4.3 or 5, the error level, Quit.
;   _MSG      the message (an NSIS string, may use registers other than $R8 and $R9)
;   _LEVEL    the error level (PADDOCK_LEVEL_*)
;   _RESTART  the full path of a program to start again as the desktop user, or "" (only used with /UPDATE and /R)
!macro PADDOCK_ABORT _MSG _LEVEL _RESTART
  DetailPrint "${PRODUCTNAME}: ${_MSG}"
  !insertmacro _PADDOCK_TEST_LOG "ABORT level ${_LEVEL}: ${_MSG}"
  !ifndef __UNINSTALL__
    ; A refusal during a fresh install removes the empty folder (not recursive; the setup.log stays on purpose).
    SetOutPath "$PLUGINSDIR"
    ReadRegStr $R9 HKLM "${UNINSTKEY}" "UninstallString"
    ${If} $R9 == ""
      RMDir "$INSTDIR"
    ${EndIf}
    ; An update that started with /UPDATE /R starts the program that is still installed again (nothing was replaced).
    StrCpy $R9 "${_RESTART}"
    ${If} $R9 != ""
    ${AndIf} $UpdateMode = 1
      ClearErrors
      ${GetOptions} $CMDLINE "/R" $R8
      ${IfNot} ${Errors}
        DetailPrint "starting $R9 again"
        !insertmacro _PADDOCK_START_AS_USER "$R9"
      ${EndIf}
    ${EndIf}
  !endif
  IfSilent +2 0
    MessageBox MB_ICONSTOP|MB_OK "${_MSG}"
  SetErrorLevel ${_LEVEL}
  Quit
!macroend

; POSTINSTALL: the installed copy of paddock-setup.exe (the updater's elevated hand-off and the uninstaller use it).
!macro PADDOCK_INSTALL_SETUP
  File "/oname=$INSTDIR\paddock-setup.exe" "${PADDOCK_SETUP_DIR}\paddock-setup.exe"
!macroend

; Last in POSTINSTALL: delete the staged update that is not in use (the next apply-update removes the rest).
!macro PADDOCK_FINISH_INSTALL
  !insertmacro _PADDOCK_RUN 'clean-staging --dir "$INSTDIR"'
!macroend

; Last in PREUNINSTALL: the template's RMDir "$INSTDIR" is not recursive, so what the include added goes first. A reinstall
; keeps updates\ (its setup.log).
!macro PADDOCK_REMOVE_PROGRAM_FILES
  Delete "$INSTDIR\paddock-setup.exe"
  ${If} $UpdateMode <> 1
    RMDir /r "$INSTDIR\updates"
  ${EndIf}
!macroend

!endif
