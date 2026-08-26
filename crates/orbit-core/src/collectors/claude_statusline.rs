//! T0: Claude Code statusline tap Collector.
//!
//! `scripts/statusline-tap.*` 가 Claude Code의 statusline stdin JSON을
//! `~/.token-orbit/claude-code.json` 에 원자적으로 떨군다. 이 Collector는 그 파일을 읽는다.
//!
//! 공식 문서 확인 (code.claude.com/docs/en/statusline, 2026-08-26):
//! ```jsonc
//! "rate_limits": {
//!   "five_hour": { "used_percentage": 94, "resets_at": 1787739000 },
//!   "seven_day": { "used_percentage": 20, "resets_at": 1787965200 }
//! },
//! "cost": { "total_cost_usd": 0.012 },
//! "model": { "id": "claude-opus-5", "display_name": "Opus" }
//! ```
//!
//! 플랜명은 statusline JSON에 없다 → `~/.claude.json` 의 `oauthAccount` 티어로 보완 (§2 각주).

use crate::collector::{Cadence, CollectError, Collector};
use crate::collectors::codex::dirs_home;
use crate::model::*;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// tap 파일이 이보다 오래되면 Degraded (Claude Code가 안 돌고 있거나 tap이 죽었음).
const STALE_AFTER_SECS: i64 = 30 * 60;

pub struct ClaudeStatuslineCollector {
    tap_file: Option<PathBuf>,
    last_health: Health,
    /// 마지막으로 **값이 실제로 달라진** 시점의 지문과 그 시각.
    ///
    /// tap 파일의 mtime은 데이터 신선도가 아니다 (실측, 2026-08-26):
    /// idle 세션도 `refreshInterval`마다 statusline을 재실행해 **같은 값을 다시 쓴다**.
    /// 파일은 3초 전에 쓰였는데 그 안의 사용률은 1시간 전 값일 수 있다.
    /// 따라서 "언제 쓰였나"가 아니라 "언제 달라졌나"를 관측 시각으로 삼는다 — §3.
    last_fingerprint: Option<(String, EpochSecs)>,
}

impl ClaudeStatuslineCollector {
    pub fn new() -> Self {
        let tap_file = dirs_home().map(|h| h.join(".token-orbit").join("claude-code.json"));
        Self { tap_file, last_health: Health::NotConfigured, last_fingerprint: None }
    }

    /// 값이 바뀌었는지 판별할 지문. rate_limits뿐 아니라 세션 진행 지표까지 포함해,
    /// "턴이 돌았는가"를 넓게 감지한다.
    fn fingerprint(v: &Value) -> String {
        format!(
            "{}|{}|{}",
            v.get("rate_limits").map(|r| r.to_string()).unwrap_or_default(),
            v.pointer("/cost/total_cost_usd").map(|c| c.to_string()).unwrap_or_default(),
            v.pointer("/context_window/total_input_tokens")
                .map(|t| t.to_string())
                .unwrap_or_default(),
        )
    }

    /// `~/.claude.json` → oauthAccount 티어에서 플랜 라벨 추출 (없으면 None).
    fn plan_from_oauth() -> Option<String> {
        let p = dirs_home()?.join(".claude.json");
        let v: Value = serde_json::from_str(&fs::read_to_string(p).ok()?).ok()?;
        v.pointer("/oauthAccount/organizationRateLimitTier")
            .and_then(|t| t.as_str())
            .map(pretty_claude_plan)
    }
}

impl Default for ClaudeStatuslineCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for ClaudeStatuslineCollector {
    fn id(&self) -> &'static str {
        "claude-code-statusline"
    }
    fn display_name(&self) -> &'static str {
        "Claude"
    }
    fn service_key(&self) -> &'static str {
        "claude"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { tokens: false, percent: true, reset_time: true, plan_name: false, cost: true }
    }
    fn cadence(&self) -> Cadence {
        match &self.tap_file {
            Some(p) => Cadence::Watch { paths: vec![p.clone()] },
            None => Cadence::Poll { secs: 300 },
        }
    }

    fn collect(&mut self) -> Result<Vec<Snapshot>, CollectError> {
        let Some(path) = self.tap_file.as_ref().filter(|p| p.is_file()) else {
            // tap 미설치는 오류가 아니라 "설정 안 됨" — HUD는 이 카드에 연동 안내만 띄운다.
            self.last_health = Health::NotConfigured;
            return Err(CollectError::NotConfigured("statusline tap not installed".into()));
        };

        let text = fs::read_to_string(path)?;
        let v: Value =
            serde_json::from_str(&text).map_err(|e| CollectError::Parse(e.to_string()))?;

        // 관측 시각 = 값이 마지막으로 **달라진** 시각 (파일이 쓰인 시각이 아님 — 위 주석 참조).
        // 첫 관측에서는 언제 받아온 값인지 알 수 없어 파일 mtime을 상한으로 쓴다.
        let fp = Self::fingerprint(&v);
        let now = now_epoch();
        let observed_at = match &self.last_fingerprint {
            Some((prev, at)) if *prev == fp => *at,
            Some(_) => now,
            None => fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(now),
        };
        self.last_fingerprint = Some((fp, observed_at));

        let plan = Self::plan_from_oauth();
        let snaps = snapshots_from_statusline(&v, observed_at, plan);

        let age = now - observed_at;
        self.last_health = if snaps.is_empty() {
            Health::Degraded { reason: "tap file has no rate_limits".into() }
        } else if age > STALE_AFTER_SECS {
            Health::Degraded { reason: format!("tap data is {age}s old") }
        } else {
            Health::Ok
        };
        Ok(snaps)
    }

    fn health(&self) -> Health {
        self.last_health.clone()
    }

    /// 세션 캐시 — 프록시가 있으면 그쪽에 양보한다.
    fn authority(&self) -> u8 {
        5
    }
}

/// 내부 티어명 → 표시명. 사용자 확정 표기: Max 20x → "Max 20", Max 5x → "Max 5",
/// 그 이하는 자기 이름 그대로 (첫 글자 대문자).
fn pretty_claude_plan(tier: &str) -> String {
    let t = tier.to_ascii_lowercase();
    if t.contains("max_20x") || t.contains("max_20") {
        return "Max 20".into();
    }
    if t.contains("max_5x") || t.contains("max_5") {
        return "Max 5".into();
    }
    // "default_claude_pro" 류 → 접두어 떼고 첫 글자만 대문자
    let stripped = t.strip_prefix("default_claude_").unwrap_or(&t).replace('_', " ");
    capitalize(&stripped)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// rate_limits 키 → (창 길이 분, 표시 라벨).
///
/// 모델 전용 버킷은 `seven_day_<model>` 형태로 붙을 것으로 보고 접두어를 분리해
/// "7d Fable" 처럼 읽히게 한다. 실측된 키는 `five_hour`/`seven_day` 둘뿐이며
/// (Opus 세션 기준), 모델 전용 창은 아직 페이로드에서 관측되지 않았다.
fn window_label_for(key: &str) -> (u64, String) {
    if key == "five_hour" {
        return (300, "5h".into());
    }
    if key == "seven_day" {
        return (10080, "7d".into());
    }
    if let Some(rest) = key.strip_prefix("seven_day_") {
        return (10080, format!("7d {}", capitalize(rest)));
    }
    if let Some(rest) = key.strip_prefix("five_hour_") {
        return (300, format!("5h {}", capitalize(rest)));
    }
    (0, key.replace('_', " "))
}

fn snapshots_from_statusline(v: &Value, observed_at: EpochSecs, plan: Option<String>) -> Vec<Snapshot> {
    let mut out = Vec::new();

    // rate_limits의 **모든** 키를 동적으로 파싱한다. 문서에는 five_hour/seven_day만
    // 명시돼 있으나(실측 페이로드도 동일), 모델별 주간 버킷(예: seven_day_fable) 같은
    // 필드가 추가되는 순간 코드 수정 없이 HUD에 나타나게 하기 위함.
    if let Some(rl) = v.get("rate_limits").and_then(|r| r.as_object()) {
        for (key, w) in rl {
            let Some(used) = w.get("used_percentage").and_then(|u| u.as_f64()) else { continue };
            let (minutes, label) = window_label_for(key);
            out.push(Snapshot {
                source_id: "claude-code-statusline".into(),
                account: "default".into(),
                metric: Metric::Percent { used_percent: used },
                window: Window::Rolling { minutes },
                limit: Limit::Known { value: 100.0 },
                resets_at: w.get("resets_at").and_then(|r| r.as_i64()),
                confidence: Confidence::Exact,
                observed_at,
                plan: plan.clone(),
                label: Some(label),
            });
        }
    }

    // 세션 비용 (클라이언트 계산 추정치 — 문서 명시에 따라 Derived).
    if let Some(usd) = v.pointer("/cost/total_cost_usd").and_then(|c| c.as_f64()) {
        out.push(Snapshot {
            source_id: "claude-code-statusline".into(),
            account: "default".into(),
            metric: Metric::Cost { usd },
            window: Window::Session,
            limit: Limit::Unknown,
            resets_at: None,
            confidence: Confidence::Derived,
            observed_at,
            plan,
            label: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "model": {"id": "claude-opus-5", "display_name": "Opus"},
      "rate_limits": {
        "five_hour": {"used_percentage": 94, "resets_at": 1787739000},
        "seven_day": {"used_percentage": 20, "resets_at": 1787965200}
      },
      "cost": {"total_cost_usd": 0.01234}
    }"#;

    #[test]
    fn parses_statusline_json() {
        let v: Value = serde_json::from_str(SAMPLE).unwrap();
        let snaps = snapshots_from_statusline(&v, 0, None);
        assert_eq!(snaps.len(), 3); // five_hour + seven_day + cost
        assert_eq!(snaps[0].window, Window::Rolling { minutes: 300 });
        assert_eq!(snaps[1].window, Window::Rolling { minutes: 10080 });
        match &snaps[1].metric {
            Metric::Percent { used_percent } => assert_eq!(*used_percent, 20.0),
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(snaps[2].confidence, Confidence::Derived);
    }

    #[test]
    fn unknown_window_keys_are_parsed_dynamically() {
        let v: Value = serde_json::from_str(
            r#"{"rate_limits":{"seven_day_fable":{"used_percentage":42,"resets_at":1}}}"#,
        )
        .unwrap();
        let snaps = snapshots_from_statusline(&v, 0, None);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].label.as_deref(), Some("7d Fable"));
        match &snaps[0].metric {
            Metric::Percent { used_percent } => assert_eq!(*used_percent, 42.0),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn plan_display_names() {
        assert_eq!(pretty_claude_plan("default_claude_max_20x"), "Max 20");
        assert_eq!(pretty_claude_plan("default_claude_max_5x"), "Max 5");
        assert_eq!(pretty_claude_plan("default_claude_pro"), "Pro");
        assert_eq!(pretty_claude_plan("free"), "Free");
    }

    #[test]
    fn missing_rate_limits_yields_empty_percent() {
        let v: Value = serde_json::from_str(r#"{"model":{"id":"x"}}"#).unwrap();
        assert!(snapshots_from_statusline(&v, 0, None).is_empty());
    }
}
