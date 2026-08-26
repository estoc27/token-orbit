# Token_Orbit

여러 AI 서비스의 사용량 · 잔량 · 요금제를 하나의 오버레이 HUD로 보여주는 **로컬 전용 데스크톱 앱**.

> Token_Orbit의 설계 문서이자 조사 기록. 최초 기획 초안(READ.ME, 현재 삭제됨)의 수집 계층
> 전제 중 상당수가 실제 API 현황과 달라 아키텍처와 로드맵을 재설계했으며, 변경 근거는
> [부록 A](#부록-a-초안-대비-변경-근거)에 남아 있습니다. 이 문서의 실측 기록은 전부
> 2026-08 시점 관찰입니다 — 각 서비스의 파일 포맷·헤더는 예고 없이 바뀔 수 있습니다.

---

## 0. 용어 정리

초안은 "플러그인"이라 불렀지만, 실제 형태는 **단독 실행 데스크톱 앱**이며
그 내부의 **Collector가 플러그인 구조**입니다. 혼동을 피하기 위해 아래처럼 씁니다.

- **Token_Orbit** — 단독 실행 앱 (포터블 바이너리)
- **Collector** — 서비스별 데이터 수집 플러그인
- **Snapshot** — Collector가 내놓는 정규화된 사용량 레코드 1건

---

## 1. 목표와 비목표

### 목표
- 내가 지금 어느 서비스를 얼마나 썼고, 한도까지 얼마나 남았는지 **항상 시야에 둔다**
- 한도 초과 / 토큰 고갈을 **사전에** 감지한다
- 모든 처리는 로컬에서. 외부 전송 없음.

### 비목표 (명시적으로 하지 않는 것)
- **타 앱 화면 OCR / 픽셀 스크래핑** — 정확도·유지보수 비용이 감당되지 않습니다. 제외합니다.
- **타 앱의 비공개 내부 DB 역공학** — 스키마가 비공개이고 무단 변경 위험이 있어 제외.
- **디버그 포트를 요구하는 모든 기능** — **원칙으로 고정합니다.** 실사용에서 마찰이
  크고(앱을 매번 특수 플래그로 기동), 인증 없는 포트를 여는 보안 비용이 따릅니다.
  CDP 기반 수집(§T3)과 인앱 위젯(§5.2)이 여기 해당하며, 분석은 보존하되 구현하지 않습니다.
- **호스트 앱 UI 조작** — 사용자가 작업 중인 앱을 Token_Orbit이 가로채 설정 화면으로
  이동시키거나 UI를 바꾸는 일은 하지 않습니다.
- **팀/기업 다중 사용자 대시보드** — 별도 제품 영역.
- **비용 최적화 자동 조치** — 표시까지만. 모델 자동 다운그레이드 같은 개입은 하지 않음.

---

## 2. 데이터 수집 계층 (핵심 재설계)

초안의 "API 기반 / 데스크톱 기반" 2분류는 실제 난이도와 신뢰도를 반영하지 못합니다.
**신뢰도 등급(Tier)** 으로 재분류합니다.

| Tier | 방식 | 정확도 | 지연 | 설정 난이도 | 도입 |
|---|---|---|---|---|---|
| **T0** | **공식 확장 지점** (statusline / OTEL) | Exact | 실시간 | 하 (설정 1줄) | **M0** ⭐ |
| **T1** | 로컬 프록시 가로채기 | Exact | 실시간 | 중 (base_url 교체) | M2 |
| **T2** | 로컬 아티팩트 파싱 | Exact | 실시간 | 없음 (자동 탐지) | **M0** |
| **T3** | 런타임 인트로스펙션 (Electron/CDP) | Exact | 실시간 | **상 — 디버그 포트** ⚠️ | **미지원** |
| **T4** | 공식 Admin / Billing API | Exact | 1시간~1일 | 상 (Admin 키) | M1 |
| **T5** | 사용자 선언 (폴백) | Declared | 입력 시점 | 하 | 선택 |
| **T6** | UI 스크래핑 / OCR | Estimated | — | — | **제외** |

**T0 + T2가 주 경로입니다.** Codex는 사용률·창 길이·리셋·플랜명이 전부 로컬 세션
파일에 매 턴 기록되고(T2), Claude Code는 statusline 확장 지점이 같은 정보를 공식
계약으로 건네줍니다(T0). 어느 쪽도 포트·API 키·수동 입력이 필요 없습니다.
T1/T4는 닿지 않는 영역(API 키 사용분, 조직 비용 집계)을 보완합니다.

### 서비스별 제공 항목 매트릭스 (2026-08-26 실측)

**핵심: 수집 *방법*만 다른 게 아니라 수집 *가능한 항목*이 다릅니다.**

| 도구 | 소스 | 토큰 | 사용률 % | 리셋 | 플랜명 | Tier |
|---|---|---|---|---|---|---|
| **Codex Desktop** | `~/.codex/sessions/**/*.jsonl` | ✅ | ✅ | ✅ | ✅ | T2 |
| **Claude Code** | statusline JSON + `~/.claude/projects/**/*.jsonl` | ✅ | ✅ | ✅ | ⚠️¹ | **T0**+T2 |
| **Claude Desktop** | — | ❌ | ❌ | ❌ | ❌ | 없음 |
| API 키 사용분 | 프록시 / Admin API | ✅ | ✅¹ | ✅¹ | — | T1 / T4 |

¹ 응답 헤더의 rate limit 값 기준. Claude Code의 플랜명은 statusline JSON에 없어 `~/.claude.json`의 `oauthAccount` 티어로 보완

- **Codex Desktop**: 세션 파일의 `originator`가 `"Codex Desktop"` — CLI가 아니라
  **데스크톱 앱 자신이 기록**합니다. 앱이 렌더러에 들고 있는 값과 동일한 데이터가 디스크에 있습니다.
- **Claude Desktop**: Electron 저장소(IndexedDB/LevelDB)를 강제 텍스트 스캔했으나
  사용량 관련 구조화 데이터가 없습니다. 검색된 `weekly`는 캐시된 도움말 문서 텍스트였습니다.
- **Gemini CLI**: 이 PC에 설치돼 있으나 세션 데이터가 없어 미조사. 실사용 환경에서 재확인 필요.
- **Microsoft Copilot (스토어 앱)**: 조사 완료 (2026-08-26) — `LocalState`가 5KB(bin 2개)뿐인
  웹뷰 래퍼. 로컬 사용량 데이터 없음, CLI·확장 지점 없음 → 현재 지원 불가로 기록 (재조사 불필요).
  GitHub Copilot(별개 제품)은 이 PC에 미설치라 미검증 — VS Code 확장 저장소 기반 T2 가능성 있음.

**→ 이 편차가 설계에 미치는 영향은 §4 `Capabilities`를 볼 것.**
공통 출력 계층은 "모든 서비스가 퍼센트를 준다"고 가정해서는 안 됩니다.

#### Claude Desktop — 검증된 막다른 길 (재조사 방지용 기록)

"앱이 화면에 사용량을 띄운다면 그 값은 프로세스 안에 있으므로 읽을 수 있어야 한다"는
추론은 **옳습니다.** 문제는 가능 여부가 아니라 **모든 경로가 실행 플래그나 인증서를
요구한다**는 점입니다. 실측 결과:

| 경로 | 결과 | 비용 |
|---|---|---|
| 로컬 파일 | ❌ 없음 | — (IndexedDB/LevelDB 강제 스캔까지 완료) |
| **UI Automation** | ❌ **실패** | 아래 참조 |
| CDP (디버그 포트) | 가능하나 미채택 | 실행 플래그 (§1 비목표) |
| 네트워크 가로채기 | 가능하나 미채택 | 로컬 CA 인증서 설치 — 포트보다 침습적 |

**UIA 실측 (2026-08-26):**
```
메인 창 FromHandle → descendant 14개 = 최소화/복구/닫기 등 창 프레임뿐
자식 창 열거      → "Intermediate D3D Window" 1개
                     (Chrome_RenderWidgetHostHWND 없음)
WM_GETOBJECT(OBJID_CLIENT) 자극 후 재조회 → descendant 0개
```
Chromium은 접근성 트리를 **지연 활성화**하며, 활성화하려면 실질적으로
`--force-renderer-accessibility` 실행 플래그가 필요합니다.
**디버그 포트와 같은 마찰 등급**이므로 동일한 원칙(§1 비목표)에 따라 채택하지 않습니다.

**대신 — Claude 구독 사용량은 Claude Desktop이 아니라 Claude Code 경유로 얻습니다.**
둘은 같은 구독의 같은 한도를 공유하므로, Claude Code의 statusline(T0)이 주는
`five_hour`/`seven_day` 수치가 곧 그 계정의 구독 사용률입니다. Desktop 앱을 뚫을
이유가 사라졌습니다.

### T0 — 공식 확장 지점 ⭐ 최우선

**대상 도구가 공식적으로 열어둔 확장 지점을 통해 데이터를 건네받습니다.**
파일을 뒤지거나(T2) 프로세스에 붙는(T3) 게 아니라, **문서화된 계약**으로 받습니다.
따라서 앱 업데이트로 조용히 깨지지 않고, 깨지면 릴리스 노트에 남습니다.

#### Claude Code — `statusLine` (검증 완료 — 단, 터미널 세션 한정)

> ⚠️ **실측 (2026-08-26): 데스크톱 앱 세션은 statusline을 실행하지 않습니다.**
> `CLAUDE_CODE_ENTRYPOINT=claude-desktop` 환경에서 statusline 설정 후 새 세션을 열고
> 대화해도 tap 호출 로그가 0건. statusline은 터미널 UI 기능이며, OTEL 메트릭에도
> rate limit 데이터가 없음을 문서로 확인. **따라서 이 T0 경로는 터미널 CLI 세션이
> 하나라도 떠 있을 때만 데이터가 흐릅니다.** 데스크톱 전용 사용자는 Claude 카드에
> 토큰·비용만 표시됨 (Capabilities가 자동 처리).
>
> 함정 기록: statusline `command`의 Windows 경로는 **반드시 포워드 슬래시**.
> Claude Code는 명령을 Git Bash로 실행하므로 백슬래시는 이스케이프로 먹혀 조용히 실패
> (재현·수정 완료).

statusline 명령은 stdin으로 세션 JSON을 받습니다. 그 안에:

```jsonc
"rate_limits": {
  "five_hour": { "used_percentage": 94, "resets_at": 1787739000 },
  "seven_day": { "used_percentage": 20, "resets_at": 1787965200 }
},
"context_window": {
  "used_percentage": 25, "remaining_percentage": 75,
  "context_window_size": 1000000
},
"cost": { "total_cost_usd": 0.01234, "total_duration_ms": 45000 },
"model": { "id": "claude-opus-5", "display_name": "Opus" },
"session_id": "...", "transcript_path": "...", "version": "..."
```

**§5.0 요구사항 3종이 전부 충족됩니다** — 사용률 %, 리셋 시각, (창 2개 모두).
`~/.claude.json`에서 발견한 내부 명칭 `five_hour`/`seven_day`와 정확히 일치합니다.

> **최상위 모델(Fable) 창 — T1 프록시로 해결 (2026-08-26, 구현 완료).**
> statusline 페이로드에는 `five_hour`/`seven_day` 둘뿐이지만, **원본 API 응답 헤더**에
> 모델 전용 창까지 실려 온다:
>
> ```
> anthropic-ratelimit-unified-5h-utilization:    0.48   (앱 "5시간 한도")
> anthropic-ratelimit-unified-7d-utilization:    0.09   (앱 "주간 · 전체 모델")
> anthropic-ratelimit-unified-7d_oi-utilization: 0.17   (앱 "주간 · Fable")  ← 이것
> anthropic-ratelimit-unified-7d_oi-reset:       (epoch, 전체 모델과 1분 차이)
> anthropic-ratelimit-unified-fallback-percentage: 0.5  (플랜 고정값)
> anthropic-ratelimit-unified-overage-status: rejected / -disabled-reason: out_of_credits
> ```
>
> `7d_oi` = Fable 주간 창임을 값 일치로 확인 (헤더 0.16 ↔ 앱 "주간·Fable 16%").
> Fable 요청을 보낼 때만 이 헤더가 나타난다.
>
> **폐기된 해석**: `fallback-percentage`를 "주간 창 위의 임계선"으로 본 모델은 틀렸다.
> Fable은 자체 리셋 시각을 가진 **독립 창**이다. 0.5는 그 창 크기가 전체 주간의 절반이라는
> 플랜 고정값이며, 사용량에 따라 변하지 않는다 (UI엔 안내문으로만 표시).
>
> **프록시가 statusline보다 권위가 높은 이유**: 헤더는 **계정 실시간 상태**인 반면,
> statusline의 `rate_limits`는 **그 세션의 마지막 API 응답**이라 idle이면 얼어붙는다
> (실측: 30초 간격 재확인에도 값 완전 동일). 같은 창을 둘 다 주면 프록시를 택한다
> (§4 `Collector::authority`).
>
> 구현: `scripts/proxy-tap.js`(관측 프록시) → `~/.token-orbit/claude-headers.json`
> → `collectors/claude_proxy.rs`. statusline 탭과 동일한 파일 경유 패턴이라 새 의존성이 없다.
> 값 유효 조건: 트래픽이 프록시를 지나야 갱신 (Passive) — 파일은 남으므로 마지막 값은 계속 표시되고,
> 나이가 창 단위로 노출된다.

**설정 방법** — `~/.claude/settings.json`:
```json
{ "statusLine": { "type": "command", "command": "~/.claude/token-orbit-tap.sh" } }
```

스크립트는 stdin을 파일로 떨구고, **원래 보여주던 상태줄 내용을 그대로 출력**합니다:
```bash
#!/bin/bash
input=$(cat)
printf '%s' "$input" > ~/.token-orbit/claude-code.json   # Token_Orbit이 감시
echo "[$(echo "$input" | jq -r .model.display_name)] ..." # 사용자 상태줄은 유지
```

**중요 — 사용자의 상태줄을 빼앗지 않습니다.** statusline은 하나만 설정 가능하므로,
이미 쓰고 있다면 기존 출력을 보존한 채 tap만 추가해야 합니다. 설치 시 기존
`statusLine` 설정을 감지해 래핑하고, 제거 시 원복합니다.

**갱신 시점**: 이벤트 기반(턴 진행 시). `refreshInterval`(초, 최소 1)로 주기 갱신 추가 가능.
Claude Code가 실행 중일 때만 갱신되지만, 사용량은 사용할 때만 변하므로 실용상 충분합니다.

#### 대안 — OpenTelemetry
Claude Code는 OTLP 메트릭 내보내기를 지원합니다(`/docs/en/monitoring-usage`).
로컬 수집기로 받으면 되지만 **statusline보다 설정이 무겁고**, 위 필드가 전부
메트릭으로 나오는지 미확인이라 1순위로 두지 않습니다.

#### 다른 도구에도 같은 것이 있는가
Codex는 T2(파일)로 이미 완전히 해결되어 확장 지점을 찾을 필요가 없었습니다.
**신규 서비스를 붙일 때는 T0부터 확인할 것** — 공식 계약이 가장 안정적입니다.

### T1 — 로컬 프록시 (초안에 없던, 가장 강력한 수단)

사용자가 `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL`을 Token_Orbit의 로컬 프록시로 지정하면,
지나가는 요청·응답을 그대로 관측할 수 있습니다.

- 응답 body의 `usage` → 실제 소비 토큰 (Exact)
- 응답 **헤더** → 서버가 알려주는 잔량. 추정할 필요가 없음:
  - `anthropic-ratelimit-requests-remaining` / `-limit` / `-reset`
  - `anthropic-ratelimit-input-tokens-remaining` / `-output-tokens-remaining`
  - `retry-after`
  - OpenAI: `x-ratelimit-remaining-tokens`, `x-ratelimit-reset-tokens` 등

**제약**: 사용자가 환경변수를 바꿔야 함(=진입 장벽). 프록시가 죽으면 사용자의 AI 작업이 같이
죽으므로 **fail-open**(관측 실패해도 요청은 그대로 통과) 설계가 필수. 스트리밍(SSE) 응답을
버퍼링하지 말고 통과시키면서 관측만 할 것.

### T2 — 로컬 아티팩트 (주 경로)

CLI 기반 에이전트는 사용량을 본인 홈 디렉터리에 남깁니다. **정확하고, 실시간이고,
100% 본인 데이터이며, 설정이 전혀 필요 없습니다.**

> **2026-08-26 실측 결과.** 아래는 추정이 아니라 실제 파일을 확인한 내용입니다.
> 도구마다 제공 수준이 크게 다르므로 하나로 뭉뚱그리면 안 됩니다.

#### Codex — 필요한 모든 것이 들어 있음 ⭐

경로: `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`
(종료된 세션은 `~/.codex/archived_sessions/`로 이동)

```jsonc
"rate_limits": {
  "limit_id": "codex",
  "primary":   { "used_percent": 88.0, "window_minutes": 10080, "resets_at": 1788140374 },
  "secondary": null,
  "credits":   { "has_credits": false, "unlimited": false, "balance": "0" },
  "plan_type": "pro",
  "rate_limit_reached_type": null
}
```

| 필드 | 의미 | 초안이 원했던 항목 |
|---|---|---|
| `used_percent` | 사용률 (%) | "요금제 한도 대비 퍼센트" ✅ |
| `window_minutes` | 창 길이 (10080 = 7일) | 창 길이를 추측할 필요 없음 ✅ |
| `resets_at` | 리셋 시각 (unix epoch) | "리셋까지 남은 시간" ✅ |
| `plan_type` | `"pro"` 등 | **"요금제 자동 감지"** ✅ |
| `credits.balance` | 크레딧 잔액 | "남은 토큰/요청 수" ✅ |
| `rate_limit_reached_type` | 한도 도달 여부 | 알림 트리거 ✅ |

**갱신 빈도: 매 턴.** 실측한 활성 세션 파일에 `rate_limits` 항목이 1,625개 있었고,
파일 mtime은 수 분 전이었습니다. 즉 **파일 감시만으로 실시간 갱신이 됩니다.**

→ Codex는 포트도, API 키도, 앵커 입력도, 창 길이 추정도 필요 없습니다.
   **파일을 읽으면 끝납니다.**

#### Claude Code — 토큰만 있고 한도는 없음

경로: `~/.claude/projects/<slug>/<session-uuid>.jsonl`

```jsonc
"usage": {
  "input_tokens": 2,
  "cache_creation_input_tokens": 85,
  "cache_read_input_tokens": 126621,
  "output_tokens": 1074,
  "output_tokens_details": { "thinking_tokens": 700 }
}
```

`limit` / `percent` / `quota` / `reset` 계열 키를 전수 조사했으나 **하나도 없습니다.**

> **단, 데이터 자체는 존재합니다.** Claude 사용량 화면에서 실제로 확인된 형태
> (2026-08-26, 사용자 제보):
>
> ```
> 플랜: Pro
> ├ 현재 세션   84% 사용 / 2시간 33분 후 재설정      ← 단기 롤링 창
> ├ 주간 한도   19% 사용 / (화) 02:00 재설정 (모든 모델)  ← 장기 창
> └ 크레딧      US$11.81 사용 / Sep 1 재설정 / 무제한
> 마지막 업데이트: 1분 전
> ```
>
> **Codex와 동일한 2중 창 구조**이며, "마지막 업데이트 1분 전"은 클라이언트가 이 값을
> **주기적으로 서버에서 받아온다**는 뜻입니다. 즉 가로챌 요청이 존재합니다.
> 문제는 데이터의 부재가 아니라 **어느 클라이언트의 어느 요청인지**입니다 (§7 M2 실험).
>
> ⚠️ **한도는 고정값이 아닙니다.** 위 화면에는 "8월 31일까지 주간 한도 50% 상향"
> 프로모션이 적용 중이었습니다. 토큰 누적량에서 퍼센트를 역산하는 방식은
> 분모가 조용히 바뀌므로 신뢰할 수 없습니다 — **서버가 준 `%`를 그대로 쓸 것.**

**전수 조사 완료 (2026-08-26).** 아래를 모두 확인했으나 사용률 수치는 어디에도 없습니다:

| 위치 | 결과 |
|---|---|
| `~/.claude/projects/**/*.jsonl` | 토큰만 |
| `~/.claude/sessions/` | 세션 키 파일 (사용량 무관) |
| `~/.claude.json` (+ `backups/`) | 설정·플래그만 |
| `AppData/Roaming/Claude` (IndexedDB/LevelDB) | 없음 |

**부산물 — 내부 창 명칭 확인.** `~/.claude.json`의 기능 플래그에서:
```jsonc
"tengu_rate_limit_promo_notices": [
  { "bar": "seven_day", "text": "+50% weekly limits promo through Aug 31", ... }
]
"oauthAccount": { "organizationRateLimitTier": "default_claude_ai" }
```
창 식별자가 **`five_hour` / `seven_day`** 임을 알 수 있습니다. 파싱 구현 시 참고할 것.
(Claude Code UI 실측: `5시간 한도 94%` / `주간·전체 모델 20%` — Codex와 같은 2중 창)

**결론: 수치는 네트워크로만 도달하며 메모리에만 존재합니다.**
로컬 파일 경로는 완전히 막혔습니다. 현재 확보 가능한 것은 다음뿐입니다:

- 절대 토큰량 및 비용 (`Exact`) — 모델별 단가로 환산
- 한도는 로컬 파일에 없음 → **단, T0(statusline)로 사용률·리셋을 공식 계약으로 받음.**
  T2는 토큰·비용 담당, T0는 퍼센트·리셋 담당으로 역할 분담
- 예외: 한도 도달 시 `"error":"rate_limit"` + 429 `errorDetails`가 기록됨 →
  알림 트리거 및 한도 자가 보정(§T5)의 신호로 사용 가능

**억지로 퍼센트를 만들어내지 않습니다.** 모르는 것은 모른다고 표시하는 편이,
추정치를 정확한 값처럼 보여주는 것보다 낫습니다 (§3 규칙 1).

#### 구현 방식
전체 재파싱 대신 파일 감시(`notify` 크레이트) + append 오프셋 추적.
로테이션·중간 삽입에 대비해 (파일 ID, size, mtime) 기반 무결성 체크.
Codex는 `rate_limits`가 매 턴 반복 기록되므로 **마지막 항목만** 읽으면 됩니다.

> ⚠️ **함정 (실측, 2026-08-26): mtime ≠ 데이터 신선도.**
> 며칠 전에 연 세션이 열린 채 idle이면 rate_limits 없는 이벤트를 계속 append해서
> mtime은 최신인데 마지막 rate_limits는 며칠 묵은 값일 수 있다. 실제로 이 함정 때문에
> HUD가 지난 주간 창의 84%를 현재값(실제 11%)인 양 표시했다.
> **mtime 1등 파일 하나가 아니라 상위 N개에서 각각 마지막 rate_limits를 뽑고,
> 항목 자체의 `timestamp`로 최신을 골라야 한다.** 관측 시각도 그 타임스탬프를 쓴다.
>
> 참고: Codex **파일**의 `used_percent`는 사용량(쓸수록 증가, 실측)이고,
> Codex **앱 화면**은 잔량("N% 남음")으로 표기한다. 같은 상태의 반대 방향 표기이므로
> HUD는 혼동 방지를 위해 사용%(게이지)와 잔량%(메타 줄)를 모두 표시한다.

### T3 — 런타임 인트로스펙션 (Electron / CDP) 🅿️ **미지원**

> **상태: 채택하지 않음.** 분석만 보존합니다.
>
> **사유 1 — 애초에 불필요했습니다.** CDP로 렌더러 메모리에서 꺼내려 했던 바로 그 값이
> **로컬 세션 파일에 그대로 기록되고 있었습니다**(§T2 실측). 같은 데이터를 훨씬 싼 방법으로
> 얻을 수 있으므로, 포트를 여는 것은 정당화되지 않습니다.
>
> **사유 2 — 디버그 포트 요구가 실사용에서 감당되지 않습니다.** 프로토타입을 직접 운용해 본
> 결과, 문제는 "포트가 열려 있는 동안의 위험"만이 아니라 **앱을 매번 그 플래그로 띄워야
> 한다는 절차 자체**였습니다. 기본값을 어디 두든 이 마찰은 사라지지 않습니다.
>
> **교훈** — 프로토타입은 렌더러 메모리라는 *가장 어려운* 경로를 먼저 찾아냈습니다.
> 새 데이터 소스를 검토할 때는 **로컬 파일부터 전수 조사**하고, 없다고 확인된 뒤에
> 프로세스 내부를 보는 순서가 맞습니다.
>
> 아래는 재검토 시를 위한 기록입니다.

Electron 기반 데스크톱 앱(Codex, ChatGPT Desktop 등)의 렌더러는 Chromium입니다.
앱을 `--remote-debugging-port`로 기동하면 CDP(Chrome DevTools Protocol)로 붙어,
**앱이 서버에서 이미 받아 들고 있는 사용량 객체를 그대로 읽을 수 있습니다.**
픽셀 추정(T6)이 아니라 구조화된 JSON이므로 정확도는 Exact입니다.

> ~~**전략적 가치: 개인 구독 한도를 얻는 유일한 수단.**~~
> **이 주장은 틀렸습니다.** 로컬 세션 파일에 같은 데이터가 있었습니다(§T2).
> 아래 스키마는 렌더러 메모리에서 관측한 것으로, 파일에 기록되는 형태와 필드명이
> 다릅니다 (`primary_window` → `primary`, `reset_at` → `resets_at`).
> **구현 시 참조해야 할 것은 §T2의 파일 스키마입니다.**

관측된 스키마 (Codex 앱 렌더러, 2026-08):

```jsonc
{
  "rate_limit": {
    "primary_window":   { "used_percent": 42.5, "reset_at": "..." },
    "secondary_window": { "used_percent": 11.0, "reset_at": "..." }
  },
  "credits": { "balance": 1234.56 }
}
```

→ **롤링 창이 2개 동시 운용됩니다** (단기 + 장기로 추정). 하나만 고르지 말고
`primary` / `secondary` **둘 다 별개 Snapshot으로** 내보낼 것. HUD도 둘 다 표시해야
"5시간 창은 여유로운데 주간 창이 90%"인 상황을 잡아낼 수 있습니다.

#### 읽는 방법 — 두 가지 중 후자를 권장

| | React fiber 순회 | `Network` 도메인 구독 |
|---|---|---|
| 방식 | `memoizedProps` 트리 DFS 탐색 | `Network.enable` + `responseReceived` → `getResponseBody` |
| 깨지는 조건 | 컴포넌트 리팩터링, prop 이름 변경, React 내부 구조 변경, 번들러 변경 | 서버 API 응답 스키마 변경 |
| 비용 | 전체 fiber 트리 순회 (매 갱신) | 이벤트 수신 시에만 |
| 안정성 | **낮음** | **높음** |

React 내부(`_internalRoot.current`, `memoizedProps`)는 비공개 구현이고 앱 업데이트마다
깨집니다. 반면 서버 API 응답 스키마는 훨씬 느리게 변합니다. **같은 CDP 연결로 접근
가능하면서 훨씬 안정적이므로, `Network` 도메인 구독을 기본 경로로 삼습니다.**

#### ⚠️ 보안 — 도입 시 반드시 고지할 것

`--remote-debugging-port`는 **인증이 전혀 없습니다.** 포트가 열려 있는 동안
로컬의 **어떤 프로세스든** 렌더러에 붙어 세션 토큰·대화 내용을 읽고 임의 JS를 실행할 수
있습니다. 웹페이지가 DNS rebinding으로 접근하는 공격 사례도 알려져 있습니다.

따라서 이 Collector는:
- **기본 비활성.** 명시적 opt-in + 위험 고지 화면을 거친 뒤에만 활성화
- "항상 켜두세요"라고 **권하지 않는다.** 필요할 때만 켜는 사용법을 안내
- 리스닝 주소가 `127.0.0.1`인지 확인하고, 외부 인터페이스 바인딩은 차단
- Token_Orbit이 사용자 앱의 실행 옵션을 **임의로 바꾸지 않는다** (사용자가 직접 설정)

이 Tier만 유일하게 "정확도는 높지만 도입을 권장하지 않을 수도 있는" 항목입니다.
편의와 보안이 정면으로 충돌하므로, 판단을 사용자에게 넘기고 근거를 충분히 제공합니다.

#### 읽기와 그리기는 분리한다
CDP로는 대상 앱 DOM에 위젯을 주입하는 것도 가능합니다(§5.2 `InAppWidget`). 다만 그것은
**표시 계층의 선택지이지 이 Collector의 일부가 아닙니다.** T3는 읽기만 담당하며,
인앱 표시를 끄더라도 T3는 그대로 동작합니다. 반대도 마찬가지입니다.

읽기는 주기적으로 잠깐 붙었다 떼면 되므로, **T3 단독 사용이 포트 노출 시간 면에서
훨씬 안전합니다.**

### T4 — 공식 Admin / Billing API

> ⚠️ **API 현황은 빠르게 바뀝니다. 구현 직전에 공식 문서로 재확인하세요.**
> 아래는 문서 작성 시점(2026-08) 기준 정리입니다.

| 서비스 | 엔드포인트 | 인증 | 집계 단위 |
|---|---|---|---|
| Anthropic | `/v1/organizations/usage_report/messages`, `/v1/organizations/cost_report` | `sk-ant-admin...` (Console 조직 소유자) | 시간 / 일 |
| OpenAI | `/v1/organization/usage/completions`, `/v1/organization/costs` | Admin 키 (조직 소유자) | 시간 / 일 |
| GitHub Copilot | `/orgs/{org}/copilot/metrics` | org admin PAT | 일 |

**세 가지 중요한 제약:**

1. **Admin 키는 권한이 매우 큽니다.** API 키 발급/삭제, 조직 멤버 관리까지 가능한 경우가
   있습니다. 사용자에게 명확히 경고하고, 가능한 최소 권한 키를 안내할 것.
2. **집계가 시간/일 단위**입니다. 60초 폴링해도 새 데이터가 없습니다. 폴링 주기는
   Collector가 스스로 정하게 하고(§4 `Cadence`), 전역 "10초 최소 주기"는 폐기합니다.
3. **개인 구독(Claude Pro/Max, ChatGPT Plus)의 사용량 한도는 어떤 API로도 조회 불가합니다.**
   다만 **Codex는 이 공백이 로컬 파일로 메워집니다**(§T2). Claude Code는 메워지지 않아
   절대값·비용만 표시합니다 — 라고 판단했으나, **T0(statusline)로 해결되었습니다.**

### T5 — 사용자 선언 (폴백)

자동 감지가 불가능한 값만 사용자에게 받습니다. **T2 실측 이후 이 Tier의 역할은 크게
줄었습니다** — Codex는 로컬 파일에 전부 있고, Claude Code는 애초에 퍼센트 개념을
로컬에서 복원할 수 없기 때문입니다.

남는 용도:

| 항목 | 이유 |
|---|---|
| 월 예산 상한 (USD) | 사용자 개인 기준이므로 어떤 API에도 없음 |
| Claude Code 한도 (선택) | 알고 있다면 입력. 모르면 `Unknown`으로 두고 절대값만 표시 |
| 미지원 서비스의 플랜명 | Collector가 감지 못 한 경우의 라벨 |

**입력을 요구하지 않는 것을 기본으로 합니다.** 아무것도 입력하지 않아도 T2만으로
Codex는 완전히, Claude Code는 절대값 기준으로 동작해야 합니다.

#### 한도 자가 보정 (Claude Code용, 선택)

Claude Code는 한도에 도달하면 JSONL에 `"error":"rate_limit"`과 429 응답이 기록됩니다.
그 시점의 창 내 누적 토큰을 한도의 하한으로 삼아 추정치를 학습할 수 있습니다.
`Confidence::Derived`로 표시하며, 사용자 입력값이 있으면 그쪽이 우선입니다.

한계: 한도에 실제로 부딪혀야 학습되므로 초기에는 `Unknown`입니다.
공급자가 한도를 조정하면 학습값이 낡으므로 만료 정책이 필요합니다.

> **폐기된 설계 — 앵커 + 증분**
>
> T2 실측 전에는 "사용자가 설정 화면의 퍼센트를 입력(앵커)하고, 이후 로컬 토큰을
> 더해 실시간 값을 만든다"는 방식을 검토했습니다. **불필요해져 폐기합니다.**
>
> 폐기 사유:
> 1. Codex는 `used_percent`가 로컬 파일에 매 턴 기록되므로 앵커가 필요 없음
> 2. Claude Code는 한도 자체를 모르므로 "퍼센트 + 토큰" 단위 불일치를 해소할 수 없음
>    (§3 규칙 2 위반). 두 번 앵커링해 눈금을 역산하는 보정이 필요했는데,
>    사용자에게 퍼센트를 반복 입력시키는 UX는 모니터링 도구의 목적에 반함
> 3. 무엇보다 **사람이 값을 옮겨 적어야 하는 순간 그것은 모니터가 아님**
>
> 기록으로만 남깁니다.


## 3. 데이터 모델

서비스마다 자원 단위가 달라(토큰 / 요청 / "5시간 창당 메시지" / USD), 단일 스키마 없이는
집계가 성립하지 않습니다.

```rust
pub struct Snapshot {
    pub source_id:    SourceId,        // "claude-code-local", "anthropic-admin", ...
    pub account:      AccountLabel,    // 다계정 구분
    pub metric:       Metric,
    pub window:       Window,
    pub used:         f64,
    pub limit:        Limit,
    pub resets_at:    Option<DateTime<Utc>>,
    pub confidence:   Confidence,      // ★ 필수
    pub observed_at:  DateTime<Utc>,
}

pub enum Metric {
    Tokens { input: u64, output: u64, cache_read: u64, cache_write: u64 },
    Requests(u64),
    Messages(u64),
    Cost(Usd),
}

pub enum Window {
    Rolling(Duration),          // 예: 5시간 롤링 창
    Calendar(CalendarPeriod),   // 월/일 경계
    Session { started_at: DateTime<Utc> },
    Lifetime,
}

pub enum Limit {
    Known(f64),          // API가 알려준 값
    Declared(f64),       // 사용자가 입력한 값
    Unknown,             // 퍼센트 표시 불가 — 절대값만 표시
}

pub enum Confidence { Exact, Derived, Declared, Estimated }
```

### 세 가지 설계 규칙 (초안에 없던 것)

**규칙 1 — `confidence`를 UI에 반드시 반영한다.**
정확한 값과 추정치를 같은 프로그레스 바에 섞어 보여주면 없느니만 못합니다.
`Exact`는 실선, `Declared`/`Estimated`는 점선·흐린 색 등으로 시각적으로 구분합니다.

**규칙 2 — 단위가 다른 값은 합산하지 않는다.**
초안의 "여러 서비스를 합산한 총 사용량"은 토큰·요청·메시지를 더하겠다는 뜻인데, 의미가 없습니다.
**교차 서비스 합산이 성립하는 유일한 축은 `Cost(Usd)`** 입니다.
통합 요약 뷰는 (a) USD 총합, (b) 서비스별 개별 퍼센트 나열 — 두 가지만 제공합니다.

**규칙 3 — 한 소스가 여러 창(window)을 가지면 전부 내보낸다.**
Codex의 `primary_window` / `secondary_window`처럼 롤링 창이 동시에 여러 개 도는 서비스가
있습니다. 하나만 골라 표시하면 "단기 창은 여유로운데 장기 창이 한계"인 상황을 놓칩니다.
Collector는 창마다 별개 `Snapshot`을 내고, HUD는 **가장 임박한 창을 강조**하되 나머지도 접근 가능하게.

### Staleness

모든 값에 관측 시각을 붙이고, Collector별 허용 신선도를 넘으면 UI에서 흐리게 처리합니다.
"5분 전 값을 현재값처럼 보여주는 것"이 이 앱에서 가장 흔한 실패 모드입니다.

**특히 금지: 읽기 실패 시 마지막 성공값을 조용히 재사용하는 것.**
읽기가 실패했으면 `Health`를 `Degraded`로 내리고 값에 나이를 표기해야 합니다.
캐시된 값을 현재값인 척 반환하면 "한도 여유 있음"을 보여주다가 실제로는 초과되는,
이 앱에서 가장 나쁜 실패가 발생합니다.

---

## 4. Collector 인터페이스

```rust
#[async_trait]
pub trait Collector: Send + Sync {
    fn id(&self) -> SourceId;
    fn cadence(&self) -> Cadence;
    async fn collect(&self, ctx: &CollectCtx) -> Result<Vec<Snapshot>, CollectError>;
    fn health(&self) -> Health;   // Ok | Degraded(reason) | Failed(reason) | NotConfigured
}

pub enum Cadence {
    Poll(Duration),       // T4: Collector가 자기 주기를 결정
    Watch(Vec<PathBuf>),  // T2: 파일 변경 이벤트 기반
    Passive,              // T1: 트래픽이 흐를 때만
}
```

### `Capabilities` — 서비스마다 줄 수 있는 것이 다르다

수집 *방법*의 차이는 Collector가 흡수합니다. 그런데 §2 매트릭스가 보여주듯
**수집 가능한 항목 자체가 다릅니다.** Codex는 퍼센트를 주고 Claude Code는 못 줍니다.
이 차이는 Collector 안에서 숨길 수 없고, 표시 계층까지 전달되어야 합니다.

```rust
pub struct Capabilities {
    pub tokens:     bool,   // 절대 토큰량
    pub percent:    bool,   // 한도 대비 사용률
    pub reset_time: bool,   // 리셋 시각
    pub plan_name:  bool,   // 요금제명
    pub cost:       bool,   // USD 환산
}

impl Collector {
    fn capabilities(&self) -> Capabilities;
}
```

**Renderer는 `Capabilities`에 따라 표현을 낮춥니다** — 없는 값을 만들어내지 않고,
있는 것만 그립니다.

| | Codex | Claude Code (T0 활성) | Claude Code (T0 미설치) |
|---|---|---|---|
| 표시 형태 | 퍼센트 바 + 리셋 + 플랜 배지 | 퍼센트 바 ×2 + 리셋 | 토큰량 + 비용 |
| 퍼센트 바 | 그림 | 그림 (`five_hour`/`seven_day`) | **그리지 않음** (`Limit::Unknown`) |
| 임계값 경고 | % 기준 | % 기준 | 429 관측 시 / 예산 기준 |

같은 서비스라도 **사용자가 어떤 수집 경로를 켰느냐에 따라 Capabilities가 달라집니다.**
statusline tap을 설치하지 않은 Claude Code는 세 번째 열로 동작합니다 — 강요하지 않고,
카드에 "statusline 연동 시 사용률 표시 가능"을 안내만 합니다.

즉 **출력 계층은 공통이되, 카드의 완성도는 서비스·설정마다 다릅니다.**
모든 서비스를 같은 모양으로 강제하면 없는 값을 추정해 채워야 하고,
그건 §3 규칙 1(정확값과 추정값을 섞지 않는다)에 정면으로 위배됩니다.

**핵심 원칙: 하나의 Collector 실패가 HUD 전체를 죽이면 안 됩니다.**
각 Collector는 격리 실행하고, 실패는 `Health`로 표면화해 해당 타일만 경고 상태로 만듭니다.
초안 4.1이 지적한 "앱 업데이트로 파싱이 깨지는 문제"의 실질적 대응책이 바로 이 격리입니다.

---

## 5. 표시 계층 (Renderer)

### 5.0 HUD 공통 프레임 — 제품 요구사항

**서비스마다 표시 형식은 달라도 큰 틀은 같습니다.** 사용자가 작은 창을 흘깃 보고
다음 세 가지를 즉시 파악할 수 있어야 합니다:

| # | 항목 | 예 |
|---|---|---|
| 1 | **내 요금제 한도 대비 몇 % 썼나** | `84%` |
| 2 | **얼마나 남았나** | `16% 남음` |
| 3 | **다음 리셋은 언제인가** | `2시간 33분 후` |

```
┌──────────────────────────────┐
│ Codex   Pro                  │
│ ████████████████░░░░  88%    │
│ 주간 · 4일 12시간 후 리셋      │
├──────────────────────────────┤
│ Claude  Pro                  │
│ ████████████████░░░░  84%    │
│ 세션 · 2시간 33분 후 리셋      │
│ 주간 19%                     │
└──────────────────────────────┘
```

**설계 규칙:**
- **창이 여러 개면 가장 임박한 것을 크게**, 나머지는 한 줄로 (Codex·Claude 모두 2중 창)
- 숫자보다 **게이지 바**가 먼저 읽혀야 함 — 흘깃 보는 용도
- 리셋은 절대 시각이 아니라 **남은 시간**으로 (`(화) 02:00` ❌ → `2시간 33분 후` ✅)
- 임계값 초과 시 색상 변화 (기본 80% 주의 / 90% 경고)

> **이 요구사항이 수집 계층의 합격선을 정합니다.**
> `%` 를 못 주는 소스는 이 프레임을 채우지 못합니다. 토큰량과 비용만으로는
> "내 요금제에서 얼마나 남았나"에 답할 수 없습니다 — 사용자가 한도를 모르기 때문입니다.
> §2 매트릭스에서 `사용률 %` 열이 결정적인 이유입니다.

---


**수집(Collector)과 표시(Renderer)는 완전히 직교합니다.** 값을 어디서 가져오는지와
어디에 그리는지는 서로를 몰라야 하며, 사용자가 각각 독립적으로 선택합니다.

```rust
pub trait Renderer: Send + Sync {
    fn id(&self) -> RendererId;
    fn render(&mut self, view: &AggregatedView) -> Result<(), RenderError>;
    fn health(&self) -> Health;
}
```

```
Collectors ──► Aggregator ──► Renderer
  T1 프록시                     ├─ OverlayHud    (Tauri 창)      [기본]
  T2 로컬파일                   ├─ TrayTooltip / stdout          [부수]
  T4 Admin API                  └─ InAppWidget   (CDP 주입)      [보류 — §5.2]
  T5 수동선언
```

**중요 — T3(CDP로 읽기)와 `InAppWidget`(CDP로 그리기)은 별개 기능입니다.**
같은 전송 수단(CDP)을 쓸 뿐, 하나를 켠다고 다른 하나가 켜지지 않습니다.
프로토타입은 이 둘이 결합되어 있었고, 분리한 덕분에 **`InAppWidget`만 보류하고
T3는 유지**하는 선택이 가능해졌습니다.

**모든 Renderer는 `AggregatedView` 하나만 먹습니다.** Renderer는 어떤 Collector가
값을 물어왔는지 알지 못하며, 알 필요도 없습니다.

---

### 5.1 `OverlayHud` — OS별 실현 가능성

초안은 3개 고정 모드를 동등하게 서술했지만, 난이도가 극단적으로 다릅니다.

| 모드 | Windows | macOS | Linux/X11 | Linux/Wayland |
|---|---|---|---|---|
| **전역 최상단** (기본) | ✅ | ✅ | ✅ | ⚠️ `wlr-layer-shell` 필요 |
| **고정 해제** (자유 배치) | ✅ | ✅ | ✅ | ✅ |
| **활성 창 상단 부착** | ✅ | ⚠️ 접근성 권한 | ✅ | ❌ **불가능** |

### 활성 창 부착의 실제 비용
- **Windows** — `SetWinEventHook`으로 `EVENT_SYSTEM_FOREGROUND`(창 전환) +
  `EVENT_OBJECT_LOCATIONCHANGE`(이동/리사이즈) 구독. 실용적.
- **macOS** — `AXObserver` 기반. **접근성 권한을 사용자가 직접 승인**해야 하고,
  앱 서명·공증이 얽힙니다. 별도 온보딩 플로우 필요.
- **Wayland** — 다른 창의 위치를 조회하는 API가 프로토콜에 **존재하지 않습니다.**
  보안 모델상 의도된 것이라 우회로도 없습니다.

**→ 결정: 기본 모드를 "전역 최상단"으로 변경합니다.**
초안은 활성 창 부착을 기본으로 뒀지만, 가장 비싸고 이식성이 낮은 모드입니다.
Windows/macOS 전용 옵션으로 격하합니다.

### 클릭 투과
Tauri v2 `set_ignore_cursor_events(true)`.
Windows 기저에서는 `WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOACTIVATE`.
**투과 상태에서는 드래그로 위치 조절이 불가능**하므로, 단축키로 투과를 잠시 끄는
"이동 모드"가 필요합니다. (초안은 투과와 드래그를 동시에 요구했으나 양립하지 않습니다.)

---

### 5.2 `InAppWidget` — 호스트 앱 내부 표시 🅿️ **보류**

> **상태: 구현하지 않음.** 설계와 위험 분석만 마쳐 보존합니다.
>
> **보류 사유** — 디버그 포트를 **상시** 열어두는 것이 전제인데(아래 참조), 아직 검증된
> 사용자 가치가 없는 단계에서 지불하기엔 대가가 큽니다.
>
> **재검토 조건** — `OverlayHud`가 안정화되고 실사용 데이터가 쌓인 뒤. 그때는
> "인앱 표시가 오버레이보다 실제로 나은가"를 추측이 아니라 경험으로 판단할 수 있습니다.
>
> 아래 내용은 그때 다시 읽기 위한 기록입니다. 특히 §"재렌더 전쟁을 피하는 방법"은
> 프로토타입에서 실제로 겪은 문제의 해법이므로 유실되면 곤란합니다.

CDP로 대상 앱 렌더러에 위젯을 그립니다. **`OverlayHud`보다 초기 구현은 쉽지만
유지보수 부채가 영구적**이라는 점에서 성격이 완전히 다릅니다.

| | `OverlayHud` | `InAppWidget` |
|---|---|---|
| 초기 구현 | 중~상 (OS별 창 제어) | 하~중 (DOM 주입) |
| 이후 유지보수 | **낮음** — 내가 통제하는 창 | **높고 영구적** — 남이 통제하는 DOM |
| 깨지는 계기 | OS 메이저 업데이트 (드묾) | **호스트 앱 업데이트마다** |
| 전제 조건 | 없음 | 디버그 포트 **상시** 개방 ⚠️ |

#### 어려운 지점은 "자리 마련"입니다

| 하위 문제 | 난이도 | 대응 |
|---|---|---|
| 앵커(삽입 위치) 탐색 | 중 | `data-testid`/`aria-label` 휴리스틱. **로케일 의존 금지** — 프로토타입의 `/남은 사용량/` 정규식은 한국어 UI에서만 동작 |
| **React 재렌더가 노드를 삭제** | **상** | ↓ 아래 우회책 |
| 호스트 레이아웃 침범 | 중 | Shadow DOM + `all: initial` (프로토타입이 이미 올바르게 처리) |
| 통합 데이터 주입 | 하 | `Runtime.addBinding` + `Page.addScriptToEvaluateOnNewDocument` |
| 앱 업데이트 추적 | **상 (영구)** | 근본 해결 불가. 실패 시 조용히 비활성화 |

#### 재렌더 전쟁을 피하는 방법 — 트리 밖에 그린다

프로토타입은 앵커의 부모에 `insertBefore`로 **React가 관리하는 서브트리 안에** 노드를
넣습니다. 그래서 재렌더될 때마다 노드가 사라지고, `MutationObserver`로 다시 넣는
무한 경쟁이 발생합니다. 위젯이 계속 보이는 건 이 경쟁에서 이기고 있어서일 뿐입니다.

**대안: `document.body` 직속에 `position: fixed`로 띄우고, 앵커의 `getBoundingClientRect()`
위치에 맞춰 좌표만 추적합니다.**

- React 재조정 대상 밖이므로 **노드가 절대 삭제되지 않음** → 재삽입 루프 불필요
- `MutationObserver(document.body, {subtree:true})` 제거 → 스트리밍 중 CPU 부담 해소
- 위치 추적은 앵커에 `ResizeObserver` + `resize`/`scroll` 리스너로 충분
- 앵커를 못 찾아도 화면 모서리에 폴백 배치 가능 (앵커 의존도 자체가 낮아짐)

트레이드오프: 앱 레이아웃과 함께 리플로우되지 않아 좁은 창에서 콘텐츠를 가릴 수 있습니다.
`ResizeObserver`로 추적하면 실사용에서는 대부분 해결됩니다.

#### 이 Renderer의 실제 비용 — 포트 상시 개방

읽기(T3)는 주기적으로 잠깐 붙었다 떼면 되지만, **인앱 위젯은 CDP 연결을 계속 유지해야
합니다.** 즉 §2 T3에서 경고한 "인증 없는 디버그 포트"를 **하루 종일 열어두는** 것이
전제가 됩니다. 잠깐 여는 것과 상시 여는 것은 위험도가 질적으로 다릅니다.

**→ 이 항목이 보류 결정의 직접적 근거입니다.**
T3 수집만 쓰면 포트는 값을 읽는 순간에만 잠깐 열려 있으면 됩니다.
반면 인앱 위젯은 연결을 유지해야 하므로 노출 시간이 사용 시간 전체로 늘어납니다.
같은 포트라도 **노출 시간이 위험의 크기를 결정**하며, 그 차이가 보류 사유입니다.

---

## 6. 기술 스택

| 구성 | 선택 | 비고 |
|---|---|---|
| 셸 / UI | **Tauri v2** | 투명·클릭투과·트레이 지원. v1 아님 |
| 코어 | **Rust 단일 언어** | 초안의 "Rust 또는 Python" 혼용은 배포를 망칩니다 — 포터블 단일 바이너리 목표와 Python 런타임 번들은 충돌 |
| 파일 감시 | `notify` | T2 |
| 프록시 | `hyper` + `rustls` | T1 |
| 키 저장 | `keyring` | OS 키체인 (Windows Credential Manager / Keychain / Secret Service) |
| 설정 | TOML | 사용자가 직접 편집 가능 |
| 히스토리 | SQLite (`rusqlite`) | M1부터 |

---

## 7. 로드맵

초안의 1단계(OpenAI + Anthropic API 우선)는 **Admin 키 장벽 + 시간 단위 집계** 때문에
"설정에 10분 걸렸는데 화면에 뜬 건 몇 시간 전 숫자"가 됩니다. 순서를 뒤집습니다.

**판단 기준: 각 마일스톤 종료 시점에 개발자 본인이 매일 켜두게 되는가?**

### M0 — 쓸모 있는 최소 버전 (Windows 전용, 1~2주)

**코드 구조** (뼈대 구현됨, 2026-08-26):
```
crates/orbit-core/   수집·집계 코어 (UI 무관, 순수 Rust — 단독 테스트 가능)
src-tauri/           Tauri v2 셸 (오버레이 창, 트레이, 이벤트 전달)
ui/                  HUD 프런트엔드 (정적 HTML/CSS/JS)
scripts/             statusline tap (ps1 / sh)
```

- [x] **뼈대**: 워크스페이스, 데이터 모델(`Snapshot`/`Capabilities`/`Health`),
      Collector 트레이트, Codex·statusline·Claude JSONL 3종 Collector, HUD 골격
- [x] **양 서비스 실데이터 검증** — Codex `rate_limits`, Claude statusline `five_hour`/`seven_day`
- [x] **카드 병합** (`service_key`) — 수집기 N개 → 서비스당 카드 1장
- [x] **`notify` 파일 감시** — 폴링 대체. 이벤트 즉시 반응 + 30초 heartbeat 안전망.
      실측상 한 변경에 이벤트가 수 ms 내 8건까지 몰려 300ms 디바운스 필수
- [x] **시스템 트레이** — HUD 표시/숨김, 클릭 투과, 종료
- [x] **전역 단축키 Ctrl+Shift+O** — 클릭 투과 토글. 투과 중엔 창이 마우스를 못 받으므로
      이것이 유일한 탈출구 (등록 실패 시 트레이가 대체 경로)
- [x] 플랜 배지 표시명 매핑 (Max 20 / Pro 20 등), 잔량% 병기, staleness·Degraded 시각 구분
- [ ] statusline tap **설치기** — 기존 `statusLine` 설정 감지·래핑·원복 (현재 수동 설치)
- [ ] 아이콘 교체 (현재 생성한 플레이스홀더)
- [ ] 설정 영속화 (TOML) — 창 위치, 임계값, 항상위 상태

**선행 조건**: Rust 툴체인(`rustup`) 미설치 상태 — 빌드 전 설치 필요. Node는 있음(v24).

→ **이 시점부터 본인이 실사용.** 이후 모든 결정의 근거가 여기서 나옵니다.

### M1 — 신뢰성과 알림
- [ ] **자동 앵커 + 증분 추정** — tap 갱신 쌍 사이의 Δ% / Δ토큰으로 토큰→% 눈금을
      자동 학습, 앵커가 낡은 동안 JSONL 증분으로 사용률을 `Derived`(점선)로 추정.
      (수동 앵커 설계의 부활 — 앵커·증분이 모두 자동이라 폐기 사유가 소멸.
      M0에는 그 전 단계로 "앵커 이후 소모 감지 ▲" 정직 플래그만 구현됨)
- [ ] 임계값 알림 (80% / 90%, 색상 + 토스트)
- [ ] SQLite 히스토리 + 24시간 그래프
- [ ] Collector `Health` 표면화 (타일 단위 경고)
- [ ] Anthropic / OpenAI Admin API Collector (T4) — **비용(USD) 뷰 중심**, `keyring` 저장

### M2 — 커버리지 확대
- [ ] 로컬 프록시 Collector (T1) — fail-open, SSE 통과.
      **전제 실증 완료 (2026-08-26)**: OAuth CLI가 `ANTHROPIC_BASE_URL` 존중,
      `anthropic-ratelimit-unified-*` 헤더 스키마 확보 (§T0 Claude 절 참조)
- [ ] **Fable 임계 눈금** — 게이지에 `fallback-percentage`(50%) 지점 표시,
      "Fable 잔량 = 임계 − 사용률" 산출. 임계값은 T1 헤더에서 라이브로 (플랜·프로모마다 다를 수 있음)
- [ ] 신규 서비스 조사 (T0 → T2 순서): Gemini CLI, Copilot 등
- [ ] 투명도 / 색상 / 폰트 / 레이아웃 커스터마이즈
- [ ] 다중 계정

### M3 — 이식과 개방
- [ ] macOS 지원 + 활성 창 부착 모드 (접근성 권한 온보딩)
- [ ] Collector SDK 공개 (프로세스 외부 실행 + JSON 계약 권장 — ABI 안정성 회피)
- [ ] 다중 모니터

### 별도 트랙 — Linux
X11은 M3에 얹을 수 있으나, Wayland는 `wlr-layer-shell` 대응이 별도 과제입니다.
컴포지터별 지원 편차가 커서 독립 트랙으로 분리합니다.

---

## 8. 선행 사례 (착수 전 확인)

JSONL 파싱은 이미 만들어진 것이 있습니다. 처음부터 짜기 전에 확인하세요.

- **`ccusage`** (npm) — Claude Code JSONL → 비용/토큰 집계. 파싱 스키마 참고용
- **`Claude-Code-Usage-Monitor`** (Python) — 롤링 창 한도 추정 로직 참고용

**Token_Orbit의 차별점은 파싱이 아니라 "멀티 서비스 통합 + 상시 오버레이 HUD"** 입니다.
파싱은 최대한 빌려오고, 차별점에 시간을 쓰세요.

---

## 9. 보안 · 법적 원칙

- **외부 전송 없음.** 텔레메트리·크래시 리포트 포함, 기본값 전송 없음.
- 크리덴셜은 OS 키체인. 설정 파일에 평문 저장 금지.
- **접근 범위를 원칙으로 고정**: 본인 로컬 파일 + 본인 크리덴셜로 호출하는 공식 API +
  본인이 명시적으로 경유시킨 자기 트래픽. 그 외는 접근하지 않음.
- T6(OCR/스크래핑)를 제외한 이유가 이 원칙입니다 — 편의를 위해 원칙을 깨지 않습니다.
- **디버그 포트를 요구하지 않습니다.** 인증 없는 포트를 여는 순간 로컬의 임의 프로세스가
  해당 앱의 세션을 탈취할 수 있습니다. 편의를 위해 사용자에게 이 위험을 지우지 않기로
  했으며(§1 비목표), 그 결과 T3와 `InAppWidget`이 미채택되었습니다.
- 주 경로인 T2는 **사용자 홈 디렉터리의 자기 파일을 읽는 것**이 전부입니다.
  타 프로세스에 붙지 않고, 네트워크도 쓰지 않습니다. 가장 권한이 적은 경로입니다.
- 프록시(T1)는 사용자의 요청 본문을 지나가게 하므로, **본문은 절대 디스크에 쓰지 않고
  usage 필드와 헤더만 추출**합니다. 이 점을 UI에 명시할 것.

---

## 부록 A: 초안 대비 변경 근거

| # | 초안 | 문제 | 변경 |
|---|---|---|---|
| 1 | `OpenAI /v1/dashboard/billing/subscription`으로 플랜 조회 | 해당 엔드포인트는 비공개였고 세션 키를 요구했으며 현재 제거됨 | Admin API로 대체, 단 집계 지연·권한 제약 명시 |
| 2 | 데스크톱 앱 **로컬 파일**에서 사용량/플랜 추출 | ChatGPT/Claude Desktop은 해당 데이터를 디스크에 저장하지 않음 | 파일 경로는 삭제. 단 **실행 중 렌더러 메모리에는 존재** → T3로 부활 (아래 12번) |
| 3 | OCR로 화면에서 사용량 인식 | 정확도·유지보수 문제 | 비목표로 명시 제외 (T3와는 구분) |
| 4 | 갱신 주기 기본 60초, 최소 10초 | T4는 시간 단위 집계라 무의미, T2는 이벤트 기반이라 폴링 불필요 | Collector별 `Cadence` 자율 결정 |
| 5 | 여러 서비스 합산 총 사용량 | 토큰/요청/메시지는 단위가 달라 합산 불가 | USD만 합산, 나머지는 개별 표시 |
| 6 | 활성 창 부착이 기본 모드 | 가장 비싸고 Wayland에서 원리적으로 불가능 | 전역 최상단을 기본으로. 부착은 Win/mac 옵션 |
| 7 | 클릭 투과 + 드래그 이동 동시 지원 | 투과 중에는 마우스 이벤트가 오지 않아 양립 불가 | 단축키 기반 "이동 모드" 도입 |
| 8 | 데이터 수집에 Rust 또는 Python | 포터블 단일 바이너리 목표와 충돌 | Rust 단일 언어 |
| 9 | MVP = OpenAI/Anthropic API | Admin 키 장벽 + 지연으로 첫 인상 실패 | MVP = Claude Code 로컬 + 수동 한도 |
| 10 | (없음) | 정확값과 추정값이 구분 없이 섞임 | `Confidence` 필드 + UI 시각 구분 도입 |
| 11 | (없음) | 실시간 정확 데이터 확보 수단 부재 | 로컬 프록시(T1) 추가 |
| 12 | (없음) | 개인 구독 한도를 얻을 경로가 전무 | **Codex는 세션 파일**(실측), **Claude Code는 statusline 확장 지점**(공식 문서 확인)으로 해결. CDP(T3)·앵커 입력 모두 불필요해져 폐기 |
| 15 | (없음) | 공식 확장 지점을 검토 대상에서 누락 | **T0 신설.** 신규 서비스는 T0(공식 계약) → T2(로컬 파일) → 그 외 순서로 조사 |
| 13 | (없음) | 롤링 창이 복수인 서비스를 단일 값으로 표시 | `primary`/`secondary` 창 동시 Snapshot 규칙 추가 (§3 규칙 3) |
| 14 | (없음) | 읽기 실패 시 캐시값을 현재값처럼 노출 | Staleness 절에 명시적 금지 규칙 추가 |

## 부록 B: `codex-usage-inapp.ps1` 프로토타입에서 얻은 것

이 저장소의 PowerShell 프로토타입이 T3 설계의 출발점입니다. 계승할 것과 버릴 것:

### 계승
- **CDP `Runtime.evaluate`로 Electron 렌더러에 접근한다는 발상** — T3의 핵심
- **`rate_limit.primary_window` / `secondary_window` / `credits.balance` 스키마** — §3 규칙 3의 근거
- **`/json/list`에서 `type === 'page'` 필터링 + 보조 창 제외** — 타깃 선별 로직 그대로 유용

### 버릴 것
| 프로토타입 | 문제 | Token_Orbit에서는 |
|---|---|---|
| React fiber 트리 DFS (`memoizedProps`) | React 비공개 내부. 앱 업데이트마다 깨짐 | `Network` 도메인 구독으로 대체 |
| 읽기와 그리기가 한 덩어리 | 둘 중 하나만 쓸 수 없음 | T3(수집)와 `InAppWidget`(표시)로 분리 → **위젯만 보류하고 수집은 유지 가능** |
| 호스트 앱 DOM에 위젯 주입 | 포트 상시 개방 필요 + 영구 유지보수 부채 | **보류** (§5.2). 오버레이 안정화 후 재검토 |
| React 서브트리에 `insertBefore` | 재렌더가 노드를 삭제 → 재삽입 무한 경쟁 | 보류 항목이나, 해법은 §5.2에 보존 (`position:fixed`) |
| `MutationObserver(document.body, subtree)` + 매번 전체 fiber 순회 | 토큰 스트리밍 중 초당 수십 회 발화 가능 → 호스트 앱 CPU 부담 | 위 해법 적용 시 Observer 자체가 불필요 |
| `/남은 사용량/` 한국어 정규식 앵커 탐색 | 로케일 바뀌면 즉시 실패 | 로케일 비의존 셀렉터 + 폴백 배치 |
| `state.lastUsage`를 읽기 실패 시에도 반환 | 실패를 성공으로 위장 (§3 Staleness 금지 규칙) | `Health::Degraded` + 나이 표기 |
| 렌더러 내부 `setInterval`로 자가 갱신 | 값이 외부로 돌아오지 않아 집계 불가 | 외부에서 수집 → Aggregator로 |

### 알려진 버그 (검증 완료)
`Set-StrictMode -Version Latest` + `$ErrorActionPreference = 'Stop'` 조합에서,
존재하지 않는 속성 접근이 **예외를 던집니다.** 실측 결과:

```
$reply.error                     -> The property 'error' cannot be found on this object.
$reply.result.exceptionDetails   -> The property 'exceptionDetails' cannot be found on this object.
$evt.id                          -> The property 'id' cannot be found on this object.
```

CDP의 **정상** 응답에는 `error` 키가 없으므로 성공 경로에서 반드시 터집니다.
위젯 주입은 `Runtime.evaluate` 실행 시점에 이미 끝나므로 화면에는 위젯이 뜨고,
그 직후 스크립트가 에러로 종료됩니다 — "동작하는 것처럼 보이는" 이유입니다.
CDP 이벤트에는 `id`가 없어 `do/until ($reply.id -eq ...)` 루프도 같은 이유로 취약합니다.

수정: `$reply.PSObject.Properties.Name -contains 'error'` 형태로 존재 여부를 먼저 확인하거나,
해당 구간만 `Set-StrictMode -Off`로 감쌀 것.
