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

$json = [Console]::In.ReadToEnd()

$dir = Join-Path $env:USERPROFILE '.token-orbit'
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }

# 원자적 쓰기: tmp에 쓴 뒤 rename. Token_Orbit이 절반 쓰인 파일을 읽는 일을 방지.
$tmp = Join-Path $dir 'claude-code.json.tmp'
$dst = Join-Path $dir 'claude-code.json'
[IO.File]::WriteAllText($tmp, $json, [Text.UTF8Encoding]::new($false))
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
