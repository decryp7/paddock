; paddock-per-machine.nsh: the shared installer include of every per-machine Windows app (family ADR 0053, section 2).
; Copied into each app's src-tauri/windows/ by paddock-service's `per_machine` test (PADDOCK_WRITE_TS=1); do not edit the copies.
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
;   NSIS_HOOK_POSTUNINSTALL   PADDOCK_FINISH_UNINSTALL
;
; Registers: the macros use $0 to $3 and $R4 to $R9 (PADDOCK_FINISH_UNINSTALL, the uninstaller's last step, also $4 to $7 and $R0 to $R3),
; and nothing else ($R6 and $R7 also by the test harness's log lines). The
; template's pages use $R0 to $R7 but do not run inside a section; its CheckIfAppIsRunning uses $R0 to $R3. $PaddockHaveSetup (1 or 0) says whether paddock-setup.exe was available to this run.
;
; $PaddockTrusted (1 or 0) says whether this run may use the contents of $INSTDIR. An uninstaller that does not trust its folder ends
; in PADDOCK_INSTALLING (the first macro of PREUNINSTALL) with level 45 (PADDOCK_LEVEL_UNTRUSTED) and "use Settings > Apps", before any app step and before the
; template deletes anything.
;
; Where paddock-setup.exe runs from: the installer extracts its embedded copy to $PLUGINSDIR; the uninstaller cannot `File`, so
; it copies the installed one to $PLUGINSDIR. Both run it from there, never from $INSTDIR: stop-all then never has to end its own
; process, and the uninstaller can delete $INSTDIR\paddock-setup.exe. When the installed copy is missing, the uninstaller says so
; in the details ("NOTE paddock-setup.exe is missing ...") and skips the steps that need it: the in-place check compares
; $EXEDIR with $INSTDIR itself, programs are asked to close through Restart Manager (best effort: it ends nothing while another
; session holds a file), the start-at-login clean-up is skipped, and the uninstall goes on.
;
; Windows Security can delete the unsigned paddock-setup.exe (a machine-learning false detection), from $PLUGINSDIR or from
; $INSTDIR. The installer never reports a success then: every _PADDOCK_RUN checks the file before and (on a failure code) after the
; call, PADDOCK_INSTALLING right after the extraction, and PADDOCK_FINISH_INSTALL as the last step of POSTINSTALL;
; a missing file ends the install with PADDOCK_LEVEL_TOOL_MISSING (44) and the message in PADDOCK_REMOVED_MSG
; (PADDOCK_REMOVED_INSTALLED_MSG when only the installed copy is missing at the end). The uninstaller skips
; and says so instead (above).
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
;   clean-staging --dir <dir> --keep <the installer's own folder, $EXEDIR>   (what runs from there is never touched)
;   hold-installing                                             stays running until the installer exits
;   check-version --dir <dir> --expect <version>                12 = the installed paddock-setup.exe has another FileVersion (only reported)
; Any other non-zero code is a failure of the tool itself (the details and $INSTDIR\updates\setup.log say why).
; The mechanism: stop-all ends every session's listed programs itself, by handle (graceful first in the caller's own session;
; Restart Manager's shutdown does nothing while another session holds a file, measured in S0). Its exit code 11 is the one
; answer to "does a listed program still run"; the include does no listing of its own, because Restart Manager also lists
; system components that only map the files (the x86 emulation cache on Arm). The template's CheckIfAppIsRunning, which uses
; RmShutdown and aborts on any non-zero code, therefore finds nothing running. The app that .onInstSuccess starts with RunAsUser gets the desktop
; user's own TEMP and TMP, so nothing is restored here.

!ifndef PADDOCK_PER_MACHINE_INCLUDED
!define PADDOCK_PER_MACHINE_INCLUDED
!define PADDOCK_INCLUDE_VERSION "10"
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

; paddock-setup.exe has its own version (the crate's), not the app's, so its bytes do not change with an app release (Dyno 1.1.1 WP-7).
; check-version compares the installed copy with the version of the embedded one, read from the file at compile time (the test
; harness's stub has a version resource too).
!getdllversion "${PADDOCK_SETUP_DIR}\paddock-setup.exe" PADDOCK_SV_
!define PADDOCK_SETUP_VERSION "${PADDOCK_SV_1}.${PADDOCK_SV_2}.${PADDOCK_SV_3}"

!include LogicLib.nsh
!include FileFunc.nsh
!include "Win\RestartManager.nsh"
!ifndef PADDOCK_STOP_IMAGES
  !include /NONFATAL "${PADDOCK_SETUP_DIR}\paddock-per-machine.generated.nsh"
!endif
!ifndef PADDOCK_STOP_IMAGES
  !error "paddock-per-machine.generated.nsh must !define PADDOCK_STOP_IMAGES (comma-separated file names); run PADDOCK_WRITE_TS=1 cargo test -p paddock-service --lib per_machine"
!endif

; Error levels of a refusal. The reason is also written to $INSTDIR\updates\setup.log through `paddock-setup note` (nothing is ever
; written to %TEMP% while elevated: it can be user-writable); for a refused folder (level 41) there is no safe folder, so the
; message and the level are all there is. The same for a removed tool (level 44): there is no tool to write the note.
; Their own numbers: NSIS itself ends with 1 (cancelled) and 2 (aborted or failed), and `paddock-setup apply-update` has its own
; refusal codes from 20 up, so a silent /S run (or a log) can tell the causes apart.
!define PADDOCK_LEVEL_PER_USER 40
!define PADDOCK_LEVEL_BAD_DIR 41
!define PADDOCK_LEVEL_STILL_RUNNING 42
!define PADDOCK_LEVEL_TOOL_FAILED 43
; Windows Security (Defender's machine-learning detection) can delete the unsigned paddock-setup.exe, from $PLUGINSDIR or from
; $INSTDIR, after the installer wrote it. The installer then ends here instead of reporting a success with steps that never ran.
!define PADDOCK_LEVEL_TOOL_MISSING 44
; An uninstaller started from a folder that is not the registered install folder ends here and runs and deletes nothing.
!define PADDOCK_LEVEL_UNTRUSTED 45

!define PADDOCK_SETUP "$PLUGINSDIR\paddock-setup.exe"
!define PADDOCK_REMOVED_INSTALLED_MSG "${PRODUCTNAME} is installed, but Windows Security removed paddock-setup.exe, a part of ${PRODUCTNAME}, as a false detection. Open Windows Security > Protection history, restore it or allow it, then run the installer again."
!define PADDOCK_REMOVED_MSG "Windows Security removed paddock-setup.exe, a part of ${PRODUCTNAME}, as a false detection. Open Windows Security > Protection history, restore it or allow it, then run the installer again."

Var PaddockHaveSetup
; 1 when this run may use the install folder's contents: always in the installer (it carries its own paddock-setup.exe), and in
; the uninstaller only when $INSTDIR is the folder the checked installer registered (see _PADDOCK_TRUST_FOLDER).
Var PaddockTrusted

; A path for a command line inside quotes: a trailing backslash (a drive root) would escape the closing quote, so it is doubled.
!macro _PADDOCK_ARG _OUT _IN
  StrCpy ${_OUT} "${_IN}"
  StrCpy $R9 ${_OUT} 1 -1
  ${If} $R9 == "\"
    StrCpy ${_OUT} "${_OUT}\"
  ${EndIf}
!macroend

; The tool is gone from where it ran from. Installer: it ends with PADDOCK_LEVEL_TOOL_MISSING and the guidance (never a success).
; Uninstaller: a stated skip, the uninstall goes on without the steps that need it (see the header). _WHERE says which check saw it.
!macro _PADDOCK_TOOL_GONE _WHERE
  DetailPrint "paddock-setup.exe is missing (${_WHERE}): $PLUGINSDIR\paddock-setup.exe"
  !ifndef __UNINSTALL__
    !insertmacro PADDOCK_ABORT "${PADDOCK_REMOVED_MSG}" ${PADDOCK_LEVEL_TOOL_MISSING} ""
  !else
    !insertmacro _PADDOCK_TEST_LOG "NOTE paddock-setup.exe is missing (${_WHERE}): Windows Security may have removed it; steps skipped"
    StrCpy $PaddockHaveSetup 0
  !endif
!macroend

; Runs paddock-setup.exe with the arguments and leaves its exit code in $0 (the details show the line and the code); with the
; folders as $R5 ($INSTDIR) and $R4 ($EXEDIR), already safe inside quotes. Without the program, $0 is "nosetup" and nothing runs.
!macro _PADDOCK_RUN _ARGS
  !insertmacro _PADDOCK_ARG $R5 "$INSTDIR"
  !insertmacro _PADDOCK_ARG $R4 "$EXEDIR"
  ${If} $PaddockHaveSetup == 1
  ${AndIfNot} ${FileExists} "${PADDOCK_SETUP}"
    !insertmacro _PADDOCK_TOOL_GONE "before a call"
  ${EndIf}
  ${If} $PaddockHaveSetup == 1
    DetailPrint 'paddock-setup ${_ARGS}'
    nsExec::ExecToLog '"${PADDOCK_SETUP}" ${_ARGS}'
    Pop $0
    DetailPrint "paddock-setup exit code: $0"
    ; Removed while it ran (or at start): a failure code with the file gone is the removal, not a failure of the tool.
    ${If} $0 != 0
    ${AndIfNot} ${FileExists} "${PADDOCK_SETUP}"
      !insertmacro _PADDOCK_TOOL_GONE "after a call"
    ${EndIf}
  ${EndIf}
  ${If} $PaddockHaveSetup != 1
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

; Sets $PaddockTrusted to 1 when $R9 (a folder without a trailing backslash) is textually "$PROGRAMFILES64\<one name>": a direct child, a
; name without "\" or ":" and not starting with "." (so no 8.3 short path such as C:\PROGRA~1\X and no forward slashes: NSIS does not
; normalise them, and the installer and the uninstaller must agree on what they accept). Otherwise 0. The test harness may name another
; root in PADDOCK_STUB_ROOT (compiled only with PADDOCK_TEST_HARNESS). Uses $0 to $3, $R6 to $R8.
!macro _PADDOCK_PF_CHILD
  StrCpy $PaddockTrusted 0
  StrCpy $R6 "$PROGRAMFILES64"
  !ifdef PADDOCK_TEST_HARNESS
    ReadEnvStr $R8 PADDOCK_STUB_ROOT
    ${If} $R8 != ""
      StrCpy $R6 $R8
    ${EndIf}
  !endif
  StrLen $R8 $R6
  IntOp $R8 $R8 + 1
  StrCpy $R7 $R9 $R8
  ${If} $R7 == "$R6\"
    StrCpy $R7 $R9 "" $R8
    StrLen $1 $R7
    StrCpy $2 $R7 1
    ${If} $1 > 0
    ${AndIf} $2 != "."
      StrCpy $PaddockTrusted 1
      StrCpy $0 0
      ${While} $0 < $1
        StrCpy $3 $R7 1 $0
        ${If} $3 == "\"
        ${OrIf} $3 == ":"
          StrCpy $PaddockTrusted 0
        ${EndIf}
        IntOp $0 $0 + 1
      ${EndWhile}
    ${EndIf}
  ${EndIf}
!macroend

; The uninstaller's trust decision, made in NSIS alone before anything of $INSTDIR is copied or run (an uninstall.exe can be copied
; to a folder the user controls, next to a planted paddock-setup.exe, and run elevated). $PaddockTrusted is 1 only when BOTH hold:
; $INSTDIR (trailing backslash trimmed, case-insensitive) is the folder the installer registered in HKLM (administrators write
; there: the quoted UninstallString is "<folder>\uninstall.exe"), and it is a direct child of $PROGRAMFILES64 (no ".", "..", ":" or
; further backslash in the name). The test harness may name another root in PADDOCK_STUB_ROOT (compiled only with
; PADDOCK_TEST_HARNESS). Uses $0 to $3 and $R6 to $R9.
!macro _PADDOCK_TRUST_FOLDER
  StrCpy $PaddockTrusted 0
  StrCpy $R9 "$INSTDIR"
  StrCpy $R8 $R9 1 -1
  ${If} $R8 == "\"
    StrCpy $R9 $R9 -1
  ${EndIf}
  ; The registered folder.
  ReadRegStr $R7 HKLM "${UNINSTKEY}" "UninstallString"
  StrCpy $R8 $R7 1
  ${If} $R8 == '"'
    StrCpy $R7 $R7 "" 1
    StrCpy $R8 $R7 1 -1
    ${If} $R8 == '"'
      StrCpy $R7 $R7 -1
    ${EndIf}
  ${EndIf}
  ${If} $R7 == "$R9\uninstall.exe"
    !insertmacro _PADDOCK_PF_CHILD
  ${EndIf}
!macroend

; The created mutex is held until the process exits (ADR 4.1 step 4). Installer: extract the embedded paddock-setup.exe.
; Uninstaller: copy the installed one (see the header). Both run it from $PLUGINSDIR. Must be the first macro of PREINSTALL /
; PREUNINSTALL.
!macro PADDOCK_INSTALLING
  InitPluginsDir
  StrCpy $PaddockHaveSetup 0
  StrCpy $PaddockTrusted 0
  !ifndef __UNINSTALL__
    File "/oname=$PLUGINSDIR\paddock-setup.exe" "${PADDOCK_SETUP_DIR}\paddock-setup.exe"
    StrCpy $PaddockHaveSetup 1
    StrCpy $PaddockTrusted 1
    ${IfNot} ${FileExists} "${PADDOCK_SETUP}"
      !insertmacro _PADDOCK_TOOL_GONE "after extraction"
    ${EndIf}
  !else
    !insertmacro _PADDOCK_TRUST_FOLDER
    ${If} $PaddockTrusted == 1
      ClearErrors
      CopyFiles /SILENT "$INSTDIR\paddock-setup.exe" "$PLUGINSDIR\paddock-setup.exe"
      ${IfNot} ${Errors}
        StrCpy $PaddockHaveSetup 1
      ${Else}
        DetailPrint "NOTE paddock-setup.exe is missing from $INSTDIR, perhaps removed by Windows Security as a false detection (Protection history): its steps are skipped (see the include's header for the fallbacks)"
        !insertmacro _PADDOCK_TEST_LOG "NOTE paddock-setup.exe is missing from the uninstaller's folder (Windows Security may have removed it): steps skipped"
      ${EndIf}
    ${Else}
      ; Nothing of this folder is copied or run: it is not the folder the installer registered, or not directly under Program Files.
      DetailPrint "NOTE $INSTDIR is not the install folder registered by the installer: nothing in it is run"
      !insertmacro _PADDOCK_TEST_LOG "NOTE untrusted uninstall folder: nothing in it is run"
      ; And nothing is deleted: the uninstaller ends here, before any step of the app and before the template's own deletions (so the
      ; template never removes the registered uninstall key of the real install on behalf of a copied uninstaller).
      !insertmacro PADDOCK_ABORT "This is not the installed copy of ${PRODUCTNAME}. Use Settings > Apps to uninstall ${PRODUCTNAME}. If ${PRODUCTNAME} is not listed in Settings > Apps, run the ${PRODUCTNAME} installer again, then uninstall." ${PADDOCK_LEVEL_UNTRUSTED} ""
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
  ; The uninstaller trusts only the textual form "$PROGRAMFILES64\<one name>" (_PADDOCK_TRUST_FOLDER), so the installer accepts only
  ; that form too: an 8.3 path (C:\PROGRA~1\X) or forward slashes would pass check-dir and leave an install that cannot be uninstalled.
  StrCpy $R9 "$INSTDIR"
  StrCpy $R8 $R9 1 -1
  ${If} $R8 == "\"
    StrCpy $R9 $R9 -1
  ${EndIf}
  !insertmacro _PADDOCK_PF_CHILD
  ${If} $PaddockTrusted != 1
    !insertmacro PADDOCK_ABORT "Install ${PRODUCTNAME} in Program Files, directly under $PROGRAMFILES64 and spelled that way (no short names or forward slashes). The folder $INSTDIR is not allowed." ${PADDOCK_LEVEL_BAD_DIR} ""
  ${EndIf}
  StrCpy $PaddockTrusted 1
!macroend

; Asks check-dir without aborting: $0 is 0 when $INSTDIR is accepted (a direct child of Program Files, owned and protected by
; administrators), "nosetup" without paddock-setup.exe or in an uninstaller that does not trust its folder ($PaddockTrusted 0: nothing
; is run then), any other value when refused. For an app step that must not run a program of
; the install folder unless the folder is accepted, in a place that cannot abort (the uninstaller). PREINSTALL has PADDOCK_CHECK_DIR.
!macro PADDOCK_DIR_ACCEPTED
  !insertmacro _PADDOCK_RUN 'check-dir --dir "$R5"'
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
    ${IfNot} ${FileExists} "${PADDOCK_SETUP}"
      !insertmacro _PADDOCK_TOOL_GONE "before guard"
    ${EndIf}
    !insertmacro _PADDOCK_ARG $R5 "$INSTDIR"
    DetailPrint "paddock-setup guard"
    nsExec::ExecToStack '"${PADDOCK_SETUP}" guard --product "${PRODUCTNAME}" --main "${MAINBINARYNAME}.exe" --dir "$R5"$2'
    Pop $0
    Pop $1
    DetailPrint "paddock-setup exit code: $0"
    ${If} $0 != 0
    ${AndIfNot} ${FileExists} "${PADDOCK_SETUP}"
      !insertmacro _PADDOCK_TOOL_GONE "after a call"
    ${EndIf}
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
!macro _PADDOCK_STOP_FALLBACK
  DetailPrint "NOTE paddock-setup.exe is missing (Windows Security may have removed it): programs are asked to close through Restart Manager only"
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
!macroend

!macro PADDOCK_STOP_ALL
  ${If} $PaddockHaveSetup == 1
    !insertmacro _PADDOCK_RUN 'stop-all --dir "$R5" --images "${PADDOCK_STOP_IMAGES}"'
    ${If} $0 == "nosetup"
      ; The tool vanished before the call (uninstaller only; the installer ended in _PADDOCK_RUN).
      !insertmacro _PADDOCK_STOP_FALLBACK
    ${ElseIf} $0 == 11
      !insertmacro PADDOCK_ABORT "${PRODUCTNAME} could not be closed for every signed-in user. Ask them to quit ${PRODUCTNAME}, then run this installer again." ${PADDOCK_LEVEL_STILL_RUNNING} "$INSTDIR\${MAINBINARYNAME}.exe"
    ${ElseIf} $0 != 0
      !insertmacro PADDOCK_ABORT "${PRODUCTNAME} could not be closed (code $0). The reason is in $INSTDIR\updates\setup.log" ${PADDOCK_LEVEL_TOOL_FAILED} "$INSTDIR\${MAINBINARYNAME}.exe"
    ${EndIf}
  ${Else}
    !insertmacro _PADDOCK_STOP_FALLBACK
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
  ${EndIf}
  ; The tool was never there or vanished before the call ($0 is "nosetup"): never let a reinstall run as a full uninstall.
  ${If} $PaddockHaveSetup != 1
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
;   _MSG      the message (an NSIS string; it is shown before any register above is written, so it may use $0 to $3)
;   _LEVEL    the error level (PADDOCK_LEVEL_*)
;   _RESTART  the full path of a program to start again as the desktop user, or "" (only used with /UPDATE and /R)
!macro PADDOCK_ABORT _MSG _LEVEL _RESTART
  DetailPrint "${PRODUCTNAME}: ${_MSG}"
  ; The reason survives the installer in $INSTDIR\updates\setup.log, written by the tool through its checked log (a refused fresh
  ; install removes only an empty $INSTDIR, and the folder holding the log stays). Not for a refused folder (nothing safe to
  ; write to) and not without the tool. Never a path under %TEMP% from this elevated process.
  !if ${_LEVEL} != ${PADDOCK_LEVEL_BAD_DIR}
  !if ${_LEVEL} != ${PADDOCK_LEVEL_TOOL_MISSING}
    ${If} $PaddockHaveSetup == 1
      !insertmacro _PADDOCK_ARG $R5 "$INSTDIR"
      !if ${_LEVEL} == ${PADDOCK_LEVEL_PER_USER}
        nsExec::ExecToLog '"${PADDOCK_SETUP}" note --dir "$R5" --text "refused (level ${_LEVEL}): an older per-user copy blocks the install"'
      !else if ${_LEVEL} == ${PADDOCK_LEVEL_STILL_RUNNING}
        nsExec::ExecToLog '"${PADDOCK_SETUP}" note --dir "$R5" --text "refused (level ${_LEVEL}): a listed program still runs"'
      !else
        nsExec::ExecToLog '"${PADDOCK_SETUP}" note --dir "$R5" --text "refused (level ${_LEVEL}): the tool failed"'
      !endif
      Pop $R7
      DetailPrint "The reason is also in $INSTDIR\updates\setup.log (note exit code $R7)"
    ${EndIf}
  !endif
  !endif
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

; The installed copy must still be there at the very end of POSTINSTALL (last step of PADDOCK_FINISH_INSTALL), so the app's own steps
; (helper refresh, clean-run, clean-staging) have finished first; the files are installed then and the message says so.
!macro _PADDOCK_CHECK_INSTALLED
  ${IfNot} ${FileExists} "$INSTDIR\paddock-setup.exe"
    DetailPrint "paddock-setup.exe is missing from $INSTDIR after it was written"
    !insertmacro PADDOCK_ABORT "${PADDOCK_REMOVED_INSTALLED_MSG}" ${PADDOCK_LEVEL_TOOL_MISSING} ""
  ${EndIf}
!macroend

; POSTINSTALL: the installed copy of paddock-setup.exe (the updater's elevated hand-off and the uninstaller use it).
!macro PADDOCK_INSTALL_SETUP
  File "/oname=$INSTDIR\paddock-setup.exe" "${PADDOCK_SETUP_DIR}\paddock-setup.exe"
  ; The file copy silently fails to overwrite a paddock-setup.exe that is still running (the `apply-update` that handed over). The
  ; build gives it its own FileVersion (PADDOCK_SETUP_VERSION, read from the embedded file), so the new copy compares the installed one: a mismatch is
  ; printed in the details and written to setup.log by the tool (exit 12), and never fails the install.
  !ifdef PADDOCK_SETUP_VERSION
    !insertmacro _PADDOCK_RUN 'check-version --dir "$R5" --expect ${PADDOCK_SETUP_VERSION}'
    ${If} $0 == 12
      DetailPrint "NOTE paddock-setup.exe was not replaced; run the installer again to fix it (see setup.log)"
      !insertmacro _PADDOCK_TEST_LOG "NOTE paddock-setup.exe version mismatch"
    ${EndIf}
  !endif
!macroend

; Last in POSTINSTALL: delete the staged update that is not in use (the next apply-update removes the rest).
!macro PADDOCK_FINISH_INSTALL
  !insertmacro _PADDOCK_RUN 'clean-staging --dir "$R5" --keep "$R4"'
  !insertmacro _PADDOCK_CHECK_INSTALLED
!macroend

; Last in NSIS_HOOK_POSTUNINSTALL (the template runs it after its own `Delete "$INSTDIR\uninstall.exe"` and `RMDir "$INSTDIR"`):
; a Delete that lost to a transient lock (measured on the Windows 11 Arm VM: 16 of 36 first uninstalls after a fresh start; Restart Manager named
; the holder in 13 of those 16: XtaCache, the x86 emulation cache service of Windows on Arm, which reads a freshly run x64 program for
; up to about 3 s; no holder is expected on x64 hardware; the launcher process is gone by then and the file is not the running copy,
; which lives in %TEMP%\~nsu.tmp) leaves
; uninstall.exe and so the folder. This retries the Delete, polling the file's state every 100 ms for at most 5 s, then removes
; the folder (not recursive: a folder that holds anything else stays, that is the answer, not a failure). Only if the file is
; still there after the bound it is left in place and the tool's note says why (no /REBOOTOK: a restart would delete the uninstall.exe
; of a reinstall made before it, and the reboot flag would ask the user to restart). The first failed attempt records why (an exclusive
; open's error code and how many programs Restart Manager lists as using the file). The outcome goes to the installer's details;
; the note goes to $INSTDIR\updates\setup.log only in that case (the folder stays then, so the log has a place).
; Outcomes: removed | removed-after-retries | kept-other-files | left-in-place | skipped-update.
!macro PADDOCK_FINISH_UNINSTALL
  ${If} $UpdateMode == 1
    !insertmacro _PADDOCK_TEST_LOG "FINISH_UNINSTALL outcome=skipped-update"
  ${Else}
    ; $R0 and $R1: why the file is held; $R2: the outcome; $R3: the start tick; $R4: tries; $0 to $7: scratch.
    System::Call 'kernel32::GetTickCount()i.R3'
    StrCpy $R4 0
    StrCpy $R0 ""
    StrCpy $R1 ""
    ${While} ${FileExists} "$INSTDIR\uninstall.exe"
      Delete "$INSTDIR\uninstall.exe"
      ${IfNot} ${FileExists} "$INSTDIR\uninstall.exe"
        ${ExitWhile}
      ${EndIf}
      ${If} $R4 == 0
        ; The cause, once: can the file be opened with no sharing, and how many programs does Restart Manager list as using it?
        System::Call 'kernel32::CreateFileW(w "$INSTDIR\uninstall.exe", i 0x80000000, i 0, p 0, i 3, i 0, p 0) p.r6 ?e'
        Pop $7
        ${If} $6 == -1
          StrCpy $R0 "open-error-$7"
        ${Else}
          System::Call 'kernel32::CloseHandle(p r6)'
          StrCpy $R0 "opened-exclusive"
        ${EndIf}
        !insertmacro RestartManager_StartSession $R8
        ${If} $R8 != ""
          !insertmacro RestartManager_RegisterFile $R8 "$INSTDIR\uninstall.exe"
          System::Alloc 4096
          Pop $5
          StrCpy $2 1
          StrCpy $6 ""
          StrCpy $7 ""
          System::Call 'RSTRTMGR::RmGetList(i $R8, *i .r1, *i r2r2, p r5, *i .r3) i .r0'
          ${If} $2 > 0
            System::Call '*$5(i .r6, l, &w256 .r7)'
          ${EndIf}
          System::Free $5
          !insertmacro RestartManager_EndSession $R8
          StrCpy $R1 "rm-holders-$1 pid-$6 $7"
        ${EndIf}
      ${EndIf}
      IntOp $R4 $R4 + 1
      ${If} $R4 >= 50
        ${ExitWhile}
      ${EndIf}
      Sleep 100
    ${EndWhile}
    System::Call 'kernel32::GetTickCount()i.r2'
    IntOp $2 $2 - $R3
    ${If} ${FileExists} "$INSTDIR\uninstall.exe"
      ; Still held after the bound: it stays. No /REBOOTOK: a restart would delete whatever is at that path then, including the
      ; uninstall.exe of a reinstall made before it, and the flag would ask the user to restart. The tool writes why into
      ; $INSTDIR\updates\setup.log (the folder stays, so the log has a place); a reinstall over it overwrites the file.
      ${If} $PaddockHaveSetup == 1
        !insertmacro _PADDOCK_ARG $R5 "$INSTDIR"
        nsExec::ExecToLog '"${PADDOCK_SETUP}" note --dir "$R5" --text "uninstall.exe could not be removed after $R4 tries ($2 ms, $R0, $R1): left in place"'
        Pop $0
      ${EndIf}
      StrCpy $R2 "left-in-place"
    ${Else}
      RMDir "$INSTDIR"
      ${If} ${FileExists} "$INSTDIR\*.*"
        StrCpy $R2 "kept-other-files"
      ${ElseIf} $R4 == 0
        StrCpy $R2 "removed"
      ${Else}
        StrCpy $R2 "removed-after-retries"
      ${EndIf}
    ${EndIf}
    DetailPrint "uninstall: $INSTDIR $R2 (tries $R4, $2 ms, $R0, $R1)"
    ; The harness line is written last: its macro uses $R6 and $R7, so the values go in first.
    StrCpy $R5 "FINISH_UNINSTALL outcome=$R2 tries=$R4 ms=$2 $R0 $R1"
    !insertmacro _PADDOCK_TEST_LOG "$R5"
  ${EndIf}
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
