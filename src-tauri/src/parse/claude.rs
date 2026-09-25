use crate::model::{AccountStatus, UsageSnapshot, Window, WindowKind};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Parses the stream-json output of `claude -p /usage`.
/// Returns None when no line carries a usage report (for example, when logged out).
pub fn parse_claude_usage(stdout: &str, fetched_at: DateTime<Utc>) -> Option<UsageSnapshot> {
    let report = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .find_map(|value| value.get("usage_report").cloned())?;
    let rate_limits = report.get("rate_limits")?;
    let windows = match rate_limits.get("limits").and_then(Value::as_array) {
        Some(limits) => limits.iter().filter_map(parse_limit).collect(),
        None => parse_legacy(rate_limits),
    };
    Some(UsageSnapshot {
        plan: None,
        windows,
        fetched_at,
        status: AccountStatus::Ok,
        last_error: None,
    })
}

fn parse_limit(limit: &Value) -> Option<Window> {
    let raw_kind = limit.get("kind")?.as_str()?;
    let used_pct = limit.get("percent")?.as_f64()? as f32;
    let kind = match raw_kind {
        "session" => WindowKind::FiveHour,
        "weekly_all" => WindowKind::Weekly,
        "weekly_scoped" => WindowKind::WeeklyScoped(
            limit
                .pointer("/scope/model/display_name")
                .and_then(Value::as_str)
                .unwrap_or("Scoped")
                .to_string(),
        ),
        other => WindowKind::Other(other.to_string()),
    };
    Some(Window { kind, used_pct, resets_at: parse_iso(limit.get("resets_at")) })
}

/// Shape used before `limits[]` existed; kept so an older CLI still works.
fn parse_legacy(rate_limits: &Value) -> Vec<Window> {
    let fields = [
        ("five_hour", WindowKind::FiveHour),
        ("seven_day", WindowKind::Weekly),
        ("seven_day_opus", WindowKind::WeeklyScoped("Opus".into())),
        ("seven_day_sonnet", WindowKind::WeeklyScoped("Sonnet".into())),
    ];
    fields
        .into_iter()
        .filter_map(|(key, kind)| {
            let window = rate_limits.get(key)?;
            let used_pct = window.get("utilization")?.as_f64()? as f32;
            Some(Window { kind, used_pct, resets_at: parse_iso(window.get("resets_at")) })
        })
        .collect()
}

pub(crate) fn parse_iso(value: Option<&Value>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?.as_str()?)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeAuthStatus {
    pub logged_in: bool,
    pub email: Option<String>,
    pub plan: Option<String>,
}

/// Parses the JSON printed by `claude auth status`.
pub fn parse_claude_auth_status(stdout: &str) -> Option<ClaudeAuthStatus> {
    let value: Value = serde_json::from_str(stdout.trim()).ok()?;
    Some(ClaudeAuthStatus {
        logged_in: value.get("loggedIn")?.as_bool()?,
        email: value.get("email").and_then(Value::as_str).map(str::to_string),
        plan: value.get("subscriptionType").and_then(Value::as_str).map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountStatus, WindowKind};
    use chrono::{TimeZone, Utc};

    const OK: &str = include_str!("../../tests/fixtures/claude_usage_ok.jsonl");
    const LOGGED_OUT: &str = include_str!("../../tests/fixtures/claude_usage_logged_out.jsonl");
    const AUTH_IN: &str = include_str!("../../tests/fixtures/claude_auth_status_logged_in.json");
    const AUTH_OUT: &str = include_str!("../../tests/fixtures/claude_auth_status_logged_out.json");

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 13, 30, 0).unwrap()
    }

    #[test]
    fn parses_current_limits_fixture() {
        let snap = parse_claude_usage(OK, now()).expect("usage report");
        assert_eq!(snap.status, AccountStatus::Ok);
        assert_eq!(snap.windows.len(), 3);
        assert_eq!(snap.windows[0].kind, WindowKind::FiveHour);
        assert_eq!(snap.windows[0].used_pct, 7.0);
        assert_eq!(
            snap.windows[0].resets_at.unwrap().timestamp(),
            Utc.with_ymd_and_hms(2026, 9, 25, 17, 10, 0).unwrap().timestamp()
        );
        assert_eq!(snap.windows[1].kind, WindowKind::Weekly);
        assert_eq!(snap.windows[1].used_pct, 25.0);
        assert_eq!(snap.windows[2].kind, WindowKind::WeeklyScoped("Fable".into()));
        assert_eq!(snap.fetched_at, now());
    }

    #[test]
    fn returns_none_when_logged_out() {
        assert!(parse_claude_usage(LOGGED_OUT, now()).is_none());
    }

    #[test]
    fn ignores_non_json_lines() {
        let noisy = format!("warning: something\n{OK}\n\n");
        assert!(parse_claude_usage(&noisy, now()).is_some());
    }

    #[test]
    fn unknown_kind_becomes_other_and_malformed_limits_are_skipped() {
        let line = r#"{"type":"assistant","usage_report":{"rate_limits":{"limits":[{"kind":"monthly_cowork","percent":12,"resets_at":null},{"kind":"session"}]}}}"#;
        let snap = parse_claude_usage(line, now()).unwrap();
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].kind, WindowKind::Other("monthly_cowork".into()));
        assert_eq!(snap.windows[0].resets_at, None);
    }

    #[test]
    fn parses_legacy_shape() {
        let line = r#"{"type":"assistant","usage_report":{"rate_limits":{"five_hour":{"utilization":9.0,"resets_at":"2026-09-14T02:10:00Z"},"seven_day":{"utilization":68.0,"resets_at":"2026-09-19T09:00:00Z"},"seven_day_opus":null,"seven_day_sonnet":{"utilization":3.0,"resets_at":null}}}}"#;
        let snap = parse_claude_usage(line, now()).unwrap();
        let kinds: Vec<_> = snap.windows.iter().map(|w| w.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![WindowKind::FiveHour, WindowKind::Weekly, WindowKind::WeeklyScoped("Sonnet".into())]
        );
        assert_eq!(snap.windows[1].used_pct, 68.0);
    }

    #[test]
    fn parses_auth_status() {
        let status = parse_claude_auth_status(AUTH_IN).unwrap();
        assert!(status.logged_in);
        assert_eq!(status.email.as_deref(), Some("me@example.com"));
        assert_eq!(status.plan.as_deref(), Some("max"));
        let status = parse_claude_auth_status(AUTH_OUT).unwrap();
        assert!(!status.logged_in);
        assert_eq!(status.plan, None);
        assert!(parse_claude_auth_status("not json").is_none());
    }
}
