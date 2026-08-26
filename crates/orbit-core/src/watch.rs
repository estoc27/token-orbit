//! 소스 파일 감시 — Collector의 `Cadence::Watch` 경로를 실제 파일 감시로 연결한다.
//!
//! 폴링(5초 고정)을 대체하는 목적이지만 **폴링을 완전히 없애지는 않는다**:
//! 감시 등록이 실패하거나(경로 부재, 플랫폼 제약) 이벤트를 놓치는 경우가 있어,
//! 호출자는 항상 heartbeat 타임아웃과 함께 써야 한다 — §4 fail-open.

use crate::collector::{Cadence, Collector};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

/// 감시 핸들. **드롭되면 감시가 멈추므로** 호출자가 살려 둬야 한다.
pub struct SourceWatch {
    _watcher: RecommendedWatcher,
    pub rx: Receiver<()>,
}

/// Collector들이 선언한 경로를 감시하고, 변경 신호를 **호출자가 준 채널로 직접** 보낸다.
/// 반환된 핸들은 감시를 살려두기 위한 것 — 드롭하면 멈춘다.
///
/// 채널을 밖에서 받는 이유: 수신 루프가 이미 자기 채널(수동 새로고침 등)을 갖고 있을 때
/// 중계 스레드를 끼우지 않기 위함이다. 중계층은 그 자체가 실패 지점이 된다.
pub fn watch_sources_into(
    collectors: &[Box<dyn Collector>],
    tx: Sender<()>,
) -> Option<RecommendedWatcher> {
    let mut targets: HashSet<PathBuf> = HashSet::new();
    for c in collectors {
        if let Cadence::Watch { paths } = c.cadence() {
            for p in paths {
                if let Some(t) = watchable_ancestor(&p) {
                    targets.insert(t);
                }
            }
        }
    }
    if targets.is_empty() {
        return None;
    }

    // 이벤트 내용은 쓰지 않는다 — "무언가 바뀌었다"는 신호만 보내고
    // 실제 판단은 Collector가 다시 읽어서 한다 (감시는 트리거일 뿐).
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = tx.send(());
        }
    })
    .ok()?;

    let mut registered = 0usize;
    for t in &targets {
        match watcher.watch(t, RecursiveMode::Recursive) {
            Ok(()) => registered += 1,
            Err(e) => eprintln!("watch 등록 실패 {}: {e}", t.display()),
        }
    }
    eprintln!("watch: {registered}/{} 경로 등록", targets.len());
    if registered == 0 {
        return None;
    }
    Some(watcher)
}

/// 단독 사용(예제·테스트)용 편의 래퍼 — 자체 채널을 만들어 돌려준다.
pub fn watch_sources(collectors: &[Box<dyn Collector>]) -> Option<SourceWatch> {
    let (tx, rx) = channel::<()>();
    let watcher = watch_sources_into(collectors, tx)?;
    Some(SourceWatch { _watcher: watcher, rx })
}

/// 감시 가능한 실제 경로를 고른다.
///
/// 파일 경로는 아직 존재하지 않을 수 있고(예: tap 파일이 첫 생성 전),
/// 존재하더라도 파일 자체를 감시하면 원자적 교체(rename)를 놓친다.
/// 따라서 **디렉터리가 아니면 부모 디렉터리를 감시**하고, 그마저 없으면
/// 존재하는 조상까지 거슬러 올라간다.
fn watchable_ancestor(p: &Path) -> Option<PathBuf> {
    if p.is_dir() {
        return Some(p.to_path_buf());
    }
    let mut cur = p.parent();
    while let Some(dir) = cur {
        if dir.is_dir() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_to_existing_directory() {
        let tmp = std::env::temp_dir();
        // 존재하지 않는 파일 → 존재하는 부모 디렉터리로 해석되어야 한다.
        let missing = tmp.join("token-orbit-no-such-file.json");
        assert_eq!(watchable_ancestor(&missing).as_deref(), Some(tmp.as_path()));
        // 디렉터리는 그대로.
        assert_eq!(watchable_ancestor(&tmp).as_deref(), Some(tmp.as_path()));
    }
}
