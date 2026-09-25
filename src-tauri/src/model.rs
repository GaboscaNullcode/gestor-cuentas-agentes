use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub fn cli_name(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }

    /// Environment variable that points the CLI at an account's config directory.
    pub fn config_env_var(self) -> &'static str {
        match self {
            Provider::Claude => "CLAUDE_CONFIG_DIR",
            Provider::Codex => "CODEX_HOME",
        }
    }

    /// Directory name the CLI uses under the home directory when the env var is unset.
    pub fn default_dir_name(self) -> &'static str {
        match self {
            Provider::Claude => ".claude",
            Provider::Codex => ".codex",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub provider: Provider,
    pub label: String,
    pub config_dir: PathBuf,
    /// Run the CLI with the config env var removed, so it uses its built-in default location.
    /// Needed because Claude derives the macOS Keychain entry name from CLAUDE_CONFIG_DIR.
    pub use_default_dir: bool,
    pub pinned: bool,
    pub alias_name: String,
    pub created_at: DateTime<Utc>,
    /// The app created `config_dir` itself, so it may offer to delete it. Missing in older
    /// accounts.json files, which therefore load as not deletable.
    #[serde(default)]
    pub created_by_app: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "name", rename_all = "camelCase")]
pub enum WindowKind {
    FiveHour,
    Weekly,
    WeeklyScoped(String),
    Other(String),
}

impl WindowKind {
    /// Stable identifier used in notification dedupe keys.
    pub fn key(&self) -> String {
        match self {
            WindowKind::FiveHour => "5h".to_string(),
            WindowKind::Weekly => "weekly".to_string(),
            WindowKind::WeeklyScoped(name) => format!("weekly:{name}"),
            WindowKind::Other(name) => format!("other:{name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub kind: WindowKind,
    pub used_pct: f32,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "message", rename_all = "camelCase")]
pub enum AccountStatus {
    Ok,
    Stale,
    NeedsLogin,
    RateLimited,
    CliMissing,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub plan: Option<String>,
    pub windows: Vec<Window>,
    /// Time of the last successful fetch whose windows are shown.
    pub fetched_at: DateTime<Utc>,
    pub status: AccountStatus,
    pub last_error: Option<String>,
}

impl UsageSnapshot {
    pub fn window(&self, kind: &WindowKind) -> Option<&Window> {
        self.windows.iter().find(|w| &w.kind == kind)
    }
}
