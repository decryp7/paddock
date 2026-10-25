@echo off
rem Same as rustc-wrapper.sh: the build folder is not part of what rustc compiles, so it stays out of the sccache key.
set "CARGO_TARGET_DIR="
set "CARGO_BUILD_TARGET_DIR="
sccache.exe %*
exit /b %ERRORLEVEL%
