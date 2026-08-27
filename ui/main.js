// Token Orbit HUD 렌더러.
// AggregatedView(orbit-core::aggregate) 하나만 먹는다 — 수집 경로는 모른다 (§5).

const { event, window: tauriWin } = window.__TAURI__;
const appWindow = tauriWin.getCurrentWindow();

const cardsEl = document.getElementById("cards");

// 임계값 — M1에서 사용자 설정으로 이동 예정 (README §7).
const WARN_PCT = 80;
const CRIT_PCT = 90;
const STALE_SECS = 30 * 60;

// 페이로드: { view: AggregatedView, enabled: {service_key: bool} }
let last = null; // 마지막 페이로드 — 토글 즉시 반영과 설정 메뉴 렌더에 사용
event.listen("usage://update", (e) => {
  last = e.payload;
  posLocked = !!last.locked;
  const ls = document.getElementById("lock-state");
  if (ls) ls.textContent = posLocked ? "켜짐" : "꺼짐";
  render(last);
  renderServiceToggles(last);
});

// 어디를 잡아도 창 이동. data-tauri-drag-region은 자식 요소 클릭을 못 받아서
// (카드가 화면을 꽉 채우면 잡을 곳이 없음) startDragging을 직접 호출한다.
// 단, 메뉴/톱니바퀴 클릭은 드래그로 먹지 않는다.
let posLocked = false; // 셸이 페이로드로 알려준다
document.addEventListener("mousedown", (e) => {
  if (e.target.closest("#topbar")) return;
  hideMenu();
  if (posLocked) return; // 위치 잠금 중에는 드래그 이동 무시
  if (e.button === 0) appWindow.startDragging().catch(() => {});
});

// Esc → HUD 종료.
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") appWindow.close().catch(() => {});
});

// ---- 톱니바퀴 메뉴 ----
const { core } = window.__TAURI__;
const menuEl = document.getElementById("menu");
let pinned = true; // tauri.conf.json의 alwaysOnTop 초기값과 일치

document.getElementById("gear").addEventListener("click", () => {
  menuEl.hidden = !menuEl.hidden;
});
function hideMenu() { menuEl.hidden = true; }

menuEl.addEventListener("click", async (e) => {
  if (e.target.closest("#service-toggles")) return; // 체크박스는 메뉴를 닫지 않는다
  const act = e.target.closest(".menu-item")?.dataset.act;
  if (!act) return;
  hideMenu();
  if (act === "quit") {
    appWindow.close().catch(() => {});
  } else if (act === "pin") {
    pinned = !pinned;
    try { await core.invoke("set_always_on_top", { on: pinned }); } catch (_) {}
    document.getElementById("pin-state").textContent = pinned ? "켜짐" : "꺼짐";
  } else if (act === "lock") {
    posLocked = !posLocked;
    document.getElementById("lock-state").textContent = posLocked ? "켜짐" : "꺼짐";
    try { await core.invoke("set_pos_locked", { locked: posLocked }); } catch (_) {}
  } else if (act === "ghost") {
    // 투과를 켜면 마우스가 창에 닿지 않는다 — 해제는 Ctrl+Shift+O 또는 트레이 메뉴.
    try { await core.invoke("toggle_click_through"); } catch (_) {}
  }
});

// 수동 새로고침 — 수집 루프를 즉시 깨운다. 로컬 파일 재수집이며,
// 서버 상태 자체는 트래픽이 흘러야 갱신된다 (버튼이 마법을 부리지 않는다).
document.getElementById("refresh").addEventListener("click", (e) => {
  const btn = e.currentTarget;
  btn.classList.remove("spin");
  void btn.offsetWidth; // 애니메이션 재시작
  btn.classList.add("spin");
  core.invoke("refresh_now").catch(() => {});
});

// 서비스 토글 — 저장 후 즉시 로컬 반영 (다음 tick을 기다리지 않는다).
document.getElementById("service-toggles").addEventListener("change", async (e) => {
  const svc = e.target?.dataset?.svc;
  if (!svc) return;
  const on = e.target.checked;
  try { await core.invoke("set_service_enabled", { service: svc, enabled: on }); } catch (_) {}
  if (last) {
    (last.enabled ||= {})[svc] = on;
    render(last);
  }
});

// 투과 상태는 셸이 알려준다 (전역 단축키·트레이로도 바뀌므로 UI가 단독 판단하면 어긋남).
event.listen("hud://click-through", (e) => {
  document.body.classList.toggle("ghost", !!e.payload);
});

function render(payload) {
  const cards = payload?.view?.cards ?? [];
  const enabled = payload?.enabled ?? {};
  // 토글이 꺼진 서비스는 그리지 않는다 (목록에 없으면 기본 표시).
  const visible = cards.filter((c) => enabled[c.source_id] !== false);
  if (visible.length === 0) {
    cardsEl.innerHTML = cards.length === 0
      ? `<div class="card placeholder">감지된 소스 없음</div>`
      : `<div class="card placeholder">모든 서비스 숨김 (⚙ 메뉴에서 선택)</div>`;
    autoResize();
    return;
  }
  cardsEl.innerHTML = visible.map(cardHtml).join("");
  autoResize();
}

// 설정 메뉴의 서비스 체크박스 — 감지된 서비스가 자동으로 나열된다.
function renderServiceToggles(payload) {
  const box = document.getElementById("service-toggles");
  const cards = payload?.view?.cards ?? [];
  const enabled = payload?.enabled ?? {};
  if (cards.length === 0) {
    box.innerHTML = `<div class="menu-label">감지된 서비스 없음</div>`;
    return;
  }
  box.innerHTML = cards
    .map((c) => {
      const on = enabled[c.source_id] !== false;
      return `<label class="svc-toggle"><input type="checkbox" data-svc="${esc(c.source_id)}" ${on ? "checked" : ""}> ${esc(c.display_name)}</label>`;
    })
    .join("");
}

// ---- 폭 기반 스케일링 ----
// 창 폭을 기준폭(320) 대비 비율로 환산해 콘텐츠를 transform:scale로 확대/축소한다.
// zoom 속성은 레이아웃 폭 계산에 되먹임돼 확대↔축소 진동을 일으켰다(실측) —
// transform은 레이아웃 계측에 영향을 주지 않아 루프가 생기지 않는다.
const BASE_W = 320;
let scale = 1;
function applyScale() {
  scale = Math.min(2.5, Math.max(0.6, window.innerWidth / BASE_W));
  const hud = document.getElementById("hud");
  hud.style.transformOrigin = "0 0";
  hud.style.transform = `scale(${scale})`;
  // 스케일된 결과가 창 폭을 정확히 채우도록 레이아웃 폭을 역보정한다.
  hud.style.width = `${window.innerWidth / scale}px`;
}
window.addEventListener("resize", () => {
  applyScale();
  autoResize();
});
applyScale();

// 내용 높이에 맞춰 창 높이 자동 조절 — 카드가 잘리지 않게.
// getBoundingClientRect는 transform이 반영된 실제 렌더 크기를 준다.
let lastH = 0;
async function autoResize() {
  const hud = document.getElementById("hud");
  const need = Math.ceil(hud.getBoundingClientRect().height) + 2;
  if (Math.abs(need - lastH) < 3) return; // 미세 변동으로 인한 루프 방지
  lastH = need;
  try {
    await appWindow.setSize(new tauriWin.LogicalSize(window.innerWidth, need));
  } catch (_) {}
}

function cardHtml(c) {
  const stale = c.data_age_secs > STALE_SECS ? " stale" : "";
  const degraded = c.health && c.health.state === "degraded" ? " degraded" : "";

  const windows = c.windows
    .map((w, i) => windowHtml(w, i === 0))
    .join("");

  // 앵커 이후 소모 감지 — 표시된 %는 하한. 수치를 지어내는 대신 정직하게 알림.
  const activity = c.activity_after_percent
    ? `<div class="activity-note">▲ 이후 사용 감지 — 실제 사용률은 표시보다 높음</div>`
    : "";

  let body = "";
  // 세션 토큰은 %가 없을 때의 폴백으로만 (tap 미연동 상태에서 카드가 비지 않게).
  // %가 있으면 세션 단위 수치는 노이즈 — 사용자 피드백으로 제거.
  if (c.tokens && c.windows.length === 0) {
    const t = c.tokens;
    body += `<div class="tokens-line">세션 토큰 · in ${fmt(t.input)} · out ${fmt(t.output)} · cache ${fmt(t.cache_read)}</div>`;
  }
  if (c.credits_usd != null) {
    body += `<div class="cost-line">크레딧 잔액 $${c.credits_usd.toFixed(2)}</div>`;
  }
  // percent 능력이 없는 카드에는 연동 안내 (§4 Capabilities — 강요하지 않고 안내만)
  if (!c.capabilities.percent && c.source_id === "claude") {
    body += `<div class="setup-hint">statusline 연동 시 사용률 표시 가능</div>`;
  }

  return `
    <div class="card${stale}${degraded}">
      <div class="card-head">
        <span class="svc-name">${esc(c.display_name)}</span>
        ${c.plan ? `<span class="plan-badge">${esc(c.plan)}</span>` : ""}
        <span class="age">${ageLabel(c.data_age_secs)}</span>
      </div>
      ${windows}${activity}${body}
    </div>`;
}

function windowHtml(w, major) {
  const cls =
    w.used_percent >= CRIT_PCT ? "crit" : w.used_percent >= WARN_PCT ? "warn" : "";
  const conf = w.confidence !== "exact" ? " derived" : "";
  // "사용 84% / 잔량 16%"를 모두 명시 — 숫자 하나만 두면 사용·잔량 혼동이 생긴다 (§5.0).
  const remaining = `잔량 ${Math.max(0, 100 - Math.round(w.used_percent))}%`;
  // 창마다 출처가 달라 신선도가 다르다 — 오래된 값은 창 단위로 표시한다.
  const stale = w.age_secs > 300 ? ` · <span class="win-age">${ageLabel(w.age_secs)}</span>` : "";
  const reset =
    (w.resets_in_secs != null ? `${esc(w.label)} · ${remaining} · ${durLabel(w.resets_in_secs)} 후 리셋`
                              : `${esc(w.label)} · ${remaining}`) + stale;

  // 모델 전용 주간 창(예: "7d Fable")은 전체 모델 주간 한도의 절반이 배정된다.
  // 사용량에 따라 변하지 않는 플랜 고정값이므로 계산하지 않고 그대로 안내만 한다.
  const modelWindow = /^7d\s+\S/.test(w.label);
  const note = modelWindow ? `<div class="quota-note">주간 임계 50%</div>` : "";

  return `
    <div class="win ${major ? "major" : "minor"}">
      <div class="bar-row">
        <div class="bar">
          <div class="bar-fill ${cls}${conf}" style="width:${clamp(w.used_percent)}%"></div>
        </div>
        <span class="pct">${Math.round(w.used_percent)}%</span>
      </div>
      <div class="win-meta">${reset}</div>
      ${note}
    </div>`;
}

// ---- helpers ----
function clamp(x) { return Math.max(0, Math.min(100, x)); }
function esc(s) { return String(s).replace(/[&<>"]/g, (m) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[m])); }
function fmt(n) {
  if (n >= 1e6) return (n / 1e6).toFixed(1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "k";
  return String(n);
}
function durLabel(secs) {
  if (secs <= 0) return "곧";
  const d = Math.floor(secs / 86400), h = Math.floor((secs % 86400) / 3600), m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}일 ${h}시간`;
  if (h > 0) return `${h}시간 ${m}분`;
  return `${m}분`;
}
function ageLabel(secs) {
  if (secs < 90) return "방금";
  if (secs < 3600) return `${Math.floor(secs / 60)}분 전`;
  return `${Math.floor(secs / 3600)}시간 전`;
}
