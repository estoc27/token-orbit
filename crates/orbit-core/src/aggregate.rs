//! Aggregator — Collector들의 Snapshot을 HUD가 그릴 수 있는 뷰로 정규화.
//!
//! 모든 Renderer는 `AggregatedView` 하나만 먹는다 — README §5.
//! Renderer는 어떤 Collector가 값을 물어왔는지 알지 못하며, 알 필요도 없다.

use crate::collector::Collector;
use crate::model::*;
use serde::Serialize;

/// HUD 카드 하나 = 서비스 하나.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceCard {
    pub source_id: String,
    pub display_name: String,
    pub capabilities: Capabilities,
    pub health: Health,
    pub plan: Option<String>,
    /// 가장 임박한 창을 맨 앞으로 정렬 — §5.0 "임박한 것을 크게".
    pub windows: Vec<WindowView>,
    /// 퍼센트가 없는 소스(Claude JSONL 등)의 절대값 표시.
    pub tokens: Option<TokenView>,
    pub session_cost_usd: Option<f64>,
    /// 유료 크레딧 잔액 (Codex `credits.balance` 등). 세션 비용과 별개.
    pub credits_usd: Option<f64>,
    /// 초 단위 데이터 나이. Renderer가 staleness 흐림 처리에 사용.
    pub data_age_secs: i64,
    /// 퍼센트 앵커 이후에 토큰 소모가 관측됨 — 표시된 %는 하한이며 실제는 더 높다.
    /// (사용자 제안: 트리거를 '사용량 소모'로 본다. 눈금 학습 전까지는 수치 추정 대신
    /// 정직한 플래그만 — §3 규칙 1)
    pub activity_after_percent: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WindowView {
    /// "5h" / "7d" / "168h" 같은 사람용 라벨.
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<EpochSecs>,
    /// 리셋까지 남은 초. 음수면 이미 지났음(리셋 임박 표시).
    pub resets_in_secs: Option<i64>,
    pub confidence: Confidence,
    /// 이 창 값의 나이(초). 소스마다 신선도가 달라 창 단위로 들고 있어야 한다
    /// (프록시는 계정 실시간, statusline은 세션 캐시라 얼어붙을 수 있음).
    pub age_secs: i64,
    #[serde(skip)]
    observed_at: EpochSecs,
    #[serde(skip)]
    authority: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenView {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AggregatedView {
    pub cards: Vec<ServiceCard>,
    pub generated_at: EpochSecs,
}

/// 등록된 모든 Collector를 돌려 뷰를 만든다.
/// 하나의 실패가 전체를 죽이지 않는다 — 실패는 카드의 `health`로만 표면화 (§4 격리).
///
/// 같은 `service_key`의 Collector들은 **카드 한 장으로 병합**된다.
/// 수집기 분할(statusline % / JSONL 토큰)은 내부 사정이고, 사용자에겐 서비스당 한 장 (§5.0).
pub fn collect_all(collectors: &mut [Box<dyn Collector>]) -> AggregatedView {
    let now = now_epoch();
    let mut order: Vec<&'static str> = Vec::new();
    let mut token_obs: std::collections::HashMap<&'static str, EpochSecs> = std::collections::HashMap::new();
    let mut map: std::collections::HashMap<&'static str, (ServiceCard, Option<EpochSecs>, Option<EpochSecs>)> =
        std::collections::HashMap::new();
    // 튜플: (카드, 퍼센트 소스의 최신 관측시각, 아무 소스의 최신 관측시각)

    for c in collectors.iter_mut() {
        let snaps = c.collect().unwrap_or_default();
        let health = c.health();

        // NotConfigured 소스는 카드에 참여하지 않는다 (설치 안 된 도구를 나열하지 않음).
        if matches!(health, Health::NotConfigured) {
            continue;
        }

        let key = c.service_key();
        if !map.contains_key(key) {
            order.push(key);
            map.insert(
                key,
                (
                    ServiceCard {
                        source_id: key.into(),
                        display_name: c.display_name().into(),
                        capabilities: Capabilities {
                            tokens: false,
                            percent: false,
                            reset_time: false,
                            plan_name: false,
                            cost: false,
                        },
                        health: Health::Ok,
                        plan: None,
                        windows: Vec::new(),
                        tokens: None,
                        session_cost_usd: None,
                        credits_usd: None,
                        data_age_secs: 0,
                        activity_after_percent: false,
                    },
                    None,
                    None,
                ),
            );
        }
        let (card, percent_obs, any_obs) = map.get_mut(key).unwrap();

        // capabilities는 합집합 — 멤버 중 하나라도 주면 카드가 그 항목을 가진다.
        let authority = c.authority();
        let cc = c.capabilities();
        card.capabilities.tokens |= cc.tokens;
        card.capabilities.percent |= cc.percent;
        card.capabilities.reset_time |= cc.reset_time;
        card.capabilities.plan_name |= cc.plan_name;
        card.capabilities.cost |= cc.cost;

        // health는 나쁜 쪽 우선 (Failed > Degraded > Ok).
        if health_rank(&health) > health_rank(&card.health) {
            card.health = health;
        }

        for s in &snaps {
            *any_obs = Some(any_obs.map_or(s.observed_at, |o: i64| o.max(s.observed_at)));
            if card.plan.is_none() {
                card.plan = s.plan.clone();
            }
            match &s.metric {
                Metric::Percent { used_percent } => {
                    *percent_obs =
                        Some(percent_obs.map_or(s.observed_at, |o: i64| o.max(s.observed_at)));
                    let minutes = match s.window {
                        Window::Rolling { minutes } => minutes,
                        _ => 0,
                    };
                    card.windows.push(WindowView {
                        label: s.label.clone().unwrap_or_else(|| window_label(minutes)),
                        used_percent: *used_percent,
                        resets_at: s.resets_at,
                        resets_in_secs: s.resets_at.map(|r| r - now),
                        confidence: s.confidence,
                        age_secs: now - s.observed_at,
                        observed_at: s.observed_at,
                        authority,
                    });
                }
                Metric::Tokens { input, output, cache_read, cache_write } => {
                    token_obs.insert(key, token_obs.get(key).copied().unwrap_or(s.observed_at).max(s.observed_at));
                    card.tokens = Some(TokenView {
                        input: *input,
                        output: *output,
                        cache_read: *cache_read,
                        cache_write: *cache_write,
                    });
                }
                Metric::Cost { usd } => {
                    match s.window {
                        Window::Session => card.session_cost_usd = Some(*usd),
                        // Calendar 창의 비용 = 크레딧 잔액 (Codex credits 등)
                        Window::Calendar { .. } => card.credits_usd = Some(*usd),
                        _ => {}
                    }
                }
                Metric::Requests { .. } => {}
            }
        }
    }

    let cards = order
        .into_iter()
        .filter_map(|k| map.remove(k))
        .map(|(mut card, percent_obs, any_obs)| {
            // 같은 창을 여러 소스가 줄 수 있다 (프록시와 statusline 모두 5h/7d 제공).
            //
            // 권위는 **신선할 때만** 유효하다. 순수 authority 정렬은 9시간 묵은
            // 프록시 값이 더 신선한 statusline 값을 영원히 가리는 사고를 냈다 (실측).
            // 규칙: 둘 다 신선(≤10분)하면 권위 우선, 아니면 최근 관측 우선.
            card.windows.sort_by(|a, b| {
                a.label.cmp(&b.label).then_with(|| {
                    let both_fresh =
                        a.age_secs <= AUTHORITY_FRESH_SECS && b.age_secs <= AUTHORITY_FRESH_SECS;
                    if both_fresh {
                        b.authority.cmp(&a.authority).then(b.observed_at.cmp(&a.observed_at))
                    } else {
                        b.observed_at.cmp(&a.observed_at).then(b.authority.cmp(&a.authority))
                    }
                })
            });
            card.windows.dedup_by(|a, b| a.label == b.label);
            // 가장 임박한 리셋이 앞으로. 리셋 미상은 뒤로.
            card.windows.sort_by_key(|w| w.resets_in_secs.unwrap_or(i64::MAX));
            // 나이는 헤드라인(퍼센트) 소스 기준 — 토큰이 신선하다고 낡은 %를
            // 신선해 보이게 하지 않는다 (§3 Staleness).
            let obs = percent_obs.or(any_obs);
            card.data_age_secs = obs.map(|o| now - o).unwrap_or(0);
            // 앵커(%) 이후 소모 판정: 토큰 소스가 앵커보다 유의미하게 새로우면
            // 표시된 %는 하한이다. (여유 60초 — 정상 갱신 지터 흡수)
            if let (Some(p), Some(t)) = (percent_obs, token_obs.get(card.source_id.as_str())) {
                card.activity_after_percent = *t > p + ACTIVITY_MARGIN_SECS;
            }
            card
        })
        .collect();

    AggregatedView { cards, generated_at: now }
}

/// 앵커 이후 소모 판정 여유 (초). 정상 갱신 지터를 소모로 오인하지 않기 위함.
const ACTIVITY_MARGIN_SECS: i64 = 60;

/// 소스 권위가 유효한 신선도 (초). 이보다 낡은 관측은 권위를 잃고 최신성으로 겨룬다.
const AUTHORITY_FRESH_SECS: i64 = 10 * 60;

fn health_rank(h: &Health) -> u8 {
    match h {
        Health::Ok => 0,
        Health::NotConfigured => 1,
        Health::Degraded { .. } => 2,
        Health::Failed { .. } => 3,
    }
}

fn window_label(minutes: u64) -> String {
    match minutes {
        0 => "?".into(),
        m if m % (24 * 60) == 0 => format!("{}d", m / (24 * 60)),
        m if m % 60 == 0 => format!("{}h", m / 60),
        m => format!("{m}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_authority_loses_to_fresh_source() {
        // 회귀: 9시간 묵은 고권위(프록시) 창이 신선한 저권위(statusline) 창을 가렸던 버그.
        let mk = |authority: u8, age: i64, pct: f64| WindowView {
            label: "5h".into(),
            used_percent: pct,
            resets_at: None,
            resets_in_secs: None,
            confidence: Confidence::Exact,
            age_secs: age,
            observed_at: 100_000 - age,
            authority,
        };
        let sort_dedup = |mut v: Vec<WindowView>| {
            v.sort_by(|a, b| {
                a.label.cmp(&b.label).then_with(|| {
                    let both_fresh =
                        a.age_secs <= AUTHORITY_FRESH_SECS && b.age_secs <= AUTHORITY_FRESH_SECS;
                    if both_fresh {
                        b.authority.cmp(&a.authority).then(b.observed_at.cmp(&a.observed_at))
                    } else {
                        b.observed_at.cmp(&a.observed_at).then(b.authority.cmp(&a.authority))
                    }
                })
            });
            v.dedup_by(|a, b| a.label == b.label);
            v
        };

        // 케이스 1: 프록시 9시간 묵음 vs statusline 5분 — 신선한 쪽이 이겨야 한다.
        let v = sort_dedup(vec![mk(10, 9 * 3600, 54.0), mk(5, 300, 16.0)]);
        assert_eq!(v[0].used_percent, 16.0);

        // 케이스 2: 둘 다 신선 — 권위(프록시)가 이긴다.
        let v = sort_dedup(vec![mk(5, 30, 16.0), mk(10, 60, 54.0)]);
        assert_eq!(v[0].used_percent, 54.0);
    }

    #[test]
    fn window_labels() {
        assert_eq!(window_label(300), "5h");
        assert_eq!(window_label(10080), "7d");
        assert_eq!(window_label(90), "90m");
        assert_eq!(window_label(0), "?");
    }
}
