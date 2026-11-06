//! GPUI版の画面の主要導線（spec 12.2節）。一時ディレクトリの実SQLiteを使い、common::gpuiのハーネスで操作する。
#![forbid(unsafe_code)]
mod common;

use chrono::{TimeZone, Utc};
use claude_usage_monitor::controllers::gui::app::{Tab, TabVm};
use claude_usage_monitor::controllers::gui::dashboard::trend::TrendRange;
use claude_usage_monitor::models::domain::usage::parse_usage;
use claude_usage_monitor::models::ports::{MaintenanceRepo, ProfileRepo, UsageRepo};
use common::gpui::GpuiHarness;
use common::gui::gui_env;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
}

#[test]
fn tabs_switch_without_leaving_the_window() {
    let env = gui_env(now());
    let mut h = env.gpui_harness();
    for t in Tab::ALL.iter().rev() {
        h.get_by_label(t.label()).click();
        h.run();
        assert_eq!(h.with_state(|c| c.vm().tab), *t);
    }
}

fn record_rising_usage(env: &common::gui::GuiEnv) {
    let p = env.store.ensure_default().unwrap();
    let mut snap = parse_usage(include_str!("fixtures/usage_ok.json")).unwrap();
    for (i, pct) in [10.0, 20.0, 30.0].iter().enumerate() {
        snap.limits[0].percent = *pct;
        snap.limits[0].resets_at = Some(now() + chrono::Duration::hours(2));
        env.store
            .record_snapshot(
                p.id,
                now() - chrono::Duration::minutes(20 - 10 * i as i64),
                &snap,
            )
            .unwrap();
    }
}

/// グラフの縦一列をマウスでなぞり、`needle`を含むツールチップが出たかを返す。
/// グラフは線の頂点の近くでしか点を拾わないため、1か所に乗せるだけでは当たらない。
fn tooltip_along(h: &mut GpuiHarness, x: f32, ys: std::ops::Range<f32>, needle: &str) -> bool {
    let mut y = ys.start;
    while y < ys.end {
        h.hover_at(x, y);
        if h.query_by_label_contains(needle).is_some() {
            return true;
        }
        y += 2.0;
    }
    false
}

#[test]
fn trend_graph_shows_the_value_under_the_pointer() {
    let env = gui_env(now());
    let p = env.store.ensure_default().unwrap();
    let mut snap = parse_usage(include_str!("fixtures/usage_ok.json")).unwrap();
    for l in &mut snap.limits {
        l.percent = 50.0;
        l.resets_at = Some(now() + chrono::Duration::hours(2));
    }
    for i in 0..60 {
        env.store
            .record_snapshot(p.id, now() - chrono::Duration::minutes(5 * i), &snap)
            .unwrap();
    }
    let mut h = env.gpui_harness();
    let r = h.get_by_label("5時間枠のグラフ").bounds();
    let (x, top) = (f32::from(r.left()) + 300.0, f32::from(r.bottom()));
    assert!(
        tooltip_along(&mut h, x, top..820.0, "50.0%"),
        "5時間枠のグラフに乗せると値が出る"
    );
    h.get_by_label_contains("default\n09/26 ");
}

#[test]
fn memory_graph_shows_the_value_under_the_pointer() {
    let env = gui_env(now());
    // 横軸は記録の範囲に合わせて伸びる。1分ごとに置き、頂点の間隔をマウスが拾える距離より狭くする。
    for i in 0..300 {
        env.store
            .record_daemon_stats(now() - chrono::Duration::minutes(i), 14 << 20)
            .unwrap();
    }
    let mut h = env.gpui_harness();
    h.get_by_label("診断").click();
    h.run();
    let r = h.get_by_label_contains("デーモンのメモリ").bounds();
    let top = f32::from(r.bottom());
    assert!(
        tooltip_along(&mut h, 640.0, top..top + 200.0, "RSS（MB）\n"),
        "メモリのグラフに乗せると値が出る"
    );
    h.get_by_label_contains("\n09/26 ");
    h.get_by_label_contains("\n14.0MB");
}

#[test]
fn trend_range_switches() {
    let env = gui_env(now());
    record_rising_usage(&env);
    let mut h = env.gpui_harness();
    h.get_by_label("5時間枠のグラフ");
    h.get_by_label("24時間").click();
    h.run();
    let range = h.with_state(|c| match &c.vm().body {
        TabVm::Dashboard(d) => d.trend.as_ref().map(|t| t.range),
        _ => None,
    });
    assert_eq!(range, Some(TrendRange::Hours24));
}

#[test]
fn diagnostics_tab_shows_fetch_status() {
    let env = gui_env(now());
    let mut h = env.gpui_harness();
    h.get_by_label("診断").click();
    h.run();
    h.get_by_label("取得の状況");
}

#[test]
fn help_icons_on_dashboard_show_their_text_on_hover() {
    use claude_usage_monitor::gpui_views::probe::Role;
    let env = gui_env(now());
    let mut h = env.gpui_harness();
    let n = h.get_all_by_label("ⓘ").count();
    assert!(n > 0);
    let labels = |h: &GpuiHarness| h.query_all_by_role(Role::Label).count();
    let mut missing = Vec::new();
    for i in 0..n {
        // 前のⓘの説明を消してから数える。
        h.hover_at(1.0, 1.0);
        let before = labels(&h);
        h.get_all_by_label("ⓘ").nth(i).unwrap().hover();
        h.step();
        if labels(&h) <= before {
            missing.push(i);
        }
    }
    assert!(missing.is_empty(), "説明が出ないⓘ: {missing:?} / {n}");
}

#[test]
fn layout_follows_window_size() {
    let env = gui_env(now());
    let mut small = env.gpui_harness_sized(1000.0, 700.0);
    let mut large = env.gpui_harness_sized(1920.0, 1200.0);
    small.run();
    large.run();
    let x = |h: &GpuiHarness| {
        f32::from(
            h.get_by_label("左の一覧からセッションを選んでください")
                .bounds()
                .left(),
        )
    };
    assert!(
        x(&large) > x(&small) + 200.0,
        "広げると詳細ペインの位置が右へ移る"
    );
    for (h, w, ht) in [(&small, 1000.0, 700.0), (&large, 1920.0, 1200.0)] {
        for label in ["推移", "左の一覧からセッションを選んでください"] {
            let r = h.get_by_label(label).bounds();
            assert!(
                f32::from(r.right()) <= w && f32::from(r.bottom()) <= ht,
                "{label} がウィンドウ内に収まる: {r:?}"
            );
        }
        assert!(f32::from(h.get_by_label_contains("デーモン停止中").bounds().right()) <= w);
    }
}

#[test]
fn cards_show_limits_remaining_and_projection() {
    let env = gui_env(now());
    record_rising_usage(&env);
    let h = env.gpui_harness();
    h.get_by_label("5時間枠");
    h.get_by_label("30%");
    h.get_by_label_contains("残り2時間（14:00）");
    h.get_by_label_contains("このペースだと 13:10 に上限");
    h.get_by_label("週間枠（Fable）");
    h.get_by_label("使用中");
}

fn log_expired(env: &common::gui::GuiEnv) {
    use claude_usage_monitor::models::domain::records::{FetchLogEntry, FetchResult};
    use claude_usage_monitor::models::ports::FetchLogRepo;
    let p = env.store.ensure_default().unwrap();
    env.store
        .log(&FetchLogEntry {
            target: format!("usage:{}", p.id),
            at: now(),
            result: FetchResult::Failed,
            http_status: Some(401),
            message: String::new(),
        })
        .unwrap();
}

#[test]
fn token_expired_is_shown_on_401_before_any_value() {
    let env = gui_env(now());
    log_expired(&env);
    let h = env.gpui_harness();
    h.get_by_label_contains("トークン期限切れ");
}

#[test]
fn token_expired_keeps_the_last_value_without_error() {
    let env = gui_env(now());
    record_rising_usage(&env);
    log_expired(&env);
    let h = env.gpui_harness();
    h.get_by_label("30%");
    assert!(h.query_by_label_contains("トークン期限切れ").is_none());
}

#[test]
fn splitter_drag_changes_the_split_ratio() {
    let env = gui_env(now());
    let mut h = env.gpui_harness();
    let before = h.with_state_mut(|c| c.forms_mut().split_ratio);
    h.get_by_label("左右の境界").drag_by(80.0);
    h.run();
    let after = h.with_state_mut(|c| c.forms_mut().split_ratio);
    assert!(after > before, "{before} -> {after}");
}

mod memory_e2e {
    use super::*;
    use claude_usage_monitor::models::domain::memory::{ProcEntry, Target, Victim};
    use std::path::PathBuf;

    const MIB: u64 = 1 << 20;

    fn desktop_env() -> common::gui::GuiEnv {
        let env = gui_env(now());
        *env.procs.procs.lock().unwrap() = vec![ProcEntry {
            pid: 100,
            parent: Some(1),
            rss: 1229 * MIB,
            start_time: 50,
            exe: Some(PathBuf::from(
                "/Applications/Claude.app/Contents/MacOS/Claude",
            )),
        }];
        env
    }

    fn notice(h: &mut GpuiHarness) -> Option<String> {
        h.with_state(|c| c.vm().notice.clone())
    }

    /// 「終了する」を押し、別スレッドの要求の結果が知らせに出るまで描き直す。
    fn confirm_exit(h: &mut GpuiHarness) {
        h.get_by_label("終了する").click();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            h.run();
            if notice(h).is_some_and(|n| !n.ends_with("に終了を要求しています")) {
                return;
            }
            assert!(std::time::Instant::now() < until, "要求の結果が届きません");
            std::thread::yield_now();
        }
    }

    #[test]
    fn desktop_card_requests_exit_after_confirm() {
        let env = desktop_env();
        let mut h = env.gpui_harness();
        h.get_by_label("Claude Desktop");
        h.get_by_label_contains("1.2GB");
        h.get_by_label("Desktopを終了").click();
        h.run();
        h.get_by_label_contains("実行中の編集が失われることがあります");
        confirm_exit(&mut h);
        assert_eq!(
            *env.procs.exits.lock().unwrap(),
            [Victim {
                pid: 100,
                start_time: 50,
                target: Target::Desktop
            }]
        );
        assert!(h.query_by_label("終了する").is_none(), "ダイアログが閉じる");
    }

    #[test]
    fn escape_closes_the_dialog_without_exit() {
        let env = desktop_env();
        let mut h = env.gpui_harness();
        h.get_by_label("Desktopを終了").click();
        h.run();
        h.get_by_label("終了する");
        h.press_key("escape");
        h.run();
        assert!(env.procs.exits.lock().unwrap().is_empty());
        assert!(h.query_by_label("終了する").is_none(), "Escで閉じる");
    }

    #[test]
    fn cancel_does_not_request_exit() {
        let env = desktop_env();
        let mut h = env.gpui_harness();
        h.get_by_label("Desktopを終了").click();
        h.run();
        h.get_by_label("やめる").click();
        h.run();
        assert!(env.procs.exits.lock().unwrap().is_empty());
        assert!(h.query_by_label("終了する").is_none());
    }
}
