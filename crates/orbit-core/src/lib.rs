//! orbit-core — Token_Orbit의 수집·집계 코어.
//!
//! UI(Tauri)와 완전히 분리된 순수 Rust 크레이트. `cargo test -p orbit-core` 로
//! 셸 없이 단독 검증 가능하며, M3의 macOS 포팅 시 이 크레이트는 그대로 재사용된다.

pub mod aggregate;
pub mod collector;
pub mod collectors;
pub mod model;
pub mod observer;
pub mod watch;

use collector::Collector;

/// M0 기본 Collector 셋. 존재하지 않는 소스는 각자 NotConfigured로 보고한다.
pub fn default_collectors() -> Vec<Box<dyn Collector>> {
    vec![
        Box::new(collectors::codex::CodexCollector::new()),
        Box::new(collectors::claude_proxy::ClaudeProxyCollector::new()),
        Box::new(collectors::claude_statusline::ClaudeStatuslineCollector::new()),
        Box::new(collectors::claude_jsonl::ClaudeJsonlCollector::new()),
        Box::new(collectors::openai_proxy::OpenAiProxyCollector::new()),
    ]
}
