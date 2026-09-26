//! 分析タブ。プロジェクト別、モデル別、ブランチ別の集計、本体とサブエージェントの割合、キャッシュヒット率、ツール統計。
use crate::controllers::gui::app::GuiDeps;
use crate::models::domain::display::{fmt_percent, fmt_ratio, fmt_tokens, fmt_usd, ratio};
use crate::models::domain::pricing::{
    ModelInfo, TokenUsage, cache_hit_rate, sum_cost, total_tokens,
};
use crate::models::domain::read_models::{GroupBy, GroupUsage};
use crate::models::ports::RepoError;
use chrono::Duration;
use std::collections::BTreeMap;

/// 表に出す行数の上限。上位だけで傾向は読めるため。
const TOP: usize = 20;

/// 集計期間。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    /// 7日。
    #[default]
    Days7,
    /// 30日。
    Days30,
}

impl Period {
    /// 文言。
    pub fn label(self) -> &'static str {
        match self {
            Period::Days7 => "7日",
            Period::Days30 => "30日",
        }
    }

    /// 日数。
    pub fn days(self) -> i64 {
        match self {
            Period::Days7 => 7,
            Period::Days30 => 30,
        }
    }
}

/// 集計1行。
#[derive(Debug, Clone, PartialEq)]
pub struct GroupRow {
    /// 軸の値。
    pub key: String,
    /// Token数。
    pub tokens: String,
    /// コスト。
    pub cost: String,
    /// 全体に対する割合（棒の長さ）。
    pub share: f32,
}

/// ツール1行。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRow {
    /// ツール名。
    pub name: String,
    /// 呼び出し回数。
    pub calls: String,
    /// エラー回数。
    pub errors: String,
    /// エラー率。
    pub error_rate: String,
}

/// 分析タブのViewModel。
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyticsVm {
    /// 期間。
    pub period: Period,
    /// プロジェクト別。
    pub by_project: Vec<GroupRow>,
    /// モデル別。
    pub by_model: Vec<GroupRow>,
    /// ブランチ別。
    pub by_branch: Vec<GroupRow>,
    /// 本体とサブエージェントの割合。
    pub agent_share: String,
    /// 日ごとのキャッシュヒット率。
    pub daily_cache: Vec<(String, String)>,
    /// ツール。
    pub tools: Vec<ToolRow>,
}

/// 同じ軸の値の行をまとめ、Token数の多い順に上位を返す。コストはモデルごとの単価で出すため、モデル別の行を保ったまま足す。
fn group_rows(rows: Vec<GroupUsage>, models: &[ModelInfo]) -> Vec<GroupRow> {
    let mut by_key: BTreeMap<String, Vec<(Option<String>, TokenUsage)>> = BTreeMap::new();
    for r in rows {
        by_key.entry(r.key).or_default().push((r.model, r.usage));
    }
    let mut totals: Vec<(String, u64, Option<f64>)> = by_key
        .into_iter()
        .map(|(k, v)| {
            let tokens = v.iter().map(|(_, u)| total_tokens(u)).sum();
            let rows: Vec<(Option<&str>, TokenUsage)> =
                v.iter().map(|(m, u)| (m.as_deref(), *u)).collect();
            (k, tokens, sum_cost(models, &rows))
        })
        .collect();
    let all: u64 = totals.iter().map(|t| t.1).sum::<u64>().max(1);
    totals.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    totals
        .into_iter()
        .take(TOP)
        .map(|(key, tokens, cost)| GroupRow {
            key: if key.is_empty() {
                "（不明）".into()
            } else {
                key
            },
            tokens: fmt_tokens(tokens),
            cost: fmt_usd(cost),
            share: tokens as f32 / all as f32,
        })
        .collect()
}

/// 分析タブを作る。
pub fn build(deps: &GuiDeps, period: Period) -> Result<AnalyticsVm, RepoError> {
    let since = deps.clock.now() - Duration::days(period.days());
    let models = deps.models.all()?;
    let a = &deps.analytics;
    let agents = a.usage_by(GroupBy::Agent, since)?;
    let part = |k: &str| -> u64 {
        agents
            .iter()
            .filter(|g| g.key == k)
            .map(|g| total_tokens(&g.usage))
            .sum()
    };
    let (main, sub) = (part("main"), part("subagent"));
    let whole = (main + sub).max(1) as f64;
    Ok(AnalyticsVm {
        period,
        by_project: group_rows(a.usage_by(GroupBy::Project, since)?, &models),
        by_model: group_rows(a.usage_by(GroupBy::Model, since)?, &models),
        by_branch: group_rows(a.usage_by(GroupBy::Branch, since)?, &models),
        agent_share: format!(
            "本体 {} / サブエージェント {}",
            fmt_percent(main as f64 * 100.0 / whole),
            fmt_percent(sub as f64 * 100.0 / whole)
        ),
        daily_cache: a
            .daily_usage(since, deps.tz.local_minus_utc())?
            .into_iter()
            .map(|d| (d.day, fmt_ratio(cache_hit_rate(&d.usage))))
            .collect(),
        tools: a
            .tool_stats(since)?
            .into_iter()
            .take(TOP)
            .map(|t| ToolRow {
                error_rate: fmt_ratio(ratio(t.errors as u64, t.calls as u64)),
                name: t.tool_name,
                calls: t.calls.to_string(),
                errors: t.errors.to_string(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::pricing::seed_models;
    use crate::models::domain::records::ToolCallRecord;
    use crate::models::domain::transcript::SessionKind;
    use crate::models::ports::{IngestRepo, ModelRepo};
    use crate::test_support::{
        FakeCreds, FakeDaemon, FixedClock, gui_deps, seed_session, seed_turn, temp_store, tokens,
    };
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    #[test]
    fn groups_shares_cache_and_tools() {
        let (_d, s) = temp_store();
        s.seed_if_empty(&seed_models()).unwrap();
        seed_session(&s, "s1", SessionKind::Interactive, None, now());
        seed_session(&s, "s2", SessionKind::Headless, None, now());
        seed_turn(
            &s,
            "s1",
            "",
            "m1",
            now() - Duration::hours(1),
            Some("claude-opus-5-5"),
            "text",
            tokens(750_000, 0),
        );
        seed_turn(
            &s,
            "s1",
            "a1",
            "m2",
            now() - Duration::hours(1),
            Some("claude-haiku-4-5"),
            "text",
            tokens(250_000, 0),
        );
        seed_turn(
            &s,
            "s2",
            "",
            "m3",
            now() - Duration::days(10),
            Some("claude-opus-5-5"),
            "text",
            tokens(9, 0),
        );
        s.upsert_tool_call(&ToolCallRecord {
            tool_use_id: "t1".into(),
            session_id: "s1".into(),
            agent_id: String::new(),
            ts: now(),
            tool_name: "Bash".into(),
        })
        .unwrap();
        s.mark_tool_error("t1").unwrap();
        s.upsert_tool_call(&ToolCallRecord {
            tool_use_id: "t2".into(),
            session_id: "s1".into(),
            agent_id: String::new(),
            ts: now(),
            tool_name: "Bash".into(),
        })
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            Arc::new(s),
            Arc::new(FixedClock::at(now())),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        let vm = build(&deps, Period::Days7).unwrap();
        assert_eq!(vm.by_project.len(), 1);
        let g = &vm.by_project[0];
        assert_eq!(
            (g.key.as_str(), g.tokens.as_str(), g.cost.as_str(), g.share),
            ("/work/s1", "1.00M", "$3.25", 1.0)
        );
        assert_eq!(
            vm.by_model
                .iter()
                .map(|g| g.key.as_str())
                .collect::<Vec<_>>(),
            ["claude-opus-5-5", "claude-haiku-4-5"]
        );
        assert_eq!(vm.agent_share, "本体 75% / サブエージェント 25%");
        assert_eq!(
            vm.daily_cache,
            [("2026-09-26".to_string(), "0%".to_string())]
        );
        assert_eq!(
            (
                vm.tools[0].name.as_str(),
                vm.tools[0].calls.as_str(),
                vm.tools[0].error_rate.as_str()
            ),
            ("Bash", "2", "50%")
        );
        assert_eq!(build(&deps, Period::Days30).unwrap().by_project.len(), 2);
        assert_eq!((Period::Days7.label(), Period::Days30.days()), ("7日", 30));
    }

    #[test]
    fn empty_key_is_unknown() {
        let rows = group_rows(
            vec![GroupUsage {
                key: String::new(),
                model: None,
                usage: TokenUsage::default(),
            }],
            &[],
        );
        assert_eq!((rows[0].key.as_str(), rows[0].share), ("（不明）", 0.0));
    }
}
