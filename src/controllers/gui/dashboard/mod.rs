//! ダッシュボード。カード、セッション一覧、詳細、推移を1画面にまとめる（spec 7.2節）。
//!
//! 区画ごとのViewModelは同じディレクトリの各ファイルが作り、ここで1つにまとめる。
use crate::controllers::gui::app::GuiDeps;
use crate::models::ports::RepoError;

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
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            mode: ListMode::Active,
            trend_open: true,
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
}

/// ダッシュボードのViewModel。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DashboardVm {
    /// 一覧の種類。
    pub mode: ListMode,
    /// 推移グラフを開いているか。
    pub trend_open: bool,
}

/// 操作を状態に反映する。
pub fn handle(st: &mut DashboardState, a: DashAction) {
    match a {
        DashAction::SetListMode(m) => st.mode = m,
        DashAction::ToggleTrend => st.trend_open = !st.trend_open,
    }
}

/// ダッシュボードのViewModelを作る。
pub fn build(_deps: &GuiDeps, st: &DashboardState) -> Result<DashboardVm, RepoError> {
    Ok(DashboardVm {
        mode: st.mode,
        trend_open: st.trend_open,
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
}
