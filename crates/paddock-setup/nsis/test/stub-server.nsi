; A stub service program for the harness (it stands for pitwall-server.exe): appends "SERVER <args>" to %PADDOCK_STUB_LOG%, exits 0.
; The harness cases assert it did NOT run where the uninstaller does not trust its folder.
Unicode true
SilentInstall silent
RequestExecutionLevel user
OutFile "harness-server.exe"
!include FileFunc.nsh
!include LogicLib.nsh
!insertmacro GetParameters
Section
  ${GetParameters} $0
  ReadEnvStr $1 PADDOCK_STUB_LOG
  ${If} $1 != ""
    FileOpen $2 $1 a
    FileSeek $2 0 END
    FileWrite $2 "SERVER $0$\r$\n"
    FileClose $2
  ${EndIf}
  SetErrorLevel 0
SectionEnd
