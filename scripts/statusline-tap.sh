#!/bin/bash
# Token_Orbit statusline tap (macOS / Linux / Git Bash).
# 동작은 statusline-tap.ps1과 동일 — 그쪽 주석 참조.

input=$(cat)

dir="$HOME/.token-orbit"
mkdir -p "$dir"

# 원자적 쓰기
printf '%s' "$input" > "$dir/claude-code.json.tmp"
mv -f "$dir/claude-code.json.tmp" "$dir/claude-code.json"

# 사용자 상태줄 출력 (jq 없으면 고정 문자열)
if command -v jq >/dev/null 2>&1; then
  model=$(printf '%s' "$input" | jq -r '.model.display_name // "?"')
  pct=$(printf '%s' "$input" | jq -r '.context_window.used_percentage // 0 | floor')
  echo "[$model] ctx ${pct}%"
else
  echo "Token Orbit"
fi
