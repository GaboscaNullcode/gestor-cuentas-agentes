pub mod claude;
pub mod codex;

use crate::cli::locator::{locate, ShellEnv};
use crate::cli::run::{run, CliCommand};
use crate::model::{Account, AccountStatus, Provider, UsageSnapshot};
use crate::parse::claude::parse_claude_auth_status;
use crate::store::settings::Settings;
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const STATUS_TIMEOUT: Duration = Duration::from_secs(15);

/// Resolved binaries plus the environment every CLI invocation shares.
#[derive(Debug, Clone, Default)]
pub struct CliContext {
    pub claude: Option<PathBuf>,
    pub codex: Option<PathBuf>,
    pub path_env: Option<String>,
    pub work_dir: PathBuf,
}

impl CliContext {
    pub fn resolve(settings: &Settings, shell: &ShellEnv, home: &Path, work_dir: PathBuf) -> Self {
        let search = shell.get("PATH").map(str::to_string).or_else(|| std::env::var("PATH").ok());
        let find = |name: &str, configured: &Option<String>| {
            locate(name, configured.as_deref().map(Path::new), search.as_deref(), home)
        };
        Self {
            claude: find("claude", &settings.claude_path),
            codex: find("codex", &settings.codex_path),
            path_env: search.clone(),
            work_dir,
        }
    }

    pub fn binary(&self, provider: Provider) -> Option<&PathBuf> {
        match provider {
            Provider::Claude => self.claude.as_ref(),
            Provider::Codex => self.codex.as_ref(),
        }
    }

    pub fn command(&self, account: &Account, args: &[&str]) -> Option<CliCommand> {
        let program = self.binary(account.provider)?.clone();
        let (env_set, env_remove) = account_env(account);
        Some(CliCommand {
            program,
            args: args.iter().map(|a| a.to_string()).collect(),
            env_set,
            env_remove,
            cwd: Some(self.work_dir.clone()),
            path_env: self.path_env.clone(),
            new_process_group: false,
        })
    }
}

pub fn account_env(account: &Account) -> (Vec<(String, String)>, Vec<String>) {
    let var = account.provider.config_env_var().to_string();
    if account.use_default_dir {
        (Vec::new(), vec![var])
    } else {
        (vec![(var, account.config_dir.to_string_lossy().into_owned())], Vec::new())
    }
}

pub fn failure(status: AccountStatus, message: Option<String>) -> UsageSnapshot {
    UsageSnapshot { plan: None, windows: Vec::new(), fetched_at: Utc::now(), status, last_error: message }
}

pub async fn fetch_usage(ctx: &CliContext, account: &Account) -> UsageSnapshot {
    match account.provider {
        Provider::Claude => claude::fetch_usage(ctx, account).await,
        Provider::Codex => codex::fetch_usage(ctx, account).await,
    }
}

pub async fn is_logged_in(ctx: &CliContext, account: &Account) -> Result<bool, String> {
    let args: &[&str] = match account.provider {
        Provider::Claude => &["auth", "status"],
        Provider::Codex => &["login", "status"],
    };
    let cmd = ctx
        .command(account, args)
        .ok_or_else(|| format!("{} CLI not found", account.provider.cli_name()))?;
    let out = run(&cmd, STATUS_TIMEOUT).await.map_err(|e| format!("{e:?}"))?;
    Ok(match account.provider {
        Provider::Claude => parse_claude_auth_status(&out.stdout).is_some_and(|s| s.logged_in),
        Provider::Codex => out.code == Some(0),
    })
}

pub fn login_args(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["auth", "login"],
        Provider::Codex => &["login"],
    }
}

pub fn logout_args(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["auth", "logout"],
        Provider::Codex => &["logout"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Account, Provider};
    use chrono::Utc;
    use std::path::PathBuf;

    fn account(provider: Provider, use_default_dir: bool) -> Account {
        Account {
            id: "a1".into(),
            provider,
            label: "Work".into(),
            config_dir: PathBuf::from("/home/me/.claude-work"),
            use_default_dir,
            pinned: false,
            alias_name: "claude-work".into(),
            created_at: Utc::now(),
            created_by_app: false,
        }
    }

    #[test]
    fn explicit_dir_sets_env_var() {
        let (set, remove) = account_env(&account(Provider::Claude, false));
        assert_eq!(set, vec![("CLAUDE_CONFIG_DIR".to_string(), "/home/me/.claude-work".to_string())]);
        assert!(remove.is_empty());
    }

    #[test]
    fn default_dir_removes_env_var() {
        let (set, remove) = account_env(&account(Provider::Codex, true));
        assert!(set.is_empty());
        assert_eq!(remove, vec!["CODEX_HOME".to_string()]);
    }

    #[test]
    fn command_is_none_without_binary_and_carries_cwd_and_path() {
        let ctx = CliContext {
            claude: Some(PathBuf::from("/bin/claude")),
            codex: None,
            path_env: Some("/usr/bin".into()),
            work_dir: PathBuf::from("/data/work"),
        };
        assert!(ctx.command(&account(Provider::Codex, false), &["app-server"]).is_none());
        let cmd = ctx.command(&account(Provider::Claude, false), &["auth", "status"]).unwrap();
        assert_eq!(cmd.args, vec!["auth", "status"]);
        assert_eq!(cmd.cwd, Some(PathBuf::from("/data/work")));
        assert_eq!(cmd.path_env.as_deref(), Some("/usr/bin"));
    }
}
