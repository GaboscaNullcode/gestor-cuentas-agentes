use crate::model::{AccountStatus, UsageSnapshot, WindowKind};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

const RESET_MIN_PREVIOUS_PCT: f32 = 50.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

pub fn window_name(kind: &WindowKind) -> String {
    match kind {
        WindowKind::FiveHour => "5-hour window".to_string(),
        WindowKind::Weekly => "Weekly limit".to_string(),
        WindowKind::WeeklyScoped(name) => format!("Weekly {name} limit"),
        WindowKind::Other(name) => name.clone(),
    }
}

fn until(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (at - now).num_minutes().max(0);
    if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 1440 {
        format!("{}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("{}d {}h", minutes / 1440, (minutes % 1440) / 60)
    }
}

/// Decides which notifications a new snapshot triggers. `sent` holds dedupe keys
/// (persisted in the cache) so each threshold fires once per window instance.
pub fn diff(
    label: &str,
    account_id: &str,
    prev: Option<&UsageSnapshot>,
    next: &UsageSnapshot,
    thresholds: &[u8],
    sent: &mut HashMap<String, i64>,
    now: DateTime<Utc>,
) -> Vec<Notification> {
    let mut notes = Vec::new();

    if next.status == AccountStatus::NeedsLogin && prev.is_none_or(|p| p.status != AccountStatus::NeedsLogin) {
        notes.push(Notification {
            title: format!("{label} needs to sign in again"),
            body: "Open Usage Monitor and choose Reconnect.".to_string(),
        });
    }
    if next.status != AccountStatus::Ok {
        return notes;
    }

    for window in &next.windows {
        // The dedupe key segment must be stable across fetches even when `resets_at` is
        // unknown, otherwise a window without a reset time would re-notify on every poll.
        // The stored value still needs a real timestamp for `UsageCache::prune_sent`.
        let reset_key_part = window.resets_at.map_or_else(|| "none".to_string(), |r| r.timestamp().to_string());
        let reset_ts = window.resets_at.map_or(now.timestamp(), |r| r.timestamp());
        let key_base = format!("{account_id}|{}|{reset_key_part}", window.kind.key());

        if let Some(&top) = thresholds.iter().filter(|t| window.used_pct >= f32::from(**t)).max() {
            let top_key = format!("th|{key_base}|{top}");
            if !sent.contains_key(&top_key) {
                let body = window
                    .resets_at
                    .map(|r| format!("Resets in {}.", until(r, now)))
                    .unwrap_or_default();
                notes.push(Notification {
                    title: format!("{label}: {} at {:.0}%", window_name(&window.kind), window.used_pct),
                    body,
                });
                for t in thresholds.iter().filter(|t| **t <= top) {
                    sent.insert(format!("th|{key_base}|{t}"), reset_ts);
                }
            }
        }

        let previous = prev.and_then(|p| p.window(&window.kind));
        if let (Some(before), Some(new_reset)) = (previous, window.resets_at) {
            if let Some(old_reset) = before.resets_at {
                let reset_key = format!("reset|{account_id}|{}|{}", window.kind.key(), old_reset.timestamp());
                if new_reset > old_reset
                    && before.used_pct >= RESET_MIN_PREVIOUS_PCT
                    && window.used_pct < before.used_pct
                    && !sent.contains_key(&reset_key)
                {
                    notes.push(Notification {
                        title: format!("{label}: {} has reset", window_name(&window.kind)),
                        body: format!("Now at {:.0}% used.", window.used_pct),
                    });
                    sent.insert(reset_key, old_reset.timestamp());
                }
            }
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountStatus, UsageSnapshot, Window, WindowKind};
    use chrono::{Duration, TimeZone, Utc};
    use std::collections::HashMap;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap()
    }

    fn snap(status: AccountStatus, pct: f32, resets_in_h: i64) -> UsageSnapshot {
        UsageSnapshot {
            plan: None,
            windows: vec![Window { kind: WindowKind::FiveHour, used_pct: pct, resets_at: Some(now() + Duration::hours(resets_in_h)) }],
            fetched_at: now(),
            status,
            last_error: None,
        }
    }

    const T: [u8; 2] = [80, 95];

    #[test]
    fn crossing_a_threshold_notifies_once() {
        let mut sent = HashMap::new();
        let first = diff("Work", "a", Some(&snap(AccountStatus::Ok, 70.0, 2)), &snap(AccountStatus::Ok, 82.0, 2), &T, &mut sent, now());
        assert_eq!(first.len(), 1);
        assert!(first[0].title.contains("82%"));
        let again = diff("Work", "a", Some(&snap(AccountStatus::Ok, 82.0, 2)), &snap(AccountStatus::Ok, 84.0, 2), &T, &mut sent, now());
        assert!(again.is_empty());
    }

    #[test]
    fn higher_threshold_notifies_and_lower_is_not_repeated() {
        let mut sent = HashMap::new();
        diff("Work", "a", None, &snap(AccountStatus::Ok, 82.0, 2), &T, &mut sent, now());
        let high = diff("Work", "a", None, &snap(AccountStatus::Ok, 96.0, 2), &T, &mut sent, now());
        assert_eq!(high.len(), 1);
        assert!(high[0].title.contains("96%"));
    }

    #[test]
    fn jumping_straight_to_the_top_sends_one_notification() {
        let mut sent = HashMap::new();
        let notes = diff("Work", "a", None, &snap(AccountStatus::Ok, 97.0, 2), &T, &mut sent, now());
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn a_new_window_can_notify_again() {
        let mut sent = HashMap::new();
        diff("Work", "a", None, &snap(AccountStatus::Ok, 85.0, 2), &T, &mut sent, now());
        let next_window = diff("Work", "a", None, &snap(AccountStatus::Ok, 85.0, 7), &T, &mut sent, now());
        assert_eq!(next_window.iter().filter(|n| n.title.contains("85%")).count(), 1);
    }

    #[test]
    fn reset_is_announced_when_previous_usage_was_high() {
        let mut sent = HashMap::new();
        let notes = diff("Work", "a", Some(&snap(AccountStatus::Ok, 90.0, 0)), &snap(AccountStatus::Ok, 3.0, 5), &T, &mut sent, now());
        assert!(notes.iter().any(|n| n.title.contains("has reset")));
        let low = diff("Work", "b", Some(&snap(AccountStatus::Ok, 20.0, 0)), &snap(AccountStatus::Ok, 3.0, 5), &T, &mut sent, now());
        assert!(low.is_empty());
    }

    #[test]
    fn disconnect_notifies_once_per_transition() {
        let mut sent = HashMap::new();
        let ok = snap(AccountStatus::Ok, 10.0, 2);
        let out = snap(AccountStatus::NeedsLogin, 10.0, 2);
        assert_eq!(diff("Work", "a", Some(&ok), &out, &T, &mut sent, now()).len(), 1);
        assert!(diff("Work", "a", Some(&out), &out, &T, &mut sent, now()).is_empty());
    }

    #[test]
    fn stale_data_does_not_trigger_threshold_notifications() {
        let mut sent = HashMap::new();
        assert!(diff("Work", "a", None, &snap(AccountStatus::Stale, 99.0, 2), &T, &mut sent, now()).is_empty());
    }

    fn snap_no_reset(status: AccountStatus, pct: f32) -> UsageSnapshot {
        UsageSnapshot {
            plan: None,
            windows: vec![Window { kind: WindowKind::FiveHour, used_pct: pct, resets_at: None }],
            fetched_at: now(),
            status,
            last_error: None,
        }
    }

    #[test]
    fn window_without_reset_time_notifies_once_across_polls() {
        let mut sent = HashMap::new();
        let snapshot = snap_no_reset(AccountStatus::Ok, 85.0);
        let first = diff("Work", "a", None, &snapshot, &T, &mut sent, now());
        assert_eq!(first.len(), 1);
        let later = diff("Work", "a", None, &snapshot, &T, &mut sent, now() + Duration::minutes(10));
        assert!(later.is_empty());
    }
}
