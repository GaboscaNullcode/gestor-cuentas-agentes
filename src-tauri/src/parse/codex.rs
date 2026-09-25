use crate::model::{AccountStatus, UsageSnapshot, Window, WindowKind};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, Lines};

pub const MAIN_LIMIT_ID: &str = "codex";
const FIVE_HOUR_MINS: i64 = 300;
const WEEK_MINS: i64 = 10080;

/// Converts the `account/rateLimits/read` result into a snapshot.
/// The main `codex` limit comes first; other limit ids (per-model limits) follow.
pub fn parse_codex_rate_limits(result: &Value, fetched_at: DateTime<Utc>) -> UsageSnapshot {
    let mut limits: Vec<&Value> = Vec::new();
    if let Some(by_id) = result.get("rateLimitsByLimitId").and_then(Value::as_object) {
        if let Some(main) = by_id.get(MAIN_LIMIT_ID) {
            limits.push(main);
        }
        limits.extend(by_id.iter().filter(|(id, _)| id.as_str() != MAIN_LIMIT_ID).map(|(_, v)| v));
    } else if let Some(main) = result.get("rateLimits") {
        limits.push(main);
    }
    let plan = limits
        .iter()
        .find_map(|l| l.get("planType").and_then(Value::as_str))
        .map(str::to_string);
    let windows = limits.iter().flat_map(|l| windows_of(l)).collect();
    UsageSnapshot { plan, windows, fetched_at, status: AccountStatus::Ok, last_error: None }
}

fn windows_of(limit: &Value) -> Vec<Window> {
    let id = limit.get("limitId").and_then(Value::as_str).unwrap_or(MAIN_LIMIT_ID);
    let name = limit.get("limitName").and_then(Value::as_str).unwrap_or(id);
    ["primary", "secondary"]
        .iter()
        .filter_map(|slot| {
            let window = limit.get(*slot)?;
            let used_pct = window.get("usedPercent")?.as_f64()? as f32;
            let mins = window.get("windowDurationMins").and_then(Value::as_i64);
            let resets_at = window
                .get("resetsAt")
                .and_then(Value::as_i64)
                .and_then(|secs| DateTime::from_timestamp(secs, 0));
            Some(Window { kind: classify(id == MAIN_LIMIT_ID, name, mins), used_pct, resets_at })
        })
        .collect()
}

/// Classifies a window by its duration. Primary/secondary position carries no meaning:
/// Pro plans currently report the weekly window as primary.
pub fn classify(is_main: bool, name: &str, mins: Option<i64>) -> WindowKind {
    match (is_main, mins) {
        (true, Some(FIVE_HOUR_MINS)) => WindowKind::FiveHour,
        (true, Some(WEEK_MINS)) => WindowKind::Weekly,
        (true, Some(m)) => WindowKind::Other(format!("{m}m")),
        (true, None) => WindowKind::Other("unknown".to_string()),
        (false, Some(WEEK_MINS)) => WindowKind::WeeklyScoped(name.to_string()),
        (false, Some(FIVE_HOUR_MINS)) => WindowKind::Other(format!("{name} 5h")),
        (false, Some(m)) => WindowKind::Other(format!("{name} {m}m")),
        (false, None) => WindowKind::Other(name.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodexRpcError {
    Auth(String),
    RateLimited(String),
    Other(String),
}

pub fn classify_rpc_error(message: &str) -> CodexRpcError {
    let lower = message.to_lowercase();
    // Auth first: the auth error message itself contains "rate limits".
    if lower.contains("authentication required") || lower.contains("not logged in") || lower.contains("unauthorized") {
        CodexRpcError::Auth(message.to_string())
    } else if lower.contains("429") || lower.contains("rate limit") || lower.contains("too many requests") {
        CodexRpcError::RateLimited(message.to_string())
    } else {
        CodexRpcError::Other(message.to_string())
    }
}

/// Runs the app-server handshake and returns the `account/rateLimits/read` result.
pub async fn read_rate_limits<R, W>(reader: R, mut writer: W) -> Result<Value, CodexRpcError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = reader.lines();
    send(
        &mut writer,
        json!({
            "method": "initialize",
            "id": 1,
            "params": { "clientInfo": { "name": "usage_monitor", "title": "Usage Monitor", "version": env!("CARGO_PKG_VERSION") } }
        }),
    )
    .await?;
    wait_for(&mut lines, 1).await?;
    send(&mut writer, json!({ "method": "initialized" })).await?;
    send(&mut writer, json!({ "method": "account/rateLimits/read", "id": 2 })).await?;
    wait_for(&mut lines, 2).await
}

async fn send<W: AsyncWrite + Unpin>(writer: &mut W, message: Value) -> Result<(), CodexRpcError> {
    let line = format!("{message}\n");
    writer.write_all(line.as_bytes()).await.map_err(|e| CodexRpcError::Other(e.to_string()))?;
    writer.flush().await.map_err(|e| CodexRpcError::Other(e.to_string()))
}

async fn wait_for<R: AsyncBufRead + Unpin>(lines: &mut Lines<R>, id: i64) -> Result<Value, CodexRpcError> {
    while let Some(line) = lines.next_line().await.map_err(|e| CodexRpcError::Other(e.to_string()))? {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        if message.get("id").and_then(Value::as_i64) != Some(id) {
            continue;
        }
        if let Some(error) = message.get("error") {
            let text = error.get("message").and_then(Value::as_str).unwrap_or("unknown app-server error");
            return Err(classify_rpc_error(text));
        }
        return Ok(message.get("result").cloned().unwrap_or(Value::Null));
    }
    Err(CodexRpcError::Other("codex app-server closed the connection".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WindowKind;
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::Value;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    const PRO: &str = include_str!("../../tests/fixtures/codex_rate_limits_pro.json");
    const PLUS: &str = include_str!("../../tests/fixtures/codex_rate_limits_plus.json");

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 13, 30, 0).unwrap()
    }

    #[test]
    fn pro_plan_has_only_weekly() {
        let value: Value = serde_json::from_str(PRO).unwrap();
        let snap = parse_codex_rate_limits(&value, now());
        assert_eq!(snap.plan.as_deref(), Some("pro"));
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].kind, WindowKind::Weekly);
        assert_eq!(snap.windows[0].used_pct, 33.0);
        assert_eq!(snap.windows[0].resets_at, DateTime::from_timestamp(1790428839, 0));
        assert!(snap.window(&WindowKind::FiveHour).is_none());
    }

    #[test]
    fn plus_plan_classifies_by_duration_and_puts_main_limit_first() {
        let value: Value = serde_json::from_str(PLUS).unwrap();
        let snap = parse_codex_rate_limits(&value, now());
        let kinds: Vec<_> = snap.windows.iter().map(|w| (w.kind.clone(), w.used_pct)).collect();
        assert_eq!(
            kinds,
            vec![
                (WindowKind::FiveHour, 55.0),
                (WindowKind::Weekly, 20.0),
                (WindowKind::Other("GPT-5.5-Codex-Max 5h".into()), 12.0),
                (WindowKind::WeeklyScoped("GPT-5.5-Codex-Max".into()), 40.0),
            ]
        );
    }

    #[test]
    fn falls_back_to_rate_limits_when_by_id_is_missing() {
        let value: Value = serde_json::json!({
            "rateLimits": { "limitId": "codex", "primary": { "usedPercent": 5, "windowDurationMins": 300, "resetsAt": null }, "planType": "plus" }
        });
        let snap = parse_codex_rate_limits(&value, now());
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].kind, WindowKind::FiveHour);
        assert_eq!(snap.windows[0].resets_at, None);
    }

    #[test]
    fn unusual_duration_becomes_other() {
        assert_eq!(classify(true, "codex", Some(1440)), WindowKind::Other("1440m".into()));
        assert_eq!(classify(false, "x", None), WindowKind::Other("x".into()));
    }

    #[test]
    fn classifies_rpc_errors() {
        assert!(matches!(
            classify_rpc_error("codex account authentication required to read rate limits"),
            CodexRpcError::Auth(_)
        ));
        assert!(matches!(classify_rpc_error("HTTP 429 Too Many Requests"), CodexRpcError::RateLimited(_)));
        assert!(matches!(classify_rpc_error("boom"), CodexRpcError::Other(_)));
    }

    #[tokio::test]
    async fn exchanges_initialize_and_reads_limits() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        let server_task = tokio::spawn(async move {
            let (server_read, mut server_write) = tokio::io::split(server);
            let mut lines = BufReader::new(server_read).lines();
            let init: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(init["method"], "initialize");
            assert_eq!(init["params"]["clientInfo"]["name"], "usage_monitor");
            server_write
                .write_all(b"{\"method\":\"remoteControl/status/changed\",\"params\":{}}\n{\"id\":1,\"result\":{}}\n")
                .await
                .unwrap();
            let initialized: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(initialized["method"], "initialized");
            let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request["method"], "account/rateLimits/read");
            let result: Value = serde_json::from_str(PRO).unwrap();
            let reply = serde_json::json!({ "id": 2, "result": result });
            server_write.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
        });
        let result = read_rate_limits(BufReader::new(client_read), client_write).await.unwrap();
        assert_eq!(result["rateLimits"]["planType"], "pro");
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn maps_auth_error_reply() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        tokio::spawn(async move {
            let (server_read, mut server_write) = tokio::io::split(server);
            let mut lines = BufReader::new(server_read).lines();
            lines.next_line().await.unwrap();
            server_write.write_all(b"{\"id\":1,\"result\":{}}\n").await.unwrap();
            lines.next_line().await.unwrap();
            lines.next_line().await.unwrap();
            server_write
                .write_all(b"{\"error\":{\"code\":-32600,\"message\":\"codex account authentication required to read rate limits\"},\"id\":2}\n")
                .await
                .unwrap();
        });
        let err = read_rate_limits(BufReader::new(client_read), client_write).await.unwrap_err();
        assert!(matches!(err, CodexRpcError::Auth(_)));
    }

    #[tokio::test]
    async fn reports_closed_connection() {
        let (client, server) = tokio::io::duplex(1024);
        drop(server);
        let (client_read, client_write) = tokio::io::split(client);
        let err = read_rate_limits(BufReader::new(client_read), client_write).await.unwrap_err();
        assert!(matches!(err, CodexRpcError::Other(_)));
    }
}
