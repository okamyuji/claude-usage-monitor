//! 取得ログの永続化。
use crate::models::domain::records::{FetchLogEntry, FetchResult};
use crate::models::ports::{FetchLogRepo, RepoError};
use crate::models::repositories::db::{SqliteStore, parse_ts, ts};
use rusqlite::params;

impl FetchLogRepo for SqliteStore {
    fn log(&self, e: &FetchLogEntry) -> Result<(), RepoError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO fetch_log(target, at, result, http_status, message) VALUES(?1,?2,?3,?4,?5)",
                params![e.target, ts(e.at), e.result.as_str(), e.http_status, e.message],
            )
            .map(|_| ())
        })
    }

    fn recent(&self, target: &str, limit: usize) -> Result<Vec<FetchLogEntry>, RepoError> {
        let rows: Vec<(String, String, Option<u16>, String)> = self.with(|c| {
            let mut st = c.prepare(
                "SELECT at, result, http_status, message FROM fetch_log WHERE target = ?1 ORDER BY at DESC, id DESC LIMIT ?2",
            )?;
            st.query_map(params![target, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect()
        })?;
        Ok(rows
            .into_iter()
            .filter_map(|(at, result, http_status, message)| {
                Some(FetchLogEntry {
                    target: target.to_string(),
                    at: parse_ts(&at)?,
                    result: if result == "ok" {
                        FetchResult::Ok
                    } else {
                        FetchResult::Failed
                    },
                    http_status,
                    message,
                })
            })
            .collect())
    }
}
#[cfg(test)]
mod tests {
    use crate::models::domain::records::{FetchLogEntry, FetchResult};
    use crate::models::ports::FetchLogRepo;
    use crate::test_support::temp_store;
    use chrono::{Duration, TimeZone, Utc};

    fn one_entry() -> (
        tempfile::TempDir,
        crate::models::repositories::db::SqliteStore,
    ) {
        let (d, s) = temp_store();
        s.log(&FetchLogEntry {
            target: "usage:1".into(),
            at: Utc.with_ymd_and_hms(2026, 9, 26, 0, 0, 0).unwrap(),
            result: FetchResult::Ok,
            http_status: Some(200),
            message: String::new(),
        })
        .unwrap();
        (d, s)
    }

    #[test]
    fn corrupt_column_is_error_not_panic() {
        for col in ["at", "result", "http_status", "message"] {
            let (_d, s) = one_entry();
            s.with(|c| c.execute(&format!("UPDATE fetch_log SET {col} = X'00'"), []))
                .unwrap();
            assert!(s.recent("usage:1", 10).is_err(), "{col}");
        }
    }

    #[test]
    fn unparsable_timestamp_row_is_skipped() {
        let (_d, s) = one_entry();
        s.with(|c| c.execute("UPDATE fetch_log SET at = 'not-a-time'", []))
            .unwrap();
        assert!(s.recent("usage:1", 10).unwrap().is_empty());
    }

    #[test]
    fn recent_returns_newest_first_for_target() {
        let (_d, s) = temp_store();
        let t = Utc.with_ymd_and_hms(2026, 9, 26, 0, 0, 0).unwrap();
        for (i, target) in ["usage:1", "usage:1", "catalog"].iter().enumerate() {
            s.log(&FetchLogEntry {
                target: target.to_string(),
                at: t + Duration::minutes(i as i64),
                result: if i == 1 {
                    FetchResult::Failed
                } else {
                    FetchResult::Ok
                },
                http_status: Some(200),
                message: format!("m{i}"),
            })
            .unwrap();
        }
        let r = s.recent("usage:1", 10).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[0].message.as_str(), r[0].result),
            ("m1", FetchResult::Failed)
        );
        assert_eq!(s.recent("usage:1", 1).unwrap().len(), 1);
    }
}
