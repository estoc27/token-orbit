//! 카드 렌더 — AggregatedView를 egui 위젯으로 그린다.
//! 기존 HTML/CSS 렌더러(ui/main.js)의 egui 이식.

use crate::theme;
use eframe::egui::{self, Margin, RichText, Rounding, Stroke};
use orbit_core::aggregate::{AggregatedView, ServiceCard, WindowView};

pub fn render(ui: &mut egui::Ui, view: Option<&AggregatedView>) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            match view {
                None => placeholder(ui, "수집 대기 중…"),
                Some(v) if v.cards.is_empty() => placeholder(ui, "감지된 소스 없음"),
                Some(v) => {
                    for card in &v.cards {
                        card_ui(ui, card);
                    }
                }
            }
        });
}

fn placeholder(ui: &mut egui::Ui, text: &str) {
    card_frame(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(text).color(theme::TEXT_FAINT).size(12.0));
        });
    });
}

/// 카드 배경 프레임 (둥근 모서리 + 반투명 배경 + 얇은 테두리).
fn card_frame<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(theme::CARD_BG)
        .stroke(Stroke::new(1.0, theme::CARD_BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::symmetric(10.0, 8.0))
        .show(ui, add)
        .inner
}

fn card_ui(ui: &mut egui::Ui, card: &ServiceCard) {
    let stale = card.data_age_secs > 30 * 60;
    card_frame(ui, |ui| {
        ui.set_width(ui.available_width());

        // 헤드: 서비스명 · 플랜 배지 · 나이
        ui.horizontal(|ui| {
            ui.label(RichText::new(&card.display_name).color(theme::TEXT).size(13.0).strong());
            if let Some(plan) = &card.plan {
                badge(ui, plan);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(age_label(card.data_age_secs)).color(theme::TEXT_FAINT).size(10.0));
            });
        });
        ui.add_space(3.0);

        for (i, w) in card.windows.iter().enumerate() {
            window_ui(ui, w, i == 0, stale);
        }

        // 앵커 이후 소모 감지
        if card.activity_after_percent {
            ui.add_space(2.0);
            ui.label(
                RichText::new("▲ 이후 사용 감지 — 실제 사용률은 표시보다 높음")
                    .color(theme::BAR_WARN)
                    .size(10.0),
            );
        }

        // 크레딧 잔액
        if let Some(usd) = card.credits_usd {
            ui.add_space(2.0);
            ui.label(RichText::new(format!("크레딧 잔액 ${usd:.2}")).color(theme::TEXT_DIM).size(11.0));
        }
    });
}

fn window_ui(ui: &mut egui::Ui, w: &WindowView, major: bool, card_stale: bool) {
    ui.add_space(if major { 0.0 } else { 4.0 });

    // 게이지 바 + 퍼센트
    ui.horizontal(|ui| {
        let bar_h = if major { 8.0 } else { 4.0 };
        let full = ui.available_width() - 40.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(full, bar_h), egui::Sense::hover());
        let painter = ui.painter();
        let r = Rounding::same(bar_h / 2.0);
        painter.rect_filled(rect, r, theme::BAR_TRACK);
        let pct = (w.used_percent / 100.0).clamp(0.0, 1.0) as f32;
        if pct > 0.0 {
            let fill = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * pct, bar_h));
            painter.rect_filled(fill, r, theme::bar_color(w.used_percent));
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!("{}%", w.used_percent.round() as i64))
                .color(theme::TEXT)
                .size(12.0)
                .strong(),
        );
    });

    // 메타: "5h · 잔량 46% · 1시간 후 리셋 · [나이]"
    let remaining = (100 - w.used_percent.round() as i64).max(0);
    let mut meta = format!("{} · 잔량 {}%", w.label, remaining);
    if let Some(secs) = w.resets_in_secs {
        meta.push_str(&format!(" · {} 후 리셋", dur_label(secs)));
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(meta).color(theme::TEXT_DIM).size(10.0));
        if w.age_secs > 300 && !card_stale {
            ui.label(RichText::new(format!("· {}", age_label(w.age_secs))).color(theme::BAR_WARN).size(10.0));
        }
    });

    // 모델 전용 주간 창 고정 안내
    if w.label.starts_with("7d ") {
        ui.label(RichText::new("주간 임계 50%").color(theme::ACCENT).size(9.0));
    }
}

fn badge(ui: &mut egui::Ui, text: &str) {
    egui::Frame::none()
        .fill(theme::BADGE_BG)
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::symmetric(6.0, 1.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(theme::TEXT).size(10.0));
        });
}

// ---- helpers ----
fn age_label(secs: i64) -> String {
    if secs < 90 {
        "방금".into()
    } else if secs < 3600 {
        format!("{}분 전", secs / 60)
    } else {
        format!("{}시간 전", secs / 3600)
    }
}

fn dur_label(secs: i64) -> String {
    if secs <= 0 {
        return "곧".into();
    }
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d}일 {h}시간")
    } else if h > 0 {
        format!("{h}시간 {m}분")
    } else {
        format!("{m}분")
    }
}
