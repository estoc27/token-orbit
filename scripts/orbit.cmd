@echo off
REM Token Orbit control script - summon/dismiss the HUD from anywhere.
REM (keep this file ASCII-only: cmd.exe reads .cmd in the OEM codepage)
REM
REM Usage: orbit.cmd [toggle|show|hide|quit|start]   (default: toggle)
REM
REM Talks to the running HUD through %USERPROFILE%\.token-orbit\control -
REM a file the app watches. No ports, no window activation tricks.
REM If the HUD is not running, toggle/show/start will launch it.

setlocal
set "ACTION=%~1"
if "%ACTION%"=="" set "ACTION=toggle"
set "CTRL=%USERPROFILE%\.token-orbit\control"
set "EXE=%~dp0..\target\release\token-orbit.exe"

REM Use the fully-qualified findstr: when invoked from Git Bash, a bare `find`
REM resolves to GNU find and silently breaks process detection (observed).
tasklist /FI "IMAGENAME eq token-orbit.exe" 2>nul | "%SystemRoot%\System32\findstr.exe" /I "token-orbit.exe" >nul
set "RUNNING=%ERRORLEVEL%"

if "%ACTION%"=="start" (
  if not "%RUNNING%"=="0" goto :launch
  echo [orbit] already running
  exit /b 0
)

if "%RUNNING%"=="0" (
  if not exist "%USERPROFILE%\.token-orbit" mkdir "%USERPROFILE%\.token-orbit"
  >"%CTRL%" echo %ACTION%
  echo [orbit] sent: %ACTION%
  exit /b 0
)

REM Not running: hide/quit are no-ops; toggle/show mean "launch it".
if "%ACTION%"=="hide" exit /b 0
if "%ACTION%"=="quit" exit /b 0

:launch
if not exist "%EXE%" (
  echo [orbit] executable not found: %EXE%
  echo         build first: cargo build --release -p token-orbit
  exit /b 1
)
REM Start-Process fully detaches the child: with cmd's `start` the HUD can
REM inherit a redirected stdout pipe and keep the caller blocked (observed).
powershell -NoProfile -Command "Start-Process -FilePath '%EXE%'"
echo [orbit] launched
exit /b 0
