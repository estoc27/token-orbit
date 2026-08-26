//! 데이터 모델 — README §3 참조.
//!
//! 스펙 대비 추가: `Metric::Percent`.
//! Codex/Claude Code 모두 서버가 계산한 사용률(%)을 그대로 주므로,
//! 토큰으로 환산하지 않고 퍼센트를 1급 메트릭으로 다룬다 (§T2 "서버가 준 %를 그대로 쓸 것").

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub type EpochSecs = i64;

pub fn now_epoch() -> EpochSecs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// "codex-local", "claude-code-statusline", "claude-code-jsonl", ...
    pub source_id: String,
    /// 다계정 구분용 라벨. 미상이면 "default".
    pub account: String,
    pub metric: Metric,
    pub window: Window,
    pub limit: Limit,
    /// unix epoch (초). 리셋 시각을 모르면 None.
    pub resets_at: Option<EpochSecs>,
    pub confidence: Confidence,
    /// 이 값을 관측한 시각 (unix epoch 초). staleness 판단 기준 — §3.
    pub observed_at: EpochSecs,
    /// 플랜명 ("pro" 등). 스냅샷 단위로 붙는 이유: 소스마다 아는 정도가 다름.
    pub plan: Option<String>,
    /// 창 표시 라벨 힌트. None이면 창 길이에서 유도("5h"/"7d").
    /// 소스가 창 길이를 모르는 미지의 키(예: 미래의 `seven_day_fable`)를 만나도
    /// 키 이름 그대로 표시할 수 있게 한다 — 스키마 확장에 자동 대응.
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Metric {
    /// 서버가 계산해 준 사용률. 0.0 ~ 100.0.
    Percent { used_percent: f64 },
    Tokens {
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
    },
    Requests { count: u64 },
    /// USD. 교차 서비스 합산이 성립하는 유일한 축 — §3 규칙 2.
    Cost { usd: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Window {
    /// 롤링 창. 예: 5시간 = 300, 7일 = 10080.
    Rolling { minutes: u64 },
    /// 달력 경계 (월간 크레딧 등).
    Calendar { period: String },
    Session,
    Lifetime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Limit {
    /// API/파일이 알려준 값. Percent 메트릭은 항상 Known(100.0).
    Known { value: f64 },
    /// 사용자가 입력한 값 (T5).
    Declared { value: f64 },
    /// 모름 — 퍼센트 바를 그리지 말고 절대값만 표시할 것 (§T2 Claude JSONL).
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Exact,
    Derived,
    Declared,
    Estimated,
}

/// 소스가 줄 수 있는 항목 — README §4. Renderer는 이걸 보고 표현을 낮춘다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub tokens: bool,
    pub percent: bool,
    pub reset_time: bool,
    pub plan_name: bool,
    pub cost: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Health {
    Ok,
    /// 값은 있으나 낡았거나 일부 실패. 마지막 성공값을 현재값처럼 보이게 하지 말 것 — §3 Staleness.
    Degraded { reason: String },
    Failed { reason: String },
    /// 소스 자체가 이 환경에 없음 (설치 안 됨 / tap 미설정). 조용히 타일 숨김.
    NotConfigured,
}
