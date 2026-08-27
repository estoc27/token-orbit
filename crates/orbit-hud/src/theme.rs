//! HUD 색·간격 토큰 — 기존 CSS 디자인을 egui로 옮긴 것.

use eframe::egui::{self, Color32};

pub const CARD_BG: Color32 = Color32::from_rgba_premultiplied(20, 20, 24, 219); // .86 alpha
pub const CARD_BORDER: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 26);
pub const TEXT: Color32 = Color32::from_rgb(236, 236, 236);
pub const TEXT_DIM: Color32 = Color32::from_rgb(170, 170, 170);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(120, 120, 120);

pub const BAR_TRACK: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 26);
pub const BAR_OK: Color32 = Color32::from_rgb(76, 175, 130); // #4caf82
pub const BAR_WARN: Color32 = Color32::from_rgb(230, 180, 34); // #e6b422
pub const BAR_CRIT: Color32 = Color32::from_rgb(224, 93, 93); // #e05d5d

// 플랜 배지 — 흰 반투명 위 흰 글씨는 안 읽혔다. 채도 있는 배경 + 밝은 글씨로 대비 확보.
pub const BADGE_BG: Color32 = Color32::from_rgb(58, 60, 74);
pub const BADGE_TEXT: Color32 = Color32::from_rgb(198, 210, 255);
pub const ACCENT: Color32 = Color32::from_rgb(216, 180, 254); // 보라 (Fable note)

// 상단 바 아이콘
pub const ICON: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 130);
pub const ICON_HOVER: Color32 = Color32::from_rgb(255, 255, 255);

pub const WARN_PCT: f64 = 80.0;
pub const CRIT_PCT: f64 = 90.0;

/// 사용률 %에 맞는 게이지 색.
pub fn bar_color(used_percent: f64) -> Color32 {
    if used_percent >= CRIT_PCT {
        BAR_CRIT
    } else if used_percent >= WARN_PCT {
        BAR_WARN
    } else {
        BAR_OK
    }
}

/// 내장 한글 폰트 — Noto Sans KR(SIL OFL 1.1)에서 이 앱이 쓰는 글자만 뽑은 서브셋.
/// 재생성: `python tools/make-font-subset.py` (한글 문구를 추가·수정한 뒤에 필요).
const KOREAN_SUBSET: &[u8] = include_bytes!("../assets/NotoSansKR-subset.ttf");

/// 한글 폰트 등록 — egui 기본 폰트엔 한글 글리프가 없어 □로 깨진다.
///
/// 시스템 폰트(맑은 고딕 12.8MB)를 런타임에 읽던 방식은 프로세스 메모리를
/// 36.8MB 먹었고(실측), Windows 폰트 경로에도 묶여 있었다. 147KB 서브셋을
/// 바이너리에 넣어 둘 다 없앤다 — 어느 OS에서도 글꼴이 동일하다.
fn install_korean_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "korean".to_owned(),
        egui::FontData::from_static(KOREAN_SUBSET).into(),
    );
    // 두 계열(비례/고정폭) 모두 한글을 폴백으로 추가 — 라틴은 기존 폰트가 우선.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("korean".to_owned());
    }
    ctx.set_fonts(fonts);
}

pub fn install(ctx: &egui::Context) {
    install_korean_font(ctx);
    let mut style = (*ctx.style()).clone();
    style.visuals.override_text_color = Some(TEXT);
    // 위젯 배경을 투명하게 — 우리가 카드를 직접 그린다.
    style.visuals.panel_fill = Color32::TRANSPARENT;
    style.visuals.window_fill = Color32::TRANSPARENT;
    style.spacing.item_spacing = egui::vec2(0.0, 6.0);
    ctx.set_style(style);
}
