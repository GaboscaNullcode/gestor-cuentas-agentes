use super::{failure, CliContext, STATUS_TIMEOUT};
use crate::cli::run::{run, CliOutput, RunError};
use crate::logging::redact;
use crate::model::{Account, AccountStatus, UsageSnapshot};
use crate::parse::claude::{parse_claude_auth_status, parse_claude_usage, synthetic_messages, ClaudeAuthStatus};
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

const AUTH_HINTS: [&str; 10] =
    ["not logged in", "log in", "login", "sign in", "expired", "invalid", "revoked", "unauthorized", "401", "oauth"];
// The first three are specific enough to trust in stderr; the rest only in synthetic messages.
const RATE_LIMIT_HINTS: [&str; 6] =
    ["rate limit", "rate_limit", "429", "too many requests", "usage limit", "hit your limit"];
const MAX_ERROR_CHARS: usize = 300;

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| text.contains(n))
}

fn last_chars(text: &str, n: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(n)).collect()
}

/// Explains why `/usage` returned no report. stdout is only trusted through the CLI's own
/// synthetic assistant messages, because the rest of stdout can be the user's hook output.
pub fn classify_missing_report(status: Option<&ClaudeAuthStatus>, out: &CliOutput) -> UsageSnapshot {
    if status.is_some_and(|s| !s.logged_in) {
        return failure(AccountStatus::NeedsLogin, None);
    }
    let synthetic = synthetic_messages(&out.stdout).join("\n");
    let synthetic_lower = synthetic.to_lowercase();
    // Auth before rate limits: an auth message may mention limits, the reverse is unlikely.
    if contains_any(&synthetic_lower, &AUTH_HINTS) {
        return failure(AccountStatus::NeedsLogin, Some(last_chars(synthetic.trim(), MAX_ERROR_CHARS)));
    }
    let stderr = out.stderr.to_lowercase();
    if contains_any(&synthetic_lower, &RATE_LIMIT_HINTS) || contains_any(&stderr, &RATE_LIMIT_HINTS[..3]) {
        return failure(AccountStatus::RateLimited, None);
    }
    let detail = [synthetic.trim().to_string(), last_chars(out.stderr.trim(), MAX_ERROR_CHARS)]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    failure(
        AccountStatus::Error("Claude did not return a usage report".into()),
        (!detail.is_empty()).then(|| last_chars(&detail, MAX_ERROR_CHARS)),
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

    fn synthetic(text: &str) -> CliOutput {
        let line = serde_json::json!({
            "type": "assistant",
            "message": { "role": "assistant", "model": "<synthetic>", "content": [{ "type": "text", "text": text }] }
        });
        CliOutput { code: Some(0), stdout: format!("{line}\n"), stderr: String::new() }
    }

    fn logged_in() -> ClaudeAuthStatus {
        ClaudeAuthStatus { logged_in: true, email: None, plan: None }
    }

    #[test]
    fn logged_out_fixture_means_needs_login_even_if_auth_status_says_logged_in() {
        let out = CliOutput {
            code: Some(0),
            stdout: include_str!("../../tests/fixtures/claude_usage_logged_out.jsonl").into(),
            stderr: String::new(),
        };
        assert_eq!(classify_missing_report(Some(&logged_in()), &out).status, AccountStatus::NeedsLogin);
    }

    #[test]
    fn synthetic_auth_messages_mean_needs_login() {
        for text in [
            "Not logged in",
            "Please run /login",
            "OAuth token has expired. Please log in again.",
            "Invalid API key",
            "OAuth token revoked",
            "API Error: 401 Unauthorized",
            "Please sign in",
        ] {
            let snap = classify_missing_report(Some(&logged_in()), &synthetic(text));
            assert_eq!(snap.status, AccountStatus::NeedsLogin, "{text}");
        }
    }

    #[test]
    fn synthetic_rate_limit_messages_mean_rate_limited() {
        for text in [
            "Claude usage rate limit reached",
            "API Error: 429",
            "rate_limit_error",
            "Too Many Requests",
            "Claude AI usage limit reached|1759075200",
            "You've hit your limit · resets 3pm",
        ] {
            let snap = classify_missing_report(Some(&logged_in()), &synthetic(text));
            assert_eq!(snap.status, AccountStatus::RateLimited, "{text}");
        }
    }

    #[test]
    fn other_synthetic_text_is_an_error_that_keeps_the_message() {
        let snap = classify_missing_report(Some(&logged_in()), &synthetic("  Something broke  "));
        assert!(matches!(snap.status, AccountStatus::Error(_)));
        assert_eq!(snap.last_error.as_deref(), Some("Something broke"));
    }

    #[test]
    fn non_synthetic_stdout_is_ignored() {
        let hook = r#"{"type":"assistant","message":{"model":"claude-fable-5-1","content":[{"type":"text","text":"Not logged in 429"}]}}"#;
        let out = CliOutput { code: Some(0), stdout: format!("{hook}\nplain hook text: log in\n"), stderr: String::new() };
        let snap = classify_missing_report(Some(&logged_in()), &out);
        assert!(matches!(snap.status, AccountStatus::Error(_)));
        assert_eq!(snap.last_error, None);
    }
}
