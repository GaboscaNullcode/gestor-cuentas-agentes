use crate::commands::accounts_changed;
use crate::model::Account;
use crate::providers;
use crate::scheduler;
use crate::state::AppState;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::ChildStdin;
use tokio::sync::{oneshot, Mutex};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginProgress {
    pub url: Option<String>,
    pub needs_code: bool,
}

/// Finds the first https URL (skipping OSC-8 escape codes) and whether the CLI asks for a code.
pub fn scan_login_output(text: &str) -> LoginProgress {
    let url = text
        .split(|c: char| c.is_whitespace() || c == '\u{1b}' || c == '\u{7}')
        .find_map(|token| token.find("https://").map(|i| token[i..].to_string()));
    LoginProgress { url, needs_code: text.to_lowercase().contains("paste code") }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginProgressEvent {
    account_id: String,
    url: Option<String>,
    needs_code: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginFinished {
    pub account_id: String,
    pub ok: bool,
    pub error: Option<String>,
}

struct Session {
    stdin: Option<ChildStdin>,
    cancel: Option<oneshot::Sender<()>>,
}

/// One sign-in at a time: `codex login` binds a fixed local port.
#[derive(Default)]
pub struct LoginManager {
    session: Mutex<Option<Session>>,
}

impl LoginManager {
    /// Spawns the CLI login for `account`. When `is_new` and sign-in fails, the account is
    /// removed and `cleanup_dir` (a dir the app created) is deleted if still empty.
    pub async fn start(&self, app: AppHandle, account: Account, cleanup_dir: Option<PathBuf>, is_new: bool) -> Result<(), String> {
        let mut guard = self.session.lock().await;
        if guard.is_some() {
            return Err("Another sign-in is already in progress.".into());
        }
        let ctx = app.state::<AppState>().cli.lock().unwrap().clone();
        let cmd = ctx
            .command(&account, providers::login_args(account.provider))
            .ok_or_else(|| format!("{} CLI not found.", account.provider.cli_name()))?;
        let mut command = cmd.to_command();
        command.stdin(Stdio::piped()).kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let (cancel_tx, cancel_rx) = oneshot::channel();
        *guard = Some(Session { stdin: child.stdin.take(), cancel: Some(cancel_tx) });
        drop(guard);

        let output = Arc::new(StdMutex::new(String::new()));
        if let Some(stdout) = child.stdout.take() {
            pump(app.clone(), account.id.clone(), stdout, output.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            pump(app.clone(), account.id.clone(), stderr, output.clone());
        }

        tauri::async_runtime::spawn(async move {
            let exited = tokio::select! {
                _ = child.wait() => true,
                _ = cancel_rx => false,
            };
            if !exited {
                let _ = child.kill().await;
            }
            let state = app.state::<AppState>();
            state.login.clear().await;
            let ok = exited && providers::is_logged_in(&ctx, &account).await.unwrap_or(false);
            if ok {
                scheduler::schedule_now(&app, &account.id);
                scheduler::run_due(&app);
            } else if is_new {
                state.accounts.lock().unwrap().retain(|a| a.id != account.id);
                if let Some(dir) = cleanup_dir {
                    let _ = std::fs::remove_dir(dir); // only succeeds when empty
                }
                accounts_changed(&app);
            }
            let error = match (ok, exited) {
                (true, _) => None,
                (false, false) => Some("Sign-in cancelled.".to_string()),
                (false, true) => Some("Sign-in did not complete.".to_string()),
            };
            let _ = app.emit("login-finished", LoginFinished { account_id: account.id.clone(), ok, error });
        });
        Ok(())
    }

    pub async fn submit_code(&self, code: &str) -> Result<(), String> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or("No sign-in in progress.")?;
        let stdin = session.stdin.as_mut().ok_or("This sign-in does not accept a code.")?;
        stdin.write_all(format!("{}\n", code.trim()).as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())
    }

    pub async fn cancel(&self) {
        if let Some(session) = self.session.lock().await.as_mut() {
            if let Some(tx) = session.cancel.take() {
                let _ = tx.send(());
            }
        }
    }

    async fn clear(&self) {
        *self.session.lock().await = None;
    }
}

/// Reads in chunks, not lines: Claude's code prompt has no trailing newline.
fn pump<R: AsyncRead + Unpin + Send + 'static>(app: AppHandle, account_id: String, mut reader: R, output: Arc<StdMutex<String>>) {
    tauri::async_runtime::spawn(async move {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 {
                break;
            }
            let progress = {
                let mut text = output.lock().unwrap();
                text.push_str(&String::from_utf8_lossy(&buf[..n]));
                scan_login_output(&text)
            };
            let _ = app.emit(
                "login-progress",
                LoginProgressEvent { account_id: account_id.clone(), url: progress.url, needs_code: progress.needs_code },
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_url_from_osc8_claude_output_and_detects_code_prompt() {
        let text = "Opening browser to sign in…\nIf the browser didn't open, visit: \u{1b}]8;;https://claude.com/cai/oauth/authorize?code=true&state=abc\u{1b}\\https://claude.com/cai/oauth/authorize?code=true&state=abc\u{1b}]8;;\u{1b}\\\nPaste code here if prompted > ";
        let progress = scan_login_output(text);
        assert_eq!(progress.url.as_deref(), Some("https://claude.com/cai/oauth/authorize?code=true&state=abc"));
        assert!(progress.needs_code);
    }

    #[test]
    fn prefers_auth_url_over_localhost_for_codex() {
        let text = "Starting local login server on http://localhost:1455.\nIf your browser did not open, navigate to this URL to authenticate:\n\nhttps://auth.openai.com/oauth/authorize?client_id=x\n";
        let progress = scan_login_output(text);
        assert_eq!(progress.url.as_deref(), Some("https://auth.openai.com/oauth/authorize?client_id=x"));
        assert!(!progress.needs_code);
    }

    #[test]
    fn nothing_found_yet() {
        assert_eq!(scan_login_output("Opening browser"), LoginProgress::default());
    }
}
