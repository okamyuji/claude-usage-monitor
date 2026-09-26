//! README用の画像を、GPUで画面外に描いて書き出す。画面のロック中でも作れ、同じデータから何度でも作り直せるようにするため。
//!
//! 架空データのDBとHOMEを環境変数で渡して手元から実行する。
//! `CUMON_DEMO_DATA=<DBのあるディレクトリ> CUMON_DEMO_HOME=<HOME> cargo test --test readme_images -- --ignored`
#![forbid(unsafe_code)]
mod common;

use chrono::Utc;
use claude_usage_monitor::controllers::gui::app::{GuiController, GuiDeps};
use claude_usage_monitor::models::repositories::db::SqliteStore;
use claude_usage_monitor::views::app::{install_fonts, show_app};
use claude_usage_monitor::views::theme;
use common::gui::{FakeAutostart, FakeDaemon, FixedClock, NoCatalog, NoCreds};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
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
    }
}

fn save(h: &mut Harness<'_, GuiController>, out: &Path, name: &str) {
    let img = h.render().expect("描画");
    img.save(out.join(name)).expect("画像の保存");
    println!("{}", out.join(name).display());
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
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(2.0)
        .with_theme(egui::Theme::Light)
        .wgpu()
        .build_ui_state(
            |ui, c: &mut GuiController| {
                c.tick();
                let (vm, forms) = c.view_parts();
                let acts = show_app(ui, vm, forms);
                c.handle_all(acts);
            },
            GuiController::new(deps(&data, &home)),
        );
    install_fonts(&h.ctx);
    theme::apply(&h.ctx);
    h.run();
    save(&mut h, &out, "dashboard.png");
    h.get_by_label("パスワード再設定").click();
    h.run();
    save(&mut h, &out, "dashboard-live-log.png");
}
