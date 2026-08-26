//! Token_Orbit Tauri 셸 — 오버레이 창 + 수집 루프.
//!
//! 역할은 셋뿐이다: (1) 주기적으로 orbit-core를 돌리고 (2) 결과를 HUD로 이벤트 전송,
//! (3) 오버레이 모드(최상단/클릭투과) 제어. 수집·집계 로직은 orbit-core에만 있다.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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

/// 수동 새로고침 — 수집 루프를 즉시 깨운다.
/// 로컬 파일을 다시 읽는 것이므로, 서버 상태 자체는 트래픽이 흘러야 갱신된다.
#[tauri::command]
fn refresh_now(state: tauri::State<'_, HudState>) -> Result<(), String> {
    if let Some(tx) = state.refresh_tx.lock().map_err(|e| e.to_string())?.as_ref() {
        let _ = tx.send(());
    }
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

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    // Pressed만 처리 — Released까지 받으면 한 번 누름에 두 번 토글된다.
                    if event.state() == ShortcutState::Pressed && shortcut == &overlay_shortcut {
                        toggle_click_through_internal(app);
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
            refresh_now
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

            // 수집 루프 — UI와 분리된 백그라운드 스레드 (README §4.4).
            std::thread::spawn(move || {
                let mut collectors = orbit_core::default_collectors();
                // 파일 감시 이벤트를 새로고침 채널로 합류 (수동 새로고침과 동일 경로).
                let watch = orbit_core::watch::watch_sources(&collectors);
                if let Some(w) = watch {
                    let fwd = tick_tx.clone();
                    std::thread::spawn(move || {
                        // _watcher가 이 스레드에 살아 있어야 감시가 유지된다.
                        while let Ok(()) = w.rx.recv() {
                            if fwd.send(()).is_err() {
                                break;
                            }
                        }
                    });
                }
                loop {
                    let view = orbit_core::aggregate::collect_all(&mut collectors);
                    // 사용자 설정(서비스 토글)을 뷰에 동봉 — UI가 필터와 설정 메뉴에 사용.
                    let enabled = handle
                        .state::<HudState>()
                        .settings
                        .lock()
                        .map(|s| s.enabled_services.clone())
                        .unwrap_or_default();
                    let payload = serde_json::json!({ "view": view, "enabled": enabled });
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
        .run(tauri::generate_context!())
        .expect("error while running Token_Orbit");
}
