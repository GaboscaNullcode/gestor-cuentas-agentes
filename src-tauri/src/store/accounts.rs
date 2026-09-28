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
        created_by_app: false,
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

pub const MAX_LABEL_CHARS: usize = 40;

/// Changes only the display label. The alias name and config dir stay as they were, so
/// renaming never breaks a terminal alias or moves a session.
pub fn rename(accounts: &mut [Account], id: &str, label: &str) -> Result<(), String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("Label is required.".into());
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        return Err(format!("Label must be at most {MAX_LABEL_CHARS} characters."));
    }
    let account = accounts.iter_mut().find(|a| a.id == id).ok_or("Account not found.")?;
    account.label = label.to_string();
    Ok(())
}

/// True when the raw path has a "." or ".." segment. `Path::components()`/`iter()` silently
/// normalize those away in the middle of a path, so this scans the raw string lexically.
pub fn has_dot_segment(path: &Path) -> bool {
    path.to_string_lossy()
        .split(std::path::is_separator)
        .any(|segment| segment == "." || segment == "..")
}

/// Turns user input into an absolute, lexically normalized config dir: trims, expands a
/// leading `~` to `home`, and rejects relative paths and "."/".." segments, so the CLI never
/// gets a cwd-relative dir and duplicates cannot hide behind `..`.
pub fn normalize_config_dir(input: &str, home: &Path) -> Result<PathBuf, String> {
    let input = input.trim();
    let path = if input == "~" {
        home.to_path_buf()
    } else if let Some(rest) = input.strip_prefix("~/").or_else(|| input.strip_prefix("~\\")) {
        home.join(rest)
    } else {
        PathBuf::from(input)
    };
    if input.is_empty() || !path.is_absolute() {
        return Err(format!("\"{input}\" must be an absolute path."));
    }
    if has_dot_segment(&path) {
        return Err(format!("\"{input}\" must not contain \".\" or \"..\" segments."));
    }
    Ok(path)
}

/// Directories the app may delete: created by the app, inside home, not home itself, not a
/// CLI default dir. `created_by_app` is what protects `~/.Claude` on a case-insensitive
/// filesystem, where the lexical default-dir check cannot see that it is `~/.claude`.
pub fn can_delete_dir(account: &Account, home: &Path) -> bool {
    let dir = &account.config_dir;
    // A config_dir like `home/.claude-x/../.claude` must never look deletable.
    account.created_by_app
        && !account.use_default_dir
        && !has_dot_segment(dir)
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
    fn rename_changes_label_only() {
        let mut accounts = vec![new_account(Provider::Claude, "Work", PathBuf::from("/h/.claude-work"), false, &[])];
        let id = accounts[0].id.clone();
        let alias = accounts[0].alias_name.clone();
        rename(&mut accounts, &id, "  Office  ").unwrap();
        assert_eq!(accounts[0].label, "Office");
        assert_eq!(accounts[0].alias_name, alias);
        assert_eq!(accounts[0].config_dir, PathBuf::from("/h/.claude-work"));
    }

    #[test]
    fn rename_rejects_empty_long_or_unknown() {
        let mut accounts = vec![new_account(Provider::Claude, "Work", PathBuf::from("/h/.claude-work"), false, &[])];
        let id = accounts[0].id.clone();
        assert!(rename(&mut accounts, &id, "   ").is_err());
        assert!(rename(&mut accounts, &id, &"x".repeat(MAX_LABEL_CHARS + 1)).is_err());
        assert!(rename(&mut accounts, "missing", "Office").is_err());
        assert_eq!(accounts[0].label, "Work");
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
        a.created_by_app = true;
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

    #[test]
    fn non_normalized_dirs_are_protected() {
        let home = PathBuf::from("/home/me");
        let mut a = new_account(Provider::Claude, "X", home.join(".claude-x").join("..").join(".claude"), false, &[]);
        a.created_by_app = true;
        assert!(!can_delete_dir(&a, &home));
        a.config_dir = home.join(".").join(".claude-work");
        assert!(!can_delete_dir(&a, &home));
    }

    #[test]
    fn dirs_the_app_did_not_create_are_never_deletable() {
        let home = PathBuf::from("/home/me");
        // `~/.Claude` passes every lexical check but is `~/.claude` on a case-insensitive disk.
        let mut a = new_account(Provider::Claude, "Main", home.join(".Claude"), false, &[]);
        assert!(!a.created_by_app);
        assert!(!can_delete_dir(&a, &home));
        a.config_dir = home.join(".claude-work");
        assert!(!can_delete_dir(&a, &home));
        a.created_by_app = true;
        assert!(can_delete_dir(&a, &home));
    }

    #[test]
    fn older_account_files_load_as_not_created_by_app() {
        let json = r#"{"id":"x","provider":"claude","label":"Main","configDir":"/home/me/.claude-x","useDefaultDir":false,"pinned":true,"aliasName":"claude-main","createdAt":"2026-09-25T00:00:00Z"}"#;
        let account: Account = serde_json::from_str(json).unwrap();
        assert!(!account.created_by_app);
    }

    #[test]
    fn normalizes_config_dirs() {
        let home = PathBuf::from("/home/me");
        assert_eq!(normalize_config_dir(" ~/.claude-work ", &home), Ok(home.join(".claude-work")));
        assert_eq!(normalize_config_dir("~", &home), Ok(home.clone()));
        assert_eq!(normalize_config_dir("/opt/claude-x", &home), Ok(PathBuf::from("/opt/claude-x")));
        assert!(normalize_config_dir("foo", &home).is_err());
        assert!(normalize_config_dir("~other/.claude", &home).is_err());
        assert!(normalize_config_dir("/h/.claude-a/../.claude", &home).is_err());
        assert!(normalize_config_dir("~/./.claude-work", &home).is_err());
        assert!(normalize_config_dir("   ", &home).is_err());
    }
}
