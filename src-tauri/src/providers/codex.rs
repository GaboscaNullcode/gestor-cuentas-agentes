use super::{failure, CliContext};
use crate::logging::redact;
use crate::model::{Account, AccountStatus, UsageSnapshot};
use crate::parse::codex::{parse_codex_rate_limits, read_rate_limits, CodexRpcError};
use chrono::Utc;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::BufReader;

const TIMEOUT: Duration = Duration::from_secs(45);

/// Starts `codex app-server` for this account, reads the rate limits and stops it.
/// Codex refreshes its own tokens while serving the request, so the app never touches them.
pub async fn fetch_usage(ctx: &CliContext, account: &Account) -> UsageSnapshot {
    let Some(cmd) = ctx.command(account, &["app-server"]) else {
        return failure(AccountStatus::CliMissing, None);
    };
    let mut command = cmd.to_command();
    command.stdin(Stdio::piped()).stderr(Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return failure(AccountStatus::CliMissing, Some(e.to_string())),
    };
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill().await;
        return failure(AccountStatus::Error("could not open codex app-server pipes".into()), None);
    };
    let result = tokio::time::timeout(TIMEOUT, read_rate_limits(BufReader::new(stdout), stdin)).await;
    let _ = child.kill().await;
    match result {
        Err(_) => failure(AccountStatus::Error("codex app-server timed out".into()), None),
        Ok(Err(CodexRpcError::Auth(_))) => failure(AccountStatus::NeedsLogin, None),
        Ok(Err(CodexRpcError::RateLimited(message))) => failure(AccountStatus::RateLimited, Some(message)),
        Ok(Err(CodexRpcError::Other(message))) => failure(AccountStatus::Error(message.clone()), Some(message)),
        Ok(Ok(value)) => {
            log::debug!("codex usage [{}]: {}", account.label, redact(&value.to_string()));
            parse_codex_rate_limits(&value, Utc::now())
        }
    }
}
