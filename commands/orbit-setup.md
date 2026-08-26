---
description: Claude 사용률 statusline tap 설치/점검 — Token Orbit이 5h/7d 창을 읽게 연결
allowed-tools: Read, Edit, Write, Bash
---

Token Orbit의 Claude 사용률 수집(statusline tap)을 설치하거나 점검하라. 순서:

1. **tap 스크립트 배치**: `${CLAUDE_PLUGIN_ROOT}/scripts/statusline-tap.ps1`(Windows)
   또는 `${CLAUDE_PLUGIN_ROOT}/scripts/statusline-tap.sh`(macOS/Linux)를
   `~/.claude/token-orbit-tap.ps1`(또는 `.sh`)로 복사하라. 이미 있으면 덮어써서 최신화하라.

2. **settings.json 연결**: `~/.claude/settings.json`을 읽어라.
   - `statusLine` 키가 **없으면** 다음을 추가하라 (다른 키는 절대 건드리지 말 것):
     ```json
     "statusLine": {
       "type": "command",
       "command": "powershell -NoProfile -ExecutionPolicy Bypass -File <홈경로>/.claude/token-orbit-tap.ps1",
       "refreshInterval": 10
     }
     ```
     (macOS/Linux는 `bash <홈경로>/.claude/token-orbit-tap.sh`)
   - ⚠️ **경로는 반드시 포워드 슬래시**로 쓸 것. Claude Code는 이 명령을 Git Bash로
     실행하므로 백슬래시는 이스케이프로 소실되어 조용히 실패한다.
   - `statusLine`이 **이미 있고 token-orbit-tap이 아니면**, 기존 상태줄을 빼앗지 마라.
     사용자에게 기존 설정을 보여주고, 기존 명령의 출력을 보존하면서 tap을 앞단에 끼우는
     래퍼 스크립트를 만들지 물어봐라. 동의 없이는 수정하지 마라.

3. **검증**: 다음 한 줄로 tap이 정상 동작하는지 확인하라 (한글 포함 — 인코딩 검증 겸용):
   ```
   printf '%s' '{"model":{"display_name":"검증"},"context_window":{"used_percentage":1},"rate_limits":{"five_hour":{"used_percentage":1,"resets_at":1}}}' | powershell -NoProfile -ExecutionPolicy Bypass -File ~/.claude/token-orbit-tap.ps1
   ```
   출력이 `[검증] ctx 1%`이고 `~/.token-orbit/claude-code.json`이 유효한 JSON이면 성공.

4. **안내**: 설치가 끝나면 사용자에게 알려라 — 설정은 **새로 여는 터미널 Claude Code
   세션부터** 적용되며(현재 세션·데스크톱 앱은 statusline을 실행하지 않음), 그 세션이
   떠 있는 동안 Token Orbit HUD에 5시간/주간 사용률이 표시된다.

절대 하지 말 것: settings.json의 statusLine 외 키 수정, 백슬래시 경로,
검증 없이 완료 보고.
