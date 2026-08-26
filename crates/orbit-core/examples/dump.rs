//! 실데이터 검증용: 이 머신의 실제 소스를 읽어 AggregatedView를 JSON으로 출력.
//! 사용: cargo run -p orbit-core --example dump

fn main() {
    let mut collectors = orbit_core::default_collectors();
    let view = orbit_core::aggregate::collect_all(&mut collectors);
    println!("{}", serde_json::to_string_pretty(&view).unwrap());
}
