//! 사용자 설정 — `~/.token-orbit/settings.json` (Tauri 버전과 같은 파일).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "yes")]
    pub always_on_top: bool,
    #[serde(default)]
    pub pos_locked: bool,
    #[serde(default)]
    pub click_through: bool,
    /// 카드 배경 불투명도 0.3~1.0 (기본 0.86 — 기존 CSS와 동일).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub window_pos: Option<(i32, i32)>,
    #[serde(default)]
    pub zoom: Option<f32>,
    #[serde(default)]
    pub enabled_services: std::collections::HashMap<String, bool>,
}

fn yes() -> bool {
    true
}
fn default_opacity() -> f32 {
    0.86
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            always_on_top: true,
            pos_locked: false,
            click_through: false,
            opacity: 0.86,
            window_pos: None,
            zoom: None,
            enabled_services: Default::default(),
        }
    }
}

fn path() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".token-orbit").join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }
}
