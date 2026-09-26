//! GUIの主要導線（spec 12.2節）。一時ディレクトリの実SQLiteを使い、egui_kittestで操作する。
#![forbid(unsafe_code)]
#[allow(dead_code)]
mod common;

use chrono::{TimeZone, Utc};
use claude_profile_switcher::controllers::gui::app::Tab;
use claude_profile_switcher::controllers::gui::dashboard::ListMode;
use common::gui::gui_env;
use egui_kittest::kittest::Queryable;
use std::sync::atomic::Ordering;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
}

#[test]
fn tabs_switch_without_leaving_the_window() {
    let env = gui_env(now());
    let mut h = env.harness();
    for t in Tab::ALL.iter().rev() {
        h.get_by_label(t.label()).click();
        h.run();
        assert_eq!(h.state().vm().tab, *t);
    }
}

#[test]
fn stopped_daemon_can_be_started_from_gui() {
    let env = gui_env(now());
    let mut h = env.harness();
    h.get_by_label_contains("デーモン停止中");
    h.get_by_label("デーモンを起動").click();
    h.run();
    assert_eq!(env.daemon.starts.load(Ordering::SeqCst), 1);
    assert!(h.query_by_label_contains("デーモン停止中").is_none());
    h.get_by_label_contains("監視中");
}

#[test]
fn dashboard_shows_three_areas_at_once() {
    let env = gui_env(now());
    let h = env.harness();
    h.get_by_label("レート制限");
    h.get_by_label("セッション");
    h.get_by_label("左の一覧からセッションを選んでください");
    h.get_by_label("推移");
}

#[test]
fn list_mode_and_trend_collapse() {
    let env = gui_env(now());
    let mut h = env.harness();
    h.get_by_label("履歴").click();
    h.run();
    assert_eq!(h.state().dash_mode(), ListMode::History);
    let before = h.get_by_label("推移").rect();
    h.get_by_label("折りたたむ").click();
    h.run();
    h.get_by_label("開く");
    let after = h.get_by_label("推移").rect();
    assert!(
        after.center().y > before.center().y,
        "推移を折りたたむと中段が下へ広がり、推移の見出しが下へ移る"
    );
}

#[test]
fn dashboard_renders_in_light_and_dark() {
    let env = gui_env(now());
    for pref in [egui::ThemePreference::Light, egui::ThemePreference::Dark] {
        let mut h = env.harness();
        claude_profile_switcher::views::theme::apply(&h.ctx);
        h.ctx.set_theme(pref);
        h.run();
        assert_eq!(
            h.ctx.global_style().visuals.dark_mode,
            pref == egui::ThemePreference::Dark
        );
        for label in ["ダッシュボード", "レート制限", "セッション", "推移"] {
            h.get_by_label(label);
        }
        h.get_by_label_contains("デーモン停止中");
    }
}

#[test]
fn layout_follows_window_size() {
    let env = gui_env(now());
    let small = env.harness_sized(1000.0, 700.0);
    let large = env.harness_sized(1920.0, 1200.0);
    let x = |h: &egui_kittest::Harness<'static, _>| {
        h.get_by_label("左の一覧からセッションを選んでください")
            .rect()
            .min
            .x
    };
    assert!(
        x(&large) > x(&small) + 200.0,
        "広げると詳細ペインの位置が右へ移る"
    );
    for (h, w, ht) in [(&small, 1000.0, 700.0), (&large, 1920.0, 1200.0)] {
        for label in ["推移", "左の一覧からセッションを選んでください"] {
            let r = h.get_by_label(label).rect();
            assert!(
                r.max.x <= w && r.max.y <= ht,
                "{label} がウィンドウ内に収まる: {r:?}"
            );
        }
        assert!(h.get_by_label_contains("デーモン停止中").rect().max.x <= w);
    }
}
