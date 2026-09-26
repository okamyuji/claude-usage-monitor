//! 保持期間を過ぎた時系列データの削除。DBを長期稼働で肥大させないため1日1回呼ぶ。
use crate::models::ports::{MaintenanceRepo, RepoError};
use chrono::{DateTime, Duration, Utc};

/// `retention_days`日より古い行を削除し、削除行数を返す。
pub fn purge_expired(
    repo: &dyn MaintenanceRepo,
    now: DateTime<Utc>,
    retention_days: i64,
) -> Result<usize, RepoError> {
    repo.purge_before(now - Duration::days(retention_days))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{ProfileRepo, UsageRepo};
    use crate::test_support::temp_store;
    use chrono::{Duration, TimeZone};

    #[test]
    fn purges_rows_older_than_retention_days() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        let now = Utc.with_ymd_and_hms(2026, 9, 26, 0, 0, 0).unwrap();
        let snap = parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap();
        s.record_snapshot(p.id, now - Duration::days(91), &snap)
            .unwrap();
        s.record_snapshot(p.id, now - Duration::days(89), &snap)
            .unwrap();
        assert!(purge_expired(&s, now, 90).unwrap() > 0);
        assert_eq!(
            s.samples(p.id, "session", now - Duration::days(365))
                .unwrap()
                .len(),
            1
        );
    }
}
