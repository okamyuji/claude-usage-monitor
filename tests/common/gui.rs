//! GUIのE2E用の組み立て。
#![allow(dead_code)]
use chrono::{DateTime, Utc};
use claude_usage_monitor::controllers::gui::app::GuiDeps;
use claude_usage_monitor::models::domain::memory::{ExitError, ProcEntry, Victim};
use claude_usage_monitor::models::domain::profile::Profile;
use claude_usage_monitor::models::ports::{
    CatalogRefresh, Clock, Credential, CredentialError, CredentialStore, DaemonControl,
    ProcessTree, RepoError,
};
use claude_usage_monitor::models::repositories::db::SqliteStore;
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

/// 登録状態を覚えるだけの自動起動。
#[derive(Default)]
pub struct FakeAutostart(pub Mutex<bool>);

impl claude_usage_monitor::models::ports::Autostart for FakeAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        Ok(*self.0.lock().unwrap())
    }
    fn enable(&self) -> Result<(), String> {
        *self.0.lock().unwrap() = true;
        Ok(())
    }
    fn disable(&self) -> Result<(), String> {
        *self.0.lock().unwrap() = false;
        Ok(())
    }
}

pub struct NoCreds;
impl CredentialStore for NoCreds {
    fn load(&self, _p: &Profile, _dir: &Path) -> Result<Credential, CredentialError> {
        Err(CredentialError::Missing)
    }
}

/// 固定の一覧を返し、終了の要求を記録するだけのプロセス一覧。利用者のプロセスに触れずにGUIを確かめるため。
#[derive(Default)]
pub struct FakeProcessTree {
    pub procs: Mutex<Vec<ProcEntry>>,
    pub exits: Mutex<Vec<Victim>>,
}

impl ProcessTree for FakeProcessTree {
    fn snapshot(&self) -> Vec<ProcEntry> {
        self.procs.lock().unwrap().clone()
    }
    fn request_exit(&self, v: &Victim) -> Result<(), ExitError> {
        self.exits.lock().unwrap().push(*v);
        Ok(())
    }
}

pub struct GuiEnv {
    pub data: tempfile::TempDir,
    pub home: tempfile::TempDir,
    pub store: Arc<SqliteStore>,
    pub clock: Arc<FixedClock>,
    pub daemon: Arc<FakeDaemon>,
    pub procs: Arc<FakeProcessTree>,
}

pub fn gui_env(now: DateTime<Utc>) -> GuiEnv {
    let data = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(&data.path().join("cumon.db")).unwrap());
    GuiEnv {
        data,
        home,
        store,
        clock: Arc::new(FixedClock(Mutex::new(now))),
        daemon: Arc::new(FakeDaemon::default()),
        procs: Arc::new(FakeProcessTree::default()),
    }
}

impl GuiEnv {
    pub fn deps(&self) -> GuiDeps {
        let s = self.store.clone();
        GuiDeps {
            clock: self.clock.clone(),
            tz: chrono::FixedOffset::east_opt(9 * 3600).unwrap(),
            home: self.home.path().to_path_buf(),
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
            autostart: Arc::new(FakeAutostart::default()),
            process_tree: self.procs.clone(),
        }
    }

    pub fn advance(&self, d: chrono::Duration) {
        let mut t = self.clock.0.lock().unwrap();
        *t += d;
    }
}
