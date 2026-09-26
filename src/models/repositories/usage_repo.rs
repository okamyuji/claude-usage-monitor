//! 使用率の永続化。
use crate::models::domain::projection::Sample;
use crate::models::domain::usage::UsageSnapshot;
use crate::models::ports::{RepoError, UsageRepo};
use crate::models::repositories::db::{SqliteStore, parse_ts, ts};
use chrono::{DateTime, Utc};
use rusqlite::params;

impl UsageRepo for SqliteStore {
    fn record_snapshot(
        &self,
        profile_id: i64,
        at: DateTime<Utc>,
        snap: &UsageSnapshot,
    ) -> Result<(), RepoError> {
        let at = ts(at);
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            for l in &snap.limits {
                tx.execute(
                    "INSERT INTO usage_samples(profile_id, fetched_at, kind, scope_label, percent, resets_at, severity) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![profile_id, at, l.kind, l.scope_label, l.percent, l.resets_at.map(ts), l.severity],
                )?;
            }
            for b in &snap.breakdown {
                tx.execute(
                    "INSERT INTO usage_breakdown(profile_id, key, display_name, percent, fetched_at) VALUES(?1,?2,?3,?4,?5)
                     ON CONFLICT(profile_id, key) DO UPDATE SET display_name = excluded.display_name, percent = excluded.percent, fetched_at = excluded.fetched_at",
                    params![profile_id, b.key, b.display_name, b.percent, at],
                )?;
            }
            if let Some(s) = &snap.spend {
                tx.execute(
                    "INSERT INTO spend_samples(profile_id, fetched_at, used_minor, limit_minor, exponent, currency) VALUES(?1,?2,?3,?4,?5,?6)",
                    params![profile_id, at, s.used_minor, s.limit_minor, s.exponent, s.currency],
                )?;
            }
            tx.commit()
        })
    }

    fn samples(
        &self,
        profile_id: i64,
        kind: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<Sample>, RepoError> {
        let rows: Vec<(String, f64)> = self.with(|c| {
            let mut st = c.prepare(
                "SELECT fetched_at, percent FROM usage_samples WHERE profile_id = ?1 AND kind = ?2 AND fetched_at >= ?3 ORDER BY fetched_at",
            )?;
            st.query_map(params![profile_id, kind, ts(since)], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
        })?;
        Ok(rows
            .into_iter()
            .filter_map(|(at, percent)| parse_ts(&at).map(|at| Sample { at, percent }))
            .collect())
    }
}
#[cfg(test)]
mod tests {
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{ProfileRepo, UsageRepo};
    use crate::test_support::temp_store;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn snapshot_is_saved_and_samples_filter_by_kind_and_time() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        let snap = parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap();
        let t0 = Utc.with_ymd_and_hms(2026, 9, 26, 0, 0, 0).unwrap();
        s.record_snapshot(p.id, t0, &snap).unwrap();
        s.record_snapshot(p.id, t0 + Duration::minutes(1), &snap)
            .unwrap();
        let all = s.samples(p.id, "session", t0).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].percent, 13.0);
        assert!(all[0].at < all[1].at);
        assert_eq!(
            s.samples(p.id, "session", t0 + Duration::seconds(30))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(s.samples(p.id, "weekly_scoped", t0).unwrap().len(), 2);
        let (rows, spend): (i64, i64) = s
            .with(|c| {
                Ok((
                    c.query_row("SELECT COUNT(*) FROM usage_breakdown", [], |r| r.get(0))?,
                    c.query_row("SELECT COUNT(*) FROM spend_samples", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!((rows, spend), (2, 2));
    }
}
