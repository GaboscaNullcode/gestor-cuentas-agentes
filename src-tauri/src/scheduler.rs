use crate::model::{Account, AccountStatus, UsageSnapshot};
use crate::providers;
use crate::state::{AppState, Runtime};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const MAX_BACKOFF: Duration = Duration::from_secs(3600);
const TICK: Duration = Duration::from_secs(10);
const RESET_GRACE_SECS: i64 = 30;
pub const MANUAL_REFRESH_GAP_SECS: i64 = 60;

pub fn backoff_delay(interval: Duration, consecutive_errors: u32) -> Duration {
    let factor = 2u32.saturating_pow(consecutive_errors.min(16));
    interval.saturating_mul(factor).min(MAX_BACKOFF.max(interval))
}

/// When the account should be fetched next. None means "wait for the user" (NeedsLogin).
/// A window reset that falls before the normal slot pulls the fetch forward.
pub fn next_due(now: DateTime<Utc>, snapshot: &UsageSnapshot, interval: Duration, errors: u32) -> Option<DateTime<Utc>> {
    if snapshot.status == AccountStatus::NeedsLogin {
        return None;
    }
    let delay = chrono::Duration::from_std(backoff_delay(interval, errors)).unwrap_or(chrono::Duration::hours(1));
    let base = now + delay;
    let reset = snapshot
        .windows
        .iter()
        .filter_map(|w| w.resets_at)
        .filter(|r| *r > now)
        .map(|r| r + chrono::Duration::seconds(RESET_GRACE_SECS))
        .min();
    Some(match reset {
        Some(r) if r < base => r,
        _ => base,
    })
}

pub fn stagger_offsets(count: usize, interval: Duration) -> Vec<Duration> {
    if count == 0 {
        return Vec::new();
    }
    let step = interval / count as u32;
    (0..count).map(|i| step * i as u32).collect()
}

/// Combines a fresh fetch with the previous snapshot so a failed fetch keeps showing old data.
pub fn merge_result(prev: Option<&UsageSnapshot>, fresh: UsageSnapshot) -> UsageSnapshot {
    let Some(prev) = prev.filter(|p| !p.windows.is_empty()) else { return fresh };
    match &fresh.status {
        AccountStatus::Ok => fresh,
        AccountStatus::Error(message) => UsageSnapshot {
            status: AccountStatus::Stale,
            last_error: Some(message.clone()),
            ..prev.clone()
        },
        _ => UsageSnapshot { status: fresh.status.clone(), last_error: fresh.last_error.clone(), ..prev.clone() },
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageUpdated {
    pub account_id: String,
    pub snapshot: UsageSnapshot,
}

/// Background loop: every tick, start fetches for accounts whose slot has come.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            run_due(&app);
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Spreads first fetches across one interval; the first account is fetched immediately.
pub fn schedule_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = Utc::now();
    let accounts = state.accounts.lock().unwrap().clone();
    let interval = state.settings.lock().unwrap().interval();
    let mut runtime = state.runtime.lock().unwrap();
    runtime.retain(|id, _| accounts.iter().any(|a| &a.id == id));
    for (account, offset) in accounts.iter().zip(stagger_offsets(accounts.len(), interval)) {
        let entry = runtime.entry(account.id.clone()).or_default();
        entry.next_due = Some(now + chrono::Duration::from_std(offset).unwrap_or_default());
        entry.errors = 0;
    }
}

pub fn schedule_now(app: &AppHandle, account_id: &str) {
    let state = app.state::<AppState>();
    let mut runtime = state.runtime.lock().unwrap();
    let entry = runtime.entry(account_id.to_string()).or_default();
    entry.next_due = Some(Utc::now());
    entry.errors = 0;
}

/// Outcome of asking for a user-triggered refresh of one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualClaim {
    Granted,
    InFlight,
    TooSoon,
}

/// One manual refresh per account per minute. Granting records the time but leaves
/// `errors` alone, so manual clicks cannot defeat the error backoff.
pub fn claim_manual(entry: &mut Runtime, now: DateTime<Utc>) -> ManualClaim {
    if entry.in_flight {
        return ManualClaim::InFlight;
    }
    if entry.last_manual.is_some_and(|last| (now - last).num_seconds() < MANUAL_REFRESH_GAP_SECS) {
        return ManualClaim::TooSoon;
    }
    entry.last_manual = Some(now);
    ManualClaim::Granted
}

/// "Refresh all" from the tray: fetches every account that passes the manual gate.
pub fn refresh_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = Utc::now();
    let granted: Vec<Account> = {
        let accounts = state.accounts.lock().unwrap();
        let mut runtime = state.runtime.lock().unwrap();
        accounts
            .iter()
            .filter(|a| claim_manual(runtime.entry(a.id.clone()).or_default(), now) == ManualClaim::Granted)
            .cloned()
            .collect()
    };
    for account in granted {
        start_fetch(app.clone(), account);
    }
}

pub fn run_due(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = Utc::now();
    let due: Vec<Account> = {
        let accounts = state.accounts.lock().unwrap();
        let mut runtime = state.runtime.lock().unwrap();
        accounts
            .iter()
            .filter(|account| {
                let entry = runtime.entry(account.id.clone()).or_default();
                !entry.in_flight && entry.next_due.is_some_and(|due| due <= now)
            })
            .cloned()
            .collect()
    };
    for account in due {
        start_fetch(app.clone(), account);
    }
}

pub fn start_fetch(app: AppHandle, account: Account) {
    {
        let state = app.state::<AppState>();
        let mut runtime = state.runtime.lock().unwrap();
        let entry = runtime.entry(account.id.clone()).or_default();
        if entry.in_flight {
            return;
        }
        entry.in_flight = true;
    }
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let ctx = state.cli.lock().unwrap().clone();
        let fresh = providers::fetch_usage(&ctx, &account).await;
        let interval = state.settings.lock().unwrap().interval();
        let thresholds = state.settings.lock().unwrap().thresholds.clone();
        let (merged, notes) = {
            // Held across the cache and runtime updates (lock order accounts -> cache -> runtime)
            // so an account removed mid-fetch is not resurrected as an orphan entry.
            let accounts = state.accounts.lock().unwrap();
            if !accounts.iter().any(|a| a.id == account.id) {
                return;
            }
            let mut cache = state.cache.lock().unwrap();
            let prev = cache.snapshots.get(&account.id).cloned();
            let merged = merge_result(prev.as_ref(), fresh.clone());
            let mut sent = std::mem::take(&mut cache.sent);
            let notes = crate::notifier::diff(&account.label, &account.id, prev.as_ref(), &merged, &thresholds, &mut sent, Utc::now());
            cache.sent = sent;
            cache.snapshots.insert(account.id.clone(), merged.clone());
            drop(cache);
            let mut runtime = state.runtime.lock().unwrap();
            let entry = runtime.entry(account.id.clone()).or_default();
            entry.in_flight = false;
            entry.errors = if fresh.status == AccountStatus::Ok { 0 } else { entry.errors + 1 };
            entry.next_due = next_due(Utc::now(), &merged, interval, entry.errors);
            (merged, notes)
        };
        {
            use tauri_plugin_notification::NotificationExt;
            for note in notes {
                let _ = app.notification().builder().title(&note.title).body(&note.body).show();
            }
        }
        log::info!("fetched {} ({:?}): {:?}", account.label, account.provider, merged.status);
        state.save_cache();
        crate::tray::refresh(&app);
        let _ = app.emit("usage-updated", UsageUpdated { account_id: account.id.clone(), snapshot: merged });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountStatus, UsageSnapshot, Window, WindowKind};
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use std::time::Duration;

    const FIVE_MIN: Duration = Duration::from_secs(300);

    #[test]
    fn manual_claim_is_granted_once_per_minute() {
        let mut entry = crate::state::Runtime { errors: 3, ..Default::default() };
        assert_eq!(claim_manual(&mut entry, now()), ManualClaim::Granted);
        assert_eq!(entry.last_manual, Some(now()));
        assert_eq!(entry.errors, 3, "a manual refresh must not reset backoff");
        assert_eq!(claim_manual(&mut entry, now() + ChronoDuration::seconds(59)), ManualClaim::TooSoon);
        assert_eq!(entry.last_manual, Some(now()), "a refused claim must not move the gate");
        assert_eq!(claim_manual(&mut entry, now() + ChronoDuration::seconds(60)), ManualClaim::Granted);
    }

    #[test]
    fn manual_claim_skips_in_flight_fetches() {
        let mut entry = crate::state::Runtime { in_flight: true, ..Default::default() };
        assert_eq!(claim_manual(&mut entry, now()), ManualClaim::InFlight);
        assert_eq!(entry.last_manual, None);
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap()
    }

    fn snap(status: AccountStatus, resets_in_min: Option<i64>) -> UsageSnapshot {
        UsageSnapshot {
            plan: Some("max".into()),
            windows: vec![Window {
                kind: WindowKind::FiveHour,
                used_pct: 40.0,
                resets_at: resets_in_min.map(|m| now() + ChronoDuration::minutes(m)),
            }],
            fetched_at: now(),
            status,
            last_error: None,
        }
    }

    #[test]
    fn backoff_doubles_and_caps_at_an_hour() {
        assert_eq!(backoff_delay(FIVE_MIN, 0), FIVE_MIN);
        assert_eq!(backoff_delay(FIVE_MIN, 1), Duration::from_secs(600));
        assert_eq!(backoff_delay(FIVE_MIN, 3), Duration::from_secs(2400));
        assert_eq!(backoff_delay(FIVE_MIN, 10), Duration::from_secs(3600));
    }

    #[test]
    fn next_due_uses_interval() {
        let due = next_due(now(), &snap(AccountStatus::Ok, Some(120)), FIVE_MIN, 0).unwrap();
        assert_eq!(due, now() + ChronoDuration::minutes(5));
    }

    #[test]
    fn next_due_brought_forward_by_reset() {
        let due = next_due(now(), &snap(AccountStatus::Ok, Some(2)), FIVE_MIN, 0).unwrap();
        assert_eq!(due, now() + ChronoDuration::minutes(2) + ChronoDuration::seconds(30));
    }

    #[test]
    fn needs_login_is_not_polled() {
        assert_eq!(next_due(now(), &snap(AccountStatus::NeedsLogin, None), FIVE_MIN, 0), None);
    }

    #[test]
    fn staggers_accounts_across_the_interval() {
        assert_eq!(
            stagger_offsets(3, FIVE_MIN),
            vec![Duration::ZERO, Duration::from_secs(100), Duration::from_secs(200)]
        );
        assert!(stagger_offsets(0, FIVE_MIN).is_empty());
    }

    #[test]
    fn merge_keeps_fresh_ok_snapshot() {
        let prev = snap(AccountStatus::Ok, Some(10));
        let mut fresh = snap(AccountStatus::Ok, Some(20));
        fresh.windows[0].used_pct = 50.0;
        assert_eq!(merge_result(Some(&prev), fresh.clone()), fresh);
    }

    #[test]
    fn merge_turns_error_into_stale_with_previous_windows() {
        let prev = snap(AccountStatus::Ok, Some(10));
        let fresh = crate::providers::failure(AccountStatus::Error("boom".into()), None);
        let merged = merge_result(Some(&prev), fresh);
        assert_eq!(merged.status, AccountStatus::Stale);
        assert_eq!(merged.windows, prev.windows);
        assert_eq!(merged.fetched_at, prev.fetched_at);
        assert_eq!(merged.last_error.as_deref(), Some("boom"));
    }

    #[test]
    fn merge_keeps_status_but_previous_windows_for_needs_login() {
        let prev = snap(AccountStatus::Ok, Some(10));
        let merged = merge_result(Some(&prev), crate::providers::failure(AccountStatus::NeedsLogin, None));
        assert_eq!(merged.status, AccountStatus::NeedsLogin);
        assert_eq!(merged.windows, prev.windows);
    }

    #[test]
    fn merge_without_previous_data_returns_fresh() {
        let fresh = crate::providers::failure(AccountStatus::CliMissing, None);
        assert_eq!(merge_result(None, fresh.clone()), fresh);
    }
}
