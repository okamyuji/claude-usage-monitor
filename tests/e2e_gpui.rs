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
