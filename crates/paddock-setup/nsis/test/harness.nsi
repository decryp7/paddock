; A minimal installer with the hook points of Tauri's NSIS template (Section Install: SetOutPath, PREINSTALL, File,
; POSTINSTALL; Section Uninstall: PREUNINSTALL, Delete, RMDir; /P, /UPDATE and /R read from $CMDLINE; $UpdateMode read
; after the hook). It runs the real include against the stub paddock-setup.exe (build.sh). Not the real template: the
; real one is checked on the VM by the per-machine suite (ADR 0053 section 9).
Unicode true
RequestExecutionLevel admin
SilentInstall silent
!define PADDOCK_TEST_HARNESS
!include LogicLib.nsh
!include FileFunc.nsh
!include StrFunc.nsh
${StrLoc}
!insertmacro GetOptions
!define PRODUCTNAME "PaddockHarness"
!define MAINBINARYNAME "harness"
!define BUNDLEID "net.decryptology.paddockharness"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
Var PassiveMode
Var UpdateMode
!include "harness-hooks.nsh"
Name "${PRODUCTNAME}"
OutFile "harness-installer.exe"
InstallDir "$PROGRAMFILES64\${PRODUCTNAME}"

Function .onInit
  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
FunctionEnd

Function un.onInit
  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
FunctionEnd

Section Install
  SetOutPath $INSTDIR
  !insertmacro NSIS_HOOK_PREINSTALL
  File "harness.txt"
  File "harness-server.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  !insertmacro NSIS_HOOK_POSTINSTALL
SectionEnd

Section Uninstall
  !insertmacro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\harness.txt"
  Delete "$INSTDIR\harness-server.exe"
  Delete "$INSTDIR\uninstall.exe"
  DeleteRegKey HKLM "${UNINSTKEY}"
  RMDir "$INSTDIR"
  !insertmacro NSIS_HOOK_POSTUNINSTALL
SectionEnd
