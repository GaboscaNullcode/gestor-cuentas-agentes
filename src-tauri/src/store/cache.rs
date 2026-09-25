use crate::model::UsageSnapshot;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageCache {
    /// Latest snapshot per account id.
    pub snapshots: HashMap<String, UsageSnapshot>,
    /// Notification dedupe keys mapped to the epoch seconds they relate to.
    pub sent: HashMap<String, i64>,
}

impl UsageCache {
    pub fn prune_sent(&mut self, now: DateTime<Utc>) {
        let cutoff = (now - chrono::Duration::days(8)).timestamp();
        self.sent.retain(|_, at| *at >= cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn prunes_sent_keys_older_than_eight_days() {
        let now = Utc.with_ymd_and_hms(2026, 9, 25, 0, 0, 0).unwrap();
        let mut cache = UsageCache::default();
        cache.sent.insert("old".into(), (now - Duration::days(9)).timestamp());
        cache.sent.insert("new".into(), (now - Duration::days(1)).timestamp());
        cache.prune_sent(now);
        assert!(cache.sent.contains_key("new") && !cache.sent.contains_key("old"));
    }
}
