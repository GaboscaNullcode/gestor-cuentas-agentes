use crate::model::{Account, Provider};
use chrono::Utc;
use std::path::{Path, PathBuf};

pub fn slugify(label: &str) -> String {
    let dashed: String = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = dashed.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    if slug.is_empty() { "account".to_string() } else { slug }
}

pub fn unique_alias(provider: Provider, label: &str, existing: &[Account]) -> String {
    let base = format!("{}-{}", provider.cli_name(), slugify(label));
    let mut candidate = base.clone();
    let mut n = 2;
    while existing.iter().any(|a| a.alias_name == candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

pub fn proposed_config_dir(provider: Provider, label: &str, home: &Path) -> PathBuf {
    home.join(format!(".{}-{}", provider.cli_name(), slugify(label)))
}

pub fn new_account(provider: Provider, label: &str, config_dir: PathBuf, use_default_dir: bool, existing: &[Account]) -> Account {
    Account {
        id: uuid::Uuid::new_v4().to_string(),
        provider,
        label: label.trim().to_string(),
        alias_name: unique_alias(provider, label, existing),
        config_dir,
        use_default_dir,
        pinned: existing.is_empty(),
        created_at: Utc::now(),
    }
}

pub fn validate_new(provider: Provider, config_dir: &Path, existing: &[Account]) -> Result<(), String> {
    if existing.iter().any(|a| a.provider == provider && a.config_dir == config_dir) {
        return Err(format!("{} is already registered", config_dir.display()));
    }
    Ok(())
}

pub fn set_pinned(accounts: &mut [Account], id: &str) {
    for account in accounts.iter_mut() {
        account.pinned = account.id == id;
    }
}

/// Directories the app may delete: inside home, not home itself, not a CLI default dir.
pub fn can_delete_dir(account: &Account, home: &Path) -> bool {
    let dir = &account.config_dir;
    !account.use_default_dir
        && dir.starts_with(home)
        && dir != home
        && [Provider::Claude, Provider::Codex]
            .iter()
            .all(|p| *dir != home.join(p.default_dir_name()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;
    use std::path::PathBuf;

    #[test]
    fn slugifies_labels() {
        assert_eq!(slugify("  Mi Cuenta Personal! "), "mi-cuenta-personal");
        assert_eq!(slugify("***"), "account");
    }

    #[test]
    fn aliases_are_unique_and_first_account_is_pinned() {
        let home = PathBuf::from("/home/me");
        let first = new_account(Provider::Claude, "Work", home.join(".claude-work"), false, &[]);
        assert_eq!(first.alias_name, "claude-work");
        assert!(first.pinned);
        let second = new_account(Provider::Claude, "work", home.join(".claude-work2"), false, &[first.clone()]);
        assert_eq!(second.alias_name, "claude-work-2");
        assert!(!second.pinned);
    }

    #[test]
    fn proposes_dir_under_home() {
        assert_eq!(
            proposed_config_dir(Provider::Codex, "Side Project", &PathBuf::from("/home/me")),
            PathBuf::from("/home/me/.codex-side-project")
        );
    }

    #[test]
    fn rejects_duplicate_dir_for_same_provider() {
        let dir = PathBuf::from("/home/me/.claude-work");
        let existing = vec![new_account(Provider::Claude, "Work", dir.clone(), false, &[])];
        assert!(validate_new(Provider::Claude, &dir, &existing).is_err());
        assert!(validate_new(Provider::Codex, &dir, &existing).is_ok());
    }

    #[test]
    fn pins_exactly_one() {
        let mut accounts = vec![
            new_account(Provider::Claude, "A", PathBuf::from("/h/.claude-a"), false, &[]),
            new_account(Provider::Codex, "B", PathBuf::from("/h/.codex-b"), false, &[]),
        ];
        let id = accounts[1].id.clone();
        set_pinned(&mut accounts, &id);
        assert!(!accounts[0].pinned && accounts[1].pinned);
    }

    #[test]
    fn default_and_home_dirs_are_protected() {
        let home = PathBuf::from("/home/me");
        let mut a = new_account(Provider::Claude, "Main", home.join(".claude"), false, &[]);
        assert!(!can_delete_dir(&a, &home));
        a.config_dir = home.join(".codex");
        assert!(!can_delete_dir(&a, &home));
        a.config_dir = home.clone();
        assert!(!can_delete_dir(&a, &home));
        a.config_dir = PathBuf::from("/etc/claude");
        assert!(!can_delete_dir(&a, &home));
        a.config_dir = home.join(".claude-work");
        assert!(can_delete_dir(&a, &home));
        a.use_default_dir = true;
        assert!(!can_delete_dir(&a, &home));
    }
}
