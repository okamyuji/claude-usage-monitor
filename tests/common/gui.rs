//! GUIのE2E用の組み立て。
#![allow(dead_code)]
use chrono::{DateTime, Utc};
use claude_profile_switcher::controllers::gui::app::{GuiController, GuiDeps};
use claude_profile_switcher::models::domain::profile::Profile;
use claude_profile_switcher::models::ports::{
    CatalogRefresh, Clock, Credential, CredentialError, CredentialStore, DaemonControl, RepoError,
};
use claude_profile_switcher::models::repositories::db::SqliteStore;
use claude_profile_switcher::views::app::show_app;
use egui_kittest::Harness;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub struct FixedClock(pub Mutex<DateTime<Utc>>);
impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

#[derive(Default)]
pub struct FakeDaemon {
    pub running: AtomicBool,
    pub starts: AtomicUsize,
}
impl DaemonControl for FakeDaemon {
    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    fn start(&self) -> Result<(), String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }
}

pub struct NoCatalog;
impl CatalogRefresh for NoCatalog {
    fn refresh(&self) -> Result<usize, RepoError> {
        Ok(0)
    }
}

pub struct NoCreds;
impl CredentialStore for NoCreds {
    fn load(&self, _p: &Profile, _dir: &Path) -> Result<Credential, CredentialError> {
        Err(CredentialError::Missing)
    }
}

pub struct GuiEnv {
    pub data: tempfile::TempDir,
    pub home: tempfile::TempDir,
    pub store: Arc<SqliteStore>,
    pub clock: Arc<FixedClock>,
    pub daemon: Arc<FakeDaemon>,
}

pub fn gui_env(now: DateTime<Utc>) -> GuiEnv {
    let data = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(&data.path().join("cps.db")).unwrap());
    GuiEnv {
        data,
        home,
        store,
        clock: Arc::new(FixedClock(Mutex::new(now))),
        daemon: Arc::new(FakeDaemon::default()),
    }
}

impl GuiEnv {
    pub fn deps(&self) -> GuiDeps {
        let s = self.store.clone();
        GuiDeps {
            clock: self.clock.clone(),
            tz: chrono::FixedOffset::east_opt(9 * 3600).unwrap(),
            home: self.home.path().to_path_buf(),
            font_path: None,
            profiles: s.clone(),
            usage: s.clone(),
            dashboard: s.clone(),
            sessions: s.clone(),
            analytics: s.clone(),
            diagnostics: s.clone(),
            logs: s.clone(),
            models: s.clone(),
            settings: s.clone(),
            creds: Arc::new(NoCreds),
            daemon: self.daemon.clone(),
            catalog: Arc::new(NoCatalog),
        }
    }

    /// 既定の大きさ（1280×820）のウィンドウで動かす。`cps gui`の初期サイズと同じにするため。
    pub fn harness(&self) -> Harness<'static, GuiController> {
        self.harness_sized(1280.0, 820.0)
    }

    /// 大きさを指定して動かす。伸縮の確認に使う。
    pub fn harness_sized(&self, w: f32, h: f32) -> Harness<'static, GuiController> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(w, h))
            .build_ui_state(
                |ui, c: &mut GuiController| {
                    // `cps gui`の`CpsApp::ui`と同じ順で呼ぶ。毎フレームの処理（ライブログの追記の読み取り）も同じ経路で確かめるため。
                    c.tick();
                    let (vm, forms) = c.view_parts();
                    let acts = show_app(ui, vm, forms);
                    c.handle_all(acts);
                },
                GuiController::new(self.deps()),
            );
        harness.run();
        harness
    }

    pub fn advance(&self, d: chrono::Duration) {
        let mut t = self.clock.0.lock().unwrap();
        *t += d;
    }
}
