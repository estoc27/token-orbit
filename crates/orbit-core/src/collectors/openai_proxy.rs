//! T1: OpenAI API 프록시 탭 Collector — `scripts/proxy-tap.js`의 openai 마운트가 기록한
//! `~/.token-orbit/openai-headers.json`을 읽는다.
//!
//! OpenAI API의 `x-ratelimit-*` 헤더는 **분 단위 롤링 제한**(RPM/TPM)이다.
//! Codex/Claude의 시간·일 단위 창과 성격이 다르지만, API 키 사용자가 한도에
//! 얼마나 가까운지는 동일한 프레임(% + 리셋)으로 보여줄 수 있다.
//!
//! 파일 형태 (탭이 누적 기록):
//! ```jsonc
//! { "buckets": {
//!     "requests": { "limit": "500",   "remaining": "499",   "reset_after": "120ms", "observed_at": 1787725026 },
//!     "tokens":   { "limit": "30000", "remaining": "29500", "reset_after": "1s",    "observed_at": 1787725026 }
//! } }
//! ```
//!
//! ⚠️ 이 Collector는 공개 문서의 헤더 스키마로 작성됐고, 실트래픽 검증은
//! 사용자가 OPENAI_BASE_URL을 프록시로 걸었을 때 이뤄진다. 파일이 없으면
//! NotConfigured로 조용히 빠지므로 미사용자에게는 보이지 않는다.

use crate::collector::{Cadence, CollectError, Collector};
use crate::collectors::codex::dirs_home;
use crate::model::*;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// 분 단위 제한이라 신선도 기준도 짧다.
const STALE_AFTER_SECS: i64 = 30 * 60;

pub struct OpenAiProxyCollector {
    file: Option<PathBuf>,
    last_health: Health,
}

impl OpenAiProxyCollector {
    pub fn new() -> Self {
        let file = dirs_home().map(|h| h.join(".token-orbit").join("openai-headers.json"));
        Self { file, last_health: Health::NotConfigured }
    }
}

impl Default for OpenAiProxyCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for OpenAiProxyCollector {
    fn id(&self) -> &'static str {
        "openai-api-proxy"
    }
    fn display_name(&self) -> &'static str {
        "OpenAI API"
    }
    fn service_key(&self) -> &'static str {
        "openai-api"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { tokens: false, percent: true, reset_time: true, plan_name: false, cost: false }
    }
    fn cadence(&self) -> Cadence {
        match &self.file {
            Some(p) => Cadence::Watch { paths: vec![p.clone()] },
            None => Cadence::Poll { secs: 300 },
        }
    }

    fn collect(&mut self) -> Result<Vec<Snapshot>, CollectError> {
        let Some(path) = self.file.as_ref().filter(|p| p.is_file()) else {
            self.last_health = Health::NotConfigured;
            return Err(CollectError::NotConfigured("no openai traffic observed".into()));
        };
        let v: Value = serde_json::from_str(&fs::read_to_string(path)?)
            .map_err(|e| CollectError::Parse(e.to_string()))?;
        let snaps = snapshots_from_buckets(&v);

        let newest = snaps.iter().map(|s| s.observed_at).max();
        self.last_health = match newest {
            None => Health::Degraded { reason: "no rate limit buckets".into() },
            Some(o) if now_epoch() - o > STALE_AFTER_SECS => {
                Health::Degraded { reason: format!("openai data is {}s old", now_epoch() - o) }
            }
            Some(_) => Health::Ok,
        };
        Ok(snaps)
    }

    fn health(&self) -> Health {
        self.last_health.clone()
    }

    fn authority(&self) -> u8 {
        10
    }
}

fn snapshots_from_buckets(v: &Value) -> Vec<Snapshot> {
    let Some(buckets) = v.get("buckets").and_then(|b| b.as_object()) else { return Vec::new() };

    let mut out = Vec::new();
    for (kind, b) in buckets {
        let label = match kind.as_str() {
            "requests" => "RPM",
            "tokens" => "TPM",
            _ => continue,
        };
        let (Some(limit), Some(remaining)) = (b.get("limit").and_then(num), b.get("remaining").and_then(num))
        else {
            continue;
        };
        if limit <= 0.0 {
            continue;
        }
        let used_percent = ((limit - remaining) / limit * 100.0).clamp(0.0, 100.0);
        let observed_at = b.get("observed_at").and_then(|o| o.as_i64()).unwrap_or_else(now_epoch);
        // reset_after는 "1s"/"6m0s" 같은 상대 시간 — 관측 시각 기준 절대 시각으로 환산.
        let resets_at = b
            .get("reset_after")
            .and_then(|r| r.as_str())
            .and_then(parse_duration_secs)
            .map(|d| observed_at + d);

        out.push(Snapshot {
            source_id: "openai-api-proxy".into(),
            account: "default".into(),
            metric: Metric::Percent { used_percent },
            window: Window::Rolling { minutes: 1 },
            limit: Limit::Known { value: 100.0 },
            resets_at,
            confidence: Confidence::Exact,
            observed_at,
            plan: None,
            label: Some(label.into()),
        });
    }
    out
}

fn num(v: &Value) -> Option<f64> {
    v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64())
}

/// OpenAI의 상대 시간 표기("120ms", "1s", "6m0s", "1h2m3s") → 초.
fn parse_duration_secs(s: &str) -> Option<i64> {
    let mut total = 0f64;
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            cur.push(c);
        } else {
            let n: f64 = cur.parse().ok()?;
            cur.clear();
            match c {
                'h' => total += n * 3600.0,
                'm' => {
                    // "ms"와 "m"(분) 구분
                    if chars.peek() == Some(&'s') {
                        chars.next();
                        total += n / 1000.0;
                    } else {
                        total += n * 60.0;
                    }
                }
                's' => total += n,
                _ => return None,
            }
        }
    }
    if !cur.is_empty() {
        return None; // 단위 없는 잔여 숫자
    }
    Some(total.ceil() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_buckets() {
        let v: Value = serde_json::from_str(
            r#"{"buckets":{
                "requests":{"limit":"500","remaining":"499","reset_after":"120ms","observed_at":1000},
                "tokens":{"limit":"30000","remaining":"22500","reset_after":"6m0s","observed_at":1000}
            }}"#,
        )
        .unwrap();
        let mut snaps = snapshots_from_buckets(&v);
        snaps.sort_by(|a, b| a.label.cmp(&b.label));
        assert_eq!(snaps.len(), 2);

        let rpm = &snaps[0]; // "RPM"
        match rpm.metric {
            Metric::Percent { used_percent } => assert!((used_percent - 0.2).abs() < 1e-9),
            ref o => panic!("{o:?}"),
        }
        assert_eq!(rpm.resets_at, Some(1001)); // 120ms → ceil 1초

        let tpm = &snaps[1]; // "TPM": (30000-22500)/30000 = 25%
        match tpm.metric {
            Metric::Percent { used_percent } => assert!((used_percent - 25.0).abs() < 1e-9),
            ref o => panic!("{o:?}"),
        }
        assert_eq!(tpm.resets_at, Some(1360)); // 6m = 360초
    }

    #[test]
    fn duration_parsing() {
        assert_eq!(parse_duration_secs("1s"), Some(1));
        assert_eq!(parse_duration_secs("6m0s"), Some(360));
        assert_eq!(parse_duration_secs("1h2m3s"), Some(3723));
        assert_eq!(parse_duration_secs("120ms"), Some(1));
        assert_eq!(parse_duration_secs("bogus"), None);
    }
}
