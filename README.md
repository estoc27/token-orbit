# Token Orbit

A local-only desktop HUD that shows your AI service usage — rate-limit windows, remaining quota, reset countdowns, and plan info — in one small always-on-top overlay.

한국어 안내는 [아래](#한국어-안내)에 있습니다. 설계 문서와 조사 기록은 [docs/DESIGN.md](docs/DESIGN.md) (한국어).

```
┌──────────────────────────────────┐
│ Codex   Pro 20              방금 │
│ ███░░░░░░░░░░░░░░░░░  13%        │
│ 7d · 잔량 87% · 4일 후 리셋       │
├──────────────────────────────────┤
│ Claude  Max 20              방금 │
│ ███████████░░░░░░░░░  54%        │
│ 5h · 잔량 46% · 1시간 후 리셋     │
│ ██░░░░░░░░░░░░░░░░░░  11%   7d   │
│ ████░░░░░░░░░░░░░░░░  19%   7d Fable │
└──────────────────────────────────┘
```

## What it does

| Service | Data shown | Source |
|---|---|---|
| **Codex** (ChatGPT desktop / CLI) | weekly window %, reset time, plan, credit balance | local session files (`~/.codex/sessions`) |
| **Claude Code** | 5-hour & 7-day window %, reset times, plan | statusline extension point (official contract) |
| **Claude** top-model weekly window (Fable) | model-specific weekly %, reset | optional local observation proxy |

Everything runs locally. No telemetry, no external servers, no debug ports, no credential access. The proxy (optional) reads rate-limit **headers only** — request/response bodies are never stored.

## Status

**Early development (M0).** Windows only. Expect manual setup steps. macOS/Linux are on the [roadmap](docs/DESIGN.md#7-로드맵).

## Requirements

- Windows 10/11
- [Rust toolchain](https://rustup.rs/) + Visual Studio Build Tools (C++ workload)
- [Node.js](https://nodejs.org/) — only for the optional Claude proxy tap
- [Claude Code CLI](https://code.claude.com/) — only for Claude usage percentages
- Codex data appears automatically if the Codex desktop app or CLI is installed

## Quick start

```bash
git clone https://github.com/estoc27/token-orbit
cd token-orbit
cargo build --release -p token-orbit
./target/release/token-orbit.exe
```

> Use `--release` — debug builds open a console window alongside the HUD.

The HUD appears as a small always-on-top overlay:

- **Drag** anywhere to move · **Esc** to quit · **Ctrl+Shift+O** toggles click-through
- Tray icon: show/hide, click-through, quit
- Codex usage appears with zero configuration if Codex is installed

### Claude usage % (optional, recommended)

Claude Code hands its session data — including rate-limit windows — to a user-configured statusline script. Token Orbit taps that:

1. Copy `scripts/statusline-tap.ps1` to `%USERPROFILE%\.claude\token-orbit-tap.ps1`
2. Add to `%USERPROFILE%\.claude\settings.json` (**forward slashes required** — the command runs under Git Bash):

```json
{
  "statusLine": {
    "type": "command",
    "command": "powershell -NoProfile -ExecutionPolicy Bypass -File C:/Users/YOU/.claude/token-orbit-tap.ps1",
    "refreshInterval": 10
  }
}
```

3. Open a new Claude Code **terminal** session. The HUD picks up `five_hour` / `seven_day` within seconds.

> Note: the Claude **desktop app** does not execute statusline scripts (measured) — a terminal CLI session must be open for this data to flow.

### Top-model weekly window & real-time account state (optional)

The statusline reflects one session's last API response, so it can lag. The proxy tap reads the account-wide rate-limit headers from live traffic instead — including the top-model weekly bucket (`7d Fable`) that the statusline does not expose:

- The HUD auto-starts the proxy (`scripts/proxy-tap.js`, port 8377) if Node is available
- Route a Claude Code session through it with `scripts/orbit-claude.cmd` (session-scoped, fail-open: if the proxy is down, Claude runs normally without it)

Captured values persist per bucket with their own timestamps, so the Fable window stays visible even when you are not using that model. Stale values show their age instead of pretending to be current.

## Design principles

- **Honesty over completeness** — unknown values are never fabricated; stale data shows its age; estimated values are visually distinct from exact ones
- **Official contracts first** — extension points, then local files; never debug ports, UI scraping, or borrowed credentials
- **Fail-open** — a dead collector degrades one card; a dead proxy never blocks your AI tools

The full design document — including the measurement pitfalls this project ran into (mtime vs. data freshness, idle-session cache re-broadcast, cmd.exe codepage traps) — is in [docs/DESIGN.md](docs/DESIGN.md) (Korean).

## License

[MIT](LICENSE)

---

## 한국어 안내

Token Orbit은 AI 서비스 사용량(사용률 %, 잔량, 리셋 시각, 요금제)을 화면 구석의 작은 오버레이로 항상 보여주는 **로컬 전용** 데스크톱 앱입니다.

- **Codex** — 설치돼 있으면 설정 0으로 주간 사용률·리셋·플랜·크레딧이 표시됩니다
- **Claude Code** — statusline 연동(위 Quick start 참조) 시 5시간/주간 창이 표시됩니다
- **Fable 주간 창** — 선택적 관측 프록시(`orbit-claude.cmd` 경유) 사용 시 표시됩니다

모든 처리는 로컬에서 이뤄지며 외부 전송이 없습니다. 디버그 포트·화면 스크래핑·자격증명 접근을 원칙적으로 배제하고, 프록시는 rate limit **헤더만** 읽습니다 (본문 저장 없음).

**조작**: 드래그 이동 · Esc 종료 · Ctrl+Shift+O 클릭 투과 · 트레이 메뉴

현재 **초기 개발 단계(M0), Windows 전용**입니다. 설계 결정과 실측 기록 전체는 [docs/DESIGN.md](docs/DESIGN.md)에 있습니다.
