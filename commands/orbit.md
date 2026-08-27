---
description: Token Orbit HUD 제어 — /orbit [toggle|show|hide|quit|start|refresh]
allowed-tools: Bash(bash *), Bash(sh *)
---

!`bash "${CLAUDE_PLUGIN_ROOT}/scripts/orbit-ctl.sh" $ARGUMENTS`

위 명령의 출력을 근거로 결과를 한 줄로만 보고해라. 다른 작업은 하지 마라.
HUD가 실행 중이 아니고 PATH에도 없다는 출력이면, 빌드 위치의
`target/release/token-orbit.exe`를 실행하라고 안내해라.
