use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub interval_minutes: u32,
    pub thresholds: Vec<u8>,
    pub claude_path: Option<String>,
    pub codex_path: Option<String>,
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { interval_minutes: 5, thresholds: vec![80, 95], claude_path: None, codex_path: None, launch_at_login: false }
    }
}

impl Settings {
    /// Clamps values into their allowed ranges and drops blank paths.
    pub fn normalized(mut self) -> Self {
        self.interval_minutes = self.interval_minutes.clamp(2, 30);
        self.thresholds.retain(|t| (1..=100).contains(t));
        self.thresholds.sort_unstable();
        self.thresholds.dedup();
        self.claude_path = self.claude_path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        self.codex_path = self.codex_path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        self
    }

    pub fn interval(&self) -> Duration {
        Duration::from_secs(u64::from(self.interval_minutes) * 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_ranges_and_blank_paths() {
        let s = Settings {
            interval_minutes: 1,
            thresholds: vec![95, 0, 80, 80, 120],
            claude_path: Some("  ".into()),
            codex_path: Some(" /bin/codex ".into()),
            launch_at_login: true,
        }
        .normalized();
        assert_eq!(s.interval_minutes, 2);
        assert_eq!(s.thresholds, vec![80, 95]);
        assert_eq!(s.claude_path, None);
        assert_eq!(s.codex_path.as_deref(), Some("/bin/codex"));
        assert_eq!(Settings { interval_minutes: 99, ..Default::default() }.normalized().interval_minutes, 30);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let s: Settings = serde_json::from_str(r#"{"intervalMinutes": 10}"#).unwrap();
        assert_eq!(s.interval_minutes, 10);
        assert_eq!(s.thresholds, vec![80, 95]);
    }
}
