use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Clone, Default)]
pub struct CliCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env_set: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// PATH for the child, so hooks and shims the CLI spawns resolve like in a terminal.
    pub path_env: Option<String>,
}

impl CliCommand {
    /// Builds a command with piped stdout/stderr, null stdin and kill-on-drop.
    /// Windows `.cmd`/`.bat` shims (npm installs) are run through `cmd /C` without a console window.
    pub fn to_command(&self) -> Command {
        let is_script = cfg!(windows)
            && self
                .program
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));
        let mut cmd = if is_script {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&self.program);
            c
        } else {
            Command::new(&self.program)
        };
        cmd.args(&self.args);
        if let Some(path) = &self.path_env {
            cmd.env("PATH", path);
        }
        for key in &self.env_remove {
            cmd.env_remove(key);
        }
        for (key, value) in &self.env_set {
            cmd.env(key, value);
        }
        if let Some(dir) = &self.cwd {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        cmd
    }
}

#[derive(Debug, Clone)]
pub struct CliOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    Timeout,
    Spawn(String),
}

/// Runs the command to completion; on timeout the child is killed (kill_on_drop).
pub async fn run(command: &CliCommand, timeout: Duration) -> Result<CliOutput, RunError> {
    let child = command.to_command().spawn().map_err(|e| RunError::Spawn(e.to_string()))?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Err(_) => Err(RunError::Timeout),
        Ok(Err(e)) => Err(RunError::Spawn(e.to_string())),
        Ok(Ok(output)) => Ok(CliOutput {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn sh(script: &str) -> CliCommand {
        CliCommand {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), script.into()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn captures_stdout_stderr_and_exit_code() {
        let out = run(&sh("echo hi; echo err >&2; exit 3"), Duration::from_secs(5)).await.unwrap();
        assert_eq!(out.stdout.trim(), "hi");
        assert_eq!(out.stderr.trim(), "err");
        assert_eq!(out.code, Some(3));
    }

    #[tokio::test]
    async fn sets_and_removes_env_vars() {
        let mut cmd = sh("printf '%s|%s' \"$FOO\" \"$HOME\"");
        cmd.env_set = vec![("FOO".into(), "bar".into())];
        cmd.env_remove = vec!["HOME".into()];
        let out = run(&cmd, Duration::from_secs(5)).await.unwrap();
        assert_eq!(out.stdout, "bar|");
    }

    #[tokio::test]
    async fn uses_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let mut cmd = sh("pwd");
        cmd.cwd = Some(dir.path().to_path_buf());
        let out = run(&cmd, Duration::from_secs(5)).await.unwrap();
        assert_eq!(
            std::fs::canonicalize(out.stdout.trim()).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[tokio::test]
    async fn times_out() {
        let err = run(&sh("sleep 5"), Duration::from_millis(200)).await.unwrap_err();
        assert_eq!(err, RunError::Timeout);
    }

    #[tokio::test]
    async fn reports_spawn_errors() {
        let cmd = CliCommand { program: PathBuf::from("/nonexistent/bin"), ..Default::default() };
        assert!(matches!(run(&cmd, Duration::from_secs(1)).await, Err(RunError::Spawn(_))));
    }
}
