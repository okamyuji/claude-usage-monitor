//! セッション画面とログ再生の読み取り。

use crate::models::domain::read_models::{
    CalendarTurn, SessionFilter, SessionModelUsage, SessionRow, SubagentRow, TimeRange, TurnRow,
};
use crate::models::domain::transcript::SessionKind;
use crate::models::ports::{RepoError, SessionQueryRepo};
use crate::models::repositories::dashboard_repo::{SESSION_SELECT, SessionTuple, session_row};
use crate::models::repositories::db::{SqliteStore, parse_ts, query_rows, ts, usage_of};
use chrono::{DateTime, Utc};
use rusqlite::params;

fn ids_json(ids: &[String]) -> Result<String, RepoError> {
    serde_json::to_string(ids).map_err(|e| RepoError::Invalid(e.to_string()))
}

type TurnTuple = (
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    i64,
);

fn turn_row(t: TurnTuple) -> Option<TurnRow> {
    let (agent_id, message_id, ts, model, kind, summary, i, o, cr, w5, w1) = t;
    Some(TurnRow {
        agent_id,
        message_id,
        ts: parse_ts(&ts)?,
        model,
        kind,
        summary,
        usage: usage_of(i, o, cr, w5, w1),
    })
}

/// `model_usage`の1行（セッション、エージェント、モデル、Token数の5列）。
type ModelUsageTuple = (String, String, Option<String>, i64, i64, i64, i64, i64);

type SubagentTuple = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn subagent_row(t: SubagentTuple) -> SubagentRow {
    let (agent_id, session_id, agent_type, description, parent_tool_use_id, first, last, last_kind) =
        t;
    SubagentRow {
        agent_id,
        session_id,
        agent_type,
        description,
        parent_tool_use_id,
        first_ts: first.as_deref().and_then(parse_ts),
        last_ts: last.as_deref().and_then(parse_ts),
        last_turn_kind: last_kind,
    }
}

type CalendarTuple = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
);

fn calendar_row(t: CalendarTuple) -> Option<CalendarTurn> {
    let (session_id, profile_name, kind, cwd, title, ts, tokens) = t;
    Some(CalendarTurn {
        session_id,
        profile_name,
        kind: SessionKind::parse(&kind)?,
        cwd,
        title,
        ts: parse_ts(&ts)?,
        tokens: tokens as u64,
    })
}

impl SessionQueryRepo for SqliteStore {
    fn list_sessions(&self, f: &SessionFilter) -> Result<Vec<SessionRow>, RepoError> {
        let sql = format!(
            "{SESSION_SELECT}
             WHERE (?1 = '' OR instr(lower(COALESCE(s.name,'') || ' ' || COALESCE(s.first_prompt,'') || ' ' || COALESCE(s.cwd,'') || ' ' || s.session_id), lower(?1)) > 0)
               AND (?2 IS NULL OR s.profile_id = ?2)
               AND (?3 IS NULL OR s.kind = ?3)
             ORDER BY s.last_activity_at DESC LIMIT ?4"
        );
        let rows: Vec<SessionTuple> = self.with(|c| {
            query_rows(
                c,
                &sql,
                params![
                    f.query,
                    f.profile_id,
                    f.kind.map(|k| k.as_str()),
                    f.limit as i64
                ],
            )
        })?;
        Ok(rows.into_iter().filter_map(session_row).collect())
    }

    fn session(&self, session_id: &str) -> Result<Option<SessionRow>, RepoError> {
        let sql = format!("{SESSION_SELECT} WHERE s.session_id = ?1");
        let rows: Vec<SessionTuple> = self.with(|c| query_rows(c, &sql, params![session_id]))?;
        Ok(rows.into_iter().find_map(session_row))
    }

    fn turns(
        &self,
        session_id: &str,
        limit: usize,
        range: Option<TimeRange>,
    ) -> Result<Vec<TurnRow>, RepoError> {
        let (from, to) = range.map_or((None, None), |(f, t)| (Some(ts(f)), Some(ts(t))));
        let rows: Vec<TurnTuple> = self.with(|c| {
            query_rows(
                c,
                "SELECT agent_id, message_id, ts, model, kind, summary, input, output, cache_read, cache_write_5m, cache_write_1h FROM (
                   SELECT *, id AS rid FROM turns
                   WHERE session_id = ?1 AND (?3 IS NULL OR ts >= ?3) AND (?4 IS NULL OR ts <= ?4)
                   ORDER BY ts DESC, id DESC LIMIT ?2
                 ) ORDER BY ts, rid",
                params![session_id, limit as i64, from, to],
            )
        })?;
        Ok(rows.into_iter().filter_map(turn_row).collect())
    }

    fn model_usage(&self, session_ids: &[String]) -> Result<Vec<SessionModelUsage>, RepoError> {
        let ids = ids_json(session_ids)?;
        let rows: Vec<ModelUsageTuple> = self.with(|c| {
            query_rows(
                c,
                "SELECT session_id, agent_id, model, SUM(input), SUM(output), SUM(cache_read), SUM(cache_write_5m), SUM(cache_write_1h)
                 FROM turns WHERE session_id IN (SELECT value FROM json_each(?1)) GROUP BY session_id, agent_id, model",
                params![ids],
            )
        })?;
        Ok(rows
            .into_iter()
            .map(
                |(session_id, agent_id, model, i, o, cr, w5, w1)| SessionModelUsage {
                    session_id,
                    agent_id,
                    model,
                    usage: usage_of(i, o, cr, w5, w1),
                },
            )
            .collect())
    }

    fn subagents(&self, session_ids: &[String]) -> Result<Vec<SubagentRow>, RepoError> {
        let ids = ids_json(session_ids)?;
        let rows: Vec<SubagentTuple> = self.with(|c| {
            query_rows(
                c,
                "SELECT a.agent_id, a.session_id, a.agent_type, a.description, a.parent_tool_use_id,
                   (SELECT MIN(t.ts) FROM turns t WHERE t.session_id = a.session_id AND t.agent_id = a.agent_id),
                   (SELECT MAX(t.ts) FROM turns t WHERE t.session_id = a.session_id AND t.agent_id = a.agent_id),
                   (SELECT t.kind FROM turns t WHERE t.session_id = a.session_id AND t.agent_id = a.agent_id ORDER BY t.ts DESC, t.id DESC LIMIT 1)
                 FROM subagents a WHERE a.session_id IN (SELECT value FROM json_each(?1)) ORDER BY 6",
                params![ids],
            )
        })?;
        Ok(rows.into_iter().map(subagent_row).collect())
    }

    fn job_id(&self, session_id: &str) -> Result<Option<String>, RepoError> {
        let rows: Vec<(String,)> = self.with(|c| {
            query_rows(
                c,
                "SELECT job_id FROM jobs WHERE session_id = ?1 ORDER BY updated_at DESC LIMIT 1",
                params![session_id],
            )
        })?;
        Ok(rows.into_iter().next().map(|(j,)| j))
    }

    fn calendar_turns(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<CalendarTurn>, RepoError> {
        // サブエージェントのターンは本体と時刻が入り混じるため、並べ替えはSQLで行う。
        let rows: Vec<CalendarTuple> = self.with(|c| {
            query_rows(
                c,
                "SELECT t.session_id, p.name, s.kind, s.cwd, COALESCE(s.name, s.first_prompt), t.ts,
                        t.input + t.output + t.cache_read + t.cache_write_5m + t.cache_write_1h
                 FROM turns t
                 JOIN sessions s ON s.session_id = t.session_id
                 JOIN profiles p ON p.id = s.profile_id
                 WHERE t.ts >= ?1 AND t.ts < ?2
                 ORDER BY t.session_id, t.ts",
                params![ts(from), ts(to)],
            )
        })?;
        Ok(rows.into_iter().filter_map(calendar_row).collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::domain::read_models::SessionFilter;
    use crate::models::domain::records::SubagentRecord;
    use crate::models::domain::transcript::SessionKind;
    use crate::models::ports::{IngestRepo, SessionQueryRepo};
    use crate::test_support::{seed_session, seed_turn, temp_store, tokens};
    use chrono::{Duration, TimeZone, Utc};

    fn t0() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    #[test]
    fn list_filters_by_query_kind_and_limit() {
        let (_d, s) = temp_store();
        seed_session(
            &s,
            "alpha",
            SessionKind::Interactive,
            None,
            t0() - Duration::minutes(3),
        );
        seed_session(
            &s,
            "beta",
            SessionKind::Headless,
            None,
            t0() - Duration::minutes(2),
        );
        seed_session(
            &s,
            "gamma",
            SessionKind::Headless,
            None,
            t0() - Duration::minutes(1),
        );
        let ids = |f: SessionFilter| -> Vec<String> {
            s.list_sessions(&f)
                .unwrap()
                .into_iter()
                .map(|r| r.session_id)
                .collect()
        };
        assert_eq!(ids(SessionFilter::default()), ["gamma", "beta", "alpha"]);
        assert_eq!(
            ids(SessionFilter {
                query: "PROMPT-BE".into(),
                ..SessionFilter::default()
            }),
            ["beta"]
        );
        assert_eq!(
            ids(SessionFilter {
                query: "/work/al".into(),
                ..SessionFilter::default()
            }),
            ["alpha"]
        );
        assert_eq!(
            ids(SessionFilter {
                kind: Some(SessionKind::Headless),
                limit: 1,
                ..SessionFilter::default()
            }),
            ["gamma"]
        );
        assert_eq!(
            ids(SessionFilter {
                profile_id: Some(999),
                ..SessionFilter::default()
            }),
            Vec::<String>::new()
        );
        assert_eq!(
            ids(SessionFilter {
                query: "%".into(),
                ..SessionFilter::default()
            }),
            Vec::<String>::new()
        );
        assert_eq!(
            s.session("beta").unwrap().unwrap().name.as_deref(),
            Some("name-beta")
        );
        assert_eq!(s.session("none").unwrap(), None);
    }

    #[test]
    fn turns_are_chronological_and_limited_to_newest() {
        let (_d, s) = temp_store();
        seed_session(&s, "s1", SessionKind::Interactive, None, t0());
        for i in 0..5 {
            seed_turn(
                &s,
                "s1",
                "",
                &format!("m{i}"),
                t0() + Duration::seconds(i),
                Some("claude-opus-5-5"),
                "text",
                tokens(i as u64, 1),
            );
        }
        let all = s.turns("s1", 100, None).unwrap();
        assert_eq!(
            all.iter()
                .map(|t| t.message_id.as_str())
                .collect::<Vec<_>>(),
            ["m0", "m1", "m2", "m3", "m4"]
        );
        assert_eq!(all[4].usage.input, 4);
        let newest = s.turns("s1", 2, None).unwrap();
        assert_eq!(
            newest
                .iter()
                .map(|t| t.message_id.as_str())
                .collect::<Vec<_>>(),
            ["m3", "m4"]
        );
    }

    #[test]
    fn turns_in_a_range_reach_past_the_newest_limit() {
        let (_d, s) = temp_store();
        seed_session(&s, "s1", SessionKind::Interactive, None, t0());
        for i in 0..5 {
            let at = t0() + Duration::seconds(i);
            seed_turn(
                &s,
                "s1",
                "",
                &format!("m{i}"),
                at,
                None,
                "text",
                tokens(1, 1),
            );
        }
        let range = Some((t0() + Duration::seconds(1), t0() + Duration::seconds(2)));
        let ids: Vec<String> = s
            .turns("s1", 2, range)
            .unwrap()
            .into_iter()
            .map(|t| t.message_id)
            .collect();
        assert_eq!(ids, ["m1", "m2"]);
    }

    #[test]
    fn model_usage_and_subagents_are_grouped() {
        let (_d, s) = temp_store();
        seed_session(&s, "s1", SessionKind::Interactive, None, t0());
        seed_turn(
            &s,
            "s1",
            "",
            "m1",
            t0(),
            Some("claude-opus-5-5"),
            "text",
            tokens(10, 1),
        );
        seed_turn(
            &s,
            "s1",
            "",
            "m2",
            t0(),
            Some("claude-opus-5-5"),
            "text",
            tokens(20, 2),
        );
        seed_turn(
            &s,
            "s1",
            "a1",
            "m3",
            t0() + Duration::seconds(5),
            Some("claude-haiku-4-5"),
            "tool_use",
            tokens(7, 3),
        );
        s.upsert_subagent(&SubagentRecord {
            agent_id: "a1".into(),
            session_id: "s1".into(),
            agent_type: Some("go-reviewer".into()),
            description: Some("レビュー".into()),
            parent_tool_use_id: Some("toolu_1".into()),
            spawn_depth: Some(1),
        })
        .unwrap();
        let mut u = s.model_usage(&["s1".to_string()]).unwrap();
        u.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
        assert_eq!(
            (u[0].agent_id.as_str(), u[0].usage.input, u[0].usage.output),
            ("", 30, 3)
        );
        assert_eq!(
            (u[1].agent_id.as_str(), u[1].model.as_deref()),
            ("a1", Some("claude-haiku-4-5"))
        );
        let subs = s.subagents(&["s1".to_string()]).unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(
            (
                subs[0].agent_type.as_deref(),
                subs[0].parent_tool_use_id.as_deref(),
                subs[0].last_turn_kind.as_deref()
            ),
            (Some("go-reviewer"), Some("toolu_1"), Some("tool_use"))
        );
        assert_eq!(subs[0].last_ts, Some(t0() + Duration::seconds(5)));
        assert!(s.model_usage(&[]).unwrap().is_empty());
    }

    #[test]
    fn corrupt_turn_row_is_error() {
        let (_d, s) = temp_store();
        seed_session(&s, "s1", SessionKind::Interactive, None, t0());
        seed_turn(&s, "s1", "", "m1", t0(), None, "prompt", tokens(0, 0));
        s.with(|c| c.execute("UPDATE turns SET input = X'00'", []))
            .unwrap();
        assert!(s.turns("s1", 10, None).is_err());
        assert!(s.model_usage(&["s1".to_string()]).is_err());
        s.with(|c| c.execute("UPDATE turns SET input = 0, ts = 'bad'", []))
            .unwrap();
        assert!(s.turns("s1", 10, None).unwrap().is_empty());
    }

    #[test]
    fn calendar_turns_returns_range_sorted_by_session_then_time_with_subagents() {
        let (_d, s) = temp_store();
        seed_session(&s, "b", SessionKind::Headless, None, t0());
        seed_session(&s, "a", SessionKind::Interactive, None, t0());
        let from = t0() - Duration::hours(1);
        let to = t0();
        let at = |m: i64| from + Duration::minutes(m);
        seed_turn(&s, "b", "", "b1", at(5), None, "prompt", tokens(1, 2));
        seed_turn(&s, "a", "agent-x", "a2", at(20), None, "text", tokens(3, 4));
        seed_turn(&s, "a", "", "a1", at(10), None, "prompt", tokens(5, 6));
        seed_turn(&s, "a", "", "a0", from, None, "prompt", tokens(0, 1));
        seed_turn(&s, "a", "", "a9", to, None, "prompt", tokens(0, 1));
        let got = s.calendar_turns(from, to).unwrap();
        let keys: Vec<(&str, i64)> = got
            .iter()
            .map(|t| (t.session_id.as_str(), (t.ts - from).num_minutes()))
            .collect();
        assert_eq!(keys, vec![("a", 0), ("a", 10), ("a", 20), ("b", 5)]);
        assert_eq!(got[2].tokens, 7);
        assert_eq!(got[0].profile_name, "default");
        assert_eq!(got[0].cwd.as_deref(), Some("/work/a"));
        assert_eq!(got[0].title.as_deref(), Some("name-a"));
        assert_eq!(
            (got[0].kind, got[3].kind),
            (SessionKind::Interactive, SessionKind::Headless)
        );
    }

    #[test]
    fn calendar_turns_sums_all_token_kinds() {
        let (_d, s) = temp_store();
        seed_session(&s, "a", SessionKind::Interactive, None, t0());
        let u = crate::models::domain::pricing::TokenUsage {
            input: 1,
            output: 10,
            cache_read: 100,
            cache_write_5m: 1000,
            cache_write_1h: 10000,
        };
        seed_turn(&s, "a", "", "a1", t0(), None, "prompt", u);
        let got = s.calendar_turns(t0(), t0() + Duration::minutes(1)).unwrap();
        assert_eq!(got[0].tokens, 11111);
    }

    #[test]
    fn calendar_turns_falls_back_to_first_prompt_and_skips_broken_rows() {
        let (_d, s) = temp_store();
        seed_session(&s, "a", SessionKind::Interactive, None, t0());
        s.with(|c| c.execute("UPDATE sessions SET name = NULL", []))
            .unwrap();
        seed_turn(&s, "a", "", "a1", t0(), None, "prompt", tokens(1, 1));
        let got = s.calendar_turns(t0(), t0() + Duration::minutes(1)).unwrap();
        assert_eq!(got[0].title.as_deref(), Some("prompt-a"));
        assert!(
            s.calendar_turns(t0() + Duration::minutes(1), t0() + Duration::minutes(2))
                .unwrap()
                .is_empty()
        );
        // 種別を読めない行は飛ばす。
        s.with(|c| c.execute("UPDATE sessions SET kind = 'unknown'", []))
            .unwrap();
        assert!(
            s.calendar_turns(t0(), t0() + Duration::minutes(1))
                .unwrap()
                .is_empty()
        );
        s.with(|c| c.execute("UPDATE sessions SET kind = 'interactive'", []))
            .unwrap();
        // 文字列比較で範囲に入るが日時として読めない行は飛ばす。
        s.with(|c| c.execute("UPDATE turns SET ts = '2026-09-26T03:00:00.000Zx'", []))
            .unwrap();
        assert!(
            s.calendar_turns(t0(), t0() + Duration::minutes(1))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn job_id_for_session() {
        let (_d, s) = temp_store();
        seed_session(&s, "js", SessionKind::BackgroundJob, Some("working"), t0());
        let p = crate::models::ports::ProfileRepo::ensure_default(&s).unwrap();
        s.upsert_job(&crate::models::domain::records::JobRecord {
            job_id: "j1".into(),
            profile_id: p.id,
            session_id: Some("js".into()),
            name: None,
            state: "working".into(),
            detail: None,
            in_flight_tasks: 0,
            tokens: None,
            created_at: None,
            updated_at: t0(),
        })
        .unwrap();
        assert_eq!(s.job_id("js").unwrap().as_deref(), Some("j1"));
        assert_eq!(s.job_id("none").unwrap(), None);
    }
}
