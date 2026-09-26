//! GUI全体の状態、タブの切り替え、読み直しの周期。
use crate::controllers::cli::profile::ProfileCommand;
use crate::controllers::gui::dashboard::live_log::{LiveAction, LiveLogState};
use crate::controllers::gui::dashboard::sessions::DetailTab;
use crate::controllers::gui::dashboard::{self, DashAction, DashboardState, DashboardVm, ListMode};
use crate::controllers::gui::header;
use crate::controllers::gui::tabs::analytics::{self, AnalyticsVm, Period};
use crate::controllers::gui::tabs::diagnostics::{self, DiagnosticsVm};
use crate::controllers::gui::tabs::profiles::{self, ProfilesAction, ProfilesVm};
use crate::controllers::gui::tabs::settings::{
    self, ModelDraft, SettingsAction, SettingsDraft, SettingsVm,
};
use crate::models::domain::display::fmt_clock;
use crate::models::domain::live_log::LiveFilter;
use crate::models::domain::pricing::seed_models;
use crate::models::domain::settings::Theme;
use crate::models::ports::{
    AnalyticsRepo, CatalogRefresh, Clock, CredentialStore, DaemonControl, DashboardRepo,
    DiagnosticsRepo, FetchLogRepo, ModelRepo, ProfileRepo, RepoError, SessionQueryRepo,
    SettingsRepo, UsageRepo,
};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

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
    /// 分析の期間を変える。
    SetPeriod(Period),
    /// プロファイルの操作。
    Profiles(ProfilesAction),
    /// 設定タブの操作。
    Settings(SettingsAction),
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
    /// 分析。
    Analytics(AnalyticsVm),
    /// プロファイル。
    Profiles(ProfilesVm),
    /// 診断。
    Diagnostics(DiagnosticsVm),
    /// 設定。
    Settings(SettingsVm),
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
    /// 直前の操作の結果。どのタブでも見えるよう画面下部に出す。
    pub notice: Option<String>,
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
    /// 再生位置のスライダー。
    pub replay_position: usize,
    /// 追加するプロファイルの名前。
    pub new_profile_name: String,
    /// 追加するプロファイルの設定ディレクトリ。空なら`~/.claude-<名前>`。
    pub new_profile_dir: String,
    /// 設定の入力欄。
    pub settings: SettingsDraft,
    /// 編集中のモデル情報。
    pub model: Option<ModelDraft>,
}

impl Default for Forms {
    fn default() -> Self {
        Self {
            split_ratio: DEFAULT_SPLIT,
            session_query: String::new(),
            live_filter: LiveFilter::default(),
            replay_position: 0,
            new_profile_name: String::new(),
            new_profile_dir: String::new(),
            settings: SettingsDraft::default(),
            model: None,
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
    pub(crate) replay_len: usize,
    period: Period,
    profile_message: Option<String>,
    pending_remove: Option<String>,
    settings_message: Option<String>,
    catalog_job: Option<Receiver<Result<usize, RepoError>>>,
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
        let _ = deps.models.seed_if_empty(&seed_models());
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
                notice: None,
            },
            forms: Forms::default(),
            dash: DashboardState::default(),
            live: None,
            jump_to_bottom: false,
            replay_len: 0,
            period: Period::default(),
            profile_message: None,
            pending_remove: None,
            settings_message: None,
            catalog_job: None,
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
            Action::SelectTab(t) => {
                if t == Tab::Settings && self.vm.tab != Tab::Settings {
                    self.forms.settings = SettingsDraft::from_settings(
                        &self.deps.settings.load().unwrap_or_default(),
                    );
                }
                self.vm.tab = t;
            }
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
                self.forms.settings.theme = t;
            }
            Action::Dash(d) => dashboard::handle(&mut self.dash, d),
            Action::Live(LiveAction::FilterChanged) => {}
            Action::Live(LiveAction::JumpToLatest) => self.jump_to_bottom = true,
            Action::Live(LiveAction::JumpDone) => self.jump_to_bottom = false,
            Action::SetPeriod(p) => self.period = p,
            Action::Profiles(a) => self.handle_profiles(a),
            Action::Settings(a) => self.handle_settings(a),
        }
        self.refresh();
    }

    /// 入力欄の状態。テストで入力済みの状態を作るため公開する。
    pub fn forms_mut(&mut self) -> &mut Forms {
        &mut self.forms
    }

    fn handle_profiles(&mut self, a: ProfilesAction) {
        let msg = match a {
            ProfilesAction::Add => self.add_profile(),
            ProfilesAction::Use(name) => {
                profiles::run_command(&self.deps, ProfileCommand::Use { name })
            }
            ProfilesAction::Remove(name) => {
                self.pending_remove = Some(name);
                return;
            }
            ProfilesAction::ConfirmRemove => match self.pending_remove.take() {
                Some(name) => profiles::run_command(&self.deps, ProfileCommand::Remove { name }),
                None => return,
            },
            ProfilesAction::CancelRemove => {
                self.pending_remove = None;
                return;
            }
        };
        self.vm.notice = Some(msg.clone());
        self.profile_message = Some(msg);
    }

    /// 入力欄の内容で追加する。成功したときだけ入力欄を空にし、失敗したときは直せるよう残す。
    fn add_profile(&mut self) -> String {
        let dir = self.forms.new_profile_dir.trim();
        let cmd = ProfileCommand::Add {
            name: self.forms.new_profile_name.trim().to_string(),
            config_dir: (!dir.is_empty()).then(|| PathBuf::from(dir)),
        };
        let before = self.deps.profiles.list().map(|l| l.len()).unwrap_or(0);
        let m = profiles::run_command(&self.deps, cmd);
        if self.deps.profiles.list().map(|l| l.len()).unwrap_or(0) > before {
            self.forms.new_profile_name.clear();
            self.forms.new_profile_dir.clear();
        }
        m
    }

    fn handle_settings(&mut self, a: SettingsAction) {
        self.settings_message = Some(match a {
            SettingsAction::Save => self.save_settings(),
            SettingsAction::EditModel(prefix) => {
                self.forms.model = self.deps.models.all().ok().and_then(|ms| {
                    ms.iter()
                        .find(|m| m.model_prefix == prefix)
                        .map(ModelDraft::from_model)
                });
                return;
            }
            SettingsAction::SaveModel => match self.forms.model.as_ref().map(ModelDraft::parse) {
                Some(Ok(m)) => match self.deps.models.set_user(&m) {
                    Ok(()) => {
                        self.forms.model = None;
                        format!(
                            "{} の単価を保存しました。以後の自動取得では上書きしません",
                            m.model_prefix
                        )
                    }
                    Err(e) => e.to_string(),
                },
                Some(Err(e)) => e,
                None => return,
            },
            SettingsAction::CancelModel => {
                self.forms.model = None;
                return;
            }
            SettingsAction::RefreshCatalog => self.start_catalog_refresh(),
        });
    }

    fn save_settings(&mut self) -> String {
        match self.forms.settings.parse() {
            Ok(s) => match self.deps.settings.save(&s) {
                Ok(()) => {
                    self.vm.theme = s.theme;
                    "保存しました。デーモンは5分以内に新しい設定で動きます".into()
                }
                Err(e) => e.to_string(),
            },
            Err(e) => e,
        }
    }

    fn start_catalog_refresh(&mut self) -> String {
        if self.catalog_job.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let catalog = self.deps.catalog.clone();
            // 公式ページの取得は数秒かかるため、描画を止めないよう別スレッドで行う。
            std::thread::spawn(move || {
                let _ = tx.send(catalog.refresh());
            });
            self.catalog_job = Some(rx);
        }
        "公式ページからモデル情報を取得しています".into()
    }

    /// 手動更新の結果を受け取る。
    fn poll_catalog(&mut self) {
        let Some(rx) = &self.catalog_job else {
            return;
        };
        match rx.try_recv() {
            Ok(r) => {
                self.settings_message = Some(match r {
                    Ok(n) => format!("モデル情報を{n}件更新しました"),
                    Err(e) => format!("モデル情報を更新できません: {e}"),
                });
                self.catalog_job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.settings_message = Some("モデル情報の更新が中断しました".into());
                self.catalog_job = None;
            }
        }
    }

    /// 手動更新の完了を待つ。テストで別スレッドの終了を待つため。
    #[cfg(test)]
    pub(crate) fn wait_catalog_for_test(&mut self) {
        if let Some(rx) = self.catalog_job.take() {
            let r = rx.recv().expect("結果");
            let (tx, rx2) = std::sync::mpsc::channel();
            tx.send(r).expect("送信");
            self.catalog_job = Some(rx2);
        }
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

    /// 毎フレーム呼ぶ。ライブログの追記を読み、再生を進め、`REFRESH_SECS`秒ごとに読み直す。
    pub fn tick(&mut self) {
        self.poll_live();
        let now = self.deps.clock.now();
        if self.vm.tab == Tab::Dashboard
            && self.dash.detail_tab == DetailTab::Replay
            && self.dash.replay.playing
        {
            let before = (self.dash.replay.position, self.dash.replay.playing);
            dashboard::replay::advance(&mut self.dash.replay, self.replay_len, now);
            if (self.dash.replay.position, self.dash.replay.playing) != before {
                self.refresh();
                return;
            }
        }
        if self
            .last_refresh
            .is_none_or(|t| now - t >= Duration::seconds(REFRESH_SECS))
        {
            self.refresh();
        }
    }

    /// 表示中のタブと上部の集計だけを読み直す。表示していないタブのデータを持たないため。
    pub fn refresh(&mut self) {
        self.poll_catalog();
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
                if let Some(r) = vm.detail.as_ref().and_then(|d| d.replay.as_ref()) {
                    self.replay_len = r.len;
                    self.forms.replay_position = r.position;
                }
                TabVm::Dashboard(Box::new(vm))
            }
            Tab::Analytics => {
                self.live = None;
                TabVm::Analytics(analytics::build(&self.deps, self.period)?)
            }
            Tab::Profiles => {
                self.live = None;
                TabVm::Profiles(profiles::build(
                    &self.deps,
                    self.profile_message.clone(),
                    self.pending_remove.clone(),
                )?)
            }
            Tab::Diagnostics => {
                self.live = None;
                TabVm::Diagnostics(diagnostics::build(&self.deps)?)
            }
            Tab::Settings => {
                self.live = None;
                TabVm::Settings(settings::build(
                    &self.deps,
                    self.settings_message.clone(),
                    self.catalog_job.is_some(),
                )?)
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
        assert_eq!(c.vm().tab, Tab::Settings);
        assert!(matches!(c.vm().body, TabVm::Settings(_)));
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

    #[test]
    fn tick_advances_replay_while_playing() {
        use crate::controllers::gui::dashboard::replay::ReplayState;
        use crate::controllers::gui::dashboard::sessions::DetailTab;
        let (_d, _h, clock, _s, mut c) = ctl(FakeDaemon::default());
        c.dash.detail_tab = DetailTab::Replay;
        c.dash.replay = ReplayState {
            playing: true,
            speed: 1,
            last_step: Some(clock.now()),
            ..ReplayState::default()
        };
        c.replay_len = 5;
        clock.advance(chrono::Duration::seconds(2));
        c.tick();
        assert_eq!(c.dash.replay.position, 2);
        c.dash.detail_tab = DetailTab::Turns;
        clock.advance(chrono::Duration::seconds(2));
        c.tick();
        assert_eq!(
            c.dash.replay.position, 2,
            "再生を表示していないときは進めない"
        );
    }

    #[test]
    fn period_and_trend_actions() {
        use crate::controllers::gui::dashboard::trend::TrendRange;
        use crate::controllers::gui::tabs::analytics::Period;
        let (_d, _h, _c, _s, mut c) = ctl(FakeDaemon::default());
        c.handle(Action::Dash(DashAction::SetTrendRange(TrendRange::Days7)));
        assert!(
            matches!(&c.vm().body, TabVm::Dashboard(d) if d.trend.as_ref().unwrap().range == TrendRange::Days7)
        );
        c.handle(Action::Dash(DashAction::ToggleTrend));
        assert!(
            matches!(&c.vm().body, TabVm::Dashboard(d) if d.trend.is_none() && d.trend_range == TrendRange::Days7)
        );
        c.handle(Action::SelectTab(Tab::Analytics));
        c.handle(Action::SetPeriod(Period::Days30));
        assert!(matches!(&c.vm().body, TabVm::Analytics(a) if a.period == Period::Days30));
    }

    #[test]
    fn profile_add_use_from_card_and_confirmed_remove() {
        use crate::controllers::gui::tabs::profiles::ProfilesAction;
        let (_d, _h, _c, _s, mut c) = ctl(FakeDaemon::default());
        c.handle(Action::SelectTab(Tab::Profiles));
        c.forms_mut().new_profile_name = "work".into();
        c.handle(Action::Profiles(ProfilesAction::Add));
        assert!(c.forms_mut().new_profile_name.is_empty());
        assert!(matches!(&c.vm().body, TabVm::Profiles(p) if p.rows.len() == 2));
        c.handle(Action::SelectTab(Tab::Dashboard));
        c.handle(Action::Profiles(ProfilesAction::Use("work".into())));
        assert!(
            c.vm()
                .notice
                .as_deref()
                .unwrap()
                .contains("work に切り替えました")
        );
        assert!(
            matches!(&c.vm().body, TabVm::Dashboard(d) if d.cards.iter().any(|k| k.name == "work" && k.is_active))
        );
        c.handle(Action::Profiles(ProfilesAction::Use("default".into())));
        c.handle(Action::SelectTab(Tab::Profiles));
        c.handle(Action::Profiles(ProfilesAction::Remove("work".into())));
        assert!(
            matches!(&c.vm().body, TabVm::Profiles(p) if p.pending_remove.as_deref() == Some("work"))
        );
        c.handle(Action::Profiles(ProfilesAction::CancelRemove));
        assert!(
            matches!(&c.vm().body, TabVm::Profiles(p) if p.pending_remove.is_none() && p.rows.len() == 2)
        );
        c.handle(Action::Profiles(ProfilesAction::ConfirmRemove));
        assert!(
            matches!(&c.vm().body, TabVm::Profiles(p) if p.rows.len() == 2),
            "確認中でなければ何もしない"
        );
        c.handle(Action::Profiles(ProfilesAction::Remove("work".into())));
        c.handle(Action::Profiles(ProfilesAction::ConfirmRemove));
        assert!(matches!(&c.vm().body, TabVm::Profiles(p) if p.rows.len() == 1));
        c.forms_mut().new_profile_name = "bad name".into();
        c.handle(Action::Profiles(ProfilesAction::Add));
        assert_eq!(
            c.forms_mut().new_profile_name,
            "bad name",
            "失敗したときは入力を残す"
        );
    }

    #[test]
    fn settings_save_model_edit_and_catalog_refresh() {
        use crate::controllers::gui::tabs::settings::SettingsAction;
        let (_d, _h, clock, _s, mut c) = ctl(FakeDaemon::default());
        c.handle(Action::SelectTab(Tab::Settings));
        assert_eq!(c.forms_mut().settings.threshold, "80");
        c.forms_mut().settings.threshold = "90".into();
        c.handle(Action::Settings(SettingsAction::Save));
        assert_eq!(
            c.deps.settings.load().unwrap().notify_threshold_percent,
            90.0
        );
        assert!(
            matches!(&c.vm().body, TabVm::Settings(s) if s.message.as_deref().unwrap().contains("5分以内"))
        );
        c.forms_mut().settings.interval = "5".into();
        c.handle(Action::Settings(SettingsAction::Save));
        assert!(
            matches!(&c.vm().body, TabVm::Settings(s) if s.message.as_deref().unwrap().contains("取得間隔"))
        );
        c.handle(Action::Settings(SettingsAction::RefreshCatalog));
        assert!(matches!(&c.vm().body, TabVm::Settings(s) if s.refreshing));
        c.wait_catalog_for_test();
        clock.advance(chrono::Duration::seconds(REFRESH_SECS));
        c.tick();
        assert!(
            matches!(&c.vm().body, TabVm::Settings(s) if !s.refreshing && s.message.as_deref() == Some("モデル情報を1件更新しました"))
        );
        let prefix = c.deps.models.all().unwrap()[0].model_prefix.clone();
        c.handle(Action::Settings(SettingsAction::EditModel(prefix.clone())));
        c.forms_mut().model.as_mut().unwrap().input = "0.5".into();
        c.handle(Action::Settings(SettingsAction::SaveModel));
        assert!(c.forms_mut().model.is_none());
        let saved = c
            .deps
            .models
            .all()
            .unwrap()
            .into_iter()
            .find(|m| m.model_prefix == prefix)
            .unwrap();
        assert_eq!(
            (saved.input, saved.source),
            (0.5, crate::models::domain::pricing::ModelSource::User)
        );
        c.handle(Action::Settings(SettingsAction::EditModel(prefix)));
        c.handle(Action::Settings(SettingsAction::CancelModel));
        assert!(c.forms_mut().model.is_none());
        c.handle(Action::Settings(SettingsAction::SaveModel));
    }

    #[test]
    fn theme_from_settings_or_header_stays_in_sync() {
        use crate::controllers::gui::tabs::settings::SettingsAction;
        let (_d, _h, _c, _s, mut c) = ctl(FakeDaemon::default());
        c.handle(Action::SelectTab(Tab::Settings));
        c.forms_mut().settings.theme = Theme::Dark;
        c.handle(Action::Settings(SettingsAction::Save));
        assert_eq!(c.vm().theme, Theme::Dark);
        c.handle(Action::SetTheme(Theme::Light));
        assert_eq!(
            c.forms_mut().settings.theme,
            Theme::Light,
            "上部のボタンで変えたテーマを入力欄にも反映する"
        );
        c.handle(Action::SelectTab(Tab::Diagnostics));
        assert!(matches!(&c.vm().body, TabVm::Diagnostics(_)));
    }
}
