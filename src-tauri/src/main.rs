//! Token_Orbit Tauri 셸 — 오버레이 창 + 수집 루프.
//!
//! 역할은 셋뿐이다: (1) 주기적으로 orbit-core를 돌리고 (2) 결과를 HUD로 이벤트 전송,
//! (3) 오버레이 모드(최상단/클릭투과) 제어. 수집·집계 로직은 orbit-core에만 있다.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod observer;

use std::collections::HashMap;
use std::sync::{mpsc, Mutex};
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// 사용자 설정 — `~/.token-orbit/settings.json`.
/// 서비스 토글처럼 앱이 관리하는 값. (statusline tap 파일들과 같은 디렉터리)
#[derive(Default, serde::Serialize, serde::Deserialize, Clone)]
struct UserSettings {
    /// service_key → 표시 여부. 목록에 없으면 기본 true (새 서비스는 자동 표시).
    #[serde(default)]
    enabled_services: HashMap<String, bool>,
    /// 마지막 창 위치 (물리 픽셀). 재시작 시 복원 — 사용자가 둔 자리가 곧 기본값.
    #[serde(default)]
    window_pos: Option<(i32, i32)>,
    /// 마지막 창 폭 (물리 픽셀). 높이는 내용에 맞춰 자동이므로 폭만 저장한다.
    #[serde(default)]
    window_width: Option<u32>,
    /// 위치 잠금 — 켜면 드래그 이동이 무시된다 (실수 방지).
    #[serde(default)]
    pos_locked: bool,
}

fn settings_path() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| std::path::PathBuf::from(h).join(".token-orbit").join("settings.json"))
}

fn load_settings() -> UserSettings {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(s: &UserSettings) {
    let Some(path) = settings_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(s) {
        // 원자적 쓰기 — 절반 쓰인 설정 파일을 읽지 않게.
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// 수집 루프 주기. 파일 감시(`notify`)가 이벤트를 주면 즉시 깨어나고,
/// 이 값은 감시가 놓친 변경에 대한 안전망(heartbeat)으로만 쓰인다.
const HEARTBEAT_SECS: u64 = 30;

/// HUD 창 라벨 — tauri.conf.json과 일치.
const HUD: &str = "hud";

/// 위치 복원이 끝났는가. 복원 전의 Moved 이벤트(창 생성 시 OS 기본 배치)를 저장하면
/// 저장된 위치가 계단식 기본값으로 덮여 재시작마다 창이 흘러내린다 (실측).
static POS_RESTORED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct HudState {
    click_through: Mutex<bool>,
    settings: Mutex<UserSettings>,
    /// 수집 루프를 즉시 깨우는 채널 (수동 새로고침 버튼).
    refresh_tx: Mutex<Option<mpsc::Sender<()>>>,
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, HudState>) -> Result<UserSettings, String> {
    Ok(state.settings.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
fn set_service_enabled(
    state: tauri::State<'_, HudState>,
    service: String,
    enabled: bool,
) -> Result<(), String> {
    let mut s = state.settings.lock().map_err(|e| e.to_string())?;
    s.enabled_services.insert(service, enabled);
    save_settings(&s);
    Ok(())
}

/// 새로고침 트리거 — 수집 루프를 깨우고, Claude 관측 세션도 1회 돌린다.
/// (관측 세션이 statusline tap을 강제 갱신 → 서버 기준 최신 사용률 확보)
/// ↻ 버튼과 control 파일의 `refresh` 동사가 공유한다.
fn trigger_refresh(app: &AppHandle) {
    let state = app.state::<HudState>();
    let Ok(guard) = state.refresh_tx.lock() else { return };
    let Some(tx) = guard.as_ref() else { return };
    let _ = tx.send(());
    // 관측 폴은 오래 걸릴 수 있어 별도 스레드로. 이미 도는 중이면 건너뜀.
    static POLLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !POLLING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let tx2 = tx.clone();
        std::thread::spawn(move || {
            observer::poll_once(Duration::from_secs(40));
            let _ = tx2.send(()); // 폴 결과 반영을 위해 한 번 더 수집
            POLLING.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }
}

#[tauri::command]
fn refresh_now(app: AppHandle) -> Result<(), String> {
    trigger_refresh(&app);
    Ok(())
}

#[tauri::command]
fn set_pos_locked(state: tauri::State<'_, HudState>, locked: bool) -> Result<(), String> {
    let mut s = state.settings.lock().map_err(|e| e.to_string())?;
    s.pos_locked = locked;
    save_settings(&s);
    Ok(())
}

/// 클릭 투과 전환. 커맨드·트레이·전역 단축키가 모두 이 하나를 부른다.
///
/// 투과가 켜지면 창이 마우스를 받지 못하므로 UI에서 되돌릴 수 없다.
/// 그래서 전역 단축키(Ctrl+Shift+O)가 유일한 탈출구이며, 상태를 UI에도 알려
/// 사용자가 지금 어떤 모드인지 인지하게 한다.
fn apply_click_through(app: &AppHandle, on: bool) -> Result<(), String> {
    let win = app.get_webview_window(HUD).ok_or("hud window missing")?;
    win.set_ignore_cursor_events(on).map_err(|e| e.to_string())?;
    let state = app.state::<HudState>();
    *state.click_through.lock().map_err(|e| e.to_string())? = on;
    let _ = app.emit("hud://click-through", on);
    Ok(())
}

fn toggle_click_through_internal(app: &AppHandle) {
    let cur = app
        .state::<HudState>()
        .click_through
        .lock()
        .map(|g| *g)
        .unwrap_or(false);
    let _ = apply_click_through(app, !cur);
}

#[tauri::command]
fn toggle_click_through(app: AppHandle) -> Result<bool, String> {
    let cur = *app
        .state::<HudState>()
        .click_through
        .lock()
        .map_err(|e| e.to_string())?;
    apply_click_through(&app, !cur)?;
    Ok(!cur)
}

#[tauri::command]
fn set_always_on_top(window: tauri::WebviewWindow, on: bool) -> Result<(), String> {
    window.set_always_on_top(on).map_err(|e| e.to_string())
}

/// HUD 표시/숨김. 트레이에서 실수로 숨겨도 트레이로 다시 부를 수 있다.
fn toggle_visibility(app: &AppHandle) {
    let Some(win) = app.get_webview_window(HUD) else { return };
    if win.is_visible().unwrap_or(true) {
        let _ = win.hide();
    } else {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// 외부 제어 파일 — `~/.token-orbit/control`.
///
/// 외부 프로세스(슬래시 커맨드, 스크립트)가 이 파일에 명령 한 줄을 쓰면
/// HUD가 반응한다: `show` / `hide` / `toggle` / `quit`.
/// 파일 감시가 이미 이 디렉터리를 보고 있어 쓰는 즉시 루프가 깨어난다.
/// 포트를 열지 않는 로컬 IPC — 프로젝트 원칙(§1 비목표)에 부합한다.
fn control_file() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| std::path::PathBuf::from(h).join(".token-orbit").join("control"))
}

fn handle_control(app: &AppHandle) {
    let Some(path) = control_file() else { return };
    let Ok(cmd) = std::fs::read_to_string(&path) else { return };
    // 읽었으면 즉시 소비 — 같은 명령이 다음 tick에 재실행되지 않게.
    let _ = std::fs::remove_file(&path);
    match cmd.trim() {
        "show" => {
            if let Some(w) = app.get_webview_window(HUD) {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        "hide" => {
            if let Some(w) = app.get_webview_window(HUD) {
                let _ = w.hide();
            }
        }
        "toggle" => toggle_visibility(app),
        "refresh" => trigger_refresh(app),
        "quit" => app.exit(0),
        other => eprintln!("unknown control command: {other:?}"),
    }
}

/// 프록시 탭(`scripts/proxy-tap.js`)을 띄운다.
///
/// 사용자가 `ANTHROPIC_BASE_URL`을 프록시로 걸어둔 상태에서 프록시가 죽어 있으면
/// **AI 작업 자체가 막힌다.** 그래서 HUD가 살아 있는 동안은 프록시도 살아 있도록
/// 앱이 직접 띄운다. 이미 떠 있으면(포트 점유) 아무것도 하지 않는다.
fn spawn_proxy_tap() {
    use std::net::TcpStream;
    // 이미 누가 듣고 있으면 중복 실행하지 않는다.
    if TcpStream::connect(("127.0.0.1", 8377)).is_ok() {
        return;
    }
    // 개발 실행(target/debug/…)과 배포 배치 모두 커버하도록 몇 곳을 훑는다.
    let Ok(exe) = std::env::current_exe() else { return };
    let mut candidates = Vec::new();
    for up in 1..=4 {
        if let Some(base) = exe.ancestors().nth(up) {
            candidates.push(base.join("scripts").join("proxy-tap.js"));
        }
    }
    let Some(script) = candidates.into_iter().find(|p| p.is_file()) else {
        eprintln!("proxy-tap.js를 찾지 못했습니다 — 프록시 수집은 비활성");
        return;
    };
    match std::process::Command::new("node").arg(&script).spawn() {
        Ok(_) => eprintln!("proxy tap 시작: {}", script.display()),
        Err(e) => eprintln!("proxy tap 시작 실패: {e} (node 설치 필요)"),
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "toggle_hud", "HUD 표시/숨김", true, None::<&str>)?,
            &MenuItem::with_id(
                app,
                "click_through",
                "클릭 투과 전환  (Ctrl+Shift+O)",
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Token Orbit")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle_hud" => toggle_visibility(app),
            "click_through" => toggle_click_through_internal(app),
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn main() {
    // Ctrl+Shift+O — 초안 READ.ME가 지정했던 오버레이 모드 단축키.
    let overlay_shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);
    // Ctrl+Shift+H — HUD 소환/숨김 (작업표시줄 없이 호출·닫기).
    let summon_shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyH);

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    // Pressed만 처리 — Released까지 받으면 한 번 누름에 두 번 토글된다.
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if shortcut == &overlay_shortcut {
                        toggle_click_through_internal(app);
                    } else if shortcut == &summon_shortcut {
                        toggle_visibility(app);
                    }
                })
                .build(),
        )
        .manage(HudState {
            click_through: Mutex::new(false),
            settings: Mutex::new(load_settings()),
            refresh_tx: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            toggle_click_through,
            set_always_on_top,
            get_settings,
            set_service_enabled,
            refresh_now,
            set_pos_locked
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            build_tray(&handle)?;
            spawn_proxy_tap();

            // 새로고침 채널: 파일 감시 이벤트와 수동 새로고침이 같은 채널로 합류한다.
            let (tick_tx, tick_rx) = mpsc::channel::<()>();
            *app.state::<HudState>().refresh_tx.lock().unwrap() = Some(tick_tx.clone());

            // 단축키 등록 실패는 치명적이지 않다(다른 앱이 선점했을 수 있음).
            // 트레이 메뉴라는 대체 경로가 있으므로 로그만 남기고 계속 진행 — fail-open.
            if let Err(e) = app.global_shortcut().register(overlay_shortcut) {
                eprintln!("global shortcut Ctrl+Shift+O 등록 실패: {e} (트레이 메뉴로 대체 가능)");
            }
            if let Err(e) = app.global_shortcut().register(summon_shortcut) {
                eprintln!("global shortcut Ctrl+Shift+H 등록 실패: {e} (트레이 메뉴로 대체 가능)");
            }
            // 이전 실행이 남긴 제어 파일 제거 — 시작하자마자 숨거나 종료되는 사고 방지.
            if let Some(p) = control_file() {
                let _ = std::fs::remove_file(p);
            }
            // 저장된 창 위치 복원 — 사용자가 마지막으로 둔 자리가 기본값.
            {
                let (pos, width) = app
                    .state::<HudState>()
                    .settings
                    .lock()
                    .map(|s| (s.window_pos, s.window_width))
                    .unwrap_or((None, None));
                if let Some(win) = app.get_webview_window(HUD) {
                    if let Some((x, y)) = pos {
                        let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                    }
                    if let Some(w) = width {
                        if let Ok(cur) = win.outer_size() {
                            let _ = win.set_size(tauri::PhysicalSize::new(w, cur.height));
                        }
                    }
                }
            }
            // 복원 이후의 Moved만 저장 대상 (위 static 주석 참조).
            POS_RESTORED.store(true, std::sync::atomic::Ordering::SeqCst);
            // 자기 경로 등록 — /orbit 등 외부 제어가 HUD 꺼진 상태에서도
            // 어디서 실행해야 하는지 알 수 있게 한다 (설치 위치 무관, 매 실행 갱신).
            if let (Ok(exe), Some(ctl)) = (std::env::current_exe(), control_file()) {
                if let Some(dir) = ctl.parent() {
                    let _ = std::fs::create_dir_all(dir);
                    let _ = std::fs::write(dir.join("app-path"), exe.to_string_lossy().as_bytes());
                }
            }

            // 수집 루프 — UI와 분리된 백그라운드 스레드 (README §4.4).
            std::thread::spawn(move || {
                let mut collectors = orbit_core::default_collectors();
                // 파일 감시 → 새로고침 채널 직결 (중계 스레드 없음 — 그 자체가 실패 지점이었다).
                // 반환된 watcher는 드롭되면 감시가 멈추므로 루프가 사는 동안 붙들어 둔다.
                let _watcher = orbit_core::watch::watch_sources_into(&collectors, tick_tx.clone());
                loop {
                    handle_control(&handle);
                    let view = orbit_core::aggregate::collect_all(&mut collectors);
                    // 사용자 설정(서비스 토글)을 뷰에 동봉 — UI가 필터와 설정 메뉴에 사용.
                    let enabled = handle
                        .state::<HudState>()
                        .settings
                        .lock()
                        .map(|s| s.enabled_services.clone())
                        .unwrap_or_default();
                    let locked = handle
                        .state::<HudState>()
                        .settings
                        .lock()
                        .map(|s| s.pos_locked)
                        .unwrap_or(false);
                    let payload =
                        serde_json::json!({ "view": view, "enabled": enabled, "locked": locked });
                    // 수신자(HUD)가 없어도 루프는 계속 — fail-open.
                    let _ = handle.emit("usage://update", &payload);

                    // 파일 변경/수동 새로고침이 오면 즉시 재수집, 없으면 heartbeat까지 대기.
                    let _ = tick_rx.recv_timeout(Duration::from_secs(HEARTBEAT_SECS));
                    // 연쇄 이벤트(한 턴에 여러 번 append) 흡수 — 디바운스.
                    std::thread::sleep(Duration::from_millis(300));
                    while tick_rx.try_recv().is_ok() {}
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // 창 폭 저장 — 높이는 autoResize가 내용에 맞추므로 저장하지 않는다.
            if let tauri::WindowEvent::Resized(size) = event {
                use std::sync::atomic::{AtomicI64, Ordering};
                static LAST_W_SAVE_MS: AtomicI64 = AtomicI64::new(0);
                if !POS_RESTORED.load(Ordering::SeqCst) {
                    return;
                }
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                if now_ms - LAST_W_SAVE_MS.load(Ordering::Relaxed) < 500 {
                    return;
                }
                LAST_W_SAVE_MS.store(now_ms, Ordering::Relaxed);
                let mut s = load_settings();
                if s.window_width != Some(size.width) {
                    s.window_width = Some(size.width);
                    save_settings(&s);
                }
                let _ = window;
            }
            // 창 이동 시 위치 저장 (드래그 중 연사되므로 500ms 스로틀).
            if let tauri::WindowEvent::Moved(pos) = event {
                use std::sync::atomic::{AtomicI64, Ordering};
                if !POS_RESTORED.load(Ordering::SeqCst) {
                    return; // 복원 전 초기 배치 이벤트 — 저장하면 안 된다
                }
                static LAST_SAVE_MS: AtomicI64 = AtomicI64::new(0);
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                if now_ms - LAST_SAVE_MS.load(Ordering::Relaxed) < 500 {
                    return;
                }
                LAST_SAVE_MS.store(now_ms, Ordering::Relaxed);
                // 파일 기준 read-modify-write — 설정 파일이 단일 진실이라
                // (set_service_enabled도 즉시 저장) 관리 상태를 거칠 필요가 없다.
                let mut s = load_settings();
                s.window_pos = Some((pos.x, pos.y));
                save_settings(&s);
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Token_Orbit");
}
