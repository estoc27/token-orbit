//! 트레이 아이콘 — 표시/숨김·클릭 투과·새로고침·종료.
//!
//! 창이 작업표시줄에 뜨지 않으므로(오버레이), 트레이가 마우스로 닿을 수 있는
//! 유일한 상시 접점이다. 클릭 투과를 켜면 창 자체가 마우스를 안 받기 때문에
//! 여기서 되돌릴 수 있어야 한다 (전역 단축키와 함께 이중 탈출구).

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// 트레이에서 발생한 명령.
pub enum TrayCmd {
    Toggle,
    ClickThrough,
    Refresh,
    Quit,
}

/// 아이콘 비트맵 — 파일 의존 없이 코드로 만든다 (배포 시 리소스 누락 사고 방지).
/// 어두운/밝은 트레이 배경 모두에서 보이도록 초록 링에 흰 중심을 둔다.
fn make_icon() -> Option<Icon> {
    const N: u32 = 32;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    let c = (N as f32 - 1.0) / 2.0;
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            let d = (dx * dx + dy * dy).sqrt();
            // 바깥 링(반지름 9~14)과 중심 점(≤4)만 채운다 — 사용량 게이지 은유.
            let (r, g, b, a) = if (9.0..=14.0).contains(&d) {
                (76, 175, 130, 255) // #4caf82
            } else if d <= 4.0 {
                (236, 236, 236, 255)
            } else {
                (0, 0, 0, 0)
            };
            let i = ((y * N + x) * 4) as usize;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = a;
        }
    }
    Icon::from_rgba(rgba, N, N).ok()
}

/// 트레이를 만들고, 메뉴 이벤트를 `on_cmd`로 넘기는 스레드를 띄운다.
/// 반환된 핸들을 살려둬야 아이콘이 유지된다.
pub fn install(on_cmd: impl Fn(TrayCmd) + Send + 'static) -> Option<TrayIcon> {
    let menu = Menu::new();
    let show = MenuItem::new("표시/숨김", true, None);
    let ghost = MenuItem::new("클릭 투과 (Ctrl+Shift+O)", true, None);
    let refresh = MenuItem::new("지금 새로고침", true, None);
    let quit = MenuItem::new("종료", true, None);
    menu.append(&show).ok()?;
    menu.append(&ghost).ok()?;
    menu.append(&refresh).ok()?;
    menu.append(&tray_icon::menu::PredefinedMenuItem::separator()).ok()?;
    menu.append(&quit).ok()?;

    let (show_id, ghost_id, refresh_id, quit_id) =
        (show.id().clone(), ghost.id().clone(), refresh.id().clone(), quit.id().clone());

    let tray = TrayIconBuilder::new()
        .with_tooltip("Token Orbit")
        .with_menu(Box::new(menu))
        .with_icon(make_icon()?)
        .build()
        .ok()?;

    std::thread::spawn(move || {
        while let Ok(ev) = MenuEvent::receiver().recv() {
            let cmd = if ev.id == show_id {
                TrayCmd::Toggle
            } else if ev.id == ghost_id {
                TrayCmd::ClickThrough
            } else if ev.id == refresh_id {
                TrayCmd::Refresh
            } else if ev.id == quit_id {
                TrayCmd::Quit
            } else {
                continue;
            };
            on_cmd(cmd);
        }
    });

    Some(tray)
}
