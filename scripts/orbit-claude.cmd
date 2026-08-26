@echo off
REM Run Claude Code through the Token_Orbit proxy tap.
REM
REM Sets ANTHROPIC_BASE_URL for THIS session only. We deliberately avoid a global
REM env var: if the proxy is down, a global setting would break Claude Code
REM entirely (fail-closed). With this wrapper, plain `claude` is unaffected.
REM
REM NOTE: keep this file ASCII-only. cmd.exe reads .cmd in the OEM codepage,
REM so non-ASCII comments corrupt parsing (observed 2026-08-26).
REM
REM Usage: orbit-claude.cmd            (interactive session)
REM        orbit-claude.cmd -p "ask"   (one-shot)

setlocal

REM Probe the proxy first; if it is not up, run without it (fail-open).
powershell -NoProfile -Command "try{$c=New-Object Net.Sockets.TcpClient('127.0.0.1',8377);$c.Close();exit 0}catch{exit 1}" >nul 2>&1
if errorlevel 1 (
  echo [Token_Orbit] proxy is not running - usage will not refresh.
  echo               Start the Token_Orbit HUD to bring it up.
) else (
  set ANTHROPIC_BASE_URL=http://127.0.0.1:8377
)

call claude.cmd %*
endlocal
