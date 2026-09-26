//! 保持期間の削除とデーモン統計の記録。
use crate::models::ports::{MaintenanceRepo, RepoError};
use crate::models::repositories::db::{SqliteStore, ts};
use chrono::{DateTime, Utc};
use rusqlite::params;

impl MaintenanceRepo for SqliteStore {
    fn purge_before(&self, cutoff: DateTime<Utc>) -> Result<usize, RepoError> {
        let cutoff = ts(cutoff);
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let mut n = 0;
            for sql in [
                "DELETE FROM usage_samples WHERE fetched_at < ?1",
                "DELETE FROM spend_samples WHERE fetched_at < ?1",
                "DELETE FROM fetch_log WHERE at < ?1",
                "DELETE FROM daemon_stats WHERE at < ?1",
                "DELETE FROM jobs WHERE updated_at < ?1",
                // ターン・ツール・サブエージェントは外部キーのON DELETE CASCADEでまとめて消える。
                "DELETE FROM sessions WHERE last_activity_at < ?1",
            ] {
                n += tx.execute(sql, params![cutoff])?;
            }
            tx.commit()?;
            c.execute_batch("PRAGMA incremental_vacuum;")?;
            Ok(n)
        })
    }

    fn record_daemon_stats(&self, at: DateTime<Utc>, rss_bytes: u64) -> Result<(), RepoError> {
        let db = self.db_bytes() as i64;
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO daemon_stats(at, rss_bytes, db_bytes) VALUES(?1,?2,?3)",
                params![ts(at), rss_bytes as i64, db],
            )
            .map(|_| ())
        })
    }
}
#[cfg(test)]
mod tests {
    use crate::models::domain::records::{FetchLogEntry, FetchResult};
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{FetchLogRepo, MaintenanceRepo, ProfileRepo, UsageRepo};
    use crate::test_support::temp_store;
    use chrono::{Duration, TimeZone, Utc};

    fn count(s: &crate::models::repositories::db::SqliteStore, table: &str) -> i64 {
        s.with(|c| c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)))
            .unwrap()
    }

    #[test]
    fn purge_removes_only_old_time_series() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        let old = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let new = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let snap = parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap();
        for at in [old, new] {
            s.record_snapshot(p.id, at, &snap).unwrap();
            s.record_daemon_stats(at, 1).unwrap();
            s.log(&FetchLogEntry {
                target: "catalog".into(),
                at,
                result: FetchResult::Ok,
                http_status: None,
                message: String::new(),
            })
            .unwrap();
        }
        let removed = s.purge_before(new - Duration::days(90)).unwrap();
        // 古い側: usage_samples 3行 + spend_samples 1行 + fetch_log 1行 + daemon_stats 1行
        assert_eq!(removed, 6);
        assert_eq!(s.samples(p.id, "session", old).unwrap().len(), 1);
        assert_eq!(
            (
                count(&s, "spend_samples"),
                count(&s, "fetch_log"),
                count(&s, "daemon_stats")
            ),
            (1, 1, 1)
        );
    }

    #[test]
    fn daemon_stats_records_rss_and_db_size() {
        let (_d, s) = temp_store();
        let t = Utc::now();
        let before = s.db_bytes();
        s.record_daemon_stats(t, 1234).unwrap();
        let (rss, db): (i64, i64) = s
            .with(|c| {
                c.query_row("SELECT rss_bytes, db_bytes FROM daemon_stats", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
            })
            .unwrap();
        assert_eq!(rss, 1234);
        assert_eq!(db as u64, before);
    }
}
