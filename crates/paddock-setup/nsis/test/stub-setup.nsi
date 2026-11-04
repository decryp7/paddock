; A stub paddock-setup.exe for testing the include without the real program. Appends "<args>" to %PADDOCK_STUB_LOG%,
; exits with %PADDOCK_STUB_EXIT_<COMMAND>% (default 0), prints %PADDOCK_STUB_OUT_GUARD% on stdout for `guard`.
; The commands: check-dir guard stop-all same-folder clean-run clean-staging hold-installing.
Unicode true
SilentInstall silent
RequestExecutionLevel user
OutFile "paddock-setup.exe"
!include FileFunc.nsh
!include LogicLib.nsh
!insertmacro GetParameters
; The setting NAME from the environment, else from C:\ProgramData\paddock-stub.ini ([env] NAME=value): an environment
; does not cross a UAC prompt, the file does.
!macro GET _VAR _NAME
  ReadEnvStr ${_VAR} ${_NAME}
  ${If} ${_VAR} == ""
    ReadINIStr ${_VAR} "C:\ProgramData\paddock-stub.ini" "env" "${_NAME}"
  ${EndIf}
!macroend
Section
  ${GetParameters} $0
  !insertmacro GET $1 PADDOCK_STUB_LOG
  ${If} $1 == ""
    StrCpy $1 "$PROGRAMFILES64\..\Users\Public\paddock-stub.log"
  ${EndIf}
  ${If} $1 != ""
    FileOpen $2 $1 a
    FileSeek $2 0 END
    FileWrite $2 "$0$\r$\n"
    FileClose $2
  ${EndIf}
  StrCpy $R0 $0 9
  StrCpy $R1 $0 5
  StrCpy $R2 $0 8
  StrCpy $R3 $0 11
  StrCpy $R4 $0 9
  StrCpy $R5 $0 13
  StrCpy $3 0
  ${If} $R0 == "check-dir"
    !insertmacro GET $3 PADDOCK_STUB_EXIT_CHECK_DIR
  ${ElseIf} $R1 == "guard"
    !insertmacro GET $3 PADDOCK_STUB_EXIT_GUARD
    !insertmacro GET $4 PADDOCK_STUB_OUT_GUARD
    ${If} $4 != ""
      System::Call 'kernel32::GetStdHandle(i -11)p.r5'
      StrLen $6 $4
      System::Call 'kernel32::WriteFile(p r5, m r4, i r6, *i .r7, p 0)'
    ${EndIf}
    !insertmacro GET $9 PADDOCK_STUB_RESTART_GUARD
    ${If} $9 != ""
      StrCpy $9 "$\r$\nRESTART:$9$\r$\n"
      System::Call 'kernel32::GetStdHandle(i -11)p.r5'
      StrLen $6 $9
      System::Call 'kernel32::WriteFile(p r5, m r9, i r6, *i .r7, p 0)'
    ${EndIf}
  ${ElseIf} $R2 == "stop-all"
    !insertmacro GET $3 PADDOCK_STUB_EXIT_STOP_ALL
  ${ElseIf} $R3 == "same-folder"
    !insertmacro GET $3 PADDOCK_STUB_EXIT_SAME_FOLDER
  ${ElseIf} $R4 == "clean-run"
    !insertmacro GET $3 PADDOCK_STUB_EXIT_CLEAN_RUN
  ${ElseIf} $R5 == "clean-staging"
    !insertmacro GET $3 PADDOCK_STUB_EXIT_CLEAN_STAGING
  ${EndIf}
  ${If} $3 == ""
    StrCpy $3 0
  ${EndIf}
  SetErrorLevel $3
SectionEnd
