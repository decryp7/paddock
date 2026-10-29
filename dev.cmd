@echo off
setlocal
rem Runs ./dev with Git for Windows' bash. It is found next to the git on the PATH (git --exec-path is <Git>\mingw64\libexec\git-core),
rem else any bash on the PATH that is not WSL's (System32).
set "DEVBASH="
for /f "delims=" %%G in ('git --exec-path 2^>nul') do for %%I in ("%%G\..\..\..") do if exist "%%~fI\bin\bash.exe" set "DEVBASH=%%~fI\bin\bash.exe"
if not defined DEVBASH for /f "delims=" %%B in ('where bash 2^>nul ^| findstr /v /i "\\system32\\"') do if not defined DEVBASH set "DEVBASH=%%B"
if not defined DEVBASH echo dev: Git for Windows bash.exe was not found. Install Git for Windows and run again. 1>&2 & exit /b 2
"%DEVBASH%" "%~dp0dev" %*
exit /b %ERRORLEVEL%
