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
; Registers: the macros use $0 to $5 and $R3 to $R5, $R8 and $R9, and nothing else (the template's pages use $R0 to $R7 but do
; not run inside a section; its CheckIfAppIsRunning uses $R0 to $R3 and runs after these macros). The test harness also uses
; $R6 and $R7. $PaddockHaveSetup (1 or 0) says whether paddock-setup.exe was available to this run.
;
; Where paddock-setup.exe runs from: the installer extracts its embedded copy to $PLUGINSDIR; the uninstaller cannot `File`, so
; it copies the installed one to $PLUGINSDIR. Both run it from there, never from $INSTDIR: stop-all then never has to end its own
; process, and the uninstaller can delete $INSTDIR\paddock-setup.exe. When the installed copy is missing, the uninstaller says so
; in the details ("NOTE paddock-setup.exe is missing ...") and skips the steps that need it: the in-place check compares
; $EXEDIR with $INSTDIR itself, programs are asked to close through Restart Manager (best effort: it ends nothing while another
; session holds a file), the start-at-login clean-up is skipped, and the uninstall goes on.
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
; The mechanism: stop-all ends every session's listed programs itself, by handle (graceful first in the caller's own session;
; Restart Manager's shutdown does nothing while another session holds a file, measured in S0). Its exit code 11 is the one
; answer to "does a listed program still run"; the include does no listing of its own, because Restart Manager also lists
; system components that only map the files (the x86 emulation cache on Arm). The template's CheckIfAppIsRunning, which uses
; RmShutdown and aborts on any non-zero code, therefore finds nothing running. The app that .onInstSuccess starts with RunAsUser gets the desktop
; user's own TEMP and TMP, so nothing is restored here.

!ifndef PADDOCK_PER_MACHINE_INCLUDED
!define PADDOCK_PER_MACHINE_INCLUDED
!define PADDOCK_INCLUDE_VERSION "1"
; Readable in the built installer's version resource: the artifact gate (ADR 0039) compares it with this file.
!ifndef PADDOCK_TEST_HARNESS ; Tauri's template sets VIProductVersion; the harness has none
  VIAddVersionKey "PaddockInclude" "${PADDOCK_INCLUDE_VERSION}"
!endif
; makensis resolves a relative !include from its working folder, not from the including file: name everything by this file's folder.
!define /redef PADDOCK_SETUP_DIR "${__FILEDIR__}"

!if /FileExists "${PADDOCK_SETUP_DIR}\paddock-setup.exe"
!else
  !error "paddock-setup.exe is not next to paddock-per-machine.nsh (${PADDOCK_SETUP_DIR}): the bundle step stages it (ADR 0053 S7) before the installer is built"
!endif

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

Var PaddockHaveSetup

; A path for a command line inside quotes: a trailing backslash (a drive root) would escape the closing quote, so it is doubled.
!macro _PADDOCK_ARG _OUT _IN
  StrCpy ${_OUT} "${_IN}"
  StrCpy $R9 ${_OUT} 1 -1
  ${If} $R9 == "\"
    StrCpy ${_OUT} "${_OUT}\"
  ${EndIf}
!macroend

; Runs paddock-setup.exe with the arguments and leaves its exit code in $0 (the details show the line and the code); with the
; folders as $R5 ($INSTDIR) and $R4 ($EXEDIR), already safe inside quotes. Without the program, $0 is "nosetup" and nothing runs.
!macro _PADDOCK_RUN _ARGS
  !insertmacro _PADDOCK_ARG $R5 "$INSTDIR"
  !insertmacro _PADDOCK_ARG $R4 "$EXEDIR"
  ${If} $PaddockHaveSetup == 1
    DetailPrint 'paddock-setup ${_ARGS}'
    nsExec::ExecToLog '"${PADDOCK_SETUP}" ${_ARGS}'
    Pop $0
    DetailPrint "paddock-setup exit code: $0"
  ${Else}
    StrCpy $0 "nosetup"
  ${EndIf}
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
; Uninstaller: copy the installed one (see the header). Both run it from $PLUGINSDIR. Must be the first macro of PREINSTALL /
; PREUNINSTALL.
!macro PADDOCK_INSTALLING
  InitPluginsDir
  StrCpy $PaddockHaveSetup 0
  !ifndef __UNINSTALL__
    File "/oname=$PLUGINSDIR\paddock-setup.exe" "${PADDOCK_SETUP_DIR}\paddock-setup.exe"
    StrCpy $PaddockHaveSetup 1
  !else
    ClearErrors
    CopyFiles /SILENT "$INSTDIR\paddock-setup.exe" "$PLUGINSDIR\paddock-setup.exe"
    ${IfNot} ${Errors}
      StrCpy $PaddockHaveSetup 1
    ${Else}
      DetailPrint "NOTE paddock-setup.exe is missing from $INSTDIR: its steps are skipped (see the include's header for the fallbacks)"
      !insertmacro _PADDOCK_TEST_LOG "NOTE paddock-setup.exe is missing from the uninstaller's folder: steps skipped"
    ${EndIf}
  !endif
  ${If} $PaddockHaveSetup == 1
    Exec '"${PADDOCK_SETUP}" hold-installing'
  ${EndIf}
!macroend

; Refuses a folder outside Program Files (ADR 2.2). First check of PREINSTALL, after PADDOCK_INSTALLING.
!macro PADDOCK_CHECK_DIR
  !insertmacro _PADDOCK_RUN 'check-dir --dir "$R5"'
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
    !insertmacro _PADDOCK_ARG $R5 "$INSTDIR"
    DetailPrint "paddock-setup guard"
    nsExec::ExecToStack '"${PADDOCK_SETUP}" guard --product "${PRODUCTNAME}" --main "${MAINBINARYNAME}.exe" --dir "$R5"$2'
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

; Stops every listed program in every session by handle (ADR 3). Exit code 11 of stop-all is the answer to "still running".
; In an uninstaller without paddock-setup.exe: Restart Manager is asked to close them (best effort, no abort) and the uninstall
; goes on; the template's own check of the main binary is the safety net there.
!macro PADDOCK_STOP_ALL
  ${If} $PaddockHaveSetup == 1
    !insertmacro _PADDOCK_RUN 'stop-all --dir "$R5" --images "${PADDOCK_STOP_IMAGES}"'
    ${If} $0 == 11
      !insertmacro PADDOCK_ABORT "${PRODUCTNAME} could not be closed for every signed-in user. Ask them to quit ${PRODUCTNAME}, then run this installer again." ${PADDOCK_LEVEL_STILL_RUNNING} "$INSTDIR\${MAINBINARYNAME}.exe"
    ${ElseIf} $0 != 0
      !insertmacro PADDOCK_ABORT "${PRODUCTNAME} could not be closed (code $0). The reason is in $INSTDIR\updates\setup.log" ${PADDOCK_LEVEL_TOOL_FAILED} "$INSTDIR\${MAINBINARYNAME}.exe"
    ${EndIf}
  ${Else}
    DetailPrint "NOTE paddock-setup.exe is missing: programs are asked to close through Restart Manager only"
    !insertmacro _PADDOCK_TEST_LOG "NOTE paddock-setup.exe is missing: Restart Manager fallback"
    !insertmacro RestartManager_StartSession $R8
    ${If} $R8 != ""
      !define _PADDOCK_LIST "${PADDOCK_STOP_IMAGES},"
      !insertmacro _PADDOCK_REGISTER_STEP
      !ifdef _PADDOCK_LIST
        !undef _PADDOCK_LIST
      !endif
      System::Call 'RSTRTMGR::RmShutdown(i $R8, i ${RmForceShutdown}, p 0) i .r0'
      DetailPrint "Restart Manager shutdown: code $0"
      !insertmacro RestartManager_EndSession $R8
    ${EndIf}
  ${EndIf}
!macroend

; A manual upgrade or a same-version "Uninstall" runs the old uninstaller in place (_?=): that is a reinstall, not an
; uninstall (ADR 2.4). First check of PREUNINSTALL after PADDOCK_INSTALLING; the template reads $UpdateMode after the hook.
; Without paddock-setup.exe the two folders are compared here (the same answer for the common case; NSIS compares without case).
!macro PADDOCK_REINSTALL_AS_UPDATE
  ${If} $PaddockHaveSetup == 1
    !insertmacro _PADDOCK_RUN 'same-folder --a "$R4" --b "$R5"'
    ${If} $0 == 0
      StrCpy $UpdateMode 1
      DetailPrint "reinstall: treated as an update"
      !insertmacro _PADDOCK_TEST_LOG "NOTE reinstall: treated as an update"
    ${EndIf}
  ${Else}
    ${If} $EXEDIR == $INSTDIR
      StrCpy $UpdateMode 1
      DetailPrint "reinstall: treated as an update (folders compared by the installer itself)"
      !insertmacro _PADDOCK_TEST_LOG "NOTE reinstall: treated as an update"
    ${EndIf}
  ${EndIf}
!macroend

; Run values naming a program in $INSTDIR (ADR 6.3): HKLM on install and uninstall; every loaded user's value on a real uninstall.
; Skipped, and said so, without paddock-setup.exe.
!macro PADDOCK_CLEAN_RUN_VALUES
  ${If} $PaddockHaveSetup == 1
    !ifndef __UNINSTALL__
      !insertmacro _PADDOCK_RUN 'clean-run --dir "$R5" --scope machine'
    !else
      ${If} $UpdateMode = 1
        !insertmacro _PADDOCK_RUN 'clean-run --dir "$R5" --scope machine'
      ${Else}
        !insertmacro _PADDOCK_RUN 'clean-run --dir "$R5" --scope all'
      ${EndIf}
    !endif
    ${If} $0 != 0
      DetailPrint "start-at-login values could not be cleaned (code $0); the install continues"
    ${EndIf}
  ${Else}
    DetailPrint "NOTE paddock-setup.exe is missing: start-at-login values were not cleaned"
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
  !insertmacro _PADDOCK_RUN 'clean-staging --dir "$R5"'
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
