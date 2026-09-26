//! ダッシュボード。カード、セッション一覧、詳細、推移を1画面にまとめる（spec 7.2節）。
//!
//! 区画ごとのViewModelは同じディレクトリの各ファイルが作り、ここで1つにまとめる。
pub mod cards;
pub mod live_log;
pub mod replay;
pub mod runs;
pub mod sessions;

use crate::controllers::gui::app::GuiDeps;
use crate::controllers::gui::dashboard::cards::ProfileCard;
use crate::controllers::gui::dashboard::replay::{ReplayAction, ReplayState};
use crate::controllers::gui::dashboard::runs::RunItem;
use crate::controllers::gui::dashboard::sessions::{DetailTab, SessionDetail, SessionItem};
use crate::models::domain::transcript::SessionKind;
use crate::models::ports::RepoError;
use std::collections::HashSet;

/// 一覧の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListMode {
    /// 稼働中の実行。
    #[default]
    Active,
    /// 完了したセッション。
    History,
}

/// ダッシュボードの選択状態。タブを移っても保ち、戻ったときに同じ表示にする。
#[derive(Debug, Clone, PartialEq)]
pub struct DashboardState {
    /// 一覧の種類。
    pub mode: ListMode,
    /// 推移グラフを開いているか。
    pub trend_open: bool,
    /// 選んだセッション。
    pub selected: Option<String>,
    /// 履歴のプロファイルの絞り込み。
    pub profile: Option<i64>,
    /// 履歴の種別の絞り込み。
    pub kind: Option<SessionKind>,
    /// 内訳を開いたターン（`<agent_id>:<message_id>`）。
    pub expanded: HashSet<String>,
    /// 詳細の表示。
    pub detail_tab: DetailTab,
    /// 再生の状態。
    pub replay: ReplayState,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            mode: ListMode::Active,
            trend_open: true,
            selected: None,
            profile: None,
            kind: None,
            expanded: HashSet::new(),
            detail_tab: DetailTab::Turns,
            replay: ReplayState::default(),
        }
    }
}

/// ダッシュボードの操作。
#[derive(Debug, Clone, PartialEq)]
pub enum DashAction {
    /// 一覧の種類を切り替える。
    SetListMode(ListMode),
    /// 推移グラフを開閉する。
    ToggleTrend,
    /// セッションを選び、詳細の表示を決める。稼働中の行はライブログ、履歴の行はターン表で開く。
    Select(String, DetailTab),
    /// 履歴をプロファイルで絞る。
    SetProfile(Option<i64>),
    /// 履歴を種別で絞る。
    SetKind(Option<SessionKind>),
    /// 検索語が変わった。検索語は`Forms.session_query`にある。
    Search,
    /// ターンの内訳を開閉する。
    ToggleTurn(String),
    /// 詳細の表示を切り替える。
    SetDetailTab(DetailTab),
    /// 再生の操作。
    Replay(ReplayAction),
}

/// ダッシュボードのViewModel。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DashboardVm {
    /// 一覧の種類。
    pub mode: ListMode,
    /// 推移グラフを開いているか。
    pub trend_open: bool,
    /// 使用率カード。
    pub cards: Vec<ProfileCard>,
    /// 稼働中の実行。一覧が履歴のときも件数を出すために作る。
    pub active: Vec<RunItem>,
    /// 完了したセッション。一覧が履歴のときだけ作る。
    pub history: Vec<SessionItem>,
    /// 履歴を1,000件で打ち切ったか。
    pub history_limited: bool,
    /// 絞り込みに使うプロファイル。
    pub profiles: Vec<(i64, String)>,
    /// 選んだセッションの詳細。
    pub detail: Option<SessionDetail>,
    /// 選んだセッションのID。一覧で選択中の行を示す。
    pub selected: Option<String>,
    /// 履歴のプロファイルの絞り込み。
    pub profile: Option<i64>,
    /// 履歴の種別の絞り込み。
    pub kind: Option<SessionKind>,
}

/// 操作を状態に反映する。
pub fn handle(st: &mut DashboardState, a: DashAction) {
    match a {
        DashAction::SetListMode(m) => st.mode = m,
        DashAction::ToggleTrend => st.trend_open = !st.trend_open,
        DashAction::Select(id, tab) => {
            st.selected = Some(id);
            st.expanded.clear();
            st.detail_tab = tab;
            st.replay = ReplayState {
                speed: st.replay.speed,
                ..ReplayState::default()
            };
        }
        DashAction::SetProfile(p) => st.profile = p,
        DashAction::SetKind(k) => st.kind = k,
        DashAction::Search => {}
        DashAction::ToggleTurn(key) => {
            if !st.expanded.remove(&key) {
                st.expanded.insert(key);
            }
        }
        DashAction::SetDetailTab(t) => st.detail_tab = t,
        DashAction::Replay(a) => replay::handle(&mut st.replay, a),
    }
}

/// ダッシュボードのViewModelを作る。履歴は一覧が履歴のときだけ読む。
pub fn build(deps: &GuiDeps, st: &DashboardState, query: &str) -> Result<DashboardVm, RepoError> {
    let (history, history_limited) = match st.mode {
        ListMode::History => sessions::history(deps, query, st.profile, st.kind)?,
        ListMode::Active => (vec![], false),
    };
    let mut detail = match &st.selected {
        Some(id) => sessions::detail(deps, id, &st.expanded, st.detail_tab)?,
        None => None,
    };
    if let Some(d) = detail.as_mut().filter(|d| d.tab == DetailTab::Replay) {
        d.replay = Some(replay::build(deps, &d.session_id, &st.replay)?);
    }
    Ok(DashboardVm {
        mode: st.mode,
        trend_open: st.trend_open,
        cards: cards::cards(deps)?,
        active: runs::runs(deps)?,
        history,
        history_limited,
        profiles: sessions::profiles(deps)?,
        detail,
        selected: st.selected.clone(),
        profile: st.profile,
        kind: st.kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_switches_mode_and_toggles_trend() {
        let mut st = DashboardState::default();
        assert_eq!((st.mode, st.trend_open), (ListMode::Active, true));
        handle(&mut st, DashAction::SetListMode(ListMode::History));
        handle(&mut st, DashAction::ToggleTrend);
        assert_eq!((st.mode, st.trend_open), (ListMode::History, false));
        handle(&mut st, DashAction::ToggleTrend);
        assert!(st.trend_open);
    }

    #[test]
    fn select_resets_expansion_and_sets_tab_and_filters() {
        let mut st = DashboardState::default();
        handle(&mut st, DashAction::Select("s1".into(), DetailTab::LiveLog));
        handle(&mut st, DashAction::ToggleTurn(":m1".into()));
        assert!(st.expanded.contains(":m1"));
        handle(&mut st, DashAction::ToggleTurn(":m1".into()));
        assert!(!st.expanded.contains(":m1"));
        handle(&mut st, DashAction::ToggleTurn(":m1".into()));
        handle(&mut st, DashAction::Select("s2".into(), DetailTab::Turns));
        assert_eq!(
            (
                st.selected.as_deref(),
                st.detail_tab,
                st.expanded.is_empty()
            ),
            (Some("s2"), DetailTab::Turns, true)
        );
        handle(&mut st, DashAction::SetDetailTab(DetailTab::Replay));
        handle(&mut st, DashAction::SetProfile(Some(3)));
        handle(&mut st, DashAction::SetKind(Some(SessionKind::Headless)));
        handle(&mut st, DashAction::Search);
        assert_eq!(
            (st.detail_tab, st.profile, st.kind),
            (DetailTab::Replay, Some(3), Some(SessionKind::Headless))
        );
    }

    #[test]
    fn replay_actions_and_reset_on_select() {
        use crate::controllers::gui::dashboard::replay::ReplayAction;
        let mut st = DashboardState::default();
        handle(&mut st, DashAction::Select("s1".into(), DetailTab::Replay));
        handle(&mut st, DashAction::Replay(ReplayAction::Seek(5)));
        handle(&mut st, DashAction::Replay(ReplayAction::Step(-2)));
        assert_eq!(st.replay.position, 3);
        handle(&mut st, DashAction::Replay(ReplayAction::Step(-10)));
        assert_eq!(st.replay.position, 0);
        handle(&mut st, DashAction::Replay(ReplayAction::SetSpeed(16)));
        handle(&mut st, DashAction::Replay(ReplayAction::TogglePlay));
        assert!(st.replay.playing && st.replay.speed == 16);
        handle(&mut st, DashAction::Replay(ReplayAction::Seek(7)));
        handle(&mut st, DashAction::Select("s2".into(), DetailTab::Replay));
        assert_eq!(
            (st.replay.position, st.replay.playing, st.replay.speed),
            (0, false, 16),
            "選び直すと先頭から、速度は保つ"
        );
    }
}
