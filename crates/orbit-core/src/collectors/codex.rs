//! T2: Codex 세션 파일 Collector.
//!
//! 실측 (2026-08-26): `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` 에
//! `rate_limits` 객체가 매 턴 기록된다. 마지막 항목만 읽으면 현재 상태다.
//!
//! ```jsonc
//! "rate_limits": {
//!   "limit_id": "codex",
//!   "primary":   { "used_percent": 88.0, "window_minutes": 10080, "resets_at": 1788140374 },
//!   "secondary": null,
//!   "credits":   { "has_credits": false, "unlimited": false, "balance": "0" },
//!   "plan_type": "pro"
//! }
//! ```
//!
//! 창이 여러 개면 전부 별개 Snapshot으로 내보낸다 — README §3 규칙 3.

use crate::collector::{Cadence, CollectError, Collector};
use crate::model::*;
use serde_json::Value;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 파일 끝에서 이만큼만 읽는다. rate_limits는 매 턴 기록되므로 tail에 반드시 있다.
const TAIL_BYTES: u64 = 256 * 1024;

pub struct CodexCollector {
    root: Option<PathBuf>,
    last_health: Health,
}

impl CodexCollector {
    pub fn new() -> Self {
        let root = dirs_home().map(|h| h.join(".codex"));
        let root = root.filter(|r| r.join("sessions").is_dir() || r.join("archived_sessions").is_dir());
        let last_health = if root.is_some() {
            Health::Ok
        } else {
            Health::NotConfigured
        };
        Self { root, last_health }
    }

    /// mtime 상위 N개 .jsonl 파일 (sessions/ + archived_sessions/).
    ///
    /// ⚠️ mtime 1등 파일 하나만 보면 안 된다 (실측 함정, 2026-08-26):
    /// 이틀 전 세션이 열린 채 idle이면 rate_limits 없는 이벤트를 계속 append해서
    /// mtime은 최신인데 마지막 rate_limits는 이틀 전 값(지난 창의 84%)일 수 있다.
    /// 후보 여러 개에서 **rate_limits 항목의 타임스탬프**로 최신을 고른다.
    fn recent_files(&self, n: usize) -> Vec<PathBuf> {
        let Some(root) = self.root.as_ref() else { return Vec::new() };
        let mut all: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        for sub in ["sessions", "archived_sessions"] {
            walk_jsonl(&root.join(sub), &mut |p, mtime| {
                all.push((mtime, p.to_path_buf()));
            });
        }
        all.sort_by(|a, b| b.0.cmp(&a.0));
        all.truncate(n);
        all.into_iter().map(|(_, p)| p).collect()
    }
}

impl Default for CodexCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for CodexCollector {
    fn id(&self) -> &'static str {
        "codex-local"
    }
    fn display_name(&self) -> &'static str {
        "Codex"
    }
    fn service_key(&self) -> &'static str {
        "codex"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { tokens: true, percent: true, reset_time: true, plan_name: true, cost: false }
    }
    fn cadence(&self) -> Cadence {
        match &self.root {
            Some(r) => Cadence::Watch { paths: vec![r.join("sessions"), r.join("archived_sessions")] },
            None => Cadence::Poll { secs: 300 },
        }
    }

    fn collect(&mut self) -> Result<Vec<Snapshot>, CollectError> {
        let files = self.recent_files(8);
        if files.is_empty() {
            self.last_health = Health::NotConfigured;
            return Err(CollectError::NotConfigured("no codex session files".into()));
        }

        // 후보 파일들의 마지막 rate_limits 중 **항목 타임스탬프가 가장 최신**인 것을 채택.
        let mut best: Option<(EpochSecs, serde_json::Value)> = None;
        for file in &files {
            let Ok(tail) = read_tail(file, TAIL_BYTES) else { continue };
            let Some((rl, line_ts)) = last_rate_limits(&tail) else { continue };
            // 라인 타임스탬프가 없으면 파일 mtime으로 폴백 (그래도 now보다는 정직).
            let ts = line_ts.or_else(|| {
                fs::metadata(file)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
            });
            let Some(ts) = ts else { continue };
            if best.as_ref().map(|(t, _)| ts > *t).unwrap_or(true) {
                best = Some((ts, rl));
            }
        }

        match best {
            Some((observed_at, rl)) => {
                let snaps = snapshots_from_rate_limits(&rl, observed_at);
                self.last_health = if snaps.is_empty() {
                    Health::Degraded { reason: "rate_limits present but no windows".into() }
                } else {
                    Health::Ok
                };
                Ok(snaps)
            }
            None => {
                self.last_health =
                    Health::Degraded { reason: "no rate_limits in recent files".into() };
                Ok(vec![])
            }
        }
    }

    fn health(&self) -> Health {
        self.last_health.clone()
    }
}

/// tail 텍스트에서 마지막 `rate_limits` 객체와 그 줄의 타임스탬프(epoch)를 찾는다.
/// 줄 단위 JSON이므로 뒤에서부터 줄을 훑고, 각 줄 안에서는 재귀 탐색한다
/// (이벤트 스키마 안 어디에 중첩되어 있는지에 의존하지 않기 위함).
fn last_rate_limits(tail: &str) -> Option<(Value, Option<EpochSecs>)> {
    for line in tail.lines().rev() {
        if !line.contains("\"rate_limits\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(rl) = find_key(&v, "rate_limits") {
            // 창도 크레딧도 없는 빈 껍데기(전부 null)는 건너뛰고 더 이전 줄을 본다.
            if rl.get("primary").map(|p| !p.is_null()).unwrap_or(false)
                || rl.get("secondary").map(|s| !s.is_null()).unwrap_or(false)
            {
                let ts = find_key(&v, "timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(iso_to_epoch);
                return Some((rl.clone(), ts));
            }
        }
    }
    None
}

/// "2026-08-24T07:35:22.847Z" 형태(UTC 고정)의 ISO 8601 → unix epoch 초.
/// 외부 crate 없이 처리 — Codex 세션 파일의 timestamp가 이 포맷임을 실측 확인.
fn iso_to_epoch(s: &str) -> Option<EpochSecs> {
    if s.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // days_from_civil (Howard Hinnant의 달력 알고리즘)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => {
            if let Some(hit) = m.get(key) {
                return Some(hit);
            }
            m.values().find_map(|c| find_key(c, key))
        }
        Value::Array(a) => a.iter().find_map(|c| find_key(c, key)),
        _ => None,
    }
}

/// plan_type → 표시명. 사용자 확정 표기: pro → "Pro 20", 하위 pro 계열 → "Pro 5",
/// 그 외(plus, free, team 등)는 자기 이름 그대로 (첫 글자 대문자).
fn pretty_codex_plan(plan: &str) -> String {
    let p = plan.to_ascii_lowercase();
    match p.as_str() {
        "pro" => "Pro 20".into(),
        _ if p.starts_with("pro") && p.contains('5') => "Pro 5".into(),
        _ => {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        }
    }
}

fn snapshots_from_rate_limits(rl: &Value, observed_at: EpochSecs) -> Vec<Snapshot> {
    let plan = rl
        .get("plan_type")
        .and_then(|p| p.as_str())
        .map(pretty_codex_plan);
    let mut out = Vec::new();

    for key in ["primary", "secondary"] {
        let Some(w) = rl.get(key).filter(|w| !w.is_null()) else { continue };
        let Some(used) = w.get("used_percent").and_then(|u| u.as_f64()) else { continue };
        out.push(Snapshot {
            source_id: "codex-local".into(),
            account: "default".into(),
            metric: Metric::Percent { used_percent: used },
            window: Window::Rolling {
                minutes: w.get("window_minutes").and_then(|m| m.as_u64()).unwrap_or(0),
            },
            limit: Limit::Known { value: 100.0 },
            resets_at: w.get("resets_at").and_then(|r| r.as_i64()),
            confidence: Confidence::Exact,
            observed_at,
            plan: plan.clone(),
            label: None,
        });
    }

    // 크레딧 잔액 — balance가 문자열("0")로 오는 것 실측 확인.
    if let Some(c) = rl.get("credits").filter(|c| !c.is_null()) {
        let bal = c
            .get("balance")
            .and_then(|b| b.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| b.as_f64()));
        if let Some(usd) = bal {
            out.push(Snapshot {
                source_id: "codex-local".into(),
                account: "default".into(),
                metric: Metric::Cost { usd },
                window: Window::Calendar { period: "billing".into() },
                limit: Limit::Unknown,
                resets_at: None,
                confidence: Confidence::Exact,
                observed_at,
                plan: plan.clone(),
                label: None,
            });
        }
    }
    out
}

fn walk_jsonl(dir: &Path, f: &mut impl FnMut(&Path, std::time::SystemTime)) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_jsonl(&p, f);
        } else if p.extension().map(|x| x == "jsonl").unwrap_or(false) {
            if let Ok(meta) = e.metadata() {
                if let Ok(mtime) = meta.modified() {
                    f(&p, mtime);
                }
            }
        }
    }
}

fn read_tail(path: &Path, max: u64) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len > max {
        file.seek(SeekFrom::Start(len - max))?;
    }
    let mut buf = Vec::with_capacity(len.min(max) as usize);
    file.read_to_end(&mut buf)?;
    // tail 시작이 줄 중간일 수 있음 — 첫 개행 이후부터 사용.
    let start = buf.iter().position(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
    Ok(String::from_utf8_lossy(&buf[start..]).into_owned())
}

pub(crate) fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 실측 구조를 그대로 축약한 샘플 (이벤트 껍데기 안에 중첩).
    const LINE: &str = r#"{"type":"event","payload":{"rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":88.0,"window_minutes":10080,"resets_at":1788140374},"secondary":null,"credits":{"has_credits":false,"unlimited":false,"balance":"0"},"plan_type":"pro","rate_limit_reached_type":null}}}"#;

    #[test]
    fn parses_rate_limits_line() {
        let tail = format!("{{\"noise\":1}}\n{}\n", LINE);
        let (rl, _ts) = last_rate_limits(&tail).expect("should find rate_limits");
        let snaps = snapshots_from_rate_limits(&rl, 0);
        assert_eq!(snaps.len(), 2); // primary + credits
        match &snaps[0].metric {
            Metric::Percent { used_percent } => assert_eq!(*used_percent, 88.0),
            other => panic!("unexpected metric: {other:?}"),
        }
        assert_eq!(snaps[0].plan.as_deref(), Some("Pro 20"));
        assert_eq!(snaps[0].resets_at, Some(1788140374));
        assert_eq!(snaps[0].window, Window::Rolling { minutes: 10080 });
    }

    #[test]
    fn skips_empty_rate_limits() {
        let empty = r#"{"rate_limits":{"limit_id":"premium","primary":null,"secondary":null}}"#;
        let tail = format!("{}\n{}\n", LINE, empty);
        // 마지막 줄은 빈 껍데기 → 그 이전의 유효한 줄을 찾아야 한다.
        let (rl, _ts) = last_rate_limits(&tail).expect("should fall back to earlier line");
        assert!(rl.get("primary").map(|p| !p.is_null()).unwrap_or(false));
    }

    #[test]
    fn plan_display_names() {
        assert_eq!(pretty_codex_plan("pro"), "Pro 20");
        assert_eq!(pretty_codex_plan("pro_5x"), "Pro 5");
        assert_eq!(pretty_codex_plan("plus"), "Plus");
        assert_eq!(pretty_codex_plan("free"), "Free");
    }

    #[test]
    fn iso_epoch_anchors() {
        assert_eq!(iso_to_epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_epoch("1970-01-02T00:00:00.500Z"), Some(86400));
        assert_eq!(iso_to_epoch("2000-01-01T00:00:00Z"), Some(946684800));
        assert_eq!(iso_to_epoch("bad"), None);
    }

    #[test]
    fn extracts_line_timestamp() {
        let line = r#"{"timestamp":"1970-01-02T00:00:00.000Z","payload":{"rate_limits":{"limit_id":"codex","primary":{"used_percent":11.0,"window_minutes":10080,"resets_at":2},"secondary":null}}}"#;
        let (_, ts) = last_rate_limits(&format!("{line}\n")).unwrap();
        assert_eq!(ts, Some(86400));
    }
}
