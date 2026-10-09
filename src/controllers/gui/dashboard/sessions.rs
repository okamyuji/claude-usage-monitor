//! セッションの履歴と、選んだセッションの詳細（合計とターン表）。
use crate::controllers::gui::app::GuiDeps;
use crate::controllers::gui::dashboard::live_log::LiveLogVm;
use crate::controllers::gui::dashboard::replay::ReplayVm;
use crate::models::domain::activity::{RunKind, is_active};
use crate::models::domain::display::{
    context_ratio, fmt_clock, fmt_duration, fmt_ratio, fmt_tokens, fmt_usd, ratio,
};
use crate::models::domain::pricing::{
    ModelInfo, TokenUsage, cache_hit_rate, cost_for, find_model, sum_cost, total_tokens,
};
use crate::models::domain::read_models::{
    SessionFilter, SessionModelUsage, SessionRow, TimeRange, TurnRow,
};
use crate::models::domain::session_summary::{requests, stats};
use crate::models::domain::transcript::{NoteKind, SessionKind, one_line};
use crate::models::ports::RepoError;
use chrono::{DateTime, FixedOffset, Utc};
use std::collections::{HashMap, HashSet};

/// 1セッションで読むターンの上限（spec 11章）。
pub const TURN_LIMIT: usize = 1000;

/// 概要の集計に読むターンの上限。ターン表の上限では長いセッションの数値が欠けるため別にする。
pub const SUMMARY_TURN_LIMIT: usize = 100_000;

/// 詳細の表示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailTab {
    /// 概要。
    Summary,
    /// ターン表。
    #[default]
    Turns,
    /// ライブログ。
    LiveLog,
    /// 再生。
    Replay,
}

/// 履歴の1行（spec 7.2節の列）。
#[derive(Debug, Clone, PartialEq)]
pub struct SessionItem {
    /// セッションID。
    pub session_id: String,
    /// 入力の冒頭、または名前。
    pub title: String,
    /// 種別。
    pub kind: RunKind,
    /// 作業ディレクトリ。
    pub project: String,
    /// 開始。
    pub started: String,
    /// 経過（開始から最終更新まで）。
    pub elapsed: String,
    /// 最後のコンテキスト使用率。
    pub context: String,
    /// Token数。
    pub tokens: String,
    /// コスト。
    pub cost: String,
    /// ターン数。
    pub turns: String,
}

/// 合計の表示。
#[derive(Debug, Clone, PartialEq)]
pub struct TotalsView {
    /// 入力。
    pub input: String,
    /// 出力。
    pub output: String,
    /// キャッシュ読込。
    pub cache_read: String,
    /// キャッシュ作成。
    pub cache_write: String,
    /// コスト。
    pub cost: String,
    /// キャッシュヒット率。
    pub cache_hit: String,
    /// サブエージェントの消費割合。
    pub subagent_share: String,
}

/// ターン表の1行（spec 7.2節の5列と、展開時の内訳）。
#[derive(Debug, Clone, PartialEq)]
pub struct TurnItem {
    /// 内訳の開閉に使うキー（`<agent_id>:<message_id>`）。
    pub key: String,
    /// 時刻。
    pub time: String,
    /// 発生元（本体、またはサブエージェントの種類）。
    pub source: String,
    /// 字下げ。
    pub depth: u8,
    /// 種類。
    pub kind: String,
    /// 内容。
    pub summary: String,
    /// コンテキスト使用率。
    pub context: String,
    /// 所要時間（次のターンまで）。
    pub duration: String,
    /// 開いているときだけの内訳。
    pub breakdown: Option<String>,
}

/// 詳細。
#[derive(Debug, Clone, PartialEq)]
pub struct SessionDetail {
    /// セッションID。
    pub session_id: String,
    /// 見出し。
    pub title: String,
    /// 作業ディレクトリ、モデル、ブランチ、開始時刻。
    pub meta: String,
    /// 合計。
    pub totals: TotalsView,
    /// ターン。「ターン」を表示しているときだけ作る。
    pub turns: Vec<TurnItem>,
    /// 表示の種類。
    pub tab: DetailTab,
    /// ライブログ。「ライブログ」を表示しているときだけcontrollerが入れる。
    pub live: Option<LiveLogVm>,
    /// 再生。「再生」を表示しているときだけ入れる。
    pub replay: Option<ReplayVm>,
    /// 概要。「概要」を表示しているときだけ作る。
    pub summary: Option<SummaryVm>,
}

/// 概要のモデル表の1行。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelLine {
    /// モデル。
    pub model: String,
    /// 要求数。
    pub requests: String,
    /// 入力。
    pub input: String,
    /// 出力。
    pub output: String,
    /// キャッシュ読込。
    pub cache_read: String,
    /// キャッシュ作成。
    pub cache_write: String,
    /// コスト。
    pub cost: String,
}

/// 概要。
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryVm {
    /// Claude Codeが書いた要約（時刻、本文）。時刻順。
    pub recaps: Vec<(String, String)>,
    /// 要約がないときに代わりに出す、Claude Codeが付けた題名。
    pub title: Option<String>,
    /// 数値（名前、値）。
    pub stats: Vec<(&'static str, String)>,
    /// モデル表。
    pub models: Vec<ModelLine>,
    /// ツール表（ツール名、呼び出し数、エラー数）。
    pub tools: Vec<(String, String, String)>,
    /// 本体セッションの依頼（時刻、1行の要約）。時刻順。
    pub prompts: Vec<(String, String)>,
    /// ターンが上限に達し、直近の分だけで集計したか。
    pub truncated: bool,
}

/// セッションの見出し。名前、最初の入力、IDの先頭の順に使う。
pub fn session_title(s: &SessionRow) -> String {
    s.name
        .clone()
        .or_else(|| s.first_prompt.as_deref().map(|p| one_line(p, 40)))
        .unwrap_or_else(|| s.session_id.chars().take(8).collect())
}

fn turn_kind_label(kind: &str) -> String {
    match kind {
        "prompt" => "入力".into(),
        "text" => "応答".into(),
        "tool_use" => "ツール".into(),
        "thinking" => "思考".into(),
        other => other.into(),
    }
}

fn breakdown(t: &TurnRow, models: &[ModelInfo]) -> String {
    let cost = fmt_usd(
        t.model
            .as_deref()
            .and_then(|m| cost_for(models, m, &t.usage)),
    );
    format!(
        "入力 {} / 出力 {} / キャッシュ読込 {} / キャッシュ作成 {} / コスト {cost}",
        fmt_tokens(t.usage.input),
        fmt_tokens(t.usage.output),
        fmt_tokens(t.usage.cache_read),
        fmt_tokens(t.usage.cache_write_5m + t.usage.cache_write_1h),
    )
}

/// ターン表の行を作る。`names`はエージェントIDから種類への対応。
pub fn turn_items(
    turns: &[TurnRow],
    models: &[ModelInfo],
    names: &HashMap<String, String>,
    expanded: &HashSet<String>,
    now: DateTime<Utc>,
    tz: FixedOffset,
) -> Vec<TurnItem> {
    turns
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let key = format!("{}:{}", t.agent_id, t.message_id);
            let window = t
                .model
                .as_deref()
                .and_then(|m| find_model(models, m))
                .and_then(|m| m.context_window);
            TurnItem {
                time: fmt_clock(t.ts, now, tz),
                source: if t.agent_id.is_empty() {
                    "本体".into()
                } else {
                    names
                        .get(&t.agent_id)
                        .cloned()
                        .unwrap_or_else(|| "サブエージェント".into())
                },
                depth: u8::from(!t.agent_id.is_empty()),
                kind: turn_kind_label(&t.kind),
                summary: t.summary.clone(),
                context: match t.model {
                    Some(_) => fmt_ratio(context_ratio(t.usage.context_tokens(), window)),
                    None => "—".into(),
                },
                duration: turns
                    .get(i + 1)
                    .map(|n| fmt_duration(n.ts - t.ts))
                    .unwrap_or_else(|| "—".into()),
                breakdown: expanded.contains(&key).then(|| breakdown(t, models)),
                key,
            }
        })
        .collect()
}

fn usage_rows<'a>(
    usage: &'a [SessionModelUsage],
    session_id: &str,
) -> Vec<(Option<&'a str>, TokenUsage)> {
    usage
        .iter()
        .filter(|u| u.session_id == session_id)
        .map(|u| (u.model.as_deref(), u.usage))
        .collect()
}

fn history_item(
    s: &SessionRow,
    usage: &[SessionModelUsage],
    models: &[ModelInfo],
    now: DateTime<Utc>,
    tz: FixedOffset,
) -> SessionItem {
    let rows = usage_rows(usage, &s.session_id);
    let total = rows
        .iter()
        .fold(TokenUsage::default(), |a, (_, u)| a.plus(u));
    let window = s
        .last_model
        .as_deref()
        .and_then(|m| find_model(models, m))
        .and_then(|m| m.context_window);
    SessionItem {
        session_id: s.session_id.clone(),
        title: session_title(s),
        kind: RunKind::from_session(s.kind),
        project: s.cwd.clone().unwrap_or_default(),
        started: fmt_clock(s.started_at, now, tz),
        elapsed: fmt_duration(s.last_activity_at - s.started_at),
        context: fmt_ratio(context_ratio(s.last_context_tokens, window)),
        tokens: fmt_tokens(total_tokens(&total)),
        cost: fmt_usd(sum_cost(models, &rows)),
        turns: s.turn_count.to_string(),
    }
}

/// 完了したセッションの一覧。稼働中のものは「稼働中」の一覧に出すので外す。
pub fn history(
    deps: &GuiDeps,
    query: &str,
    profile: Option<i64>,
    kind: Option<SessionKind>,
) -> Result<(Vec<SessionItem>, bool), RepoError> {
    let now = deps.clock.now();
    let rules = deps.settings.load()?.rules();
    let models = deps.models.all()?;
    let filter = SessionFilter {
        query: query.trim().to_string(),
        profile_id: profile,
        kind,
        ..SessionFilter::default()
    };
    let rows = deps.sessions.list_sessions(&filter)?;
    let limited = rows.len() >= filter.limit;
    let done: Vec<&SessionRow> = rows
        .iter()
        .filter(|s| !is_active(s.kind, s.status.as_deref(), s.last_activity_at, now, &rules))
        .collect();
    let ids: Vec<String> = done.iter().map(|s| s.session_id.clone()).collect();
    let usage = deps.sessions.model_usage(&ids)?;
    Ok((
        done.iter()
            .map(|s| history_item(s, &usage, &models, now, deps.tz))
            .collect(),
        limited,
    ))
}

/// 絞り込みに使うプロファイル（IDと名前）。
pub fn profiles(deps: &GuiDeps) -> Result<Vec<(i64, String)>, RepoError> {
    Ok(deps
        .profiles
        .list()?
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect())
}

/// 選んだセッションの詳細。見つからなければ`None`。
pub fn detail(
    deps: &GuiDeps,
    id: &str,
    expanded: &HashSet<String>,
    tab: DetailTab,
    range: Option<TimeRange>,
) -> Result<Option<SessionDetail>, RepoError> {
    let Some(s) = deps.sessions.session(id)? else {
        return Ok(None);
    };
    let now = deps.clock.now();
    let models = deps.models.all()?;
    let ids = [id.to_string()];
    let usage = deps.sessions.model_usage(&ids)?;
    let names: HashMap<String, String> = deps
        .sessions
        .subagents(&ids)?
        .into_iter()
        .map(|a| {
            (
                a.agent_id,
                a.agent_type.unwrap_or_else(|| "サブエージェント".into()),
            )
        })
        .collect();
    let rows = usage_rows(&usage, id);
    let total = rows
        .iter()
        .fold(TokenUsage::default(), |a, (_, u)| a.plus(u));
    let sub: u64 = usage
        .iter()
        .filter(|u| !u.agent_id.is_empty())
        .map(|u| total_tokens(&u.usage))
        .sum();
    let share = ratio(sub, total_tokens(&total));
    let turns = match tab {
        DetailTab::Turns => turn_items(
            &deps.sessions.turns(id, TURN_LIMIT, range)?,
            &models,
            &names,
            expanded,
            now,
            deps.tz,
        ),
        DetailTab::Summary | DetailTab::LiveLog | DetailTab::Replay => vec![],
    };
    let summary = match tab {
        DetailTab::Summary => Some(summary_vm(deps, &s, &usage, &models, names.len(), now)?),
        _ => None,
    };
    Ok(Some(SessionDetail {
        session_id: s.session_id.clone(),
        title: session_title(&s),
        meta: format!(
            "{} · {} · {} · 開始 {}",
            s.cwd.as_deref().unwrap_or("—"),
            s.last_model.as_deref().unwrap_or("—"),
            s.git_branch.as_deref().unwrap_or("—"),
            fmt_clock(s.started_at, now, deps.tz)
        ),
        totals: TotalsView {
            input: fmt_tokens(total.input),
            output: fmt_tokens(total.output),
            cache_read: fmt_tokens(total.cache_read),
            cache_write: fmt_tokens(total.cache_write_5m + total.cache_write_1h),
            cost: fmt_usd(sum_cost(&models, &rows)),
            cache_hit: fmt_ratio(cache_hit_rate(&total)),
            subagent_share: fmt_ratio(share),
        },
        turns,
        tab,
        live: None,
        replay: None,
        summary,
    }))
}

fn summary_vm(
    deps: &GuiDeps,
    s: &SessionRow,
    usage: &[SessionModelUsage],
    models: &[ModelInfo],
    subagents: usize,
    now: DateTime<Utc>,
) -> Result<SummaryVm, RepoError> {
    let id = s.session_id.as_str();
    let turns = deps.sessions.turns(id, SUMMARY_TURN_LIMIT, None)?;
    let notes = deps.sessions.notes(id)?;
    let tools = deps.sessions.session_tools(id)?;
    let st = stats(&turns);
    let prompts: Vec<(String, String)> = requests(&turns)
        .into_iter()
        .map(|(t, text)| (fmt_clock(t, now, deps.tz), text))
        .collect();
    let clock = |t| fmt_clock(t, now, deps.tz);
    let (calls, errors) = tools
        .iter()
        .fold((0, 0), |a, t| (a.0 + t.calls, a.1 + t.errors));
    let compacts = notes.iter().filter(|n| n.kind == NoteKind::Compact).count();
    let recaps: Vec<(String, String)> = notes
        .iter()
        .filter(|n| n.kind == NoteKind::Recap)
        .map(|n| (clock(n.ts), n.text.clone()))
        .collect();
    Ok(SummaryVm {
        title: s.ai_title.clone().filter(|_| recaps.is_empty()),
        recaps,
        stats: vec![
            (
                "期間",
                format!(
                    "{}〜{}（{}）",
                    clock(s.started_at),
                    clock(s.last_activity_at),
                    fmt_duration(s.last_activity_at - s.started_at)
                ),
            ),
            ("作業時間", fmt_duration(st.active)),
            ("依頼", format!("{}件", prompts.len())),
            ("API要求", format!("{}件", st.requests)),
            (
                "ツール",
                if errors > 0 {
                    format!("{calls}回（エラー{errors}回）")
                } else {
                    format!("{calls}回")
                },
            ),
            ("圧縮", format!("{compacts}回")),
            ("サブエージェント", format!("{subagents}件")),
        ],
        models: model_lines(&st.by_model, usage, models),
        tools: tools
            .into_iter()
            .map(|t| (t.tool_name, t.calls.to_string(), t.errors.to_string()))
            .collect(),
        prompts,
        truncated: turns.len() >= SUMMARY_TURN_LIMIT,
    })
}

fn model_lines(
    by_model: &[(String, usize)],
    usage: &[SessionModelUsage],
    models: &[ModelInfo],
) -> Vec<ModelLine> {
    by_model
        .iter()
        .map(|(m, n)| {
            let u = usage
                .iter()
                .filter(|x| x.model.as_deref() == Some(m.as_str()))
                .fold(TokenUsage::default(), |a, x| a.plus(&x.usage));
            ModelLine {
                model: m.clone(),
                requests: n.to_string(),
                input: fmt_tokens(u.input),
                output: fmt_tokens(u.output),
                cache_read: fmt_tokens(u.cache_read),
                cache_write: fmt_tokens(u.cache_write_5m + u.cache_write_1h),
                cost: fmt_usd(cost_for(models, m, &u)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::pricing::seed_models;
    use crate::models::domain::records::SubagentRecord;
    use crate::models::domain::transcript::NoteKind;
    use crate::models::ports::{IngestRepo, ModelRepo, ProfileRepo};
    use crate::test_support::{
        FakeCreds, FakeDaemon, FixedClock, gui_deps, seed_session, seed_turn, temp_store, tokens,
    };
    use chrono::{Duration, TimeZone};
    use std::sync::Arc;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, GuiDeps) {
        let (d, s) = temp_store();
        let s = Arc::new(s);
        s.seed_if_empty(&seed_models()).unwrap();
        s.ensure_default().unwrap();
        seed_session(
            &s,
            "s1",
            SessionKind::Interactive,
            Some("busy"),
            now() - Duration::minutes(1),
        );
        seed_session(
            &s,
            "s2",
            SessionKind::Headless,
            None,
            now() - Duration::hours(3),
        );
        seed_session(
            &s,
            "s3",
            SessionKind::Interactive,
            None,
            now() - Duration::hours(1),
        );
        seed_turn(
            &s,
            "s1",
            "",
            "m1",
            now() - Duration::minutes(3),
            None,
            "prompt",
            tokens(0, 0),
        );
        seed_turn(
            &s,
            "s1",
            "",
            "m2",
            now() - Duration::minutes(2),
            Some("claude-opus-5-5"),
            "tool_use",
            tokens(200_000, 1_000),
        );
        seed_turn(
            &s,
            "s1",
            "a1",
            "m3",
            now() - Duration::minutes(1),
            Some("<synthetic>"),
            "text",
            tokens(10, 1),
        );
        seed_turn(
            &s,
            "s3",
            "",
            "m4",
            now() - Duration::hours(1),
            Some("claude-opus-5-5"),
            "text",
            tokens(100_000, 0),
        );
        s.upsert_subagent(&SubagentRecord {
            agent_id: "a1".into(),
            session_id: "s1".into(),
            agent_type: Some("go-reviewer".into()),
            description: None,
            parent_tool_use_id: None,
            spawn_depth: Some(1),
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
        (d, home, deps)
    }

    fn ids(items: &[SessionItem]) -> Vec<&str> {
        items.iter().map(|i| i.session_id.as_str()).collect()
    }

    #[test]
    fn history_excludes_active_and_filters() {
        let (_d, _h, deps) = setup();
        let (items, limited) = history(&deps, "", None, None).unwrap();
        assert_eq!(ids(&items), ["s3", "s2"]);
        assert!(!limited);
        assert_eq!(
            ids(&history(&deps, "", None, Some(SessionKind::Headless))
                .unwrap()
                .0),
            ["s2"]
        );
        assert_eq!(
            ids(&history(&deps, "prompt-s3", None, None).unwrap().0),
            ["s3"]
        );
        assert!(history(&deps, "", Some(9999), None).unwrap().0.is_empty());
        let s3 = &items[0];
        assert_eq!(
            (
                s3.title.as_str(),
                s3.started.as_str(),
                s3.elapsed.as_str(),
                s3.context.as_str(),
                s3.tokens.as_str(),
                s3.cost.as_str(),
                s3.turns.as_str()
            ),
            ("name-s3", "10:55", "5分", "10%", "100k", "$0.40", "1")
        );
        assert_eq!(
            profiles(&deps)
                .unwrap()
                .into_iter()
                .map(|p| p.1)
                .collect::<Vec<_>>(),
            ["default"]
        );
    }

    #[test]
    fn detail_has_totals_turns_and_unpriced_marker() {
        let (_d, _h, deps) = setup();
        let d = detail(&deps, "s1", &HashSet::new(), DetailTab::Turns, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                d.totals.input.as_str(),
                d.totals.cost.as_str(),
                d.totals.subagent_share.as_str()
            ),
            ("200k", "単価未登録", "0%")
        );
        let kinds: Vec<(&str, &str, u8)> = d
            .turns
            .iter()
            .map(|t| (t.kind.as_str(), t.source.as_str(), t.depth))
            .collect();
        assert_eq!(
            kinds,
            [
                ("入力", "本体", 0),
                ("ツール", "本体", 0),
                ("応答", "go-reviewer", 1)
            ]
        );
        assert_eq!(
            (d.turns[1].context.as_str(), d.turns[0].context.as_str()),
            ("20%", "—")
        );
        assert_eq!(
            (d.turns[0].duration.as_str(), d.turns[2].duration.as_str()),
            ("1分", "—")
        );
        assert!(d.turns.iter().all(|t| t.breakdown.is_none()));
        assert_eq!(d.tab, DetailTab::Turns);
    }

    #[test]
    fn unpriced_model_turns_show_unpriced() {
        let (_d, _h, deps) = setup();
        let expanded: HashSet<String> = ["a1:m3".to_string(), ":m2".to_string()]
            .into_iter()
            .collect();
        let d = detail(&deps, "s1", &expanded, DetailTab::Turns, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            d.turns[1].breakdown.as_deref(),
            Some("入力 200k / 出力 1.00k / キャッシュ読込 0 / キャッシュ作成 0 / コスト $0.82")
        );
        assert!(
            d.turns[2]
                .breakdown
                .as_deref()
                .unwrap()
                .ends_with("コスト 単価未登録")
        );
    }

    #[test]
    fn cache_write_total_adds_both_lifetimes() {
        let (_d, s) = temp_store();
        let s = Arc::new(s);
        s.seed_if_empty(&seed_models()).unwrap();
        s.ensure_default().unwrap();
        seed_session(&s, "c1", SessionKind::Headless, None, now());
        let u = TokenUsage {
            cache_write_5m: 2_000,
            cache_write_1h: 1_000,
            ..tokens(0, 0)
        };
        seed_turn(
            &s,
            "c1",
            "",
            "m1",
            now(),
            Some("claude-opus-5-5"),
            "text",
            u,
        );
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            s,
            Arc::new(FixedClock::at(now())),
            home.path(),
            Arc::new(FakeCreds(std::collections::HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        let d = detail(&deps, "c1", &HashSet::new(), DetailTab::Turns, None)
            .unwrap()
            .unwrap();
        assert_eq!(d.totals.cache_write, fmt_tokens(3_000));
    }

    #[test]
    fn missing_session_has_no_detail() {
        let (_d, _h, deps) = setup();
        assert!(
            detail(&deps, "gone", &HashSet::new(), DetailTab::Turns, None)
                .unwrap()
                .is_none()
        );
    }
    fn summary_deps() -> (tempfile::TempDir, tempfile::TempDir, GuiDeps) {
        let (d, s) = temp_store();
        let s = Arc::new(s);
        s.seed_if_empty(&seed_models()).unwrap();
        let at = |m: i64| now() + Duration::minutes(m);
        seed_session(&s, "w", SessionKind::Interactive, None, at(30));
        seed_session(&s, "empty", SessionKind::Interactive, None, at(0));
        for (id, agent, m, model, kind) in [
            ("p1", "", 0, None, "prompt"),
            ("r1", "", 1, Some("claude-opus-5-5"), "text"),
            ("r2", "", 2, Some("claude-opus-5-5"), "tool_use"),
            ("r3", "", 3, Some("claude-opus-5-5"), "text"),
            ("sp", "a1", 5, None, "prompt"),
            ("p2", "", 30, None, "prompt"),
        ] {
            // モデルのない依頼のTokenはモデル表に入らないことを確かめるため、応答と違う値にする。
            let usage = match model {
                Some(_) => TokenUsage {
                    cache_write_5m: 100,
                    cache_write_1h: 20,
                    ..tokens(1_000, 10)
                },
                None => tokens(7, 0),
            };
            seed_turn(&s, "w", agent, id, at(m), model, kind, usage);
        }
        for (id, name) in [("t1", "Bash"), ("t2", "Bash")] {
            s.upsert_tool_call(&crate::models::domain::records::ToolCallRecord {
                tool_use_id: id.into(),
                session_id: "w".into(),
                agent_id: String::new(),
                ts: at(2),
                tool_name: name.into(),
            })
            .unwrap();
        }
        s.mark_tool_error("t2").unwrap();
        // 題名は要約がないときだけ出す。要約のある"w"にも入れ、出ないことを確かめる。
        for id in ["w", "empty"] {
            s.set_ai_title(id, &format!("{id}の題名")).unwrap();
        }
        for (id, text) in [
            ("c1", "<command-name>/effort</command-name>"),
            ("c2", "<command-name>/effort</command-name>"),
        ] {
            s.upsert_turn(&crate::models::domain::records::TurnRecord {
                session_id: "w".into(),
                agent_id: String::new(),
                message_id: id.into(),
                ts: at(30),
                model: None,
                kind: "prompt".into(),
                summary: text.into(),
                usage: TokenUsage::default(),
            })
            .unwrap();
        }
        s.upsert_turn(&crate::models::domain::records::TurnRecord {
            session_id: "w".into(),
            agent_id: String::new(),
            message_id: "tn".into(),
            ts: at(30),
            model: None,
            kind: "prompt".into(),
            summary: "<task-notification> done".into(),
            usage: TokenUsage::default(),
        })
        .unwrap();
        for (uuid, m, kind, text) in [
            ("n1", 4, NoteKind::Recap, "終わりました"),
            ("n2", 6, NoteKind::Compact, ""),
            ("n4", 7, NoteKind::Recap, "続きも終わりました"),
        ] {
            s.upsert_note(&crate::models::domain::records::NoteRecord {
                uuid: uuid.into(),
                session_id: "w".into(),
                ts: at(m),
                kind,
                text: text.into(),
            })
            .unwrap();
        }
        s.upsert_subagent(&SubagentRecord {
            agent_id: "a1".into(),
            session_id: "w".into(),
            agent_type: Some("Explore".into()),
            description: None,
            parent_tool_use_id: None,
            spawn_depth: Some(1),
        })
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            s,
            Arc::new(FixedClock::at(at(60))),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        (d, home, deps)
    }

    fn stat<'a>(s: &'a SummaryVm, name: &str) -> &'a str {
        s.stats
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
            .unwrap()
    }

    #[test]
    fn summary_collects_recaps_stats_models_tools_and_prompts() {
        let (_d, _h, deps) = summary_deps();
        let d = detail(&deps, "w", &HashSet::new(), DetailTab::Summary, None)
            .unwrap()
            .unwrap();
        let s = d.summary.expect("概要");
        assert_eq!(s.title, None);
        assert_eq!(
            s.recaps.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(),
            ["終わりました", "続きも終わりました"]
        );
        assert_eq!(
            s.stats.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            [
                "期間",
                "作業時間",
                "依頼",
                "API要求",
                "ツール",
                "圧縮",
                "サブエージェント"
            ]
        );
        assert_eq!(
            [
                stat(&s, "作業時間"),
                stat(&s, "依頼"),
                stat(&s, "API要求"),
                stat(&s, "ツール"),
                stat(&s, "圧縮"),
                stat(&s, "サブエージェント")
            ],
            ["5分", "3件", "3件", "2回（エラー1回）", "1回", "1件"]
        );
        assert_eq!(s.models.len(), 1);
        let m = &s.models[0];
        assert_eq!(
            (
                m.model.as_str(),
                m.requests.as_str(),
                m.input.as_str(),
                m.cache_write.as_str()
            ),
            ("claude-opus-5-5", "3", "3.00k", "360")
        );
        assert_ne!(m.cost, "単価未登録");
        assert_eq!(
            s.tools,
            [("Bash".to_string(), "2".to_string(), "1".to_string())]
        );
        assert_eq!(
            s.prompts.iter().map(|p| p.1.as_str()).collect::<Vec<_>>(),
            ["summary-p1", "summary-p2", "/effort"]
        );
        assert!(!s.truncated);
    }

    #[test]
    fn summary_of_empty_session_has_no_rows() {
        let (_d, _h, deps) = summary_deps();
        let d = detail(&deps, "empty", &HashSet::new(), DetailTab::Summary, None)
            .unwrap()
            .unwrap();
        let s = d.summary.expect("概要");
        assert_eq!(s.title.as_deref(), Some("emptyの題名"));
        assert!(s.recaps.is_empty() && s.models.is_empty() && s.tools.is_empty());
        assert!(s.prompts.is_empty());
        assert_eq!(
            [stat(&s, "依頼"), stat(&s, "ツール"), stat(&s, "作業時間")],
            ["0件", "0回", "0秒"]
        );
    }

    #[test]
    fn summary_is_built_only_for_summary_tab() {
        let (_d, _h, deps) = summary_deps();
        for tab in [DetailTab::Turns, DetailTab::LiveLog, DetailTab::Replay] {
            let d = detail(&deps, "w", &HashSet::new(), tab, None)
                .unwrap()
                .unwrap();
            assert!(d.summary.is_none(), "{tab:?}");
        }
        let d = detail(&deps, "w", &HashSet::new(), DetailTab::Summary, None)
            .unwrap()
            .unwrap();
        assert!(d.turns.is_empty());
    }
}
