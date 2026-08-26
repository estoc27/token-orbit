//! Collector 트레이트 — README §4.
//!
//! 뼈대 단계에서는 동기 트레이트로 시작한다. 모든 M0 수집이 로컬 파일 읽기라
//! async 런타임이 아직 필요 없다. T1(프록시)/T4(HTTP) 도입 시 async 전환 예정.

use crate::model::{Capabilities, Health, Snapshot};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CollectError {
    #[error("source not present on this machine: {0}")]
    NotConfigured(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(String),
}

#[derive(Debug, Clone)]
pub enum Cadence {
    /// T4처럼 Collector가 자기 주기를 정하는 경우 (초).
    Poll { secs: u64 },
    /// T0/T2 — 파일 변경 이벤트 기반. M0 뼈대에서는 앱 루프가 폴링으로 대신한다.
    Watch { paths: Vec<PathBuf> },
    /// T1 — 트래픽이 흐를 때만.
    Passive,
}

pub trait Collector: Send + Sync {
    fn id(&self) -> &'static str;
    /// HUD 카드 제목.
    fn display_name(&self) -> &'static str;
    /// 서비스 그룹 키. 같은 키의 Collector들은 HUD에서 카드 한 장으로 병합된다.
    /// (예: statusline %와 JSONL 토큰은 수집기 2개지만 사용자에겐 "Claude" 하나)
    fn service_key(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn cadence(&self) -> Cadence;

    /// 한 번 수집. 실패해도 다른 Collector에 영향을 주면 안 된다 — §4 격리 원칙.
    /// `&mut self`: 마지막 결과를 저장해 `health()`가 정직하게 보고하기 위함.
    fn collect(&mut self) -> Result<Vec<Snapshot>, CollectError>;

    /// 마지막 collect 결과 기반 상태. 기본 구현 없음 — 각 Collector가 정직하게 보고할 것.
    fn health(&self) -> Health;

    /// 같은 창을 여러 소스가 제공할 때의 우선순위 (높을수록 우선).
    ///
    /// "가장 최근에 관측된 값"으로 고르면 안 된다 — 세션 캐시를 계속 재전송하는 소스가
    /// 항상 최신처럼 보이기 때문이다(실측). **계정 실시간 상태 > 세션 캐시** 순으로
    /// 소스의 권위를 명시한다.
    fn authority(&self) -> u8 {
        0
    }
}
