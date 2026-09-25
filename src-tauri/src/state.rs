use crate::cli::locator::{read_login_shell_env, ShellEnv};
use crate::model::Account;
use crate::providers::CliContext;
use crate::store::cache::UsageCache;
use crate::store::settings::Settings;
use crate::store::{read_json, write_json_atomic};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub home: PathBuf,
}

impl AppPaths {
    pub fn accounts_file(&self) -> PathBuf { self.data_dir.join("accounts.json") }
    pub fn settings_file(&self) -> PathBuf { self.data_dir.join("settings.json") }
    pub fn cache_file(&self) -> PathBuf { self.data_dir.join("usage-cache.json") }
    /// Neutral cwd for CLI runs, so no project hooks or settings apply.
    pub fn work_dir(&self) -> PathBuf { self.data_dir.join("work") }
    pub fn aliases_sh(&self) -> PathBuf { self.config_dir.join("aliases.sh") }
    pub fn aliases_ps1(&self) -> PathBuf { self.config_dir.join("aliases.ps1") }
}

#[derive(Debug, Clone, Default)]
pub struct Runtime {
    pub next_due: Option<DateTime<Utc>>,
    pub errors: u32,
    pub in_flight: bool,
    pub last_manual: Option<DateTime<Utc>>,
}

/// Shared state. std Mutexes are held only for short, non-async sections.
pub struct AppState {
    pub paths: AppPaths,
    pub shell: ShellEnv,
    pub accounts: Mutex<Vec<Account>>,
    pub settings: Mutex<Settings>,
    pub cache: Mutex<UsageCache>,
    pub runtime: Mutex<HashMap<String, Runtime>>,
    pub cli: Mutex<CliContext>,
}

impl AppState {
    pub fn load(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let paths = AppPaths {
            data_dir: app.path().app_data_dir()?,
            config_dir: app.path().app_config_dir()?,
            home: app.path().home_dir()?,
        };
        std::fs::create_dir_all(paths.work_dir())?;
        let shell = read_login_shell_env();
        let settings = read_json::<Settings>(&paths.settings_file()).normalized();
        let accounts: Vec<Account> = read_json(&paths.accounts_file());
        let mut cache: UsageCache = read_json(&paths.cache_file());
        cache.prune_sent(Utc::now());
        let cli = CliContext::resolve(&settings, &shell, &paths.home, paths.work_dir());
        log::info!("claude CLI: {:?}, codex CLI: {:?}", cli.claude, cli.codex);
        Ok(Self {
            paths,
            shell,
            accounts: Mutex::new(accounts),
            settings: Mutex::new(settings),
            cache: Mutex::new(cache),
            runtime: Mutex::new(HashMap::new()),
            cli: Mutex::new(cli),
        })
    }

    pub fn save_accounts(&self) {
        let accounts = self.accounts.lock().unwrap().clone();
        if let Err(e) = write_json_atomic(&self.paths.accounts_file(), &accounts) {
            log::error!("saving accounts failed: {e}");
        }
    }

    pub fn save_cache(&self) {
        let cache = self.cache.lock().unwrap().clone();
        if let Err(e) = write_json_atomic(&self.paths.cache_file(), &cache) {
            log::error!("saving cache failed: {e}");
        }
    }

    pub fn save_settings(&self) {
        let settings = self.settings.lock().unwrap().clone();
        if let Err(e) = write_json_atomic(&self.paths.settings_file(), &settings) {
            log::error!("saving settings failed: {e}");
        }
    }
}
