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
    /// Unix: start the child in its own process group so `kill_tree` can reach the
    /// grandchildren an npm shim spawns. Only for long-lived children that get cancelled.
    pub new_process_group: bool,
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
        #[cfg(unix)]
        if self.new_process_group {
            cmd.process_group(0);
        }
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Grace period between asking the process tree to stop and killing it.
#[cfg(unix)]
const TERM_GRACE: Duration = Duration::from_millis(500);

/// Stops `child` and everything it spawned (an npm-installed CLI is a node shim whose real
/// process would otherwise keep running and, for `codex login`, keep port 1455 bound).
///
/// Unix: signals the child's process group, so the child must have been spawned with
/// `new_process_group`; SIGTERM first, SIGKILL after a short grace period.
/// Windows: `taskkill /T /F` on the child's pid, which walks the process tree.
pub async fn kill_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(unix)]
        {
            let pgid = pid as libc::pid_t;
            // SAFETY: killpg only sends a signal; an unknown group yields ESRCH, which is ignored.
            unsafe { libc::killpg(pgid, libc::SIGTERM) };
            let _ = tokio::time::timeout(TERM_GRACE, child.wait()).await;
            // The leader may be gone while grandchildren ignore SIGTERM; the group id stays
            // reserved while any member lives, so this cannot hit an unrelated process.
            unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
        #[cfg(windows)]
        {
            let mut taskkill = Command::new("taskkill");
            taskkill
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW);
            let _ = tokio::time::timeout(Duration::from_secs(5), taskkill.status()).await;
        }
    }
    let _ = child.kill().await; // reaps the child; a no-op error when it already exited
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

    fn is_alive(pid: i32) -> bool {
        // SAFETY: signal 0 only checks for existence.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[tokio::test]
    async fn kill_tree_stops_grandchildren() {
        let mut cmd = sh("sleep 30 & echo $!; wait");
        cmd.new_process_group = true;
        let mut child = cmd.to_command().spawn().unwrap();
        let mut stdout = tokio::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut stdout, &mut line).await.unwrap();
        let grandchild: i32 = line.trim().parse().unwrap();
        assert!(is_alive(grandchild));
        kill_tree(&mut child).await;
        // The orphaned grandchild is reaped by init shortly after SIGKILL.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while is_alive(grandchild) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(!is_alive(grandchild), "grandchild {grandchild} survived kill_tree");
    }

    #[tokio::test]
    async fn reports_spawn_errors() {
        let cmd = CliCommand { program: PathBuf::from("/nonexistent/bin"), ..Default::default() };
        assert!(matches!(run(&cmd, Duration::from_secs(1)).await, Err(RunError::Spawn(_))));
    }
}
