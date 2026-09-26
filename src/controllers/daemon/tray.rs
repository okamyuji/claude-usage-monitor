//! トレイの表示内容と操作。
//!
//! tray-iconに依存しないので、画面なしでテストできる。描画は`views/tray.rs`が担う。
use crate::controllers::cli::profile::{ProfileCommand, execute};
use crate::models::domain::display::fmt_percent;
use crate::models::ports::{DashboardRepo, GuiLauncher, ProfileRepo, RepoError};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// トレイの依存。
pub struct TrayDeps {
    /// プロファイル。
    pub profiles: Arc<dyn ProfileRepo>,
    /// 最新の使用率。
    pub dashboard: Arc<dyn DashboardRepo>,
    /// GUIの起動。
    pub launcher: Arc<dyn GuiLauncher>,
    /// 一時停止の旗。周期処理のスレッドと共有する。
    pub paused: Arc<AtomicBool>,
    /// 終了の旗。周期処理のスレッドと共有する。
    pub shutdown: Arc<AtomicBool>,
    /// ホームディレクトリ。
    pub home: PathBuf,
}

/// トレイの表示内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayVm {
    /// メニューバーの文字（`13%・67%`）。
    pub title: String,
    /// ホバー時の説明。
    pub tooltip: String,
    /// プロファイル名と使用中か。
    pub profiles: Vec<(String, bool)>,
    /// 一時停止中か。
    pub paused: bool,
}

/// トレイの操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAction {
    /// 画面を開く。
    OpenGui,
    /// プロファイルを使用中にする。
    UseProfile(String),
    /// 一時停止と再開を切り替える。
    TogglePause,
    /// 終了する。
    Quit,
}

/// 操作の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayOutcome {
    /// そのまま続ける。
    Continue,
    /// イベントループを終える。
    Quit,
    /// 利用者に知らせる文（通知で出す）。
    Message(String),
}

/// トレイのcontroller。
pub struct TrayController {
    pub(crate) deps: TrayDeps,
}

impl TrayController {
    /// 依存から作る。
    pub fn new(deps: TrayDeps) -> Self {
        Self { deps }
    }

    /// 表示内容を作る。まだ取得していないときは`—`を出す。
    pub fn vm(&self) -> Result<TrayVm, RepoError> {
        let profiles = self.deps.profiles.list()?;
        let active = profiles.iter().find(|p| p.is_active);
        let usage = match active {
            Some(p) => self.deps.dashboard.latest_usage(p.id)?,
            None => None,
        };
        let pct = |kind: &str| {
            usage
                .as_ref()
                .and_then(|u| u.limits.iter().find(|l| l.kind == kind))
                .map(|l| fmt_percent(l.percent))
        };
        let (title, tooltip) = match (pct("session"), pct("weekly_all")) {
            (Some(s), Some(w)) => (
                format!("{s}・{w}"),
                format!(
                    "{}: 5時間枠 {s} / 週間枠 {w}",
                    active.map(|p| p.name.as_str()).unwrap_or("")
                ),
            ),
            _ => ("—".to_string(), "まだ取得していません".to_string()),
        };
        Ok(TrayVm {
            title,
            tooltip,
            profiles: profiles
                .iter()
                .map(|p| (p.name.clone(), p.is_active))
                .collect(),
            paused: self.deps.paused.load(Ordering::SeqCst),
        })
    }

    /// 操作を処理する。
    pub fn handle(&self, a: TrayAction) -> TrayOutcome {
        match a {
            TrayAction::OpenGui => match self.deps.launcher.open() {
                Ok(()) => TrayOutcome::Continue,
                Err(e) => TrayOutcome::Message(e),
            },
            TrayAction::UseProfile(name) => {
                let mut out = Vec::new();
                TrayOutcome::Message(
                    match execute(
                        self.deps.profiles.as_ref(),
                        &self.deps.home,
                        ProfileCommand::Use { name },
                        &mut out,
                    ) {
                        Ok(()) => String::from_utf8_lossy(&out).trim().to_string(),
                        Err(e) => e.to_string(),
                    },
                )
            }
            TrayAction::TogglePause => {
                self.deps.paused.fetch_xor(true, Ordering::SeqCst);
                TrayOutcome::Continue
            }
            TrayAction::Quit => {
                self.deps.shutdown.store(true, Ordering::SeqCst);
                TrayOutcome::Quit
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{ProfileRepo, UsageRepo};
    use crate::test_support::temp_store;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Launcher(Mutex<usize>, bool);
    impl GuiLauncher for Launcher {
        fn open(&self) -> Result<(), String> {
            *self.0.lock().unwrap() += 1;
            if self.1 {
                Err("GUIを起動できません: テスト".into())
            } else {
                Ok(())
            }
        }
    }

    fn setup(
        fail_launch: bool,
    ) -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Arc<crate::models::repositories::db::SqliteStore>,
        Arc<Launcher>,
        TrayController,
    ) {
        let (d, s) = temp_store();
        let s = Arc::new(s);
        s.ensure_default().unwrap();
        let home = tempfile::tempdir().unwrap();
        let launcher = Arc::new(Launcher(Mutex::new(0), fail_launch));
        let c = TrayController::new(TrayDeps {
            profiles: s.clone(),
            dashboard: s.clone(),
            launcher: launcher.clone(),
            paused: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            home: home.path().to_path_buf(),
        });
        (d, home, s, launcher, c)
    }

    #[test]
    fn title_shows_active_profile_session_and_weekly() {
        let (_d, _h, s, _l, c) = setup(false);
        assert_eq!(c.vm().unwrap().title, "—");
        let p = s.ensure_default().unwrap();
        let snap = parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap();
        s.record_snapshot(p.id, chrono::Utc::now(), &snap).unwrap();
        let vm = c.vm().unwrap();
        assert_eq!(vm.title, "13%・67%");
        assert_eq!(vm.tooltip, "default: 5時間枠 13% / 週間枠 67%");
        assert_eq!(vm.profiles, [("default".to_string(), true)]);
        assert!(!vm.paused);
    }

    #[test]
    fn use_profile_matches_cli() {
        let (_d, _h, s, _l, c) = setup(false);
        s.add("sub", None).unwrap();
        assert!(
            matches!(c.handle(TrayAction::UseProfile("sub".into())), TrayOutcome::Message(m) if m.contains("sub に切り替えました"))
        );
        assert_eq!(s.active().unwrap().unwrap().name, "sub");
        assert!(
            matches!(c.handle(TrayAction::UseProfile("none".into())), TrayOutcome::Message(m) if !m.is_empty())
        );
    }

    #[test]
    fn pause_open_and_quit() {
        let (_d, _h, _s, l, c) = setup(false);
        assert_eq!(c.handle(TrayAction::TogglePause), TrayOutcome::Continue);
        assert!(c.vm().unwrap().paused);
        c.handle(TrayAction::TogglePause);
        assert!(!c.vm().unwrap().paused);
        assert_eq!(c.handle(TrayAction::OpenGui), TrayOutcome::Continue);
        assert_eq!(*l.0.lock().unwrap(), 1);
        assert_eq!(c.handle(TrayAction::Quit), TrayOutcome::Quit);
        assert!(c.deps.shutdown.load(Ordering::SeqCst));
        let (_d2, _h2, _s2, _l2, failing) = setup(true);
        assert!(
            matches!(failing.handle(TrayAction::OpenGui), TrayOutcome::Message(m) if m.contains("起動できません"))
        );
    }
}
