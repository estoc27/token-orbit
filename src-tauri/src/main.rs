//! Token_Orbit Tauri 셸 — 오버레이 창 + 수집 루프.
//!
//! 역할은 셋뿐이다: (1) 주기적으로 orbit-core를 돌리고 (2) 결과를 HUD로 이벤트 전송,
//! (3) 오버레이 모드(최상단/클릭투과) 제어. 수집·집계 로직은 orbit-core에만 있다.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// 수집 루프 주기. 파일 감시(`notify`)가 이벤트를 주면 즉시 깨어나고,
/// 이 값은 감시가 놓친 변경에 대한 안전망(heartbeat)으로만 쓰인다.
const HEARTBEAT_SECS: u64 = 30;

/// HUD 창 라벨 — tauri.conf.json과 일치.
const HUD: &str = "hud";

struct HudState {
    click_through: Mutex<bool>,
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
        .manage(HudState { click_through: Mutex::new(false) })
        .invoke_handler(tauri::generate_handler![toggle_click_through, set_always_on_top])
        .setup(move |app| {
            let handle = app.handle().clone();
            build_tray(&handle)?;
            spawn_proxy_tap();

            // 단축키 등록 실패는 치명적이지 않다(다른 앱이 선점했을 수 있음).
            // 트레이 메뉴라는 대체 경로가 있으므로 로그만 남기고 계속 진행 — fail-open.
            if let Err(e) = app.global_shortcut().register(overlay_shortcut) {
                eprintln!("global shortcut Ctrl+Shift+O 등록 실패: {e} (트레이 메뉴로 대체 가능)");
            }

            // 수집 루프 — UI와 분리된 백그라운드 스레드 (README §4.4).
            std::thread::spawn(move || {
                let mut collectors = orbit_core::default_collectors();
                let watch = orbit_core::watch::watch_sources(&collectors);
                loop {
                    let view = orbit_core::aggregate::collect_all(&mut collectors);
                    // 수신자(HUD)가 없어도 루프는 계속 — fail-open.
                    let _ = handle.emit("usage://update", &view);

                    // 파일 변경이 오면 즉시 재수집, 없으면 heartbeat까지 대기.
                    // 감시가 없거나 죽어도 heartbeat가 폴링처럼 동작한다.
                    match &watch {
                        Some(w) => {
                            let _ = w.rx.recv_timeout(Duration::from_secs(HEARTBEAT_SECS));
                            // 연쇄 이벤트(한 턴에 여러 번 append) 흡수 — 디바운스.
                            std::thread::sleep(Duration::from_millis(300));
                            while w.rx.try_recv().is_ok() {}
                        }
                        None => std::thread::sleep(Duration::from_secs(5)),
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Token_Orbit");
}
