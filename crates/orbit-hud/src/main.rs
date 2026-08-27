//! Token Orbit HUD — egui 네이티브 렌더러.
//!
//! Tauri/WebView2(~330MB) 대체. 같은 `orbit-core`를 소비하며, WebView 없이
//! GPU로 직접 그린다. 목표: 투명·최상단·무테두리 오버레이, 수집은 백그라운드 스레드.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod theme;
mod ui;

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
}

impl HudApp {
    fn new(cc: &eframe::CreationContext<'_>, rx: mpsc::Receiver<Msg>) -> Self {
        theme::install(&cc.egui_ctx);
        Self { rx, view: None }
    }
}

impl eframe::App for HudApp {
    // 창 밖(프레임버퍼)을 투명하게 — 무테두리 오버레이의 핵심.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 새 뷰 수신 (논블로킹).
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::View(v) => self.view = Some(v),
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none()) // 패널 자체 배경 없음 — 카드만 불투명
            .show(ctx, |ui_ctx| {
                ui::render(ui_ctx, self.view.as_ref());
            });

        // 수집 스레드가 UI를 깨우도록 주기적 리페인트 (나이 표시 갱신 등).
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}
