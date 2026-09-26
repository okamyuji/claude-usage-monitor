//! 分析、推移、診断の読み取り。

use crate::models::domain::read_models::{DaemonStat, DailyUsage, GroupBy, GroupUsage, ToolStat};
use crate::models::ports::{AnalyticsRepo, DiagnosticsRepo, RepoError};
use crate::models::repositories::db::{SqliteStore, parse_ts, query_rows, ts, usage_of};
use chrono::{DateTime, Utc};
use rusqlite::params;

/// 軸ごとのSQL式。列名の断片だけをここで選び、利用者の入力はSQLに入れない。
fn group_expr(g: GroupBy) -> &'static str {
    match g {
        GroupBy::Project => "COALESCE(s.cwd, '')",
        GroupBy::Model => "COALESCE(t.model, '')",
        GroupBy::Branch => "COALESCE(s.git_branch, '')",
        GroupBy::Agent => "CASE WHEN t.agent_id = '' THEN 'main' ELSE 'subagent' END",
    }
}

type SumTuple = (String, Option<String>, i64, i64, i64, i64, i64);

impl AnalyticsRepo for SqliteStore {
    fn usage_by(&self, group: GroupBy, since: DateTime<Utc>) -> Result<Vec<GroupUsage>, RepoError> {
        let sql = format!(
            "SELECT {} AS k, t.model, SUM(t.input), SUM(t.output), SUM(t.cache_read), SUM(t.cache_write_5m), SUM(t.cache_write_1h)
             FROM turns t JOIN sessions s ON s.session_id = t.session_id
             WHERE t.ts >= ?1 GROUP BY k, t.model",
            group_expr(group)
        );
        let rows: Vec<SumTuple> = self.with(|c| query_rows(c, &sql, params![ts(since)]))?;
        Ok(rows
            .into_iter()
            .map(|(key, model, i, o, cr, w5, w1)| GroupUsage {
                key,
                model,
                usage: usage_of(i, o, cr, w5, w1),
            })
            .collect())
    }

    fn daily_usage(
        &self,
        since: DateTime<Utc>,
        tz_offset_secs: i32,
    ) -> Result<Vec<DailyUsage>, RepoError> {
        let modifier = format!("{tz_offset_secs:+} seconds");
        let rows: Vec<(String, i64, i64, i64, i64, i64)> = self.with(|c| {
            query_rows(
                c,
                "SELECT date(ts, ?2) AS d, SUM(input), SUM(output), SUM(cache_read), SUM(cache_write_5m), SUM(cache_write_1h)
                 FROM turns WHERE ts >= ?1 GROUP BY d ORDER BY d",
                params![ts(since), modifier],
            )
        })?;
        Ok(rows
            .into_iter()
            .map(|(day, i, o, cr, w5, w1)| DailyUsage {
                day,
                usage: usage_of(i, o, cr, w5, w1),
            })
            .collect())
    }

    fn tool_stats(&self, since: DateTime<Utc>) -> Result<Vec<ToolStat>, RepoError> {
        let rows: Vec<(String, i64, i64)> = self.with(|c| {
            query_rows(
                c,
                "SELECT tool_name, COUNT(*), SUM(is_error) FROM tool_calls WHERE ts >= ?1
                 GROUP BY tool_name ORDER BY COUNT(*) DESC, tool_name",
                params![ts(since)],
            )
        })?;
        Ok(rows
            .into_iter()
            .map(|(tool_name, calls, errors)| ToolStat {
                tool_name,
                calls,
                errors,
            })
            .collect())
    }

    fn reset_times(
        &self,
        profile_id: i64,
        kind: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<DateTime<Utc>>, RepoError> {
        let rows: Vec<(String,)> = self.with(|c| {
            query_rows(
                c,
                "SELECT DISTINCT substr(resets_at, 1, 16) FROM usage_samples
                 WHERE profile_id = ?1 AND kind = ?2 AND fetched_at >= ?3 AND resets_at IS NOT NULL ORDER BY 1",
                params![profile_id, kind, ts(since)],
            )
        })?;
        Ok(rows
            .into_iter()
            .filter_map(|(m,)| parse_ts(&format!("{m}:00.000Z")))
            .collect())
    }
}

impl DiagnosticsRepo for SqliteStore {
    fn daemon_stats(&self, since: DateTime<Utc>) -> Result<Vec<DaemonStat>, RepoError> {
        let rows: Vec<(String, i64, i64)> = self.with(|c| {
            query_rows(
                c,
                "SELECT at, rss_bytes, db_bytes FROM daemon_stats WHERE at >= ?1 ORDER BY at",
                params![ts(since)],
            )
        })?;
        Ok(rows
            .into_iter()
            .filter_map(|(at, rss, db)| {
                Some(DaemonStat {
                    at: parse_ts(&at)?,
                    rss_bytes: rss as u64,
                    db_bytes: db as u64,
                })
            })
            .collect())
    }

    fn db_size(&self) -> u64 {
        self.db_bytes()
    }
}

#[cfg(test)]
mod tests {
    use crate::models::domain::read_models::GroupBy;
    use crate::models::domain::records::ToolCallRecord;
    use crate::models::domain::transcript::SessionKind;
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{
        AnalyticsRepo, DiagnosticsRepo, IngestRepo, MaintenanceRepo, ProfileRepo, UsageRepo,
    };
    use crate::test_support::{seed_session, seed_turn, temp_store, tokens};
    use chrono::{Duration, TimeZone, Utc};

    fn t0() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 16, 0, 0).unwrap()
    }

    fn seeded() -> (
        tempfile::TempDir,
        crate::models::repositories::db::SqliteStore,
    ) {
        let (d, s) = temp_store();
        seed_session(&s, "s1", SessionKind::Interactive, None, t0());
        seed_session(&s, "s2", SessionKind::Headless, None, t0());
        seed_turn(
            &s,
            "s1",
            "",
            "m1",
            t0() - Duration::minutes(61),
            Some("claude-opus-5-5"),
            "text",
            tokens(10, 1),
        );
        seed_turn(
            &s,
            "s1",
            "a1",
            "m2",
            t0() + Duration::minutes(1),
            Some("claude-haiku-4-5"),
            "text",
            tokens(5, 1),
        );
        seed_turn(
            &s,
            "s2",
            "",
            "m3",
            t0() + Duration::minutes(2),
            Some("claude-opus-5-5"),
            "tool_use",
            tokens(7, 1),
        );
        (d, s)
    }

    #[test]
    fn usage_by_each_axis() {
        let (_d, s) = seeded();
        let since = t0() - Duration::days(1);
        let mut by_model = s.usage_by(GroupBy::Model, since).unwrap();
        by_model.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(
            by_model
                .iter()
                .map(|g| (g.key.as_str(), g.usage.input))
                .collect::<Vec<_>>(),
            [("claude-haiku-4-5", 5), ("claude-opus-5-5", 17)]
        );
        let mut by_project = s.usage_by(GroupBy::Project, since).unwrap();
        by_project.sort_by(|a, b| {
            (a.key.clone(), a.model.clone()).cmp(&(b.key.clone(), b.model.clone()))
        });
        assert_eq!(by_project[0].key, "/work/s1");
        assert_eq!(by_project.len(), 3);
        let by_branch = s.usage_by(GroupBy::Branch, since).unwrap();
        assert!(by_branch.iter().all(|g| g.key == "main"));
        let mut by_agent = s.usage_by(GroupBy::Agent, since).unwrap();
        by_agent.sort_by(|a, b| {
            (a.key.clone(), a.model.clone()).cmp(&(b.key.clone(), b.model.clone()))
        });
        assert_eq!(
            by_agent.iter().map(|g| g.key.as_str()).collect::<Vec<_>>(),
            ["main", "subagent"]
        );
        assert!(
            s.usage_by(GroupBy::Model, t0() + Duration::hours(1))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn daily_usage_uses_local_date() {
        let (_d, s) = seeded();
        let days = s.daily_usage(t0() - Duration::days(1), 9 * 3600).unwrap();
        assert_eq!(
            days.iter()
                .map(|d| (d.day.as_str(), d.usage.input))
                .collect::<Vec<_>>(),
            [("2026-09-25", 10), ("2026-09-26", 12)]
        );
        let utc = s.daily_usage(t0() - Duration::days(1), 0).unwrap();
        assert_eq!(
            utc.iter().map(|d| d.day.as_str()).collect::<Vec<_>>(),
            ["2026-09-25"]
        );
    }

    #[test]
    fn tool_stats_count_errors() {
        let (_d, s) = seeded();
        for (id, name) in [("t1", "Bash"), ("t2", "Bash"), ("t3", "Read")] {
            s.upsert_tool_call(&ToolCallRecord {
                tool_use_id: id.into(),
                session_id: "s1".into(),
                agent_id: String::new(),
                ts: t0(),
                tool_name: name.into(),
            })
            .unwrap();
        }
        s.mark_tool_error("t2").unwrap();
        let st = s.tool_stats(t0() - Duration::hours(1)).unwrap();
        assert_eq!(
            st.iter()
                .map(|t| (t.tool_name.as_str(), t.calls, t.errors))
                .collect::<Vec<_>>(),
            [("Bash", 2, 1), ("Read", 1, 0)]
        );
    }

    #[test]
    fn reset_times_are_deduplicated_per_minute() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        let mut snap = parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap();
        let reset = Utc.with_ymd_and_hms(2026, 9, 26, 5, 20, 0).unwrap();
        for (i, ms) in [386, 462].iter().enumerate() {
            snap.limits[0].resets_at = Some(reset + Duration::milliseconds(*ms));
            s.record_snapshot(p.id, t0() + Duration::minutes(i as i64), &snap)
                .unwrap();
        }
        assert_eq!(
            s.reset_times(p.id, "session", t0() - Duration::hours(1))
                .unwrap(),
            [reset]
        );
        assert!(
            s.reset_times(p.id, "weekly_all", t0() + Duration::hours(1))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn daemon_stats_and_db_size() {
        let (_d, s) = temp_store();
        s.record_daemon_stats(t0(), 14 * 1024 * 1024).unwrap();
        let st = s.daemon_stats(t0() - Duration::hours(1)).unwrap();
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].rss_bytes, 14 * 1024 * 1024);
        assert!(st[0].db_bytes > 0);
        assert!(s.db_size() > 0);
    }

    #[test]
    fn corrupt_aggregate_rows_are_errors() {
        let (_d, s) = seeded();
        s.with(|c| c.execute("UPDATE turns SET input = X'00'", []))
            .unwrap();
        assert!(
            s.usage_by(GroupBy::Model, t0() - Duration::days(1))
                .is_err()
        );
        assert!(s.daily_usage(t0() - Duration::days(1), 0).is_err());
        s.record_daemon_stats(t0(), 1).unwrap();
        s.with(|c| c.execute("UPDATE daemon_stats SET rss_bytes = X'00'", []))
            .unwrap();
        assert!(s.daemon_stats(t0() - Duration::hours(1)).is_err());
    }
}
