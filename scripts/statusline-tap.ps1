# Token_Orbit statusline tap (Windows).
# Claude Code가 statusline 명령의 stdin으로 주는 세션 JSON을
# ~/.token-orbit/claude-code.json 에 원자적으로 기록하고,
# 사용자에게는 기존과 같은 상태줄 한 줄을 계속 보여준다.
#
# 설치(수동, M0 설치기 전까지) — ~/.claude/settings.json:
#   "statusLine": { "type": "command", "refreshInterval": 10,
#     "command": "powershell -NoProfile -ExecutionPolicy Bypass -File C:/path/to/statusline-tap.ps1" }
#
# ⚠️ 경로는 반드시 포워드 슬래시. Claude Code(Windows)는 statusline 명령을
# Git Bash로 실행하므로, 백슬래시 경로는 이스케이프로 먹혀 조용히 실패한다 (실측 재현됨).
#
# 주의: 이 스크립트는 매 statusline 갱신마다 실행된다. 가볍게 유지할 것.

$ErrorActionPreference = 'SilentlyContinue'

# stdin을 **바이트 그대로** 읽는다. [Console]::In.ReadToEnd()는 PowerShell 5.1에서
# 콘솔 코드페이지로 디코드해 UTF-8 한글(예: 세션 이름)을 파손시키고, 그 과정에서
# JSON 구조 자체가 깨질 수 있다 (실측: 따옴표 소실로 파싱 불가 파일 생성).
$stdin = [Console]::OpenStandardInput()
$ms = New-Object IO.MemoryStream
$stdin.CopyTo($ms)
$bytes = $ms.ToArray()
$json = [Text.Encoding]::UTF8.GetString($bytes)

$dir = Join-Path $env:USERPROFILE '.token-orbit'
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }

# 원자적 쓰기: tmp에 쓴 뒤 rename. 받은 바이트를 재인코딩 없이 그대로 기록한다.
$tmp = Join-Path $dir 'claude-code.json.tmp'
$dst = Join-Path $dir 'claude-code.json'
[IO.File]::WriteAllBytes($tmp, $bytes)
Move-Item -Force $tmp $dst

# 사용자 상태줄 출력 — 기존 statusline을 쓰고 있었다면 설치기가 이 부분을 래핑한다 (M0 TODO).
try {
    $j = $json | ConvertFrom-Json
    $model = $j.model.display_name
    $pct = [int]($j.context_window.used_percentage)
    Write-Output "[$model] ctx $pct%"
} catch {
    Write-Output "Token Orbit"
}
