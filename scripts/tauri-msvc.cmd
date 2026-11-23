@echo off
setlocal

set "REPOSITORY_ROOT=%~dp0.."
set "VSDEVCMD=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"

if exist "%VSDEVCMD%" (
  call "%VSDEVCMD%" -arch=x64 -host_arch=x64 >nul
  if errorlevel 1 exit /b %errorlevel%
)

if exist "%REPOSITORY_ROOT%\.tooling\cargo\bin\cargo.exe" (
  set "CARGO_HOME=%REPOSITORY_ROOT%\.tooling\cargo"
  set "RUSTUP_HOME=%REPOSITORY_ROOT%\.tooling\rustup"
  set "PATH=%REPOSITORY_ROOT%\.tooling\cargo\bin;%PATH%"
) else if exist "%REPOSITORY_ROOT%\..\Termexo\.tooling\cargo\bin\cargo.exe" (
  set "CARGO_HOME=%REPOSITORY_ROOT%\..\Termexo\.tooling\cargo"
  set "RUSTUP_HOME=%REPOSITORY_ROOT%\..\Termexo\.tooling\rustup"
  set "PATH=%REPOSITORY_ROOT%\..\Termexo\.tooling\cargo\bin;%PATH%"
)

"%REPOSITORY_ROOT%\apps\desktop-ui\node_modules\.bin\tauri.cmd" %*
exit /b %errorlevel%
