//! T2: Claude Code 세션 JSONL Collector — 토큰 절대량 담당.
//!
//! 실측 (2026-08-26): `~/.claude/projects/<slug>/<uuid>.jsonl` 의 usage 필드:
//! ```jsonc
//! "usage": { "input_tokens": 2, "cache_creation_input_tokens": 85,
//!            "cache_read_input_tokens": 126621, "output_tokens": 1074 }
//! ```
//! limit/percent/reset 계열 키는 전수 조사 결과 없음 — 퍼센트는 T0(statusline) 담당.
//! 역할 분담: 이 Collector는 토큰만, 한도는 `Limit::Unknown` 고정.

use crate::collector::{Cadence, CollectError, Collector};
use crate::collectors::codex::dirs_home;
use crate::model::*;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

pub struct ClaudeJsonlCollector {
    projects_dir: Option<PathBuf>,
    last_health: Health,
}

impl ClaudeJsonlCollector {
    pub fn new() -> Self {
        let projects_dir = dirs_home().map(|h| h.join(".claude").join("projects"));
        let projects_dir = projects_dir.filter(|p| p.is_dir());
        let last_health = if projects_dir.is_some() { Health::Ok } else { Health::NotConfigured };
        Self { projects_dir, last_health }
    }

    /// mtime 최신 세션 파일 하나 (= 현재 활성 세션일 가능성이 가장 높은 파일).
    fn newest_session_file(&self) -> Option<PathBuf> {
        let dir = self.projects_dir.as_ref()?;
        let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
        for proj in fs::read_dir(dir).ok()?.flatten() {
            let Ok(files) = fs::read_dir(proj.path()) else { continue };
            for f in files.flatten() {
                let p = f.path();
                if p.extension().map(|x| x == "jsonl").unwrap_or(false) {
                    if let Ok(mtime) = f.metadata().and_then(|m| m.modified()) {
                        if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                            best = Some((mtime, p));
                        }
                    }
                }
            }
        }
        best.map(|(_, p)| p)
    }
}

impl Default for ClaudeJsonlCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for ClaudeJsonlCollector {
    fn id(&self) -> &'static str {
        "claude-code-jsonl"
    }
    fn display_name(&self) -> &'static str {
        "Claude"
    }
    fn service_key(&self) -> &'static str {
        "claude"
    }
    fn capabilities(&self) -> Capabilities {
        // cost: false — 모델별 단가표 없이는 USD 환산 불가. M0 후반 TODO (README §7).
        Capabilities { tokens: true, percent: false, reset_time: false, plan_name: false, cost: false }
    }
    fn cadence(&self) -> Cadence {
        match &self.projects_dir {
            Some(p) => Cadence::Watch { paths: vec![p.clone()] },
            None => Cadence::Poll { secs: 300 },
        }
    }

    fn collect(&mut self) -> Result<Vec<Snapshot>, CollectError> {
        let Some(file) = self.newest_session_file() else {
            self.last_health = Health::NotConfigured;
            return Err(CollectError::NotConfigured("no claude session files".into()));
        };
        // 세션 파일은 수 MB 수준 — M0 뼈대는 전체 읽기. 오프셋 추적은 감시 연결 시 함께 도입.
        let text = fs::read_to_string(&file)?;
        let totals = sum_usage(&text);
        // 관측 시각 = 파일 mtime. "마지막 소모가 언제였나"의 신호로 쓰인다
        // (Aggregator가 앵커(%)와 비교해 '이후 사용 있음'을 판정 — §3 Staleness).
        let observed_at = fs::metadata(&file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or_else(now_epoch);
        self.last_health = Health::Ok;
        Ok(vec![Snapshot {
            source_id: "claude-code-jsonl".into(),
            account: "default".into(),
            metric: Metric::Tokens {
                input: totals.input,
                output: totals.output,
                cache_read: totals.cache_read,
                cache_write: totals.cache_write,
            },
            window: Window::Session,
            limit: Limit::Unknown, // 퍼센트 바를 그리지 말 것 — §T2
            resets_at: None,
            confidence: Confidence::Exact,
            observed_at,
            plan: None,
            label: None,
        }])
    }

    fn health(&self) -> Health {
        self.last_health.clone()
    }
}

#[derive(Default)]
struct Totals {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

fn sum_usage(text: &str) -> Totals {
    let mut t = Totals::default();
    for line in text.lines() {
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(u) = find_usage(&v) else { continue };
        let g = |k: &str| u.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        t.input += g("input_tokens");
        t.output += g("output_tokens");
        t.cache_read += g("cache_read_input_tokens");
        t.cache_write += g("cache_creation_input_tokens");
    }
    t
}

/// usage 객체를 재귀 탐색. input_tokens 키를 가진 객체만 인정한다
/// (대화 텍스트 안의 "usage" 문자열 오탐 방지 — 조사 중 실제로 겪은 함정).
fn find_usage(v: &Value) -> Option<&Value> {
    match v {
        Value::Object(m) => {
            if let Some(u) = m.get("usage") {
                if u.get("input_tokens").is_some() {
                    return Some(u);
                }
            }
            m.values().find_map(find_usage)
        }
        Value::Array(a) => a.iter().find_map(find_usage),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_usage_lines() {
        let text = concat!(
            r#"{"type":"assistant","message":{"usage":{"input_tokens":2,"cache_creation_input_tokens":85,"cache_read_input_tokens":126621,"output_tokens":1074}}}"#, "\n",
            r#"{"type":"user","content":"the word usage appears in text"}"#, "\n",
            r#"{"type":"assistant","message":{"usage":{"input_tokens":3,"output_tokens":10}}}"#, "\n",
        );
        let t = sum_usage(text);
        assert_eq!(t.input, 5);
        assert_eq!(t.output, 1084);
        assert_eq!(t.cache_read, 126621);
        assert_eq!(t.cache_write, 85);
    }
}
