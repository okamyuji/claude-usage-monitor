//! GUI全体の状態、タブの切り替え、読み直しの周期。
use crate::controllers::gui::dashboard::live_log::{LiveAction, LiveLogState};
use crate::controllers::gui::dashboard::sessions::DetailTab;
use crate::controllers::gui::dashboard::{self, DashAction, DashboardState, DashboardVm, ListMode};
use crate::controllers::gui::header;
use crate::models::domain::display::fmt_clock;
use crate::models::domain::live_log::LiveFilter;
use crate::models::domain::settings::Theme;
use crate::models::ports::{
    AnalyticsRepo, CatalogRefresh, Clock, CredentialStore, DaemonControl, DashboardRepo,
    DiagnosticsRepo, FetchLogRepo, ModelRepo, ProfileRepo, RepoError, SessionQueryRepo,
    SettingsRepo, UsageRepo,
};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use std::path::PathBuf;
use std::sync::Arc;

/// 表示中のタブを読み直す間隔（秒）。
pub const REFRESH_SECS: i64 = 5;
/// 中段の左（一覧）の幅の既定の比率（spec 7.3節）。
pub const DEFAULT_SPLIT: f32 = 0.45;

/// 上部のタブ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// ダッシュボード。
    Dashboard,
    /// 分析。
    Analytics,
    /// プロファイル。
    Profiles,
    /// 設定。
    Settings,
    /// 診断。
    Diagnostics,
}

impl Tab {
    /// 上部バーの並び。
    pub const ALL: [Tab; 5] = [
        Tab::Dashboard,
        Tab::Analytics,
        Tab::Profiles,
        Tab::Settings,
        Tab::Diagnostics,
    ];

    /// タブの文言。
    pub fn label(self) -> &'static str {
        match self {
            Tab::Dashboard => "ダッシュボード",
            Tab::Analytics => "分析",
            Tab::Profiles => "プロファイル",
            Tab::Settings => "設定",
            Tab::Diagnostics => "診断",
        }
    }
}

/// GUIの依存。`main.rs`だけが具象型で組み立てる。
pub struct GuiDeps {
    /// 時計。
    pub clock: Arc<dyn Clock>,
    /// 表示用の時差。
    pub tz: FixedOffset,
    /// ホームディレクトリ。既定プロファイルの`~/.claude`の解決に使う。
    pub home: PathBuf,
    /// プロファイル。
    pub profiles: Arc<dyn ProfileRepo>,
    /// 使用率の時系列。
    pub usage: Arc<dyn UsageRepo>,
    /// ダッシュボードの読み取り。
    pub dashboard: Arc<dyn DashboardRepo>,
    /// セッションの読み取り。
    pub sessions: Arc<dyn SessionQueryRepo>,
    /// 集計の読み取り。
    pub analytics: Arc<dyn AnalyticsRepo>,
    /// 診断の読み取り。
    pub diagnostics: Arc<dyn DiagnosticsRepo>,
    /// 取得ログ。
    pub logs: Arc<dyn FetchLogRepo>,
    /// モデル情報。
    pub models: Arc<dyn ModelRepo>,
    /// 設定。
    pub settings: Arc<dyn SettingsRepo>,
    /// 認証情報。プロファイルタブでトークン期限を出す。
    pub creds: Arc<dyn CredentialStore>,
    /// デーモン制御。
    pub daemon: Arc<dyn DaemonControl>,
    /// モデル情報の手動更新。
    pub catalog: Arc<dyn CatalogRefresh>,
}

/// 利用者の操作。
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// タブを切り替える。
    SelectTab(Tab),
    /// デーモンを起動する。
    StartDaemon,
    /// テーマを切り替えて保存する。
    SetTheme(Theme),
    /// ダッシュボードの操作。
    Dash(DashAction),
    /// ライブログの操作。
    Live(LiveAction),
}

/// 上部の今日と今週の集計（「1.26M $4.12」の形）。空なら表示しない。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HeaderVm {
    /// 今日。
    pub today: String,
    /// 直近7日。
    pub week: String,
}

/// 表示中のタブのViewModel。
#[derive(Debug, Clone, PartialEq)]
pub enum TabVm {
    /// ダッシュボード。区画が多く大きいため`Box`に入れる。
    Dashboard(Box<DashboardVm>),
    /// まだ作っていないタブ。
    Pending(Tab),
}

/// 画面全体のViewModel。
#[derive(Debug, Clone, PartialEq)]
pub struct AppVm {
    /// 表示中のタブ。
    pub tab: Tab,
    /// 上部の集計。
    pub header: HeaderVm,
    /// タブの中身。
    pub body: TabVm,
    /// デーモンが稼働中か。
    pub daemon_running: bool,
    /// 最終読み込み時刻の表示。
    pub updated: String,
    /// 読み込みの失敗。画面下部に出す。
    pub error: Option<String>,
    /// テーマ。
    pub theme: Theme,
}

/// 入力欄と描画側で動かす値の状態。eguiの入力欄は`&mut`で値を書き換えるため、ViewModelと分けて可変で渡す。
#[derive(Debug, Clone, PartialEq)]
pub struct Forms {
    /// 中段の左の幅の比率。境界のつまみで変わる。
    pub split_ratio: f32,
    /// 履歴の検索語。
    pub session_query: String,
    /// ライブログの絞り込み。
    pub live_filter: LiveFilter,
}

impl Default for Forms {
    fn default() -> Self {
        Self {
            split_ratio: DEFAULT_SPLIT,
            session_query: String::new(),
            live_filter: LiveFilter::default(),
        }
    }
}

/// GUIのcontroller。
pub struct GuiController {
    pub(crate) deps: GuiDeps,
    vm: AppVm,
    forms: Forms,
    pub(crate) dash: DashboardState,
    pub(crate) live: Option<LiveLogState>,
    pub(crate) jump_to_bottom: bool,
    last_refresh: Option<DateTime<Utc>>,
}

impl GuiController {
    /// 依存を受け取り、ダッシュボードを読み込んだ状態で作る。
    pub fn new(deps: GuiDeps) -> Self {
        let theme = deps
            .settings
            .load()
            .map(|s| s.theme)
            .unwrap_or(Theme::System);
        let mut c = Self {
            deps,
            vm: AppVm {
                tab: Tab::Dashboard,
                header: HeaderVm::default(),
                body: TabVm::Dashboard(Box::default()),
                daemon_running: false,
                updated: String::new(),
                error: None,
                theme,
            },
            forms: Forms::default(),
            dash: DashboardState::default(),
            live: None,
            jump_to_bottom: false,
            last_refresh: None,
        };
        c.refresh();
        c
    }

    /// 現在のViewModel。
    pub fn vm(&self) -> &AppVm {
        &self.vm
    }

    /// 描画に渡す組。入力欄だけを可変で渡す。
    pub fn view_parts(&mut self) -> (&AppVm, &mut Forms) {
        (&self.vm, &mut self.forms)
    }

    /// 一覧の種類。E2Eから選択状態を確かめるために公開する。
    pub fn dash_mode(&self) -> ListMode {
        self.dash.mode
    }

    /// 操作を1つ処理し、画面を読み直す。
    pub fn handle(&mut self, a: Action) {
        match a {
            Action::SelectTab(t) => self.vm.tab = t,
            Action::StartDaemon => {
                if let Err(e) = self.deps.daemon.start() {
                    self.vm.error = Some(e);
                    return;
                }
            }
            Action::SetTheme(t) => {
                if let Err(e) = self.save_theme(t) {
                    self.vm.error = Some(format!("テーマを保存できません: {e}"));
                    return;
                }
            }
            Action::Dash(d) => dashboard::handle(&mut self.dash, d),
            Action::Live(LiveAction::FilterChanged) => {}
            Action::Live(LiveAction::JumpToLatest) => self.jump_to_bottom = true,
            Action::Live(LiveAction::JumpDone) => self.jump_to_bottom = false,
        }
        self.refresh();
    }

    fn save_theme(&mut self, t: Theme) -> Result<(), RepoError> {
        let mut s = self.deps.settings.load()?;
        s.theme = t;
        self.deps.settings.save(&s)?;
        self.vm.theme = t;
        Ok(())
    }

    /// 1フレーム分の操作をまとめて処理する。操作がなければ何もしない。
    pub fn handle_all(&mut self, acts: Vec<Action>) {
        for a in acts {
            self.handle(a);
        }
    }

    /// 毎フレーム呼ぶ。前回の読み込みから`REFRESH_SECS`秒たっていれば読み直す。
    pub fn tick(&mut self) {
        self.poll_live();
        let now = self.deps.clock.now();
        if self
            .last_refresh
            .is_none_or(|t| now - t >= Duration::seconds(REFRESH_SECS))
        {
            self.refresh();
        }
    }

    /// 表示中のタブと上部の集計だけを読み直す。表示していないタブのデータを持たないため。
    pub fn refresh(&mut self) {
        let now = self.deps.clock.now();
        self.vm.daemon_running = self.deps.daemon.is_running();
        match self.build_all() {
            Ok((head, body)) => {
                self.vm.header = head;
                self.vm.body = body;
                self.vm.error = None;
            }
            Err(e) => self.vm.error = Some(format!("読み込みに失敗しました: {e}")),
        }
        self.vm.updated = fmt_clock(now, now, self.deps.tz);
        self.last_refresh = Some(now);
    }

    fn build_all(&mut self) -> Result<(HeaderVm, TabVm), RepoError> {
        Ok((header::build(&self.deps)?, self.build_body()?))
    }

    fn build_body(&mut self) -> Result<TabVm, RepoError> {
        Ok(match self.vm.tab {
            Tab::Dashboard => {
                let mut vm = dashboard::build(&self.deps, &self.dash, &self.forms.session_query)?;
                self.sync_live(&mut vm);
                TabVm::Dashboard(Box::new(vm))
            }
            t @ (Tab::Analytics | Tab::Profiles | Tab::Settings | Tab::Diagnostics) => {
                self.live = None;
                TabVm::Pending(t)
            }
        })
    }

    /// ライブログを開く、読み進める、捨てるの判断をする。
    fn sync_live(&mut self, vm: &mut DashboardVm) {
        let Some(d) = vm.detail.as_mut().filter(|d| d.tab == DetailTab::LiveLog) else {
            self.live = None;
            return;
        };
        match &mut self.live {
            Some(l) if l.session_id() == d.session_id => {
                l.poll();
            }
            _ => self.live = LiveLogState::open(&self.deps, &d.session_id).ok(),
        }
        d.live = self.live_vm();
    }

    fn live_vm(&self) -> Option<crate::controllers::gui::dashboard::live_log::LiveLogVm> {
        self.live.as_ref().map(|l| {
            let mut v = l.vm(&self.forms.live_filter, self.deps.tz);
            v.jump_to_bottom = self.jump_to_bottom;
            v
        })
    }

    /// 毎フレーム、ライブログの追記だけを読む。行が増えたときだけ詳細のViewModelを差し替える。
    fn poll_live(&mut self) {
        let changed = self.live.as_mut().is_some_and(LiveLogState::poll);
        if !changed {
            return;
        }
        let live = self.live_vm();
        if let TabVm::Dashboard(d) = &mut self.vm.body
            && let Some(det) = d.detail.as_mut()
        {
            det.live = live;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ports::ProfileRepo;
    use crate::test_support::{FakeCreds, FakeDaemon, FixedClock, gui_deps, temp_store};
    use chrono::TimeZone;
    use std::collections::HashMap;

    type Store = Arc<crate::models::repositories::db::SqliteStore>;

    /// controllerと、行を入れるための同じ実SQLiteを返す。
    fn ctl(
        daemon: FakeDaemon,
    ) -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Arc<FixedClock>,
        Store,
        GuiController,
    ) {
        let (d, s) = temp_store();
        s.ensure_default().unwrap();
        let s = Arc::new(s);
        let home = tempfile::tempdir().unwrap();
        let clock = Arc::new(FixedClock::at(
            Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap(),
        ));
        let deps = gui_deps(
            s.clone(),
            clock.clone(),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(daemon),
        );
        (d, home, clock, s, GuiController::new(deps))
    }

    #[test]
    fn labels_are_in_tab_order() {
        let labels: Vec<&str> = Tab::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(
            labels,
            ["ダッシュボード", "分析", "プロファイル", "設定", "診断"]
        );
    }

    #[test]
    fn tick_refreshes_only_after_interval() {
        let (_d, _h, clock, _s, mut c) = ctl(FakeDaemon::default());
        assert_eq!(c.vm().updated, "12:00");
        clock.advance(chrono::Duration::seconds(REFRESH_SECS - 1));
        c.tick();
        assert_eq!(c.vm().updated, "12:00");
        clock.advance(chrono::Duration::seconds(61));
        c.tick();
        assert_eq!(c.vm().updated, "12:01");
    }

    #[test]
    fn start_daemon_success_and_failure() {
        let (_d, _h, _c, _s, mut ok) = ctl(FakeDaemon::default());
        assert!(!ok.vm().daemon_running);
        ok.handle(Action::StartDaemon);
        assert!(ok.vm().daemon_running);
        let (_d2, _h2, _c2, _s, mut ng) = ctl(FakeDaemon {
            fail_start: true,
            ..FakeDaemon::default()
        });
        ng.handle(Action::StartDaemon);
        assert!(ng.vm().error.as_deref().unwrap().contains("起動できません"));
    }

    #[test]
    fn select_tab_builds_only_that_tab() {
        let (_d, _h, _c, _s, mut c) = ctl(FakeDaemon::default());
        assert!(matches!(c.vm().body, TabVm::Dashboard(_)));
        c.handle_all(vec![Action::SelectTab(Tab::Settings)]);
        assert_eq!(
            (c.vm().tab, &c.vm().body),
            (Tab::Settings, &TabVm::Pending(Tab::Settings))
        );
        c.handle(Action::SelectTab(Tab::Dashboard));
        assert!(matches!(c.vm().body, TabVm::Dashboard(_)));
    }

    #[test]
    fn set_theme_saves_and_updates_vm() {
        let (_d, _h, _c, _s, mut c) = ctl(FakeDaemon::default());
        c.handle(Action::SetTheme(Theme::Dark));
        assert_eq!(c.vm().theme, Theme::Dark);
        assert_eq!(c.deps.settings.load().unwrap().theme, Theme::Dark);
    }

    #[test]
    fn dash_actions_reach_dashboard_vm_and_forms_default_split() {
        let (_d, _h, _c, _s, mut c) = ctl(FakeDaemon::default());
        c.handle(Action::Dash(DashAction::ToggleTrend));
        c.handle(Action::Dash(DashAction::SetListMode(ListMode::History)));
        assert!(
            matches!(&c.vm().body, TabVm::Dashboard(d) if !d.trend_open && d.mode == ListMode::History)
        );
        assert_eq!(c.dash_mode(), ListMode::History);
        assert_eq!(c.view_parts().1.split_ratio, DEFAULT_SPLIT);
    }

    #[test]
    fn refresh_fills_header_and_dashboard() {
        let (_d, _h, _c, _s, c) = ctl(FakeDaemon::default());
        assert_eq!(c.vm().header.today, "0 $0.00");
        assert!(
            matches!(&c.vm().body, TabVm::Dashboard(d) if d.cards.len() == 1 && d.active.is_empty())
        );
    }

    fn with_live_session(
        store: &crate::models::repositories::db::SqliteStore,
        home: &std::path::Path,
    ) -> std::path::PathBuf {
        use crate::models::domain::transcript::SessionKind;
        use crate::test_support::seed_session;
        seed_session(
            store,
            "s1",
            SessionKind::Interactive,
            Some("busy"),
            Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap(),
        );
        seed_session(
            store,
            "s2",
            SessionKind::Interactive,
            None,
            Utc.with_ymd_and_hms(2026, 9, 26, 2, 0, 0).unwrap(),
        );
        let f = home.join(".claude/projects/-w/s1.jsonl");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "{\"type\":\"user\",\"timestamp\":\"2026-09-26T02:59:00.000Z\",\"sessionId\":\"s1\",\"message\":{\"content\":\"hi\"}}\n").unwrap();
        f
    }

    fn live_total(c: &GuiController) -> usize {
        match &c.vm().body {
            TabVm::Dashboard(d) => d
                .detail
                .as_ref()
                .and_then(|x| x.live.as_ref())
                .map(|l| l.total)
                .unwrap_or(0),
            _ => 0,
        }
    }

    #[test]
    fn switching_detail_or_session_discards_live_state() {
        use crate::controllers::gui::dashboard::sessions::DetailTab;
        use std::io::Write;
        let (_d, home, _c, s, mut c) = ctl(FakeDaemon::default());
        let f = with_live_session(&s, home.path());
        c.handle(Action::Dash(DashAction::Select(
            "s1".into(),
            DetailTab::LiveLog,
        )));
        assert_eq!(c.live.as_ref().map(|l| l.session_id()), Some("s1"));
        let before = live_total(&c);
        let mut w = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        writeln!(w, "{{\"type\":\"user\",\"timestamp\":\"2026-09-26T02:59:30.000Z\",\"sessionId\":\"s1\",\"message\":{{\"content\":\"again\"}}}}").unwrap();
        c.tick();
        assert_eq!(live_total(&c), before + 1, "毎フレームの`tick`で追記を読む");
        c.handle(Action::Dash(DashAction::SetDetailTab(DetailTab::Turns)));
        assert!(c.live.is_none());
        c.handle(Action::Dash(DashAction::SetDetailTab(DetailTab::LiveLog)));
        assert!(c.live.is_some());
        c.handle(Action::Dash(DashAction::Select(
            "s2".into(),
            DetailTab::LiveLog,
        )));
        assert_eq!(c.live.as_ref().map(|l| l.session_id()), Some("s2"));
        assert_eq!(live_total(&c), 0, "前のセッションの行を持ち越さない");
        c.handle(Action::Live(LiveAction::JumpToLatest));
        assert!(c.jump_to_bottom);
        c.handle(Action::Live(LiveAction::JumpDone));
        assert!(!c.jump_to_bottom);
        c.handle(Action::Dash(DashAction::Select(
            "s1".into(),
            DetailTab::LiveLog,
        )));
        c.handle(Action::SelectTab(Tab::Settings));
        assert!(c.live.is_none(), "別のタブへ移ったら捨てる");
    }
}
