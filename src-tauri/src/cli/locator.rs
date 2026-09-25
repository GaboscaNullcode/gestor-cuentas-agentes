use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const MARKER: &str = "__USAGE_MONITOR_ENV__";
const CAPTURED: [&str; 4] = ["PATH", "CLAUDE_CONFIG_DIR", "CODEX_HOME", "SHELL"];
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);

/// Environment variables read from the user's login shell.
#[derive(Debug, Clone, Default)]
pub struct ShellEnv {
    pub vars: HashMap<String, String>,
}

impl ShellEnv {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str).filter(|v| !v.is_empty())
    }
}

/// GUI apps on macOS (and some Linux launchers) do not inherit the shell's PATH or exports,
/// so the values are read from an interactive login shell. Falls back to the process env.
pub fn read_login_shell_env() -> ShellEnv {
    let mut vars: HashMap<String, String> = CAPTURED
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_string(), value)))
        .collect();
    #[cfg(unix)]
    for (key, value) in capture_from_shell() {
        if !value.is_empty() {
            vars.insert(key, value);
        }
    }
    ShellEnv { vars }
}

#[cfg(unix)]
fn capture_from_shell() -> Vec<(String, String)> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let script = CAPTURED
        .iter()
        .map(|key| format!("printf '{MARKER}{key}=%s\\n' \"${key}\""))
        .collect::<Vec<_>>()
        .join("; ");
    let Ok(mut child) = Command::new(&shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    let stdout = child.stdout.take();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut text);
        }
        let _ = tx.send(text);
    });
    match rx.recv_timeout(SHELL_TIMEOUT) {
        Ok(text) => {
            let _ = child.wait();
            parse_marked_env(&text)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Vec::new()
        }
    }
}

pub fn parse_marked_env(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| line.split_once(MARKER))
        .filter_map(|(_, rest)| rest.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// Finds a CLI binary: the configured path, then the search PATH, then well-known dirs.
/// A configured path that does not exist yields None so the UI can report it.
pub fn locate(name: &str, configured: Option<&Path>, search_path: Option<&str>, home: &Path) -> Option<PathBuf> {
    if let Some(path) = configured {
        return path.is_file().then(|| path.to_path_buf());
    }
    let names = executable_names(name);
    let from_path = search_path.into_iter().flat_map(std::env::split_paths);
    from_path
        .chain(well_known_dirs(home))
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)).collect::<Vec<_>>())
        .find(|candidate| candidate.is_file())
}

fn executable_names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![format!("{name}.exe"), format!("{name}.cmd"), format!("{name}.bat")]
    } else {
        vec![name.to_string()]
    }
}

pub fn well_known_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        home.join(".local").join("bin"),
        home.join(".claude").join("local"),
        home.join(".npm-global").join("bin"),
        home.join(".bun").join("bin"),
        home.join(".cargo").join("bin"),
    ];
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    if cfg!(target_os = "linux") {
        dirs.extend(["/usr/local/bin", "/usr/bin", "/home/linuxbrew/.linuxbrew/bin"].map(PathBuf::from));
    }
    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Programs").join("claude"));
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn exe_name(name: &str) -> String {
        if cfg!(windows) { format!("{name}.cmd") } else { name.to_string() }
    }

    #[test]
    fn parses_marked_env_and_ignores_rc_noise() {
        let out = "Welcome!\n__USAGE_MONITOR_ENV__PATH=/usr/bin:/opt/homebrew/bin\n__USAGE_MONITOR_ENV__CLAUDE_CONFIG_DIR=/Users/me/.claude\n__USAGE_MONITOR_ENV__CODEX_HOME=\n";
        let vars = parse_marked_env(out);
        assert_eq!(vars[0], ("PATH".to_string(), "/usr/bin:/opt/homebrew/bin".to_string()));
        assert_eq!(vars[1], ("CLAUDE_CONFIG_DIR".to_string(), "/Users/me/.claude".to_string()));
        assert_eq!(vars[2], ("CODEX_HOME".to_string(), String::new()));
    }

    #[test]
    fn locate_finds_binary_on_search_path() {
        let bin = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        fs::write(bin.path().join(exe_name("claude")), "").unwrap();
        let search = std::env::join_paths([bin.path()]).unwrap();
        let found = locate("claude", None, search.to_str(), home.path()).unwrap();
        assert_eq!(found, bin.path().join(exe_name("claude")));
    }

    #[test]
    fn locate_prefers_configured_path_and_rejects_missing_configured_path() {
        let bin = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let configured = bin.path().join("my-claude");
        fs::write(&configured, "").unwrap();
        assert_eq!(locate("claude", Some(&configured), None, home.path()), Some(configured.clone()));
        let missing = bin.path().join("nope");
        assert_eq!(locate("claude", Some(&missing), None, home.path()), None);
    }

    #[test]
    fn locate_uses_well_known_dirs_when_path_misses() {
        let home = tempfile::tempdir().unwrap();
        let local_bin = home.path().join(".local").join("bin");
        fs::create_dir_all(&local_bin).unwrap();
        fs::write(local_bin.join(exe_name("codex")), "").unwrap();
        let empty = tempfile::tempdir().unwrap();
        let search = std::env::join_paths([empty.path()]).unwrap();
        assert_eq!(
            locate("codex", None, search.to_str(), home.path()),
            Some(local_bin.join(exe_name("codex")))
        );
    }

    #[test]
    fn locate_returns_none_when_absent() {
        let home = tempfile::tempdir().unwrap();
        let empty = tempfile::tempdir().unwrap();
        let search = std::env::join_paths([empty.path()]).unwrap();
        assert_eq!(locate("definitely-not-a-cli", None, search.to_str(), home.path()), None);
    }
}
