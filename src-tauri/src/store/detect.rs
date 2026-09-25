use crate::cli::locator::ShellEnv;
use crate::model::{Account, Provider};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedAccount {
    pub provider: Provider,
    pub config_dir: PathBuf,
    pub use_default_dir: bool,
}

/// Finds CLI config dirs that already hold a session and are not registered yet.
/// A config env var exported in the login shell wins over the default location, because
/// Claude's macOS Keychain entry name depends on whether that variable is set.
pub fn detect_existing(home: &Path, shell: &ShellEnv, existing: &[Account]) -> Vec<DetectedAccount> {
    [Provider::Claude, Provider::Codex]
        .into_iter()
        .filter_map(|provider| {
            let (config_dir, use_default_dir) = match shell.get(provider.config_env_var()) {
                Some(dir) => (PathBuf::from(dir), false),
                None => (home.join(provider.default_dir_name()), true),
            };
            let has_session = match provider {
                Provider::Claude => config_dir.is_dir(),
                Provider::Codex => config_dir.join("auth.json").is_file(),
            };
            let registered = existing.iter().any(|a| a.provider == provider && a.config_dir == config_dir);
            (has_session && !registered).then_some(DetectedAccount { provider, config_dir, use_default_dir })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::locator::ShellEnv;
    use crate::model::Provider;
    use std::fs;

    #[test]
    fn detects_default_dirs() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        fs::write(home.path().join(".codex").join("auth.json"), "{}").unwrap();
        let found = detect_existing(home.path(), &ShellEnv::default(), &[]);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|d| d.use_default_dir));
    }

    #[test]
    fn codex_without_auth_file_is_not_detected() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        assert!(detect_existing(home.path(), &ShellEnv::default(), &[]).is_empty());
    }

    #[test]
    fn detect_prefers_shell_env_dir() {
        let home = tempfile::tempdir().unwrap();
        let custom = home.path().join(".claude");
        fs::create_dir_all(&custom).unwrap();
        let mut shell = ShellEnv::default();
        shell.vars.insert("CLAUDE_CONFIG_DIR".into(), custom.to_string_lossy().into_owned());
        let found = detect_existing(home.path(), &shell, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].provider, Provider::Claude);
        assert_eq!(found[0].config_dir, custom);
        assert!(!found[0].use_default_dir);
    }

    #[test]
    fn skips_already_registered_dirs() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        let existing = vec![crate::store::accounts::new_account(Provider::Claude, "Main", home.path().join(".claude"), true, &[])];
        assert!(detect_existing(home.path(), &ShellEnv::default(), &existing).is_empty());
    }
}
