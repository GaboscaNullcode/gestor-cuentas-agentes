use super::{failure, CliContext, STATUS_TIMEOUT};
use crate::cli::run::{run, CliOutput, RunError};
use crate::logging::redact;
use crate::model::{Account, AccountStatus, UsageSnapshot};
use crate::parse::claude::{parse_claude_auth_status, parse_claude_usage, ClaudeAuthStatus};
use chrono::Utc;
use std::time::Duration;

const USAGE_ARGS: [&str; 6] = ["-p", "/usage", "--no-session-persistence", "--output-format", "stream-json", "--verbose"];
const USAGE_TIMEOUT: Duration = Duration::from_secs(45);

pub async fn fetch_usage(ctx: &CliContext, account: &Account) -> UsageSnapshot {
    let Some(cmd) = ctx.command(account, &USAGE_ARGS) else {
        return failure(AccountStatus::CliMissing, None);
    };
    let out = match run(&cmd, USAGE_TIMEOUT).await {
        Ok(out) => out,
        Err(RunError::Timeout) => return failure(AccountStatus::Error("claude /usage timed out".into()), None),
        Err(RunError::Spawn(e)) => return failure(AccountStatus::CliMissing, Some(e)),
    };
    let interesting: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| l.contains("usage_report") || l.contains("\"type\":\"result\""))
        .collect();
    log::debug!("claude usage [{}]: {}", account.label, redact(&interesting.join("\n")));
    let status = auth_status(ctx, account).await;
    match parse_claude_usage(&out.stdout, Utc::now()) {
        Some(mut snapshot) => {
            snapshot.plan = status.and_then(|s| s.plan);
            snapshot
        }
        None => classify_missing_report(status.as_ref(), &out),
    }
}

async fn auth_status(ctx: &CliContext, account: &Account) -> Option<ClaudeAuthStatus> {
    let cmd = ctx.command(account, &["auth", "status"])?;
    let out = run(&cmd, STATUS_TIMEOUT).await.ok()?;
    parse_claude_auth_status(&out.stdout)
}

/// Explains why `/usage` returned no report. Only stderr is inspected for rate limits,
/// because stdout also carries the user's hook output.
pub fn classify_missing_report(status: Option<&ClaudeAuthStatus>, out: &CliOutput) -> UsageSnapshot {
    if status.is_some_and(|s| !s.logged_in) {
        return failure(AccountStatus::NeedsLogin, None);
    }
    let stderr = out.stderr.to_lowercase();
    if stderr.contains("429") || stderr.contains("rate limit") || stderr.contains("rate_limit") {
        return failure(AccountStatus::RateLimited, None);
    }
    let tail: String = out.stderr.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
    failure(
        AccountStatus::Error("Claude did not return a usage report".into()),
        (!tail.trim().is_empty()).then_some(tail),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::run::CliOutput;
    use crate::model::AccountStatus;
    use crate::parse::claude::ClaudeAuthStatus;

    fn output(stderr: &str) -> CliOutput {
        CliOutput { code: Some(0), stdout: String::new(), stderr: stderr.into() }
    }

    #[test]
    fn logged_out_means_needs_login() {
        let status = ClaudeAuthStatus { logged_in: false, email: None, plan: None };
        let snap = classify_missing_report(Some(&status), &output(""));
        assert_eq!(snap.status, AccountStatus::NeedsLogin);
    }

    #[test]
    fn stderr_429_means_rate_limited() {
        let status = ClaudeAuthStatus { logged_in: true, email: None, plan: None };
        let snap = classify_missing_report(Some(&status), &output("API Error: 429 rate_limit_error"));
        assert_eq!(snap.status, AccountStatus::RateLimited);
    }

    #[test]
    fn anything_else_is_an_error() {
        let snap = classify_missing_report(None, &output("weird"));
        assert!(matches!(snap.status, AccountStatus::Error(_)));
    }
}
