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

; The Microsoft Visual C++ runtime (vc_redist.x64.exe, pinned in vc-redist.lock.json, staged next to this file by
; apps/pitwall/scripts/ci/stage-vc-redist.mjs before the installer is built). The llama.cpp engine Pitwall downloads imports
; vcruntime140.dll and msvcp140.dll, which a clean Windows PC does not have: the engine then exits with 0xC0000135 and its
; self-test fails. Microsoft's own advice is central deployment through the redistributable package (it is serviced by Windows
; Update); the installer is elevated (per machine), so it can run the package. makensis resolves a relative path from its working
; folder, so the folder is named here, where __FILEDIR__ is this file's.
!define /redef PITWALL_WINDOWS_DIR "${__FILEDIR__}"
!if /FileExists "${PITWALL_WINDOWS_DIR}\vc_redist.x64.exe"
!else
  !error "vc_redist.x64.exe is not next to installer-hooks.nsh (${PITWALL_WINDOWS_DIR}): the bundle step stages it (node apps/pitwall/scripts/ci/stage-vc-redist.mjs) before the installer is built"
!endif

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

; Installs the VC++ runtime when the PC lacks it or has an older one (the package itself also skips the install when a newer one is there).
; The pinned version is 14.52.37029 (vc-redist.lock.json): the registry value Major.Minor.Bld of HKLM\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64
; says what is installed, and nothing is run when that is the pinned version or newer. The x64 key is read in the 64-bit registry view:
; this installer is for an x64 target, whose template already works in that view (SetRegView 64), and the macro sets it again and leaves it.
; Exit codes of the package: 0 installed, 1638 a newer version is installed, 3010 and 1641 installed but a restart is advised (the engine
; starts without one; /norestart keeps the PC from restarting; no reboot flag is set, which would make the finish page offer "Reboot now"
; first), 1618 another installation is running. A package that could not be started ("error"), one that was stopped after the time limit
; ("timeout": nsExec ends the process) and any other code are shown in the details and, in a normal run, in a message: Pitwall installs, and the
; engine's self-test then names the missing runtime and the next step (engines.rs). The limit is 5 minutes: the package took 25 s on the
; test VM (measured 2026-11-11, fresh install of this installer, redist log dd_vcredist_amd64_*.log in %TEMP%), so a longer wait means it is stuck.
; ARM64: no ARM64 installer is built; the x64 installer was run only under emulation on Windows 11 ARM64 (it installed the runtime there).
!define PITWALL_VC_MAJOR 14
!define PITWALL_VC_MINOR 52
!define PITWALL_VC_BLD 37029
!define PITWALL_VC_TIMEOUT_MS 300000
!macro _PITWALL_INSTALL_VC_RUNTIME
  Push $0
  Push $1
  Push $2
  SetRegView 64
  ReadRegDWORD $0 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Installed"
  ReadRegDWORD $1 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Major"
  ReadRegDWORD $2 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Minor"
  ${If} $0 == 1
    ${If} $1 > ${PITWALL_VC_MAJOR}
      Goto vc_runtime_current
    ${ElseIf} $1 == ${PITWALL_VC_MAJOR}
      ${If} $2 > ${PITWALL_VC_MINOR}
        Goto vc_runtime_current
      ${ElseIf} $2 == ${PITWALL_VC_MINOR}
        ReadRegDWORD $1 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Bld"
        ${If} $1 >= ${PITWALL_VC_BLD}
          Goto vc_runtime_current
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "${PITWALL_WINDOWS_DIR}\vc_redist.x64.exe"
  DetailPrint "Installing the Microsoft Visual C++ runtime (vc_redist.x64.exe)"
  ; nsExec stops (terminates) the package after the limit, so a stuck one cannot hold the installer for ever. The result is the
  ; exit code, "error" (it could not be started) or "timeout".
  nsExec::Exec /TIMEOUT=${PITWALL_VC_TIMEOUT_MS} '"$PLUGINSDIR\vc_redist.x64.exe" /install /quiet /norestart'
  Pop $0
  DetailPrint "vc_redist.x64.exe: exit code $0"
  Delete "$PLUGINSDIR\vc_redist.x64.exe"
  ${If} $0 == 3010
  ${OrIf} $0 == 1641
    DetailPrint "The Visual C++ runtime is installed; Windows advises a restart (the engine starts without one)."
  ${ElseIf} $0 == 0
  ${OrIf} $0 == 1638
    ; installed, or a newer version is there already
  ${ElseIf} $0 == "timeout"
    DetailPrint "WARNING the Visual C++ runtime package did not finish within 5 minutes and was stopped (see dd_vcredist_amd64_*.log in %TEMP%). Install it from https://aka.ms/vc14/vc_redist.x64.exe, then restart Pitwall."
    IfSilent vc_runtime_done
      MessageBox MB_ICONEXCLAMATION|MB_OK "Pitwall is installed, but the Microsoft Visual C++ runtime did not finish installing within 5 minutes and was stopped. The llama.cpp engine cannot start without it. Install it from https://aka.ms/vc14/vc_redist.x64.exe, then restart Pitwall."
  ${ElseIf} $0 == "error"
    DetailPrint "WARNING the Visual C++ runtime package could not be started. Install it from https://aka.ms/vc14/vc_redist.x64.exe"
    IfSilent vc_runtime_done
      MessageBox MB_ICONEXCLAMATION|MB_OK "Pitwall is installed, but the Microsoft Visual C++ runtime package could not be started. The llama.cpp engine cannot start without it. Install it from https://aka.ms/vc14/vc_redist.x64.exe, then restart Pitwall."
  ${ElseIf} $0 == 1618
    DetailPrint "WARNING the Visual C++ runtime was not installed: another installation is running (exit code 1618)."
    IfSilent vc_runtime_done
      MessageBox MB_ICONEXCLAMATION|MB_OK "Pitwall is installed, but the Microsoft Visual C++ runtime could not be installed because another installation is running (code 1618). Finish or restart that installation, then run the Pitwall installer again; the llama.cpp engine cannot start without it."
  ${Else}
    DetailPrint "WARNING the Visual C++ runtime was not installed (exit code $0). The llama.cpp engine needs it: install vc_redist.x64.exe from https://aka.ms/vc14/vc_redist.x64.exe"
    IfSilent vc_runtime_done
      MessageBox MB_ICONEXCLAMATION|MB_OK "Pitwall is installed, but the Microsoft Visual C++ runtime could not be installed (code $0). The llama.cpp engine cannot start without it. Install it from https://aka.ms/vc14/vc_redist.x64.exe, then restart Pitwall."
  ${EndIf}
  Goto vc_runtime_done
  vc_runtime_current:
  DetailPrint "The Visual C++ runtime ${PITWALL_VC_MAJOR}.${PITWALL_VC_MINOR}.${PITWALL_VC_BLD} or newer is installed already"
  vc_runtime_done:
  SetOutPath "$INSTDIR"
  Pop $2
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro _PITWALL_INSTALL_VC_RUNTIME
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
