//! README用の画像を、Metalで画面の外に描いて書き出す。窓を画面に出さず、画面のロック中でも作れ、同じデータから何度でも作り直せるようにするため。
//!
//! 架空データのDBとHOMEを環境変数で渡して、macOSの手元から実行する。
//! `CUMON_DEMO_DATA=<DBのあるディレクトリ> CUMON_DEMO_HOME=<HOME> cargo test --test readme_images -- --ignored`
// Metalの画面外の描画とmacOSの文字の仕組みで描くため、macOSだけでコンパイルする。
#![cfg(target_os = "macos")]
#![forbid(unsafe_code)]
mod common;

use chrono::Utc;
use claude_usage_monitor::controllers::gui::app::{Action, GuiController, GuiDeps, Tab, TabVm};
use claude_usage_monitor::controllers::gui::dashboard::DashAction;
use claude_usage_monitor::controllers::gui::dashboard::sessions::DetailTab;
use claude_usage_monitor::controllers::gui::tabs::calendar::CalendarAction;

use claude_usage_monitor::models::domain::settings::Theme;
use claude_usage_monitor::models::repositories::db::SqliteStore;
use claude_usage_monitor::views::app::CumonRoot;
use common::gui::{FakeAutostart, FakeDaemon, FakeProcessTree, FixedClock, NoCatalog, NoCreds};
use gpui::{AppContext as _, Entity, HeadlessAppContext, WindowHandle, px, size};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

fn deps(data: &Path, home: &Path) -> GuiDeps {
    let s = Arc::new(SqliteStore::open(&data.join("cumon.db")).unwrap());
    let daemon = Arc::new(FakeDaemon::default());
    daemon.running.store(true, Ordering::SeqCst);
    GuiDeps {
        clock: Arc::new(FixedClock(Mutex::new(Utc::now()))),
        tz: *chrono::Local::now().offset(),
        home: home.to_path_buf(),
        profiles: s.clone(),
        usage: s.clone(),
        dashboard: s.clone(),
        sessions: s.clone(),
        analytics: s.clone(),
        diagnostics: s.clone(),
        logs: s.clone(),
        models: s.clone(),
        settings: s,
        creds: Arc::new(NoCreds),
        daemon,
        catalog: Arc::new(NoCatalog),
        autostart: Arc::new(FakeAutostart::default()),
        process_tree: Arc::new(FakeProcessTree::default()),
    }
}

struct Shot {
    cx: HeadlessAppContext,
    window: WindowHandle<gpui_component::Root>,
    root: Entity<CumonRoot>,
}

impl Shot {
    /// `cumon gui`の初期サイズと同じ1280×820の窓を、画面に出さずに開く。テスト窓の倍率は2。
    fn open(ctl: GuiController) -> Self {
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(gpui_apple::AppleTextSystem::new()),
            Arc::new(()),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            gpui_component::init(cx);
            claude_usage_monitor::views::fonts::install(cx).expect("同梱のフォント");
        });
        let mut root = None;
        let window = cx
            .open_window(size(px(1280.0), px(820.0)), |w, cx| {
                let r = cx.new(|cx| CumonRoot::new(ctl, w, cx));
                root = Some(r.clone());
                cx.new(|cx| gpui_component::Root::new(r, w, cx))
            })
            .expect("画面の外の窓");
        Self {
            cx,
            window,
            root: root.expect("根のビュー"),
        }
    }

    fn act(&mut self, a: Action) {
        let root = self.root.clone();
        self.cx
            .update_window(self.window.into(), |_, _, cx| {
                root.update(cx, |r, cx| r.act(a, cx))
            })
            .expect("窓");
        self.cx.run_until_parked();
    }

    fn with_vm<R>(&mut self, f: impl FnOnce(&TabVm) -> R) -> R {
        let root = self.root.clone();
        self.cx
            .update(|cx| f(&root.read(cx).controller().vm().body))
    }

    fn save(&mut self, out: &Path, name: &str) {
        // 描き直しを1回回してから撮る。直前の操作の結果が次の描画に出るため。
        self.cx
            .update_window(self.window.into(), |_, w, _| w.refresh())
            .expect("窓");
        self.cx.run_until_parked();
        let img = self
            .cx
            .capture_screenshot(self.window.into())
            .expect("画面の外の描画");
        img.save(out.join(name)).expect("画像の保存");
        println!("{}", out.join(name).display());
    }
}

#[test]
#[ignore]
fn write_readme_images() {
    let data = PathBuf::from(std::env::var("CUMON_DEMO_DATA").expect("CUMON_DEMO_DATA"));
    let home = PathBuf::from(std::env::var("CUMON_DEMO_HOME").expect("CUMON_DEMO_HOME"));
    let out = PathBuf::from(
        std::env::var("CUMON_README_IMAGES").unwrap_or_else(|_| "docs/images".into()),
    );
    std::fs::create_dir_all(&out).unwrap();
    let mut shot = Shot::open(GuiController::new(deps(&data, &home)));
    // READMEの画像はOSの明暗に左右されないよう、いつもライトで撮る。
    shot.act(Action::SetTheme(Theme::Light));
    shot.save(&out, "dashboard.png");
    let live = shot.with_vm(|b| match b {
        TabVm::Dashboard(d) => d
            .active
            .iter()
            .find(|r| r.title == "パスワード再設定")
            .map(|r| r.session_id.clone()),
        _ => None,
    });
    shot.act(Action::Dash(DashAction::Select(
        live.expect("稼働中のパスワード再設定"),
        DetailTab::LiveLog,
    )));
    shot.save(&out, "dashboard-live-log.png");
    shot.act(Action::SelectTab(Tab::Calendar));
    // 架空データは曜日によらず7日分そろうよう、前の週に入れてある。
    shot.act(Action::Calendar(CalendarAction::PrevWeek));
    // 帯は説明の1行目にセッションの見出しを持つので、その文言で1本を選ぶ。
    let band = shot.with_vm(|b| match b {
        TabVm::Calendar(c) => c
            .days
            .iter()
            .flat_map(|d| &d.bands)
            .find(|b| b.tooltip.contains("トップページの表示を速くする"))
            .map(|b| (b.session_id.clone(), b.seg_start)),
        _ => None,
    });
    let (sid, seg) = band.expect("カレンダーの帯");
    shot.act(Action::Calendar(CalendarAction::SelectBand(sid, seg)));
    shot.save(&out, "calendar.png");
}
