//! T1: Claude 프록시 탭 Collector — `scripts/proxy-tap.js`가 기록한 헤더를 읽는다.
//!
//! 실측 헤더 (2026-08-26, `anthropic-ratelimit-unified-` 접두어 제거 후):
//! ```jsonc
//! {
//!   "5h-utilization": "0.46",   "5h-reset": "1787730600",
//!   "7d-utilization": "0.09",   "7d-reset": "1788195600",
//!   "7d_oi-utilization": "0.16","7d_oi-reset": "1788195600",  // ← 최상위 모델(Fable) 창
//!   "fallback-percentage": "0.5",
//!   "overage-status": "rejected", "overage-disabled-reason": "out_of_credits"
//! }
//! ```
//!
//! statusline(T0) 대비:
//! - **모델 전용 창(7d_oi)을 준다.** statusline 페이로드엔 없다.
//! - **계정 실시간 상태**다. statusline은 그 세션의 마지막 응답이라 idle이면 얼어붙는다.
//!
//! 파일은 트래픽이 흐를 때만 갱신되므로, 값은 남아 있어도 낡을 수 있다.
//! 그래서 `observed_at`을 파일에 적힌 관측 시각 그대로 쓰고 나이를 노출한다 — §3.

use crate::collector::{Cadence, CollectError, Collector};
use crate::collectors::codex::dirs_home;
use crate::model::*;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// 이보다 오래되면 Degraded. 프록시를 경유하는 요청이 한동안 없었다는 뜻.
const STALE_AFTER_SECS: i64 = 6 * 3600;

pub struct ClaudeProxyCollector {
    file: Option<PathBuf>,
    last_health: Health,
}

impl ClaudeProxyCollector {
    pub fn new() -> Self {
        let file = dirs_home().map(|h| h.join(".token-orbit").join("claude-headers.json"));
        Self { file, last_health: Health::NotConfigured }
    }
}

impl Default for ClaudeProxyCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for ClaudeProxyCollector {
    fn id(&self) -> &'static str {
        "claude-proxy"
    }
    fn display_name(&self) -> &'static str {
        "Claude"
    }
    fn service_key(&self) -> &'static str {
        "claude"
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
            return Err(CollectError::NotConfigured("proxy tap not running".into()));
        };
        let v: Value = serde_json::from_str(&fs::read_to_string(path)?)
            .map_err(|e| CollectError::Parse(e.to_string()))?;

        let snaps = snapshots_from_buckets(&v);

        // 버킷마다 관측 시각이 다르다 (모델 전용 창은 그 모델을 쓸 때만 갱신).
        // 카드 건강도는 **가장 신선한** 버킷 기준으로 본다 — 오래된 Fable 값 하나 때문에
        // 카드 전체가 경고로 바뀌면 안 된다. 개별 낡음은 창별 나이로 드러난다.
        let newest = snaps.iter().map(|s| s.observed_at).max();
        self.last_health = match newest {
            None => Health::Degraded { reason: "no rate limit buckets captured".into() },
            Some(o) if now_epoch() - o > STALE_AFTER_SECS => {
                Health::Degraded { reason: format!("proxy data is {}s old", now_epoch() - o) }
            }
            Some(_) => Health::Ok,
        };
        Ok(snaps)
    }

    fn health(&self) -> Health {
        self.last_health.clone()
    }

    /// 계정 실시간 상태 — statusline(세션 캐시)보다 우선한다.
    fn authority(&self) -> u8 {
        10
    }
}

/// 버킷 키 → (창 길이 분, 표시 라벨).
///
/// `7d_oi`가 최상위 모델(Fable) 주간 창이라는 것은 실측으로 확인했다:
/// Fable 요청 시에만 헤더가 나타나고, 값(0.16)이 앱의 "주간 · Fable 16%"와 일치했다.
fn bucket_label(key: &str) -> Option<(u64, String)> {
    match key {
        "5h" => Some((300, "5h".into())),
        "7d" => Some((10080, "7d".into())),
        "7d_oi" => Some((10080, "7d Fable".into())),
        other if other.starts_with("7d") => Some((10080, format!("7d {other}"))),
        other if other.starts_with("5h") => Some((300, format!("5h {other}"))),
        _ => None,
    }
}

/// 탭이 누적한 버킷 맵을 스냅샷으로 변환한다.
///
/// 파일 형태 (버킷마다 자체 관측 시각):
/// ```jsonc
/// { "buckets": {
///     "5h":    { "utilization": "0.5",  "reset": "1787730600", "observed_at": 1787725026 },
///     "7d_oi": { "utilization": "0.17", "reset": "1788195600", "observed_at": 1787723828 }
/// } }
/// ```
fn snapshots_from_buckets(v: &Value) -> Vec<Snapshot> {
    let Some(buckets) = v.get("buckets").and_then(|b| b.as_object()) else { return Vec::new() };

    let mut out = Vec::new();
    for (bucket, b) in buckets {
        let Some((minutes, label)) = bucket_label(bucket) else { continue };
        // 헤더 값은 문자열로 온다 (숫자로 저장된 경우도 허용).
        let Some(frac) = b.get("utilization").and_then(num) else { continue };
        out.push(Snapshot {
            source_id: "claude-proxy".into(),
            account: "default".into(),
            metric: Metric::Percent { used_percent: frac * 100.0 },
            window: Window::Rolling { minutes },
            limit: Limit::Known { value: 100.0 },
            resets_at: b.get("reset").and_then(num).map(|r| r as i64),
            confidence: Confidence::Exact,
            observed_at: b.get("observed_at").and_then(|o| o.as_i64()).unwrap_or_else(now_epoch),
            plan: None,
            label: Some(label),
        });
    }
    out
}

fn num(v: &Value) -> Option<f64> {
    v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "buckets": {
        "5h":    { "utilization": "0.46", "reset": "1787730600", "observed_at": 100 },
        "7d":    { "utilization": "0.09", "reset": "1788195600", "observed_at": 100 },
        "7d_oi": { "utilization": "0.16", "reset": "1788195600", "observed_at": 50 }
      }
    }"#;

    #[test]
    fn parses_all_three_buckets() {
        let v: Value = serde_json::from_str(SAMPLE).unwrap();
        let mut snaps = snapshots_from_buckets(&v);
        snaps.sort_by(|a, b| a.label.cmp(&b.label));
        let labels: Vec<_> = snaps.iter().filter_map(|s| s.label.as_deref()).collect();
        assert_eq!(labels, vec!["5h", "7d", "7d Fable"]);

        let fable = snaps.iter().find(|s| s.label.as_deref() == Some("7d Fable")).unwrap();
        match fable.metric {
            Metric::Percent { used_percent } => assert!((used_percent - 16.0).abs() < 1e-9),
            ref other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(fable.resets_at, Some(1788195600));
        // 모델 전용 창은 다른 창보다 오래된 관측일 수 있다 — 그대로 보존해야 한다.
        assert_eq!(fable.observed_at, 50);
    }

    #[test]
    fn ignores_unknown_buckets() {
        let v: Value = serde_json::from_str(
            r#"{"buckets":{"weird":{"utilization":"0.5","observed_at":1}}}"#,
        )
        .unwrap();
        assert!(snapshots_from_buckets(&v).is_empty());
    }
}
