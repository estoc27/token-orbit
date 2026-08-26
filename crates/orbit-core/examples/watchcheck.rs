//! 파일 감시 동작 확인: 실제 소스 경로를 감시하고 이벤트 수신을 보고한다.
//! 사용: cargo run -p orbit-core --example watchcheck

use std::time::{Duration, Instant};

fn main() {
    let collectors = orbit_core::default_collectors();
    let Some(watch) = orbit_core::watch::watch_sources(&collectors) else {
        println!("watch: 등록된 경로 없음 (None)");
        return;
    };
    println!("watch: 등록됨. 10초간 이벤트 수신 대기...");

    let start = Instant::now();
    let mut n = 0usize;
    while start.elapsed() < Duration::from_secs(10) {
        match watch.rx.recv_timeout(Duration::from_millis(500)) {
            Ok(_) => {
                n += 1;
                println!("  [{:>5}ms] 변경 이벤트 #{n}", start.elapsed().as_millis());
            }
            Err(_) => {}
        }
    }
    println!("총 {n}건 수신");
}
