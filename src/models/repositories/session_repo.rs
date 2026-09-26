//! JSONL取り込み結果の永続化。
//!
//! 同じ行を再度読んでも結果が変わらないように、すべての書き込みを冪等なUPSERTにする。
//! ファイルが縮んで先頭から読み直した場合にTokenを二重に数えないため。
use crate::models::domain::records::{
    FileOffset, JobRecord, SessionUpsert, SubagentRecord, ToolCallRecord, TurnRecord,
};
use crate::models::ports::{IngestRepo, RepoError};
use crate::models::repositories::db::{SqliteStore, ts};
use rusqlite::{OptionalExtension, params};
use std::path::{Path, PathBuf};

const KIND_RANK: &str = "CASE {} WHEN 'background_job' THEN 3 WHEN 'headless' THEN 2 ELSE 1 END";

impl IngestRepo for SqliteStore {
    fn offset(&self, path: &Path) -> Result<Option<FileOffset>, RepoError> {
        let key = path.to_string_lossy().into_owned();
        self.with(|c| {
            c.query_row(
                "SELECT offset, size, mtime FROM ingest_offsets WHERE path = ?1",
                [&key],
                |r| {
                    Ok(FileOffset {
                        path: PathBuf::from(&key),
                        offset: r.get::<_, i64>(0)? as u64,
                        size: r.get::<_, i64>(1)? as u64,
                        mtime_ms: r.get(2)?,
                    })
                },
            )
            .optional()
        })
    }

    fn save_offset(&self, o: &FileOffset) -> Result<(), RepoError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO ingest_offsets(path, offset, size, mtime) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(path) DO UPDATE SET offset = excluded.offset, size = excluded.size, mtime = excluded.mtime",
                params![o.path.to_string_lossy(), o.offset as i64, o.size as i64, o.mtime_ms],
            )
            .map(|_| ())
        })
    }

    fn upsert_session(&self, s: &SessionUpsert) -> Result<(), RepoError> {
        let rank_new = KIND_RANK.replace("{}", "excluded.kind");
        let rank_old = KIND_RANK.replace("{}", "sessions.kind");
        let sql = format!(
            "INSERT INTO sessions(session_id, profile_id, kind, entrypoint, cwd, git_branch, name, first_prompt, started_at, last_activity_at, status)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(session_id) DO UPDATE SET
               kind = CASE WHEN ({rank_new}) > ({rank_old}) THEN excluded.kind ELSE sessions.kind END,
               entrypoint = COALESCE(sessions.entrypoint, excluded.entrypoint),
               cwd = COALESCE(excluded.cwd, sessions.cwd),
               git_branch = COALESCE(excluded.git_branch, sessions.git_branch),
               name = COALESCE(excluded.name, sessions.name),
               first_prompt = COALESCE(sessions.first_prompt, excluded.first_prompt),
               started_at = MIN(sessions.started_at, excluded.started_at),
               last_activity_at = MAX(sessions.last_activity_at, excluded.last_activity_at),
               status = COALESCE(excluded.status, sessions.status)"
        );
        self.with(|c| {
            c.execute(
                &sql,
                params![
                    s.session_id,
                    s.profile_id,
                    s.kind.as_str(),
                    s.entrypoint,
                    s.cwd,
                    s.git_branch,
                    s.name,
                    s.first_prompt,
                    ts(s.started_at),
                    ts(s.last_activity_at),
                    s.status
                ],
            )
            .map(|_| ())
        })
    }

    fn upsert_turn(&self, t: &TurnRecord) -> Result<(), RepoError> {
        let u = &t.usage;
        self.with(|c| {
            c.execute(
                "INSERT INTO turns(session_id, agent_id, message_id, ts, model, kind, summary, input, output, cache_read, cache_write_5m, cache_write_1h)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(session_id, agent_id, message_id) DO UPDATE SET
                   kind = CASE WHEN excluded.kind = 'thinking' THEN turns.kind ELSE excluded.kind END,
                   summary = CASE WHEN excluded.kind = 'thinking' THEN turns.summary ELSE excluded.summary END",
                params![
                    t.session_id, t.agent_id, t.message_id, ts(t.ts), t.model, t.kind, t.summary,
                    u.input as i64, u.output as i64, u.cache_read as i64, u.cache_write_5m as i64, u.cache_write_1h as i64
                ],
            )
            .map(|_| ())
        })
    }

    fn upsert_tool_call(&self, c: &ToolCallRecord) -> Result<(), RepoError> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO tool_calls(tool_use_id, session_id, agent_id, ts, tool_name) VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(tool_use_id) DO NOTHING",
                params![c.tool_use_id, c.session_id, c.agent_id, ts(c.ts), c.tool_name],
            )
            .map(|_| ())
        })
    }

    fn end_missing_interactive(
        &self,
        profile_id: i64,
        alive_session_ids: &[String],
    ) -> Result<usize, RepoError> {
        let alive = serde_json::to_string(alive_session_ids)
            .map_err(|e| RepoError::Invalid(e.to_string()))?;
        self.with(|c| {
            c.execute(
                "UPDATE sessions SET status = 'ended'
                 WHERE profile_id = ?1 AND kind = 'interactive' AND status IS NOT NULL AND status != 'ended'
                   AND session_id NOT IN (SELECT value FROM json_each(?2))",
                params![profile_id, alive],
            )
        })
    }

    fn mark_tool_error(&self, tool_use_id: &str) -> Result<(), RepoError> {
        self.with(|c| {
            c.execute(
                "UPDATE tool_calls SET is_error = 1 WHERE tool_use_id = ?1",
                [tool_use_id],
            )
            .map(|_| ())
        })
    }

    fn upsert_subagent(&self, s: &SubagentRecord) -> Result<(), RepoError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO subagents(agent_id, session_id, agent_type, description, parent_tool_use_id, spawn_depth) VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(agent_id) DO UPDATE SET agent_type = excluded.agent_type, description = excluded.description,
                   parent_tool_use_id = excluded.parent_tool_use_id, spawn_depth = excluded.spawn_depth",
                params![s.agent_id, s.session_id, s.agent_type, s.description, s.parent_tool_use_id, s.spawn_depth],
            )
            .map(|_| ())
        })
    }

    fn upsert_job(&self, j: &JobRecord) -> Result<(), RepoError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO jobs(job_id, profile_id, session_id, name, state, detail, in_flight_tasks, tokens, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(profile_id, job_id) DO UPDATE SET session_id = excluded.session_id, name = excluded.name,
                   state = excluded.state, detail = excluded.detail, in_flight_tasks = excluded.in_flight_tasks,
                   tokens = excluded.tokens, created_at = excluded.created_at, updated_at = excluded.updated_at",
                params![
                    j.job_id, j.profile_id, j.session_id, j.name, j.state, j.detail, j.in_flight_tasks, j.tokens,
                    j.created_at.map(ts), ts(j.updated_at)
                ],
            )
            .map(|_| ())
        })
    }
}
#[cfg(test)]
mod tests {
    use crate::models::domain::pricing::TokenUsage;
    use crate::models::domain::records::*;
    use crate::models::domain::transcript::SessionKind;
    use crate::models::ports::{IngestRepo, ProfileRepo};
    use crate::test_support::temp_store;
    use chrono::{Duration, TimeZone, Utc};
    use std::path::{Path, PathBuf};

    fn t(m: i64) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 0, 0, 0).unwrap() + Duration::minutes(m)
    }

    fn session(pid: i64, kind: SessionKind, at: i64) -> SessionUpsert {
        SessionUpsert {
            session_id: "s1".into(),
            profile_id: pid,
            kind,
            entrypoint: Some("cli".into()),
            cwd: Some("/w".into()),
            git_branch: None,
            name: None,
            first_prompt: None,
            started_at: t(at),
            last_activity_at: t(at),
            status: None,
        }
    }

    fn turn(kind: &str, summary: &str) -> TurnRecord {
        TurnRecord {
            session_id: "s1".into(),
            agent_id: String::new(),
            message_id: "msg_1".into(),
            ts: t(0),
            model: None,
            kind: kind.into(),
            summary: summary.into(),
            usage: TokenUsage {
                input: 2,
                output: 273,
                cache_read: 100,
                ..Default::default()
            },
        }
    }

    #[test]
    fn duplicate_message_lines_count_usage_once() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        s.upsert_session(&session(p.id, SessionKind::Interactive, 0))
            .unwrap();
        s.upsert_turn(&turn("thinking", "")).unwrap();
        s.upsert_turn(&turn("tool_use", "Bash: ls")).unwrap();
        s.upsert_turn(&turn("thinking", "")).unwrap();
        let (count, output, kind, summary): (i64, i64, String, String) = s
            .with(|c| {
                c.query_row(
                    "SELECT COUNT(*), SUM(output), MAX(kind), MAX(summary) FROM turns",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
            })
            .unwrap();
        assert_eq!(
            (count, output, kind.as_str(), summary.as_str()),
            (1, 273, "tool_use", "Bash: ls")
        );
    }

    #[test]
    fn session_upsert_merges_kind_time_and_first_prompt() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        s.upsert_session(&session(p.id, SessionKind::Headless, 5))
            .unwrap();
        let mut later = session(p.id, SessionKind::Interactive, 10);
        later.first_prompt = Some("最初".into());
        later.status = Some("busy".into());
        s.upsert_session(&later).unwrap();
        let mut earlier = session(p.id, SessionKind::BackgroundJob, 1);
        earlier.first_prompt = Some("二番目".into());
        earlier.cwd = None;
        s.upsert_session(&earlier).unwrap();
        let row: (String, String, String, String, String, String) = s
            .with(|c| c.query_row(
                "SELECT kind, started_at, last_activity_at, first_prompt, status, cwd FROM sessions",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            ))
            .unwrap();
        assert_eq!(row.0, "background_job");
        assert_eq!(row.1, crate::models::repositories::db::ts(t(1)));
        assert_eq!(row.2, crate::models::repositories::db::ts(t(10)));
        assert_eq!(
            (row.3.as_str(), row.4.as_str(), row.5.as_str()),
            ("最初", "busy", "/w")
        );
    }

    #[test]
    fn headless_does_not_downgrade_to_interactive() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        s.upsert_session(&session(p.id, SessionKind::Headless, 0))
            .unwrap();
        s.upsert_session(&session(p.id, SessionKind::Interactive, 1))
            .unwrap();
        let kind: String = s
            .with(|c| c.query_row("SELECT kind FROM sessions", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(kind, "headless");
    }

    #[test]
    fn tool_calls_and_errors() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        s.upsert_session(&session(p.id, SessionKind::Interactive, 0))
            .unwrap();
        let call = ToolCallRecord {
            tool_use_id: "toolu_1".into(),
            session_id: "s1".into(),
            agent_id: String::new(),
            ts: t(0),
            tool_name: "Bash".into(),
        };
        s.upsert_tool_call(&call).unwrap();
        s.upsert_tool_call(&call).unwrap();
        s.mark_tool_error("toolu_1").unwrap();
        s.mark_tool_error("unknown").unwrap();
        let (n, err): (i64, i64) = s
            .with(|c| {
                c.query_row("SELECT COUNT(*), SUM(is_error) FROM tool_calls", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
            })
            .unwrap();
        assert_eq!((n, err), (1, 1));
    }

    #[test]
    fn subagent_and_job_upserts_replace_values() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        s.upsert_session(&session(p.id, SessionKind::Interactive, 0))
            .unwrap();
        let mut a = SubagentRecord {
            agent_id: "a1".into(),
            session_id: "s1".into(),
            agent_type: Some("go-reviewer".into()),
            description: None,
            parent_tool_use_id: Some("toolu_9".into()),
            spawn_depth: Some(1),
        };
        s.upsert_subagent(&a).unwrap();
        a.description = Some("レビュー".into());
        s.upsert_subagent(&a).unwrap();
        let mut j = JobRecord {
            job_id: "b27".into(),
            profile_id: p.id,
            session_id: Some("s1".into()),
            name: None,
            state: "working".into(),
            detail: Some("1/8".into()),
            in_flight_tasks: 1,
            tokens: Some(10),
            created_at: Some(t(0)),
            updated_at: t(1),
        };
        s.upsert_job(&j).unwrap();
        j.state = "done".into();
        s.upsert_job(&j).unwrap();
        let (desc, state): (String, String) = s
            .with(|c| {
                Ok((
                    c.query_row("SELECT description FROM subagents", [], |r| r.get(0))?,
                    c.query_row("SELECT state FROM jobs", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!((desc.as_str(), state.as_str()), ("レビュー", "done"));
    }

    #[test]
    fn offsets_round_trip() {
        let (_d, s) = temp_store();
        assert_eq!(s.offset(Path::new("/x.jsonl")).unwrap(), None);
        let o = FileOffset {
            path: PathBuf::from("/x.jsonl"),
            offset: 10,
            size: 20,
            mtime_ms: 30,
        };
        s.save_offset(&o).unwrap();
        s.save_offset(&FileOffset {
            offset: 15,
            ..o.clone()
        })
        .unwrap();
        assert_eq!(s.offset(Path::new("/x.jsonl")).unwrap().unwrap().offset, 15);
    }

    #[test]
    fn purging_old_sessions_cascades_to_children() {
        use crate::models::ports::MaintenanceRepo;
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        s.upsert_session(&session(p.id, SessionKind::Interactive, 0))
            .unwrap();
        s.upsert_turn(&turn("text", "x")).unwrap();
        s.upsert_tool_call(&ToolCallRecord {
            tool_use_id: "toolu_1".into(),
            session_id: "s1".into(),
            agent_id: String::new(),
            ts: t(0),
            tool_name: "Bash".into(),
        })
        .unwrap();
        s.upsert_subagent(&SubagentRecord {
            agent_id: "a1".into(),
            session_id: "s1".into(),
            agent_type: None,
            description: None,
            parent_tool_use_id: None,
            spawn_depth: None,
        })
        .unwrap();
        s.upsert_job(&JobRecord {
            job_id: "j".into(),
            profile_id: p.id,
            session_id: None,
            name: None,
            state: "done".into(),
            detail: None,
            in_flight_tasks: 0,
            tokens: None,
            created_at: None,
            updated_at: t(0),
        })
        .unwrap();
        assert_eq!(s.purge_before(t(1)).unwrap(), 2);
        for table in ["sessions", "turns", "tool_calls", "subagents", "jobs"] {
            let n: i64 = s
                .with(|c| c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)))
                .unwrap();
            assert_eq!(n, 0, "{table}");
        }
    }

    #[test]
    fn end_missing_interactive_only_touches_live_interactive_rows() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        let t = chrono::Utc::now();
        let mk = |id: &str, kind: SessionKind, status: Option<&str>| SessionUpsert {
            session_id: id.into(),
            profile_id: p.id,
            kind,
            entrypoint: None,
            cwd: None,
            git_branch: None,
            name: None,
            first_prompt: None,
            started_at: t,
            last_activity_at: t,
            status: status.map(str::to_string),
        };
        s.upsert_session(&mk("alive", SessionKind::Interactive, Some("busy")))
            .unwrap();
        s.upsert_session(&mk("gone", SessionKind::Interactive, Some("idle")))
            .unwrap();
        s.upsert_session(&mk("hist", SessionKind::Interactive, None))
            .unwrap();
        s.upsert_session(&mk("job", SessionKind::BackgroundJob, Some("working")))
            .unwrap();
        let n = s
            .end_missing_interactive(p.id, &["alive".to_string()])
            .unwrap();
        assert_eq!(n, 1);
        let status = |id: &str| -> Option<String> {
            s.with(|c| {
                c.query_row(
                    "SELECT status FROM sessions WHERE session_id=?1",
                    [id],
                    |r| r.get(0),
                )
            })
            .unwrap()
        };
        assert_eq!(status("alive").as_deref(), Some("busy"));
        assert_eq!(status("gone").as_deref(), Some("ended"));
        assert_eq!(status("hist"), None);
        assert_eq!(status("job").as_deref(), Some("working"));
    }
}
