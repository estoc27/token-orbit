//! Token Orbit HUD — egui 네이티브 렌더러.
//!
//! Tauri/WebView2(~330MB) 대체. 같은 `orbit-core`를 소비하며, WebView 없이
//! GPU로 직접 그린다. 목표: 투명·최상단·무테두리 오버레이, 수집은 백그라운드 스레드.

// 콘솔 창을 항상 숨긴다 (디버그 빌드 포함) — 오버레이 앱이라 콘솔이 작업표시줄에 뜨면 안 된다.
#![windows_subsystem = "windows"]

mod settings;
mod theme;
#[cfg(windows)]
mod tray;
mod ui;

use settings::Settings;

use eframe::egui;
use orbit_core::aggregate::AggregatedView;
use std::sync::mpsc;
use std::time::Duration;

/// 백그라운드 수집 스레드가 UI로 보내는 것.
enum Msg {
    View(AggregatedView),
}

/// 관측 세션 1회 트리거 — 어느 스레드에서든 부를 수 있게 전역 가드.
fn trigger_observer() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static POLLING: AtomicBool = AtomicBool::new(false);
    if POLLING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        orbit_core::observer::poll_once(Duration::from_secs(40));
        POLLING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

/// 클릭 투과 상태 — UI 체크 표시와 스레드들이 공유한다.
static CLICK_THROUGH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn toggle_click_through_global() {
    use std::sync::atomic::Ordering;
    let on = !CLICK_THROUGH.load(Ordering::SeqCst);
    CLICK_THROUGH.store(on, Ordering::SeqCst);
    #[cfg(windows)]
    win32::set_click_through(on);
}

/// Win32 직접 호출 — eframe의 ViewportCommand::Visible이 투명·무테두리 창에서
/// 동작하지 않아(실측: 명령 전송 후에도 IsWindowVisible=TRUE) OS API를 직접 쓴다.
#[cfg(windows)]
mod win32 {
    pub type Hwnd = isize;

    #[link(name = "user32")]
    extern "system" {
        fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
        fn GetWindowLongW(hwnd: Hwnd, idx: i32) -> i32;
        fn SetWindowLongW(hwnd: Hwnd, idx: i32, val: i32) -> i32;
        fn SetForegroundWindow(hwnd: Hwnd) -> i32;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
    }

    use std::sync::atomic::{AtomicIsize, Ordering};
    static HWND: AtomicIsize = AtomicIsize::new(0);

    /// 시작 시 eframe이 만든 **진짜** 창 핸들을 등록한다.
    /// 제목으로 FindWindow 하면 안 된다: eframe은 창 제목을 비워두고,
    /// Process.MainWindowHandle은 winit의 숨은 이벤트 타깃 창을 가리킨다 (실측).
    pub fn register(h: Hwnd) {
        HWND.store(h, Ordering::Relaxed);
    }

    fn hwnd() -> Hwnd {
        HWND.load(Ordering::Relaxed)
    }

    pub fn is_visible() -> bool {
        let h = hwnd();
        h != 0 && unsafe { IsWindowVisible(h) } != 0
    }

    pub fn set_visible(on: bool) {
        let h = hwnd();
        if h == 0 {
            return;
        }
        unsafe {
            // SW_SHOWNOACTIVATE(4) — 포커스를 뺏지 않고 표시. SW_HIDE(0).
            ShowWindow(h, if on { 4 } else { 0 });
            if on {
                SetForegroundWindow(h);
            }
        }
    }

    /// 클릭 투과: WS_EX_TRANSPARENT(0x20) + WS_EX_LAYERED(0x80000) 토글.
    pub fn set_click_through(on: bool) {
        let h = hwnd();
        if h == 0 {
            return;
        }
        const GWL_EXSTYLE: i32 = -20;
        const FLAGS: i32 = 0x20 | 0x8_0000;
        unsafe {
            let ex = GetWindowLongW(h, GWL_EXSTYLE);
            SetWindowLongW(h, GWL_EXSTYLE, if on { ex | FLAGS } else { ex & !0x20 });
        }
    }
}

/// 외부 제어 파일 — Tauri 버전과 동일한 계약 (/orbit 플러그인이 쓴다).
fn control_file() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| std::path::PathBuf::from(h).join(".token-orbit").join("control"))
}

/// 자기 exe 경로 등록 — /orbit이 HUD를 콜드런치할 때 찾는 파일.
fn register_app_path() {
    let (Some(dir), Ok(exe)) = (
        std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join(".token-orbit")),
        std::env::current_exe(),
    ) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("app-path"), exe.to_string_lossy().as_bytes());
}

fn main() -> eframe::Result<()> {
    register_app_path();
    // 이전 실행이 남긴 제어 파일 제거 — 시작하자마자 숨거나 종료되는 사고 방지.
    if let Some(p) = control_file() {
        let _ = std::fs::remove_file(p);
    }

    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([320.0, 240.0])
        .with_min_inner_size([192.0, 60.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_resizable(false);

    let options = eframe::NativeOptions {
        viewport,
        // 투명 창 배경 (프레임버퍼 클리어 색). egui가 그 위에 카드를 그린다.
        ..Default::default()
    };

    eframe::run_native(
        "Token Orbit",
        options,
        Box::new(|cc| Ok(Box::new(HudApp::new(cc)))),
    )
}

struct HudApp {
    rx: mpsc::Receiver<Msg>,
    view: Option<AggregatedView>,
    menu_open: bool,
    /// 마지막으로 설정한 창 높이 — 자동 조절 시 떨림 방지.
    last_height: f32,
    settings: Settings,
    /// 시작 시 저장된 창 상태(위치·모드)를 아직 적용하지 않았으면 true.
    needs_restore: bool,
    /// 전역 단축키 등록을 살려두는 핸들.
    _hotkeys: Option<global_hotkey::GlobalHotKeyManager>,
    /// 트레이 아이콘 — drop되면 아이콘이 사라지므로 붙잡아 둔다.
    #[cfg(windows)]
    _tray: Option<tray_icon::TrayIcon>,
}

impl HudApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 창 제어(숨김·투과)에 쓸 실제 HWND를 여기서 확보한다.
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = cc.window_handle() {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    win32::register(isize::from(w.hwnd));
                }
            }
        }
        theme::install(&cc.egui_ctx);
        let settings = Settings::load();
        if let Some(z) = settings.zoom {
            cc.egui_ctx.set_zoom_factor(z);
        }

        let (tx, rx) = mpsc::channel::<Msg>();

        // 수집 루프 — orbit-core를 돌려 뷰를 UI로 보낸다. 파일 감시 + heartbeat.
        // ~/.token-orbit 디렉터리 감시에 control 파일 변경도 걸리므로, 여기서
        // 제어 명령도 함께 소비한다 (쓰기 → ~0.3초 내 반응, Tauri 버전과 동일).
        {
            let tx = tx.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                // 수집 최소 간격 — 바쁜 소스가 루프를 돌리지 못하게 하는 바닥값.
                const MIN_COLLECT: Duration = Duration::from_secs(5);
                let mut collectors = orbit_core::default_collectors();
                let (tick_tx, tick_rx) = mpsc::channel::<()>();
                let _watcher = orbit_core::watch::watch_sources_into(&collectors, tick_tx);
                let mut last_collect = std::time::Instant::now() - MIN_COLLECT;
                loop {
                    // 제어 파일 확인 — 읽었으면 즉시 소비해 재실행을 막는다.
                    // 창 제어는 여기서 Win32로 **직접** 처리한다: 숨김 상태에선 UI의
                    // update()가 돌지 않아 메시지로 보내면 show가 영원히 처리되지 않는다(실측).
                    if let Some(path) = control_file() {
                        if let Ok(cmd) = std::fs::read_to_string(&path) {
                            let _ = std::fs::remove_file(&path);
                            let cmd = cmd.trim();
                            #[cfg(windows)]
                            match cmd {
                                "show" => win32::set_visible(true),
                                "hide" => win32::set_visible(false),
                                "toggle" => win32::set_visible(!win32::is_visible()),
                                "refresh" => trigger_observer(),
                                "quit" => std::process::exit(0),
                                _ => {}
                            }
                        }
                    }
                    // 수집에는 하한 간격을 둔다. **활성 Codex 세션은 자기 JSONL을
                    // 계속 쓰기 때문에**(실측) 감시 이벤트만 믿으면 루프가 초당 여러 번
                    // 돌아 유휴 CPU를 태운다 (1.6% → 0.27%). 사용량 수치는 초 단위로
                    // 의미가 바뀌지 않으며, 제어 파일 처리는 이 하한 위에 있어
                    // 응답성(~0.3초)은 그대로다.
                    if last_collect.elapsed() >= MIN_COLLECT {
                        let view = orbit_core::aggregate::collect_all(&mut collectors);
                        last_collect = std::time::Instant::now();
                        if tx.send(Msg::View(view)).is_err() {
                            break; // UI 종료됨
                        }
                        // 숨김 상태에서도 갱신이 반영되도록 명시적으로 깨운다.
                        ctx.request_repaint();
                    }
                    let _ = tick_rx.recv_timeout(Duration::from_secs(30));
                    std::thread::sleep(Duration::from_millis(300)); // 디바운스
                    while tick_rx.try_recv().is_ok() {}
                }
            });
        }

        // 전역 단축키 — Ctrl+Shift+O(투과 토글: 투과 중 유일한 탈출구),
        // Ctrl+Shift+H(소환/숨김). 이벤트는 별도 스레드에서 받아 UI를 깨운다.
        let mut hk_ghost = 0;
        let mut hk_summon = 0;
        let hotkeys = (|| {
            use global_hotkey::hotkey::{Code, HotKey, Modifiers};
            use global_hotkey::GlobalHotKeyManager;
            let mgr = GlobalHotKeyManager::new().ok()?;
            let mods = Modifiers::CONTROL | Modifiers::SHIFT;
            let ghost = HotKey::new(Some(mods), Code::KeyO);
            let summon = HotKey::new(Some(mods), Code::KeyH);
            hk_ghost = ghost.id();
            hk_summon = summon.id();
            mgr.register(ghost).ok()?;
            mgr.register(summon).ok()?;
            Some(mgr)
        })();
        {
            let ctx = cc.egui_ctx.clone();
            let (ghost_id, summon_id) = (hk_ghost, hk_summon);
            std::thread::spawn(move || {
                use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
                while let Ok(ev) = GlobalHotKeyEvent::receiver().recv() {
                    if ev.state() != HotKeyState::Pressed {
                        continue;
                    }
                    // 숨김/투과 상태에선 update()가 안 돌 수 있어 여기서 직접 처리.
                    if ev.id() == ghost_id {
                        toggle_click_through_global();
                    } else if ev.id() == summon_id {
                        #[cfg(windows)]
                        win32::set_visible(!win32::is_visible());
                    }
                    ctx.request_repaint(); // 체크 표시 등 UI 동기화
                }
            });
        }

        // 트레이 — 창이 작업표시줄에 없으므로 마우스로 닿는 유일한 상시 접점.
        // 명령은 여기서 직접 처리한다 (숨김·투과 상태에서도 동작해야 하므로).
        #[cfg(windows)]
        let tray = {
            let ctx = cc.egui_ctx.clone();
            tray::install(move |cmd| {
                match cmd {
                    tray::TrayCmd::Toggle => win32::set_visible(!win32::is_visible()),
                    tray::TrayCmd::ClickThrough => toggle_click_through_global(),
                    tray::TrayCmd::Refresh => trigger_observer(),
                    tray::TrayCmd::Quit => std::process::exit(0),
                }
                ctx.request_repaint();
            })
        };

        Self {
            rx,
            view: None,
            menu_open: false,
            last_height: 0.0,
            settings,
            needs_restore: true,
            _hotkeys: hotkeys,
            #[cfg(windows)]
            _tray: tray,
        }
    }

}

impl eframe::App for HudApp {
    // 창 밖(프레임버퍼)을 투명하게 — 무테두리 오버레이의 핵심.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 시작 시 저장된 창 상태 복원 (한 번만).
        if self.needs_restore {
            self.needs_restore = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.settings.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            }));
            if let Some((x, y)) = self.settings.window_pos {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x as f32, y as f32)));
            }
            // 클릭 투과는 세션 모드 — 재시작 시 항상 상호작용 가능으로 시작한다.
            // (지난 세션의 투과 상태가 복원되면 시작하자마자 마우스가 안 닿는다.)
            self.settings.click_through = false;
        }

        // 백그라운드 메시지 수신 (논블로킹). 창 제어는 스레드가 직접 처리한다.
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::View(v) => self.view = Some(v),
            }
        }

        // Esc → 종료 (Tauri 버전과 동일한 계약).
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let sview = ui::SettingsView {
            always_on_top: self.settings.always_on_top,
            pos_locked: self.settings.pos_locked,
            click_through: CLICK_THROUGH.load(std::sync::atomic::Ordering::SeqCst),
            opacity: self.settings.opacity,
            enabled: &self.settings.enabled_services,
        };

        let mut action = None;
        let mut resize_dx = None;
        let content_height = egui::CentralPanel::default()
            .frame(egui::Frame::none()) // 패널 자체 배경 없음 — 카드만 불투명
            .show(ctx, |ui_ctx| {
                let r = ui::render(ui_ctx, self.view.as_ref(), self.menu_open, sview);
                action = r.action;
                resize_dx = r.resize_dx;
                r.content_height
            })
            .inner;

        // 그립 드래그 → 폭 조절 + 콘텐츠 스케일. egui는 zoom_factor로 전체 배율을
        // 한 번에 바꾸므로 CSS zoom 진동 같은 문제가 없다.
        if let Some(dx) = resize_dx {
            let cur_w = ctx.input(|i| i.screen_rect().width());
            let base_zoom = ctx.zoom_factor();
            // 물리 폭 기준으로 배율 계산 (base 320px 논리폭 = zoom 1.0).
            let new_w = (cur_w + dx / base_zoom).clamp(192.0, 800.0);
            let new_zoom = (new_w / 320.0).clamp(0.6, 2.5);
            if (new_zoom - base_zoom).abs() > 0.001 {
                ctx.set_zoom_factor(new_zoom);
                self.settings.zoom = Some(new_zoom);
                self.settings.save();
                self.last_height = 0.0; // 높이 재측정 강제
            }
        }

        // 빈 여백을 드래그하면 창 이동 — 위치 잠금·클릭 투과 중엔 안 함.
        if !self.settings.pos_locked
            && !CLICK_THROUGH.load(std::sync::atomic::Ordering::SeqCst)
            && ctx.input(|i| i.pointer.primary_down())
            && !ctx.is_using_pointer()
            && ctx.input(|i| i.pointer.is_decidedly_dragging())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        // 창 이동이 끝났을 때 위치 저장.
        if !self.settings.pos_locked {
            if let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
                let pos = (rect.min.x as i32, rect.min.y as i32);
                if self.settings.window_pos != Some(pos) && rect.min.x.is_finite() {
                    self.settings.window_pos = Some(pos);
                    self.settings.save();
                }
            }
        }

        match action {
            Some(ui::Action::Refresh) => {
                self.menu_open = false;
                trigger_observer();
            }
            Some(ui::Action::ToggleMenu) => self.menu_open = !self.menu_open,
            Some(ui::Action::ToggleAlwaysOnTop) => {
                self.settings.always_on_top = !self.settings.always_on_top;
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    if self.settings.always_on_top {
                        egui::WindowLevel::AlwaysOnTop
                    } else {
                        egui::WindowLevel::Normal
                    },
                ));
                self.settings.save();
            }
            Some(ui::Action::TogglePosLock) => {
                self.settings.pos_locked = !self.settings.pos_locked;
                self.settings.save();
            }
            Some(ui::Action::ToggleClickThrough) => {
                toggle_click_through_global();
                self.menu_open = false; // 투과되면 메뉴도 못 누르므로 닫아준다
            }
            Some(ui::Action::ToggleService(key)) => {
                let on = self.settings.enabled_services.get(&key).copied().unwrap_or(true);
                self.settings.enabled_services.insert(key, !on);
                self.settings.save();
                self.last_height = 0.0; // 카드가 빠지면 높이 재측정
            }
            Some(ui::Action::SetOpacity(o)) => {
                self.settings.opacity = o;
                self.settings.save();
            }
            Some(ui::Action::Quit) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }

        // 내용 높이에 맞춰 창 높이 자동 조절 — 카드가 잘리지 않게.
        // 히스테리시스 4px로 미세 변동 시 창이 떨리지 않게 한다.
        let want_h = (content_height + 6.0).clamp(60.0, 1200.0);
        if (want_h - self.last_height).abs() > 4.0 {
            self.last_height = want_h;
            let w = ctx.input(|i| i.screen_rect().width());
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, want_h)));
        }

        // 주기적 리페인트는 표시 나이·리셋 카운트다운을 흐르게 하는 용도뿐이고,
        // 그 표시는 분 단위다. 데이터 변경·마우스 입력은 각각 수집 스레드와 egui가
        // 즉시 깨우므로, 여기서 매초 깨울 이유가 없다.
        ctx.request_repaint_after(Duration::from_secs(10));
    }
}
