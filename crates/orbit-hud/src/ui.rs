//! 카드 렌더 — AggregatedView를 egui 위젯으로 그린다.
//! 기존 HTML/CSS 렌더러(ui/main.js)의 egui 이식.

use crate::theme;
use eframe::egui::{self, Color32, Margin, RichText, Rounding, Stroke};
use orbit_core::aggregate::{AggregatedView, ServiceCard, WindowView};

/// UI에서 발생한 사용자 액션 — main이 처리한다.
#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Refresh,
    ToggleMenu,
    ToggleAlwaysOnTop,
    TogglePosLock,
    ToggleClickThrough,
    SetOpacity(f32),
    Quit,
}

/// render 결과 — 액션과, 자동 높이 조절용 실제 콘텐츠 높이.
pub struct RenderResult {
    pub action: Option<Action>,
    pub content_height: f32,
    /// 우하단 그립을 드래그한 누적 델타(x). 폭 조절에 쓴다.
    pub resize_dx: Option<f32>,
}

/// 현재 설정 스냅샷 — 메뉴 체크 표시용.
#[derive(Clone, Copy)]
pub struct SettingsView {
    pub always_on_top: bool,
    pub pos_locked: bool,
    pub click_through: bool,
    pub opacity: f32,
}

/// 한 프레임을 그리고, 클릭된 액션과 콘텐츠 높이를 반환한다.
pub fn render(
    ui: &mut egui::Ui,
    view: Option<&AggregatedView>,
    menu_open: bool,
    settings: SettingsView,
) -> RenderResult {
    OPACITY.with(|o| o.set(settings.opacity));
    let mut action = None;

    // 상단 바 — 우측 정렬 아이콘 (⚙ 메뉴, ↻ 새로고침)
    let mut gear_rect = egui::Rect::NOTHING;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let gear = icon_button(ui, "⚙");
            gear_rect = gear.rect;
            if gear.clicked() {
                action = Some(Action::ToggleMenu);
            }
            if icon_button(ui, "⟳").clicked() {
                action = Some(Action::Refresh);
            }
        });
    });

    // 메뉴 — ⚙ **오른쪽**에 뜨는 팝업 (Area로 레이아웃 흐름 밖에 배치).
    if menu_open {
        let menu_pos = egui::pos2(gear_rect.right() + 4.0, gear_rect.top());
        egui::Area::new(ui.id().with("menu"))
            .fixed_pos(menu_pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                if let Some(a) = menu_ui(ui, settings) {
                    action = Some(a);
                }
            });
    }

    // 스크롤 없이 세로로 쌓는다 — 창이 내용 높이에 맞춰 자동 조절되므로.
    match view {
        None => placeholder(ui, "수집 대기 중…"),
        Some(v) if v.cards.is_empty() => placeholder(ui, "감지된 소스 없음"),
        Some(v) => {
            for card in &v.cards {
                card_ui(ui, card);
            }
        }
    }

    let content_height = ui.next_widget_position().y - ui.min_rect().top();

    // 우하단 그립 — 유일한 리사이즈 지점.
    // 콘텐츠 하단(마지막 카드 아래) 우측 안쪽에 둔다. 창 클라이언트 영역의 max를
    // 그대로 쓰면 카드 외곽선 밖으로 나가므로(실측), 안쪽으로 4px 당긴다.
    let clip = ui.clip_rect();
    let grip_size = egui::vec2(14.0, 14.0);
    let grip_max = egui::pos2(clip.right() - 4.0, content_height + ui.min_rect().top() - 2.0);
    let grip_rect = egui::Rect::from_min_size(grip_max - grip_size, grip_size);
    let grip = ui.interact(grip_rect, ui.id().with("grip"), egui::Sense::drag());
    let color = if grip.hovered() || grip.dragged() { theme::ICON_HOVER } else { theme::ICON };
    // ◢ 대각선 삼각형
    let p = ui.painter();
    let (r, b) = (grip_rect.right(), grip_rect.bottom());
    for i in 0..3 {
        let o = i as f32 * 3.5;
        p.line_segment([egui::pos2(r - o, b), egui::pos2(r, b - o)], Stroke::new(1.5, color));
    }
    if grip.hovered() || grip.dragged() {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeNwSe);
    }
    let resize_dx = grip.dragged().then(|| grip.drag_delta().x);

    RenderResult { action, content_height, resize_dx }
}

fn icon_button(ui: &mut egui::Ui, glyph: &str) -> egui::Response {
    let resp = ui.add(
        egui::Button::new(RichText::new(glyph).size(14.0).color(theme::ICON))
            .frame(false)
            .min_size(egui::vec2(20.0, 16.0)),
    );
    if resp.hovered() {
        ui.painter().text(
            resp.rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::proportional(14.0),
            theme::ICON_HOVER,
        );
    }
    resp
}

fn menu_ui(ui: &mut egui::Ui, s: SettingsView) -> Option<Action> {
    let mut action = None;
    egui::Frame::none()
        .fill(Color32::from_rgb(28, 28, 34))
        .stroke(Stroke::new(1.0, theme::CARD_BORDER))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::same(5.0))
        .show(ui, |ui| {
            ui.set_min_width(160.0);
            let mark = |on: bool| if on { "☑" } else { "☐" };

            if menu_item(ui, &format!("{} 항상 위", mark(s.always_on_top))) {
                action = Some(Action::ToggleAlwaysOnTop);
            }
            if menu_item(ui, &format!("{} 위치 잠금", mark(s.pos_locked))) {
                action = Some(Action::TogglePosLock);
            }
            if menu_item(ui, &format!("{} 클릭 투과", mark(s.click_through))) {
                action = Some(Action::ToggleClickThrough);
            }

            ui.add_space(4.0);
            ui.label(RichText::new("투명도").size(9.0).color(theme::TEXT_FAINT));
            let mut op = s.opacity;
            if ui
                .add(egui::Slider::new(&mut op, 0.3..=1.0).show_value(false))
                .changed()
            {
                action = Some(Action::SetOpacity(op));
            }

            ui.add_space(4.0);
            if menu_item(ui, "↻ 지금 새로고침") {
                action = Some(Action::Refresh);
            }
            if menu_item(ui, "✕ 종료") {
                action = Some(Action::Quit);
            }
        });
    action
}

fn menu_item(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(
        egui::Button::new(RichText::new(text).size(11.0).color(theme::TEXT))
            .frame(false)
            .min_size(egui::vec2(ui.available_width(), 20.0)),
    )
    .clicked()
}

fn placeholder(ui: &mut egui::Ui, text: &str) {
    card_frame(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(text).color(theme::TEXT_FAINT).size(12.0));
        });
    });
}

// 현재 프레임의 카드 불투명도 — render 진입 시 설정한다 (thread-local).
thread_local! {
    static OPACITY: std::cell::Cell<f32> = const { std::cell::Cell::new(0.86) };
}

/// 카드 배경 프레임 (둥근 모서리 + 반투명 배경 + 얇은 테두리).
fn card_frame<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let a = (OPACITY.with(|o| o.get()) * 255.0) as u8;
    let bg = Color32::from_rgba_unmultiplied(20, 20, 24, a);
    egui::Frame::none()
        .fill(bg)
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
            ui.label(RichText::new(text).color(theme::BADGE_TEXT).size(10.0).strong());
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
