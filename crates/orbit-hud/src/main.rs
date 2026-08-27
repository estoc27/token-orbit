//! Token Orbit HUD — egui 네이티브 렌더러.
//!
//! Tauri/WebView2(~330MB) 대체. 같은 `orbit-core`를 소비하며, WebView 없이
//! GPU로 직접 그린다. 목표: 투명·최상단·무테두리 오버레이, 수집은 백그라운드 스레드.

// 콘솔 창을 항상 숨긴다 (디버그 빌드 포함) — 오버레이 앱이라 콘솔이 작업표시줄에 뜨면 안 된다.
#![windows_subsystem = "windows"]

mod settings;
mod theme;
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

fn main() -> eframe::Result<()> {
    let (tx, rx) = mpsc::channel::<Msg>();

    // 수집 루프 — orbit-core를 돌려 뷰를 UI로 보낸다. 파일 감시 + heartbeat.
    std::thread::spawn(move || {
        let mut collectors = orbit_core::default_collectors();
        let (tick_tx, tick_rx) = mpsc::channel::<()>();
        let _watcher = orbit_core::watch::watch_sources_into(&collectors, tick_tx);
        loop {
            let view = orbit_core::aggregate::collect_all(&mut collectors);
            if tx.send(Msg::View(view)).is_err() {
                break; // UI 종료됨
            }
            let _ = tick_rx.recv_timeout(Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(300)); // 디바운스
            while tick_rx.try_recv().is_ok() {}
        }
    });

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
        Box::new(|cc| Ok(Box::new(HudApp::new(cc, rx)))),
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
    /// 관측 세션이 이미 도는 중이면 중복 실행 방지.
    observing: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl HudApp {
    fn new(cc: &eframe::CreationContext<'_>, rx: mpsc::Receiver<Msg>) -> Self {
        theme::install(&cc.egui_ctx);
        let settings = Settings::load();
        if let Some(z) = settings.zoom {
            cc.egui_ctx.set_zoom_factor(z);
        }
        Self {
            rx,
            view: None,
            menu_open: false,
            last_height: 0.0,
            settings,
            needs_restore: true,
            observing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// 수동 새로고침 — Claude 관측 세션을 1회 돌려 statusline tap을 강제 갱신.
    fn trigger_refresh(&self) {
        use std::sync::atomic::Ordering;
        if self.observing.swap(true, Ordering::SeqCst) {
            return; // 이미 도는 중
        }
        let flag = self.observing.clone();
        std::thread::spawn(move || {
            orbit_core::observer::poll_once(std::time::Duration::from_secs(40));
            flag.store(false, Ordering::SeqCst);
        });
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
        }

        // 새 뷰 수신 (논블로킹).
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::View(v) => self.view = Some(v),
            }
        }

        let sview = ui::SettingsView {
            always_on_top: self.settings.always_on_top,
            pos_locked: self.settings.pos_locked,
            click_through: self.settings.click_through,
            opacity: self.settings.opacity,
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
            && !self.settings.click_through
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
                self.trigger_refresh();
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
                self.settings.click_through = !self.settings.click_through;
                // egui: 이 창을 마우스 입력에 투과시킴.
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(
                    self.settings.click_through,
                ));
                self.settings.save();
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

        // 수집 스레드가 UI를 깨우도록 주기적 리페인트 (나이 표시 갱신 등).
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}
