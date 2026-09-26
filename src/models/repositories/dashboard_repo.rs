//! 概要画面の読み取り。

use crate::models::domain::read_models::{JobInfo, LatestUsage, RunRow, SessionRow};
use crate::models::domain::transcript::SessionKind;
use crate::models::domain::usage::{BreakdownRow, LimitWindow, Spend};
use crate::models::ports::{DashboardRepo, RepoError};
use crate::models::repositories::db::{SqliteStore, parse_ts, query_rows, ts};
use chrono::{DateTime, Utc};
use rusqlite::params;

/// セッション行の共通のSELECT。本体の最後のターンを相関サブクエリで引く。Task 6の一覧と詳細でも使う。
pub(crate) const SESSION_SELECT: &str = "SELECT s.session_id, s.profile_id, s.kind, s.name, s.cwd, s.git_branch, s.first_prompt, s.status,
  s.started_at, s.last_activity_at,
  (SELECT t.kind FROM turns t WHERE t.session_id = s.session_id AND t.agent_id = '' ORDER BY t.ts DESC, t.id DESC LIMIT 1),
  (SELECT t.model FROM turns t WHERE t.session_id = s.session_id AND t.agent_id = '' AND t.model IS NOT NULL ORDER BY t.ts DESC, t.id DESC LIMIT 1),
  (SELECT t.input + t.cache_read + t.cache_write_5m + t.cache_write_1h FROM turns t
     WHERE t.session_id = s.session_id AND t.agent_id = '' AND t.model IS NOT NULL ORDER BY t.ts DESC, t.id DESC LIMIT 1),
  (SELECT COUNT(*) FROM turns t WHERE t.session_id = s.session_id)
FROM sessions s";

/// `SESSION_SELECT`の1行。
pub(crate) type SessionTuple = (
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    i64,
);

/// タプルからセッション行を作る。時刻か種別が読めない行は`None`にして一覧から外す。1行の破損で画面全体を止めないため。
pub(crate) fn session_row(t: SessionTuple) -> Option<SessionRow> {
    let (
        session_id,
        profile_id,
        kind,
        name,
        cwd,
        git_branch,
        first_prompt,
        status,
        started,
        last,
        last_kind,
        last_model,
        ctx,
        turn_count,
    ) = t;
    Some(SessionRow {
        session_id,
        profile_id,
        kind: SessionKind::parse(&kind)?,
        name,
        cwd,
        git_branch,
        first_prompt,
        status,
        started_at: parse_ts(&started)?,
        last_activity_at: parse_ts(&last)?,
        last_turn_kind: last_kind,
        last_model,
        last_context_tokens: ctx.unwrap_or(0) as u64,
        turn_count,
    })
}

type LimitTuple = (String, Option<String>, f64, Option<String>, String);

fn limit_window(t: LimitTuple) -> LimitWindow {
    let (kind, scope_label, percent, resets_at, severity) = t;
    LimitWindow {
        kind,
        group: String::new(),
        percent,
        severity,
        resets_at: resets_at.as_deref().and_then(parse_ts),
        scope_label,
    }
}

impl SqliteStore {
    fn jobs_for(&self, session_ids: &[String]) -> Result<Vec<(String, JobInfo)>, RepoError> {
        let ids =
            serde_json::to_string(session_ids).map_err(|e| RepoError::Invalid(e.to_string()))?;
        let rows: Vec<(String, String, String, Option<String>, i64)> = self.with(|c| {
            query_rows(
                c,
                "SELECT session_id, job_id, state, detail, in_flight_tasks FROM jobs
                 WHERE session_id IN (SELECT value FROM json_each(?1)) ORDER BY updated_at",
                params![ids],
            )
        })?;
        Ok(rows
            .into_iter()
            .map(|(sid, job_id, state, detail, in_flight_tasks)| {
                (
                    sid,
                    JobInfo {
                        job_id,
                        state,
                        detail,
                        in_flight_tasks,
                    },
                )
            })
            .collect())
    }
}

impl DashboardRepo for SqliteStore {
    fn latest_usage(&self, profile_id: i64) -> Result<Option<LatestUsage>, RepoError> {
        let latest: Vec<(Option<String>,)> = self.with(|c| {
            query_rows(
                c,
                "SELECT MAX(fetched_at) FROM usage_samples WHERE profile_id = ?1",
                params![profile_id],
            )
        })?;
        let Some(at) = latest.into_iter().next().and_then(|(a,)| a) else {
            return Ok(None);
        };
        let limits: Vec<LimitTuple> = self.with(|c| {
            query_rows(
                c,
                "SELECT kind, scope_label, percent, resets_at, severity FROM usage_samples
                 WHERE profile_id = ?1 AND fetched_at = ?2 ORDER BY id",
                params![profile_id, at],
            )
        })?;
        let breakdown: Vec<(String, String, f64)> = self.with(|c| {
            query_rows(
                c,
                "SELECT key, display_name, percent FROM usage_breakdown WHERE profile_id = ?1 ORDER BY percent DESC, key",
                params![profile_id],
            )
        })?;
        let spend: Vec<(i64, Option<i64>, i64, String)> = self.with(|c| {
            query_rows(
                c,
                "SELECT used_minor, limit_minor, exponent, currency FROM spend_samples
                 WHERE profile_id = ?1 ORDER BY fetched_at DESC, id DESC LIMIT 1",
                params![profile_id],
            )
        })?;
        Ok(parse_ts(&at).map(|fetched_at| LatestUsage {
            fetched_at,
            limits: limits.into_iter().map(limit_window).collect(),
            breakdown: breakdown
                .into_iter()
                .map(|(key, display_name, percent)| BreakdownRow {
                    key,
                    display_name,
                    percent,
                })
                .collect(),
            spend: spend
                .into_iter()
                .next()
                .map(|(used_minor, limit_minor, exponent, currency)| Spend {
                    used_minor,
                    limit_minor,
                    exponent: exponent as u32,
                    currency,
                }),
        }))
    }

    fn recent_runs(&self, since: DateTime<Utc>) -> Result<Vec<RunRow>, RepoError> {
        let sql = format!(
            "{SESSION_SELECT}
             WHERE s.last_activity_at >= ?1
                OR (s.kind = 'interactive' AND s.status IS NOT NULL AND s.status != 'ended')
             ORDER BY s.last_activity_at DESC LIMIT 200"
        );
        let rows: Vec<SessionTuple> = self.with(|c| query_rows(c, &sql, params![ts(since)]))?;
        let sessions: Vec<SessionRow> = rows.into_iter().filter_map(session_row).collect();
        let ids: Vec<String> = sessions.iter().map(|s| s.session_id.clone()).collect();
        let jobs = self.jobs_for(&ids)?;
        Ok(sessions
            .into_iter()
            .map(|session| {
                let job = jobs
                    .iter()
                    .rev()
                    .find(|(sid, _)| *sid == session.session_id)
                    .map(|(_, j)| j.clone());
                RunRow { session, job }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::domain::records::JobRecord;
    use crate::models::domain::transcript::SessionKind;
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{DashboardRepo, IngestRepo, ProfileRepo, UsageRepo};
    use crate::test_support::{seed_session, seed_turn, temp_store, tokens};
    use chrono::{Duration, TimeZone, Utc};

    fn t0() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    #[test]
    fn latest_usage_is_none_before_first_fetch() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        assert_eq!(s.latest_usage(p.id).unwrap(), None);
    }

    #[test]
    fn latest_usage_returns_only_the_newest_fetch() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        let snap = parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap();
        s.record_snapshot(p.id, t0(), &snap).unwrap();
        let mut newer = snap.clone();
        newer.limits[0].percent = 42.0;
        s.record_snapshot(p.id, t0() + Duration::minutes(1), &newer)
            .unwrap();
        let got = s.latest_usage(p.id).unwrap().unwrap();
        assert_eq!(got.fetched_at, t0() + Duration::minutes(1));
        assert_eq!(got.limits.len(), 3);
        assert_eq!(
            (got.limits[0].kind.as_str(), got.limits[0].percent),
            ("session", 42.0)
        );
        assert_eq!(got.limits[2].scope_label.as_deref(), Some("Fable"));
        assert_eq!(got.breakdown[0].display_name, "Claude Code");
        assert_eq!(got.spend.unwrap().used_minor, 1850);
    }

    #[test]
    fn recent_runs_include_live_interactive_and_recent_others() {
        let (_d, s) = temp_store();
        let now = t0();
        seed_session(
            &s,
            "live-old",
            SessionKind::Interactive,
            Some("idle"),
            now - Duration::hours(6),
        );
        seed_session(
            &s,
            "ended-old",
            SessionKind::Interactive,
            Some("ended"),
            now - Duration::hours(6),
        );
        seed_session(
            &s,
            "hist-old",
            SessionKind::Interactive,
            None,
            now - Duration::hours(6),
        );
        seed_session(
            &s,
            "headless-new",
            SessionKind::Headless,
            None,
            now - Duration::seconds(30),
        );
        seed_session(
            &s,
            "job",
            SessionKind::BackgroundJob,
            Some("working"),
            now - Duration::minutes(1),
        );
        seed_turn(
            &s,
            "headless-new",
            "",
            "m1",
            now - Duration::seconds(40),
            Some("claude-opus-5-5"),
            "text",
            tokens(100, 5),
        );
        seed_turn(
            &s,
            "headless-new",
            "",
            "m2",
            now - Duration::seconds(30),
            Some("claude-opus-5-5"),
            "tool_use",
            tokens(300, 5),
        );
        seed_turn(
            &s,
            "headless-new",
            "a1",
            "m3",
            now - Duration::seconds(20),
            Some("claude-haiku-4-5"),
            "text",
            tokens(9, 1),
        );
        let p = s.ensure_default().unwrap();
        s.upsert_job(&JobRecord {
            job_id: "j1".into(),
            profile_id: p.id,
            session_id: Some("job".into()),
            name: Some("夜間ジョブ".into()),
            state: "working".into(),
            detail: Some("3/8件目".into()),
            in_flight_tasks: 2,
            tokens: None,
            created_at: None,
            updated_at: now - Duration::minutes(1),
        })
        .unwrap();
        let runs = s.recent_runs(now - Duration::minutes(10)).unwrap();
        let ids: Vec<&str> = runs.iter().map(|r| r.session.session_id.as_str()).collect();
        assert_eq!(ids, ["headless-new", "job", "live-old"]);
        let h = &runs[0].session;
        assert_eq!(
            (
                h.last_turn_kind.as_deref(),
                h.last_model.as_deref(),
                h.last_context_tokens,
                h.turn_count
            ),
            (Some("tool_use"), Some("claude-opus-5-5"), 300, 3)
        );
        let job = runs[1].job.as_ref().unwrap();
        assert_eq!(
            (job.detail.as_deref(), job.in_flight_tasks),
            (Some("3/8件目"), 2)
        );
        assert!(runs[2].job.is_none());
    }

    #[test]
    fn corrupt_rows_are_errors_or_skipped() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        let snap = parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap();
        s.record_snapshot(p.id, t0(), &snap).unwrap();
        s.with(|c| c.execute("UPDATE usage_samples SET percent = X'00'", []))
            .unwrap();
        assert!(s.latest_usage(p.id).is_err());
        seed_session(&s, "bad", SessionKind::Headless, None, t0());
        s.with(|c| c.execute("UPDATE sessions SET kind = 'unknown'", []))
            .unwrap();
        assert!(s.recent_runs(t0() - Duration::hours(1)).unwrap().is_empty());
        s.with(|c| c.execute("UPDATE sessions SET kind = X'00'", []))
            .unwrap();
        assert!(s.recent_runs(t0() - Duration::hours(1)).is_err());
    }
}
