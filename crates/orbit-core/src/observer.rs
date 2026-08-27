//! Claude 관측 세션 — HUD가 ConPTY로 Claude Code를 잠깐 열었다 닫아
//! statusline tap을 강제로 갱신시킨다.
//!
//! 배경 (전부 실측):
//! - Claude는 사용률을 로컬에 남기지 않는다. statusline이 유일한 무설정 통로.
//! - statusline은 터미널 세션에서만 돈다. 데스크톱 앱은 실행하지 않는다.
//! - 세션 시작 핸드셰이크가 당시 기준 최신 사용률을 statusline에 싣는다
//!   (채팅 불필요 — 2회 관측). idle 유지는 무의미 (캐시 재전송만 함).
//! - 숨김/최소화 콘솔로는 TUI가 진행되지 않았다. **진짜 PTY가 필요하다** —
//!   그래서 ConPTY(portable-pty). Windows Terminal이 쓰는 그 메커니즘이다.
//!
//! 동작: PTY(120x30)에 claude를 띄우고, tap 파일이 갱신되면 즉시(또는 타임아웃에)
//! 세션을 종료한다. 전용 작업폴더(~/.token-orbit/observer)를 써서 세션 목록
//! 오염을 한 항목으로 가둔다.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn tap_file() -> Option<PathBuf> {
    home().map(|h| h.join(".token-orbit").join("claude-code.json"))
}

fn observer_dir() -> Option<PathBuf> {
    let d = home()?.join(".token-orbit").join("observer");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

/// claude 실행 파일 탐색: 네이티브 설치 → PATH의 셸 심.
fn find_claude() -> Option<PathBuf> {
    if let Some(h) = home() {
        let native = h.join(".local").join("bin").join("claude.exe");
        if native.is_file() {
            return Some(native);
        }
    }
    // PATH 검색 (claude.exe / claude.cmd)
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["claude.exe", "claude.cmd"] {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn mtime(p: &PathBuf) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// 관측 세션 1회 실행. tap이 갱신되면 true.
///
/// 주의: PTY reader를 소비하는 스레드가 반드시 필요하다 — 출력 버퍼가 차면
/// TUI가 멈춘다. 종료는 child.kill()로 확실히 한다 (세션을 남겨두지 않는다).
pub fn poll_once(timeout: Duration) -> bool {
    let Some(exe) = find_claude() else {
        eprintln!("observer: claude 실행 파일을 찾지 못함 — 건너뜀");
        return false;
    };
    let (Some(tap), Some(cwd)) = (tap_file(), observer_dir()) else { return false };
    let before = mtime(&tap);

    let pty = native_pty_system();
    let Ok(pair) = pty.openpty(PtySize { rows: 30, cols: 120, pixel_width: 0, pixel_height: 0 })
    else {
        eprintln!("observer: PTY 생성 실패");
        return false;
    };

    let mut cmd = CommandBuilder::new(&exe);
    cmd.cwd(&cwd);
    // 참고(실측, 2026-08-27): 핸드셰이크의 rate_limits는 ANTHROPIC_BASE_URL 경유
    // 트래픽이 아니다 — 프록시 경유 + --model fable로 띄워도 unified 헤더가 전혀
    // 채집되지 않았다. 따라서 관측 세션이 갱신하는 것은 statusline의 5h/7d뿐이고,
    // 모델 전용 창(7d Fable)은 실제 Fable 요청이 프록시를 지날 때만 갱신된다.
    let Ok(mut child) = pair.slave.spawn_command(cmd) else {
        eprintln!("observer: claude 기동 실패");
        return false;
    };
    drop(pair.slave);

    // 출력 소비 — 없으면 파이프가 차서 TUI가 멈춘다.
    // 앞부분 64KB는 진단용으로 남긴다 (observer-screen.log): statusline이 안 뜰 때
    // claude가 실제로 무엇을 그리고 있는지 보는 유일한 창구다.
    if let Ok(mut reader) = pair.master.try_clone_reader() {
        let log = home().map(|h| h.join(".token-orbit").join("observer-screen.log"));
        std::thread::spawn(move || {
            let mut file = log.and_then(|p| std::fs::File::create(p).ok());
            let mut written = 0usize;
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if written < 65536 {
                    if let Some(f) = file.as_mut() {
                        use std::io::Write;
                        let _ = f.write_all(&buf[..n.min(65536 - written)]);
                        let _ = f.flush();
                    }
                    written += n;
                }
            }
        });
    }

    // 첫 실행 시 자기 작업폴더(~/.token-orbit/observer)에 대한 신뢰 프롬프트가 뜬다
    // (실측: 화면 덤프로 확인). 기본 선택지가 "Yes, I trust"이므로 Enter로 수락한다.
    // 이 폴더는 Token Orbit이 만든 빈 전용 폴더라 수락이 안전하며, Claude가 수락을
    // 기록하므로 이후 폴에서는 프롬프트 자체가 나타나지 않는다.
    let mut writer = pair.master.take_writer().ok();

    let start = Instant::now();
    let mut updated = false;
    let mut enters_sent = 0u8;
    while start.elapsed() < timeout {
        // 성공 조건: 파일이 새로 쓰였고 **rate_limits가 실제로 담겨 있을 것**.
        // 첫 statusline 렌더는 핸드셰이크 완료 전이라 rate_limits가 비어 있다(실측 7초
        // 시점) — mtime만 보고 죽이면 빈 페이로드에서 멈춘다. 몇 초 더 기다려야 한다.
        if mtime(&tap) != before
            && std::fs::read_to_string(&tap)
                .map(|t| t.contains("\"five_hour\""))
                .unwrap_or(false)
        {
            updated = true;
            break;
        }
        // 5초·10초 시점에 Enter — 프롬프트가 없으면 빈 입력이라 무해하다.
        let due = match enters_sent {
            0 => start.elapsed() >= Duration::from_secs(5),
            1 => start.elapsed() >= Duration::from_secs(10),
            _ => false,
        };
        if due {
            if let Some(w) = writer.as_mut() {
                use std::io::Write;
                let _ = w.write_all(b"\r");
                let _ = w.flush();
            }
            enters_sent += 1;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let _ = child.kill();
    let _ = child.wait();
    drop(pair.master);
    eprintln!(
        "observer: {} ({}초)",
        if updated { "tap 갱신 성공" } else { "타임아웃" },
        start.elapsed().as_secs()
    );
    updated
}
