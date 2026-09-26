//! 稼働中の実行の一覧。対話、ヘッドレス、ジョブと、その下のサブエージェント。
use crate::controllers::gui::app::GuiDeps;
use crate::models::domain::activity::{RunKind, is_active, run_state, subagent_active};
use crate::models::domain::display::{context_ratio, fmt_duration, fmt_ratio, fmt_tokens, fmt_usd};
use crate::models::domain::pricing::{ModelInfo, TokenUsage, find_model, sum_cost, total_tokens};
use crate::models::domain::read_models::{RunRow, SessionModelUsage, SubagentRow};
use crate::models::domain::transcript::{SUMMARY_CHARS, one_line};
use crate::models::ports::RepoError;
use chrono::{DateTime, Duration, Utc};

/// 稼働中の実行1行。
#[derive(Debug, Clone, PartialEq)]
pub struct RunItem {
    /// セッションID。行を押したときの選択に使う。
    pub session_id: String,
    /// 種別。
    pub kind: RunKind,
    /// 状態の文言。
    pub state: String,
    /// 見出し。
    pub title: String,
    /// プロジェクト（作業ディレクトリの末尾）。
    pub project: String,
    /// コンテキスト使用率。
    pub context: String,
    /// 経過時間。
    pub elapsed: String,
    /// Token数。
    pub tokens: String,
    /// コスト。
    pub cost: String,
    /// ジョブの進捗。
    pub detail: Option<String>,
    /// 字下げ。サブエージェントは1。
    pub depth: u8,
}

/// 稼働中の実行を、親の直後にそのサブエージェントを並べる順で作る。
pub fn runs(deps: &GuiDeps) -> Result<Vec<RunItem>, RepoError> {
    let now = deps.clock.now();
    let settings = deps.settings.load()?;
    let models = deps.models.all()?;
    let rules = settings.rules();
    let window = rules.headless_active.max(rules.job_active) + Duration::minutes(1);
    let active: Vec<RunRow> = deps
        .dashboard
        .recent_runs(now - window)?
        .into_iter()
        .filter(|r| {
            is_active(
                r.session.kind,
                r.session.status.as_deref(),
                r.session.last_activity_at,
                now,
                &rules,
            )
        })
        .collect();
    let ids: Vec<String> = active
        .iter()
        .map(|r| r.session.session_id.clone())
        .collect();
    let usage = deps.sessions.model_usage(&ids)?;
    let subs: Vec<SubagentRow> = deps
        .sessions
        .subagents(&ids)?
        .into_iter()
        .filter(|a| a.last_ts.is_some_and(|t| subagent_active(t, now, &rules)))
        .collect();
    let mut out = vec![];
    for r in &active {
        out.push(run_item(r, &usage, &models, now));
        out.extend(
            subs.iter()
                .filter(|a| a.session_id == r.session.session_id)
                .map(|a| sub_item(a, &usage, &models, now)),
        );
    }
    Ok(out)
}

fn cost_and_tokens(
    rows: Vec<(Option<&str>, TokenUsage)>,
    models: &[ModelInfo],
) -> (String, String) {
    let total = rows
        .iter()
        .fold(TokenUsage::default(), |acc, (_, u)| acc.plus(u));
    (
        fmt_usd(sum_cost(models, &rows)),
        fmt_tokens(total_tokens(&total)),
    )
}

fn run_item(
    r: &RunRow,
    usage: &[SessionModelUsage],
    models: &[ModelInfo],
    now: DateTime<Utc>,
) -> RunItem {
    let s = &r.session;
    let rows: Vec<(Option<&str>, TokenUsage)> = usage
        .iter()
        .filter(|u| u.session_id == s.session_id)
        .map(|u| (u.model.as_deref(), u.usage))
        .collect();
    let (cost, tokens) = cost_and_tokens(rows, models);
    let window = s
        .last_model
        .as_deref()
        .and_then(|m| find_model(models, m))
        .and_then(|m| m.context_window);
    RunItem {
        session_id: s.session_id.clone(),
        kind: RunKind::from_session(s.kind),
        state: run_state(s.status.as_deref(), s.last_turn_kind.as_deref()).label(),
        title: s
            .name
            .clone()
            .or_else(|| s.first_prompt.as_deref().map(|p| one_line(p, 40)))
            .unwrap_or_else(|| s.session_id.chars().take(8).collect()),
        project: project_name(s.cwd.as_deref()),
        context: fmt_ratio(context_ratio(s.last_context_tokens, window)),
        elapsed: fmt_duration(now - s.started_at),
        tokens,
        cost,
        detail: r.job.as_ref().map(|j| match &j.detail {
            Some(d) => format!(
                "{} / 実行中タスク {}",
                one_line(d, SUMMARY_CHARS),
                j.in_flight_tasks
            ),
            None => format!("実行中タスク {}", j.in_flight_tasks),
        }),
        depth: 0,
    }
}

fn sub_item(
    a: &SubagentRow,
    usage: &[SessionModelUsage],
    models: &[ModelInfo],
    now: DateTime<Utc>,
) -> RunItem {
    let rows: Vec<(Option<&str>, TokenUsage)> = usage
        .iter()
        .filter(|u| u.session_id == a.session_id && u.agent_id == a.agent_id)
        .map(|u| (u.model.as_deref(), u.usage))
        .collect();
    let (cost, tokens) = cost_and_tokens(rows, models);
    RunItem {
        session_id: a.session_id.clone(),
        kind: RunKind::Subagent,
        state: run_state(None, a.last_turn_kind.as_deref()).label(),
        title: format!(
            "{}「{}」",
            a.agent_type.as_deref().unwrap_or("サブエージェント"),
            a.description.as_deref().unwrap_or("")
        ),
        project: String::new(),
        context: "—".into(),
        elapsed: a
            .first_ts
            .map(|t| fmt_duration(now - t))
            .unwrap_or_default(),
        tokens,
        cost,
        detail: None,
        depth: 1,
    }
}

/// 作業ディレクトリの末尾。
fn project_name(cwd: Option<&str>) -> String {
    cwd.and_then(|c| std::path::Path::new(c).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::pricing::seed_models;
    use crate::models::domain::records::{JobRecord, SubagentRecord};
    use crate::models::domain::transcript::SessionKind;
    use crate::models::ports::{IngestRepo, ModelRepo, ProfileRepo};
    use crate::test_support::{
        FakeCreds, FakeDaemon, FixedClock, gui_deps, seed_session, seed_turn, temp_store, tokens,
    };
    use chrono::TimeZone;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    #[test]
    fn active_runs_with_subagents_jobs_and_costs() {
        let (_d, s) = temp_store();
        let s = Arc::new(s);
        s.seed_if_empty(&seed_models()).unwrap();
        seed_session(
            &s,
            "h1",
            SessionKind::Headless,
            None,
            now() - Duration::seconds(30),
        );
        seed_turn(
            &s,
            "h1",
            "",
            "m1",
            now() - Duration::seconds(30),
            Some("claude-opus-5-5"),
            "tool_use",
            tokens(100_000, 1_000),
        );
        seed_turn(
            &s,
            "h1",
            "a1",
            "m2",
            now() - Duration::seconds(10),
            Some("claude-haiku-4-5"),
            "text",
            tokens(1_000, 10),
        );
        s.upsert_subagent(&SubagentRecord {
            agent_id: "a1".into(),
            session_id: "h1".into(),
            agent_type: Some("go-reviewer".into()),
            description: Some("レビュー".into()),
            parent_tool_use_id: None,
            spawn_depth: Some(1),
        })
        .unwrap();
        seed_session(
            &s,
            "old",
            SessionKind::Headless,
            None,
            now() - Duration::minutes(5),
        );
        seed_session(
            &s,
            "job",
            SessionKind::BackgroundJob,
            Some("working"),
            now() - Duration::minutes(1),
        );
        let p = s.ensure_default().unwrap();
        s.upsert_job(&JobRecord {
            job_id: "j1".into(),
            profile_id: p.id,
            session_id: Some("job".into()),
            name: None,
            state: "working".into(),
            detail: Some("3/8件目".into()),
            in_flight_tasks: 2,
            tokens: None,
            created_at: None,
            updated_at: now() - Duration::minutes(1),
        })
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            s,
            Arc::new(FixedClock::at(now())),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        let rs = runs(&deps).unwrap();
        let ids: Vec<(&str, u8)> = rs
            .iter()
            .map(|r| (r.session_id.as_str(), r.depth))
            .collect();
        assert_eq!(ids, [("h1", 0), ("h1", 1), ("job", 0)]);
        let h = &rs[0];
        assert_eq!(
            (
                h.kind,
                h.state.as_str(),
                h.context.as_str(),
                h.elapsed.as_str()
            ),
            (RunKind::Headless, "ツール実行中", "10%", "5分")
        );
        assert_eq!(
            (h.tokens.as_str(), h.cost.as_str(), h.project.as_str()),
            ("102k", "$0.42", "h1")
        );
        let sub = &rs[1];
        assert_eq!(
            (sub.kind, sub.title.as_str(), sub.state.as_str()),
            (RunKind::Subagent, "go-reviewer「レビュー」", "応答生成中")
        );
        assert_eq!(rs[2].detail.as_deref(), Some("3/8件目 / 実行中タスク 2"));
    }

    fn deps_for(
        s: Arc<crate::models::repositories::db::SqliteStore>,
        home: &std::path::Path,
    ) -> GuiDeps {
        gui_deps(
            s,
            Arc::new(FixedClock::at(now())),
            home,
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        )
    }

    #[test]
    fn job_updated_near_its_active_limit_is_listed() {
        let (_d, s) = temp_store();
        let s = Arc::new(s);
        // ジョブの稼働判定は既定で10分。9分30秒前の更新はまだ稼働中。
        seed_session(
            &s,
            "job",
            SessionKind::BackgroundJob,
            Some("working"),
            now() - Duration::seconds(570),
        );
        let home = tempfile::tempdir().unwrap();
        let rs = runs(&deps_for(s, home.path())).unwrap();
        assert_eq!(rs.len(), 1);
    }

    #[test]
    fn subagent_tokens_count_only_its_own_turns() {
        let (_d, s) = temp_store();
        let s = Arc::new(s);
        s.seed_if_empty(&seed_models()).unwrap();
        for sid in ["h1", "h2"] {
            seed_session(
                &s,
                sid,
                SessionKind::Headless,
                None,
                now() - Duration::seconds(30),
            );
        }
        let t = now() - Duration::seconds(10);
        seed_turn(
            &s,
            "h1",
            "",
            "m1",
            t,
            Some("claude-opus-5-5"),
            "text",
            tokens(100_000, 0),
        );
        seed_turn(
            &s,
            "h1",
            "a1",
            "m2",
            t,
            Some("claude-opus-5-5"),
            "text",
            tokens(1_000, 0),
        );
        seed_turn(
            &s,
            "h1",
            "a2",
            "m3",
            t,
            Some("claude-opus-5-5"),
            "text",
            tokens(20_000, 0),
        );
        seed_turn(
            &s,
            "h2",
            "a1",
            "m4",
            t,
            Some("claude-opus-5-5"),
            "text",
            tokens(300_000, 0),
        );
        s.upsert_subagent(&SubagentRecord {
            agent_id: "a1".into(),
            session_id: "h1".into(),
            agent_type: Some("go-reviewer".into()),
            description: None,
            parent_tool_use_id: None,
            spawn_depth: Some(1),
        })
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let rs = runs(&deps_for(s, home.path())).unwrap();
        let sub = rs.iter().find(|r| r.kind == RunKind::Subagent).unwrap();
        assert_eq!(
            sub.tokens,
            crate::models::domain::display::fmt_tokens(1_000)
        );
    }

    #[test]
    fn project_name_is_last_path_part() {
        assert_eq!(project_name(Some("/work/app")), "app");
        assert_eq!(project_name(None), "");
    }
}
