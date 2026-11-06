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
    use claude_usage_monitor::views::probe::Role;
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

#[test]
fn stopped_daemon_can_be_started_from_gui() {
    use std::sync::atomic::Ordering;
    let env = gui_env(now());
    let mut h = env.gpui_harness();
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
    let h = env.gpui_harness();
    h.get_by_label("レート制限");
    h.get_by_label("セッション");
    h.get_by_label("左の一覧からセッションを選んでください");
    h.get_by_label("推移");
}

#[test]
fn list_mode_and_trend_collapse() {
    use claude_usage_monitor::controllers::gui::dashboard::ListMode;
    let env = gui_env(now());
    let mut h = env.gpui_harness();
    h.get_by_label("履歴").click();
    h.run();
    assert_eq!(h.with_state(|c| c.dash_mode()), ListMode::History);
    let before = h.get_by_label("推移").bounds();
    h.get_by_label("折りたたむ").click();
    h.run();
    h.get_by_label("開く");
    let after = h.get_by_label("推移").bounds();
    assert!(
        after.center().y > before.center().y,
        "推移を折りたたむと中段が下へ広がり、推移の見出しが下へ移る"
    );
}

#[test]
fn dashboard_renders_in_light_and_dark() {
    use claude_usage_monitor::controllers::gui::app::Action;
    use claude_usage_monitor::models::domain::settings::Theme;
    let env = gui_env(now());
    for t in [Theme::Light, Theme::Dark] {
        let mut h = env.gpui_harness();
        h.with_state_mut(|c| c.handle(Action::SetTheme(t)));
        h.run();
        assert_eq!(h.with_state(|c| c.vm().theme), t);
        for label in ["ダッシュボード", "レート制限", "セッション", "推移"] {
            h.get_by_label(label);
        }
        h.get_by_label_contains("デーモン停止中");
    }
}

#[test]
fn empty_db_shows_not_fetched_and_daemon_stopped() {
    let env = gui_env(now());
    env.store.ensure_default().unwrap();
    let h = env.gpui_harness();
    h.get_by_label_contains("まだ取得していません");
    h.get_by_label_contains("デーモン停止中");
    h.get_by_label("稼働中の実行はありません");
}

use claude_usage_monitor::controllers::daemon::ingest::Ingestor;
use claude_usage_monitor::models::domain::pricing::TokenUsage;
use claude_usage_monitor::models::domain::records::{SessionUpsert, TurnRecord};
use claude_usage_monitor::models::domain::transcript::SessionKind;
use claude_usage_monitor::models::gateways::process::SysProcessInfo;
use claude_usage_monitor::models::ports::IngestRepo;
use std::sync::Arc;

fn ingest(env: &common::gui::GuiEnv) {
    let p = env.store.ensure_default().unwrap();
    let ing = Ingestor::new(
        env.store.clone(),
        env.store.clone(),
        Arc::new(SysProcessInfo::new()),
        env.clock.clone(),
        chrono::Duration::days(90),
    );
    let cfg = env.home.path().join(".claude");
    ing.scan_all(&p, &cfg).unwrap();
    ing.ingest_live_state(&p, &cfg).unwrap();
}

fn append(path: &std::path::Path, line: &str) {
    use std::io::Write;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(f, "{line}").unwrap();
}

fn headless_line(ts: chrono::DateTime<Utc>) -> String {
    format!(
        r#"{{"type":"assistant","entrypoint":"sdk-cli","sessionId":"hl1","cwd":"/work/app","timestamp":"{}","message":{{"id":"m1","model":"claude-opus-5-5","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"cargo test"}}}}],"usage":{{"input_tokens":1000,"output_tokens":10}}}}}}"#,
        claude_usage_monitor::models::repositories::db::ts(ts)
    )
}

#[test]
fn headless_run_appears_then_leaves_after_threshold() {
    let env = gui_env(now());
    append(
        &env.home.path().join(".claude/projects/-work-app/hl1.jsonl"),
        &headless_line(now() - chrono::Duration::seconds(5)),
    );
    ingest(&env);
    let mut h = env.gpui_harness();
    h.get_by_label("ヘッドレス");
    h.get_by_label("ツール実行中");
    h.get_by_label("稼働中 1");
    env.advance(chrono::Duration::seconds(130));
    h.with_state_mut(|c| c.refresh());
    h.run();
    assert!(h.query_by_label("ヘッドレス").is_none());
    h.get_by_label("稼働中 0");
}

#[test]
fn job_shows_badge_and_progress() {
    let env = gui_env(now());
    let job = env.home.path().join(".claude/jobs/j1/state.json");
    std::fs::create_dir_all(job.parent().unwrap()).unwrap();
    std::fs::write(
        &job,
        format!(
            r#"{{"state":"working","detail":"3/8件目","sessionId":"js1","name":"夜間ジョブ","cwd":"/work/app","inFlight":{{"tasks":2}},"createdAt":"{0}","updatedAt":"{0}"}}"#,
            claude_usage_monitor::models::repositories::db::ts(now() - chrono::Duration::minutes(1))
        ),
    )
    .unwrap();
    ingest(&env);
    let h = env.gpui_harness();
    h.get_by_label("ジョブ");
    h.get_by_label_contains("3/8件目 / 実行中タスク 2");
}

#[test]
fn header_shows_today_and_week_totals() {
    use claude_usage_monitor::models::domain::pricing::seed_models;
    use claude_usage_monitor::models::ports::ModelRepo;
    let env = gui_env(now());
    env.store.seed_if_empty(&seed_models()).unwrap();
    append(
        &env.home.path().join(".claude/projects/-work-app/hl1.jsonl"),
        &headless_line(now() - chrono::Duration::hours(1)),
    );
    ingest(&env);
    let h = env.gpui_harness();
    h.get_by_label("今日");
    h.get_by_label("今週");
    assert_eq!(h.get_all_by_label("1.01k <$0.01").count(), 2);
}

fn seed_session_with_turns(
    env: &common::gui::GuiEnv,
    sid: &str,
    name: &str,
    kind: SessionKind,
    status: Option<&str>,
) {
    let p = env.store.ensure_default().unwrap();
    env.store
        .upsert_session(&SessionUpsert {
            session_id: sid.into(),
            profile_id: p.id,
            kind,
            entrypoint: None,
            cwd: Some("/work/app".into()),
            git_branch: Some("main".into()),
            name: Some(name.into()),
            first_prompt: None,
            started_at: now() - chrono::Duration::minutes(10),
            last_activity_at: now() - chrono::Duration::minutes(5),
            status: status.map(str::to_string),
        })
        .unwrap();
    for (i, (kind, summary)) in [
        ("prompt", "設計を見直して"),
        ("tool_use", "Read: src/main.rs"),
        ("text", "見直し案です"),
    ]
    .iter()
    .enumerate()
    {
        env.store
            .upsert_turn(&TurnRecord {
                session_id: sid.into(),
                agent_id: String::new(),
                message_id: format!("{sid}-m{i}"),
                ts: now() - chrono::Duration::minutes(8 - i as i64),
                model: (i > 0).then(|| "claude-opus-5-5".to_string()),
                kind: kind.to_string(),
                summary: summary.to_string(),
                usage: TokenUsage {
                    input: 1200 * i as u64,
                    output: 10,
                    ..TokenUsage::default()
                },
            })
            .unwrap();
    }
}

#[test]
fn history_search_and_kind_filter_keep_cards_and_trend() {
    use claude_usage_monitor::views::probe::Role;
    let env = gui_env(now());
    seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
    seed_session_with_turns(&env, "s2", "夜間の集計", SessionKind::Headless, None);
    let mut h = env.gpui_harness();
    h.get_by_label("履歴").click();
    h.run();
    h.get_by_label("設計の相談");
    h.get_by_role_and_label(Role::TextInput, "検索").click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "検索")
        .type_text("夜間");
    h.run();
    h.get_by_label("夜間の集計");
    assert!(h.query_by_label("設計の相談").is_none());
    h.get_by_label("対話").click();
    h.run();
    assert!(h.query_by_label("夜間の集計").is_none());
    h.get_by_label("レート制限");
    h.get_by_label("推移");
}

#[test]
fn controller_clearing_forms_clears_input() {
    use claude_usage_monitor::views::probe::Role;
    let env = gui_env(now());
    let mut h = env.gpui_harness();
    h.get_by_label("履歴").click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "検索")
        .type_text("夜間");
    h.run();
    assert_eq!(h.with_state(|c| c.forms().session_query.clone()), "夜間");
    assert_eq!(h.input_value(0), "夜間");
    h.with_state_mut(|c| c.forms_mut().session_query.clear());
    h.run();
    assert_eq!(h.input_value(0), "", "controllerが空にした値が入力欄に戻る");
}

fn seed_live_session(env: &common::gui::GuiEnv) -> std::path::PathBuf {
    use claude_usage_monitor::models::domain::records::SubagentRecord;
    seed_session_with_turns(
        env,
        "s1",
        "設計の相談",
        SessionKind::Interactive,
        Some("busy"),
    );
    env.store
        .upsert_subagent(&SubagentRecord {
            agent_id: "a1".into(),
            session_id: "s1".into(),
            agent_type: Some("go-reviewer".into()),
            description: Some("メモリ機能のレビュー".into()),
            parent_tool_use_id: Some("toolu_1".into()),
            spawn_depth: Some(1),
        })
        .unwrap();
    let proj = env.home.path().join(".claude/projects/-work-app");
    append(
        &proj.join("s1.jsonl"),
        r#"{"type":"assistant","timestamp":"2026-09-26T02:59:00.000Z","sessionId":"s1","message":{"id":"m9","content":[{"type":"tool_use","id":"toolu_1","name":"Agent","input":{}}]}}"#,
    );
    proj
}

mod memory_rows_e2e {
    use super::*;
    use claude_usage_monitor::controllers::gui::app::TabVm;
    use claude_usage_monitor::models::domain::memory::{ProcEntry, Target, Victim};
    use std::path::PathBuf;

    const MIB: u64 = 1 << 20;
    const TERM: &str = "/Users/u/.local/bin/claude";

    fn proc(pid: u32, parent: u32, mib: u64, start: i64, exe: &str) -> ProcEntry {
        ProcEntry {
            pid,
            parent: Some(parent),
            rss: mib * MIB,
            start_time: start as u64,
            exe: Some(PathBuf::from(exe)),
        }
    }

    fn live_file(env: &common::gui::GuiEnv, pid: u32, sid: &str, started: i64) {
        let dir = env.home.path().join(".claude/sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{pid}.json")),
            format!(
                r#"{{"pid":{pid},"sessionId":"{sid}","startedAt":{}}}"#,
                started * 1000
            ),
        )
        .unwrap();
    }

    fn start() -> i64 {
        (now() - chrono::Duration::minutes(10)).timestamp()
    }

    fn setup() -> common::gui::GuiEnv {
        let env = gui_env(now());
        seed_live_session(&env);
        live_file(&env, 4242, "s1", start() + 2);
        *env.procs.procs.lock().unwrap() = vec![
            proc(4242, 1, 400, start(), TERM),
            proc(4243, 4242, 100, start(), "/bin/zsh"),
        ];
        env
    }

    fn victim() -> Victim {
        Victim {
            pid: 4242,
            start_time: start() as u64,
            target: Target::TerminalSession,
        }
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

    fn open_exit_dialog(h: &mut GpuiHarness) {
        h.get_by_label("プロセスを終了").click();
        h.run();
    }

    #[test]
    fn row_shows_memory_and_exit_button_requests_exit_after_confirm() {
        let env = setup();
        let mut h = env.gpui_harness();
        h.get_by_label_contains("500MB");
        open_exit_dialog(&mut h);
        h.get_by_label_contains("実行中の編集が失われることがあります");
        confirm_exit(&mut h);
        assert_eq!(*env.procs.exits.lock().unwrap(), [victim()]);
        assert!(notice(&mut h).unwrap().contains("claude --resume s1"));
        assert!(h.query_by_label("終了する").is_none(), "ダイアログが閉じる");
    }

    #[test]
    fn cancel_from_row_does_not_request_exit() {
        let env = setup();
        let mut h = env.gpui_harness();
        open_exit_dialog(&mut h);
        h.get_by_label("やめる").click();
        h.run();
        assert!(env.procs.exits.lock().unwrap().is_empty());
        assert!(h.query_by_label("終了する").is_none());
    }

    #[test]
    fn refresh_while_dialog_is_open_keeps_the_same_victim() {
        let env = setup();
        let mut h = env.gpui_harness();
        open_exit_dialog(&mut h);
        seed_session_with_turns(
            &env,
            "s0",
            "新しい相談",
            SessionKind::Interactive,
            Some("busy"),
        );
        live_file(&env, 5000, "s0", start() + 1);
        env.procs
            .procs
            .lock()
            .unwrap()
            .insert(0, proc(5000, 1, 50, start(), TERM));
        env.advance(chrono::Duration::seconds(6));
        h.run();
        h.get_by_label_contains("50MB");
        confirm_exit(&mut h);
        assert_eq!(*env.procs.exits.lock().unwrap(), [victim()]);
    }

    #[test]
    fn still_running_target_is_reported_after_grace_period() {
        let env = setup();
        let mut h = env.gpui_harness();
        open_exit_dialog(&mut h);
        confirm_exit(&mut h);
        env.advance(chrono::Duration::seconds(6));
        h.run();
        assert_eq!(
            notice(&mut h).as_deref(),
            Some("設計の相談 に終了を要求しましたが、まだ動いています")
        );
    }

    #[test]
    fn subagent_row_has_no_exit_button() {
        let env = setup();
        env.store
            .upsert_turn(&TurnRecord {
                session_id: "s1".into(),
                agent_id: "a1".into(),
                message_id: "a1-m1".into(),
                ts: now() - chrono::Duration::minutes(1),
                model: Some("claude-opus-5-5".into()),
                kind: "tool_use".into(),
                summary: "Grep: mutex".into(),
                usage: TokenUsage::default(),
            })
            .unwrap();
        let mut h = env.gpui_harness();
        let sub = "go-reviewer「メモリ機能のレビュー」";
        h.get_by_label(sub);
        assert_eq!(
            h.get_all_by_label("プロセスを終了").count(),
            1,
            "親の行だけ"
        );
        h.with_state(|c| match &c.vm().body {
            TabVm::Dashboard(d) => {
                let row = d.active.iter().find(|r| r.title == sub).unwrap();
                assert_eq!(row.depth, 1);
                assert!(row.memory.is_none());
            }
            other => panic!("{other:?}"),
        });
    }

    #[test]
    fn unbound_row_shows_dash_for_memory() {
        let env = gui_env(now());
        seed_session_with_turns(
            &env,
            "u1",
            "結び付かない相談",
            SessionKind::Interactive,
            Some("busy"),
        );
        let h = env.gpui_harness();
        h.get_by_label_contains("· —");
    }

    #[test]
    fn code_tab_session_has_desktop_badge() {
        let env = gui_env(now());
        seed_session_with_turns(
            &env,
            "c1",
            "Codeタブの相談",
            SessionKind::Interactive,
            Some("busy"),
        );
        live_file(&env, 7000, "c1", start());
        *env.procs.procs.lock().unwrap() = vec![proc(
            7000,
            1,
            300,
            start(),
            "/U/Library/Application Support/Claude/claude-code/2.1.284/claude.app/Contents/MacOS/claude",
        )];
        let mut h = env.gpui_harness();
        h.get_by_label("Desktop");
        h.with_state(|c| match &c.vm().body {
            TabVm::Dashboard(d) => assert!(d.active[0].memory.as_ref().unwrap().desktop),
            other => panic!("{other:?}"),
        });
    }
}

fn live_depth(h: &mut GpuiHarness) -> u8 {
    use claude_usage_monitor::controllers::gui::app::TabVm;
    h.with_state(|c| match &c.vm().body {
        TabVm::Dashboard(d) => {
            d.detail
                .as_ref()
                .unwrap()
                .live
                .as_ref()
                .unwrap()
                .lines
                .last()
                .unwrap()
                .depth
        }
        other => panic!("{other:?}"),
    })
}

#[test]
fn active_session_opens_live_log_and_streams_subagent_lines() {
    let env = gui_env(now());
    let proj = seed_live_session(&env);
    let mut h = env.gpui_harness();
    h.get_by_label("設計の相談").click();
    h.run();
    h.get_by_label_contains("go-reviewer「メモリ機能のレビュー」を起動");
    h.get_by_label("レート制限");
    append(
        &proj.join("s1/subagents/agent-a1.jsonl"),
        r#"{"type":"assistant","timestamp":"2026-09-26T02:59:30.000Z","sessionId":"s1","message":{"id":"s9","content":[{"type":"tool_use","id":"t9","name":"Grep","input":{"pattern":"mutex"}}]}}"#,
    );
    h.run();
    h.get_by_label_contains("mutex");
    assert_eq!(live_depth(&mut h), 1);
}

#[test]
fn live_log_stays_at_bottom_while_appending() {
    let env = gui_env(now());
    let proj = seed_live_session(&env);
    let mut h = env.gpui_harness();
    h.get_by_label("設計の相談").click();
    h.run();
    let line = |i: usize| {
        format!(
            r#"{{"type":"assistant","timestamp":"2026-09-26T02:59:{:02}.000Z","sessionId":"s1","message":{{"id":"x{i}","content":[{{"type":"tool_use","id":"tx{i}","name":"Grep","input":{{"pattern":"needle{i}"}}}}]}}}}"#,
            i % 60
        )
    };
    for i in 0..60 {
        append(&proj.join("s1.jsonl"), &line(i));
    }
    h.run();
    h.run();
    let visible = |h: &GpuiHarness, needle: &str| {
        let line = h.get_by_label_contains(needle).bounds();
        let trend = h.get_by_label("推移").bounds();
        line.bottom() <= trend.top()
    };
    assert!(visible(&h, "needle59"), "開いた直後は最後の行が見える");
    append(&proj.join("s1.jsonl"), &line(60));
    h.run();
    h.run();
    assert!(visible(&h, "needle60"), "追記しても下端のまま");
    assert!(
        h.query_by_label("最新へ戻る").is_none(),
        "下端にいる間は出さない"
    );
    let at = h.get_by_label_contains("needle60").bounds().center();
    h.scroll_at(f32::from(at.x), f32::from(at.y), 2000.0);
    h.run();
    assert!(!visible(&h, "needle60"), "上へ戻すと最後の行は見えない");
    h.get_by_label("最新へ戻る").click();
    h.run();
    h.run();
    assert!(visible(&h, "needle60"), "最新へ戻るで最後の行へ戻る");
    assert!(h.query_by_label("最新へ戻る").is_none());
}

#[test]
fn job_timeline_appears_in_live_log() {
    let env = gui_env(now());
    seed_live_session(&env);
    let p = env.store.ensure_default().unwrap();
    env.store
        .upsert_job(&claude_usage_monitor::models::domain::records::JobRecord {
            job_id: "j1".into(),
            profile_id: p.id,
            session_id: Some("s1".into()),
            name: None,
            state: "working".into(),
            detail: Some("2/8件目".into()),
            in_flight_tasks: 1,
            tokens: None,
            created_at: None,
            updated_at: now(),
        })
        .unwrap();
    append(
        &env.home.path().join(".claude/jobs/j1/timeline.jsonl"),
        r#"{"at":"2026-09-26T02:59:40.000Z","state":"working","detail":"2/8件目"}"#,
    );
    let mut h = env.gpui_harness();
    h.get_by_label("設計の相談").click();
    h.run();
    h.get_by_label_contains("ジョブ: working（2/8件目）");
}

#[test]
fn history_row_opens_turns_beside_the_list() {
    let env = gui_env(now());
    seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
    let mut h = env.gpui_harness();
    h.get_by_label("履歴").click();
    h.run();
    h.get_by_label("設計の相談").click();
    h.run();
    h.get_by_label("Read: src/main.rs");
    h.get_by_label("レート制限");
    h.get_by_label("推移");
    h.get_all_by_label("内訳").nth(1).unwrap().click();
    h.run();
    h.get_by_label_contains("入力 1.20k / 出力 10");
}

#[test]
fn replay_next_button_changes_displayed_turn() {
    let env = gui_env(now());
    seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
    let mut h = env.gpui_harness();
    h.get_by_label("履歴").click();
    h.run();
    h.get_by_label("設計の相談").click();
    h.run();
    h.get_by_label("再生").click();
    h.run();
    h.get_by_label_contains("1 / 3");
    assert_eq!(h.replay_slider_value(), Some(0.0));
    h.get_by_label_contains("次のターン").click();
    h.run();
    h.get_by_label_contains("2 / 3");
    assert_eq!(
        h.replay_slider_value(),
        Some(1.0),
        "つまみも次のターンへ動く"
    );
    h.get_by_label_contains("現在: Read: src/main.rs");
    h.get_by_label("推移");
}

#[test]
fn replay_slider_is_named_and_seeks() {
    use claude_usage_monitor::views::probe::Role;
    let env = gui_env(now());
    seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
    let mut h = env.gpui_harness();
    h.get_by_label("履歴").click();
    h.run();
    h.get_by_label("設計の相談").click();
    h.run();
    h.get_by_label("再生").click();
    h.run();
    let s = h.get_by_role_and_label(Role::Slider, "再生位置").bounds();
    h.click_at(f32::from(s.right()) - 2.0, f32::from(s.center().y));
    h.run();
    h.get_by_label_contains("3 / 3");
}

#[test]
fn detail_tabs_show_summary_and_live_filters() {
    let env = gui_env(now());
    seed_live_session(&env);
    let mut h = env.gpui_harness();
    h.get_by_label("設計の相談").click();
    h.run();
    for label in ["ツール", "本文", "エラーのみ", "発生元"] {
        h.get_by_label(label);
    }
    h.get_by_label_contains("行を保持（最大2,000行）");
    h.get_by_label("概要").click();
    h.run();
    h.get_by_label("要約");
    h.get_by_label("依頼の一覧");
    // モデルの表は行の順に読む。見出しの行がすべて先に来る。
    let labels = h.labels();
    let at = |s: &str| labels.iter().position(|l| *l == s).unwrap();
    assert!(at("claude-opus-5-5") > at("要求"), "{labels:?}");
    for label in [
        "入力",
        "出力",
        "キャッシュ読込",
        "コスト",
        "キャッシュヒット率",
    ] {
        h.get_by_label(label);
    }
}

#[test]
fn live_source_combo_opens_filters_and_closes() {
    use claude_usage_monitor::views::probe::Role;
    let env = gui_env(now());
    seed_live_session(&env);
    let mut h = env.gpui_harness();
    h.get_by_label("設計の相談").click();
    h.run();
    assert!(
        h.query_by_label("すべて").is_none(),
        "閉じているときは候補を出さない"
    );
    h.get_by_role_and_label(Role::ComboBox, "発生元").click();
    h.run();
    h.get_by_label("すべて");
    h.get_by_role_and_label(Role::Tab, "本体").click();
    h.run();
    assert_eq!(
        h.with_state(|c| c.forms().live_filter.source.clone()),
        Some("本体".to_string())
    );
    assert!(h.query_by_label("すべて").is_none(), "選ぶと閉じる");
    h.get_by_role_and_label(Role::ComboBox, "発生元").click();
    h.run();
    h.get_by_label("すべて");
    h.click_at(5.0, 400.0);
    h.run();
    assert!(h.query_by_label("すべて").is_none(), "外を押すと閉じる");
}

mod calendar_e2e {
    use super::*;

    fn open_calendar(env: &common::gui::GuiEnv) -> GpuiHarness {
        let mut h = env.gpui_harness();
        h.get_by_label("カレンダー").click();
        h.run();
        assert_eq!(h.with_state(|c| c.vm().tab), Tab::Calendar);
        h
    }

    #[test]
    fn calendar_tab_shows_week_legend_and_bands() {
        let env = gui_env(now());
        seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
        let h = open_calendar(&env);
        h.get_by_label("2026年9/20〜9/26");
        h.get_by_label("09/20(日)");
        h.get_by_label("09/26(土)");
        h.get_by_label("1 セッション");
        h.get_by_label("app 1");
        h.get_by_label_contains("設計の相談");
        h.get_by_label("帯を押すと詳細を出します");
    }

    #[test]
    fn calendar_color_by_and_week_navigation() {
        let env = gui_env(now());
        seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
        let mut h = open_calendar(&env);
        h.get_by_label("プロファイル別").click();
        h.run();
        h.get_by_label("default 1");
        assert!(h.query_by_label("app 1").is_none());
        h.get_by_label_contains("前週").click();
        h.run();
        h.get_by_label("2026年9/13〜9/19");
        h.get_by_label("この週の記録はありません");
        h.get_by_label("今週へ").click();
        h.run();
        h.get_by_label("2026年9/20〜9/26");
    }

    #[test]
    fn next_week_is_disabled_on_this_week() {
        let env = gui_env(now());
        let mut h = open_calendar(&env);
        h.get_by_label_contains("次週").click();
        h.run();
        h.get_by_label("2026年9/20〜9/26");
    }

    #[test]
    fn headless_runs_are_hidden_until_checked() {
        let env = gui_env(now());
        seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
        seed_session_with_turns(&env, "h1", "一括の要約", SessionKind::Headless, None);
        let mut h = open_calendar(&env);
        assert!(h.query_by_label_contains("一括の要約").is_none());
        h.get_by_label("1 セッション");
        h.get_by_label("ヘッドレスも出す（1件）").click();
        h.run();
        h.get_by_label_contains("一括の要約");
        h.get_by_label("2 セッション");
    }

    #[test]
    fn calendar_band_opens_detail_without_touching_dashboard_selection() {
        let env = gui_env(now());
        seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
        let mut h = open_calendar(&env);
        h.get_by_label_contains("設計の相談").click();
        h.run();
        h.get_by_label("ターン").click();
        h.run();
        h.get_by_label("Read: src/main.rs");
        assert_eq!(
            h.with_state(|c| c.calendar_selected().map(str::to_string)),
            Some("s1".to_string())
        );
        h.get_by_label_contains("のターン");
        h.get_by_label("ダッシュボード").click();
        h.run();
        assert_eq!(
            h.with_state(|c| c.dash_selected().map(str::to_string)),
            None
        );
        h.get_by_label("左の一覧からセッションを選んでください");
        h.get_by_label("カレンダー").click();
        h.run();
        h.get_by_label("Read: src/main.rs");
    }

    #[test]
    fn calendar_detail_replay_has_its_own_slider() {
        let env = gui_env(now());
        seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
        let mut h = env.gpui_harness();
        h.get_by_label("履歴").click();
        h.run();
        h.get_by_label("設計の相談").click();
        h.run();
        h.get_by_label("再生").click();
        h.run();
        h.get_by_label_contains("次のターン").click();
        h.run();
        assert_eq!(h.replay_slider_value(), Some(1.0));
        h.get_by_label("カレンダー").click();
        h.run();
        h.get_by_label_contains("設計の相談").click();
        h.run();
        h.get_by_label("再生").click();
        h.run();
        h.get_by_label_contains("1 / 3");
        assert_eq!(
            h.replay_slider_value(),
            Some(0.0),
            "カレンダーの詳細のつまみは、ダッシュボードの位置を持ち込まない"
        );
        h.get_by_label_contains("次のターン").click();
        h.run();
        h.get_by_label_contains("2 / 3");
        assert_eq!(
            h.replay_slider_value(),
            Some(1.0),
            "つまみも次のターンへ動く"
        );
    }

    #[test]
    fn calendar_band_opens_summary_with_recap() {
        use claude_usage_monitor::models::domain::records::NoteRecord;
        use claude_usage_monitor::models::domain::transcript::NoteKind;
        let env = gui_env(now());
        seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
        seed_session_with_turns(&env, "s2", "別の作業", SessionKind::Interactive, None);
        env.store
            .upsert_note(&NoteRecord {
                uuid: "n1".into(),
                session_id: "s1".into(),
                ts: now(),
                kind: NoteKind::Recap,
                text: "トップページを速くしました".into(),
            })
            .unwrap();
        for id in ["s1", "s2"] {
            env.store.set_ai_title(id, &format!("{id}の題名")).unwrap();
        }
        let mut h = open_calendar(&env);
        h.get_by_label_contains("設計の相談").click();
        h.run();
        h.get_by_label("要約");
        h.get_by_label_contains("トップページを速くしました");
        h.get_by_label("依頼");
        h.get_by_label("依頼の一覧");
        h.get_by_label("ツール名");
        assert!(h.query_by_label("s1の題名").is_none());
        h.get_by_label_contains("別の作業").click();
        h.run();
        h.get_by_label_contains("離席中の要約はありません");
        h.get_by_label("s2の題名");
    }

    #[test]
    fn moving_week_clears_calendar_detail() {
        let env = gui_env(now());
        seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
        let mut h = open_calendar(&env);
        h.get_by_label_contains("設計の相談").click();
        h.run();
        h.get_by_label("ターン").click();
        h.run();
        h.get_by_label("Read: src/main.rs");
        h.get_by_label_contains("前週").click();
        h.run();
        h.get_by_label("帯を押すと詳細を出します");
        assert_eq!(
            h.with_state(|c| c.calendar_selected().map(str::to_string)),
            None
        );
    }

    #[test]
    fn headless_only_week_is_not_called_empty() {
        let env = gui_env(now());
        seed_session_with_turns(&env, "h1", "一括の要約", SessionKind::Headless, None);
        let h = open_calendar(&env);
        h.get_by_label("0 セッション");
        h.get_by_label("ヘッドレスも出す（1件）");
        assert!(h.query_by_label("この週の記録はありません").is_none());
    }

    #[test]
    fn calendar_renders_in_light_and_dark() {
        use claude_usage_monitor::controllers::gui::app::Action;
        use claude_usage_monitor::models::domain::settings::Theme;
        let env = gui_env(now());
        seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
        for t in [Theme::Light, Theme::Dark] {
            let mut h = env.gpui_harness();
            h.with_state_mut(|c| c.handle(Action::SetTheme(t)));
            h.get_by_label("カレンダー").click();
            h.run();
            assert_eq!(h.with_state(|c| c.vm().theme), t);
            h.get_by_label_contains("設計の相談");
            h.get_by_label("2026年9/20〜9/26");
        }
    }

    #[test]
    fn calendar_splitter_moves_only_the_calendar_ratio() {
        let env = gui_env(now());
        let mut h = open_calendar(&env);
        let (dash, cal) =
            h.with_state_mut(|c| (c.forms_mut().split_ratio, c.forms_mut().calendar_split));
        h.get_by_label("左右の境界").drag_by(-80.0);
        h.run();
        let (dash2, cal2) =
            h.with_state_mut(|c| (c.forms_mut().split_ratio, c.forms_mut().calendar_split));
        assert_eq!(dash2, dash, "ダッシュボードの比率は変えない");
        assert!(cal2 < cal, "{cal} -> {cal2}");
    }
}

mod analytics_profiles_e2e {
    use super::*;
    use claude_usage_monitor::views::probe::Role;

    #[test]
    fn analytics_tab_renders_groups_and_switches_period() {
        let env = gui_env(now());
        record_rising_usage(&env);
        seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
        let mut h = env.gpui_harness();
        h.get_by_label("分析").click();
        h.run();
        for t in [
            "プロジェクト別",
            "モデル別",
            "ブランチ別",
            "キャッシュヒット率（日ごと）",
            "ツール",
        ] {
            h.get_by_label(t);
        }
        h.get_by_label("/work/app");
        h.get_by_label("30日").click();
        h.run();
        assert!(h.with_state(|c| matches!(
            &c.vm().body,
            TabVm::Analytics(a) if a.period == claude_usage_monitor::controllers::gui::tabs::analytics::Period::Days30
        )));
    }

    #[test]
    fn analytics_empty_period_says_so() {
        let env = gui_env(now());
        let mut h = env.gpui_harness();
        h.get_by_label("分析").click();
        h.run();
        assert_eq!(
            h.get_all_by_label("この期間の記録はありません").count(),
            3,
            "3つの軸のカードがそれぞれ空と出す"
        );
    }

    #[cfg(unix)]
    #[test]
    fn profile_added_in_tab_and_activated_from_card_is_used_by_cumon_run() {
        let env = gui_env(now());
        env.store.ensure_default().unwrap();
        let dir = env.home.path().join("work-config");
        let mut h = env.gpui_harness();
        h.get_by_label("プロファイル").click();
        h.run();
        h.get_by_role_and_label(Role::TextInput, "プロファイル名")
            .type_text("work");
        h.run();
        h.get_by_role_and_label(Role::TextInput, "設定ディレクトリ")
            .type_text(dir.to_str().unwrap());
        h.run();
        h.get_by_label("追加").click();
        h.run();
        assert_eq!(h.input_value(1), "", "追加した後は名前の欄を空に戻す");
        assert_eq!(
            h.input_value(2),
            "",
            "追加した後はディレクトリの欄を空に戻す"
        );
        h.get_by_label("ダッシュボード").click();
        h.run();
        h.get_by_label("使用中にする").click();
        h.run();
        h.get_by_label_contains("work に切り替えました");
        assert!(h.with_state(|c| matches!(&c.vm().body,
            TabVm::Dashboard(d) if d.cards.iter().any(|k| k.name == "work" && k.is_active))));
        let bin = tempfile::tempdir().unwrap();
        common::fake_claude(bin.path());
        common::cumon(env.data.path(), env.home.path(), Some(bin.path()))
            .args(["run", "-p", "hi"])
            .assert()
            .success()
            .stdout(predicates::str::contains(format!(
                "CONFIG={} ARGS=-p hi",
                dir.display()
            )));
    }

    #[test]
    fn profile_remove_asks_before_deleting() {
        let env = gui_env(now());
        env.store.ensure_default().unwrap();
        let mut h = env.gpui_harness();
        h.get_by_label("プロファイル").click();
        h.run();
        h.get_by_role_and_label(Role::TextInput, "プロファイル名")
            .type_text("tmp");
        h.run();
        h.get_by_label("追加").click();
        h.run();
        h.get_by_label("削除").click();
        h.run();
        h.get_by_label_contains("tmp を削除すると");
        h.get_by_label("やめる").click();
        h.run();
        assert!(h.query_by_label_contains("tmp を削除すると").is_none());
        h.get_by_label("tmp");
        h.get_by_label("削除").click();
        h.run();
        h.get_by_label("本当に削除する").click();
        h.run();
        assert!(h.query_by_label("tmp").is_none(), "確認の後に消える");
    }
}

mod settings_e2e {
    use super::*;
    use claude_usage_monitor::models::ports::{Autostart, CatalogRefresh, RepoError, SettingsRepo};
    use claude_usage_monitor::views::probe::Role;
    use std::sync::Mutex;
    use std::sync::mpsc::{Receiver, Sender, channel};

    fn open_settings(h: &mut GpuiHarness) {
        h.get_by_label("設定").click();
        h.run();
    }

    #[test]
    fn settings_threshold_is_saved_for_daemon() {
        let env = gui_env(now());
        let mut h = env.gpui_harness();
        open_settings(&mut h);
        h.get_by_role_and_label(Role::TextInput, "通知閾値（%）");
        h.with_state_mut(|c| c.forms_mut().settings.threshold = "95".into());
        h.run();
        assert_eq!(h.input_value(4), "95", "controllerが書いた値を入力欄へ戻す");
        h.get_by_label("保存").click();
        h.run();
        assert_eq!(env.store.load().unwrap().notify_threshold_percent, 95.0);
        h.get_by_label_contains("5分以内");
        h.get_by_label("診断").click();
        h.run();
        h.get_by_label("JSONLの取り込み");
    }

    #[test]
    fn toggling_autostart_registers_it() {
        let env = gui_env(now());
        let mut h = env.gpui_harness();
        open_settings(&mut h);
        h.get_by_label("ログイン時にデーモンを起動する").click();
        h.run();
        h.get_by_label("ログイン時にデーモンを起動するよう登録しました");
        assert!(h.with_state(
            |c| matches!(&c.vm().body, TabVm::Settings(s) if s.autostart == Some(true))
        ));
    }

    struct Unreadable;
    impl Autostart for Unreadable {
        fn is_enabled(&self) -> Result<bool, String> {
            Err("読めない".into())
        }
        fn enable(&self) -> Result<(), String> {
            Err("読めない".into())
        }
        fn disable(&self) -> Result<(), String> {
            Err("読めない".into())
        }
    }

    #[test]
    fn unreadable_autostart_shows_reason_instead_of_checkbox() {
        let env = gui_env(now());
        let mut deps = env.deps();
        deps.autostart = Arc::new(Unreadable);
        let mut h = env.gpui_harness_with(deps);
        open_settings(&mut h);
        h.get_by_label("ログイン時の起動の状態を読めません");
        assert!(h.query_by_label("ログイン時にデーモンを起動する").is_none());
    }

    /// 合図があるまで返らない取得。取得中の画面を確かめるため。
    struct Held(Mutex<Receiver<()>>);
    impl CatalogRefresh for Held {
        fn refresh(&self) -> Result<usize, RepoError> {
            let _ = self.0.lock().unwrap().recv();
            Ok(0)
        }
    }

    #[test]
    fn refreshing_replaces_the_button_until_done() {
        let env = gui_env(now());
        let (tx, rx): (Sender<()>, Receiver<()>) = channel();
        let mut deps = env.deps();
        deps.catalog = Arc::new(Held(Mutex::new(rx)));
        let mut h = env.gpui_harness_with(deps);
        open_settings(&mut h);
        h.get_by_label("今すぐ更新").click();
        h.run();
        h.get_by_label("取得中…");
        assert!(h.query_by_label("今すぐ更新").is_none());
        tx.send(()).unwrap();
    }

    #[test]
    fn model_editor_saves_or_cancels() {
        let env = gui_env(now());
        // 編集の欄はモデルの一覧の下に開き、既定の高さでは窓の外に出るので、窓を縦に広げる。
        let mut h = env.gpui_harness_sized(1280.0, 1100.0);
        open_settings(&mut h);
        h.get_all_by_label("編集")
            .next()
            .expect("モデルの行")
            .click();
        h.run();
        let prefix = h
            .with_state(|c| c.forms().model.as_ref().map(|m| m.prefix.clone()))
            .expect("編集中");
        h.get_by_label(&format!("{prefix} を編集"));
        h.get_by_role_and_label(Role::TextInput, "入力")
            .type_text("9");
        h.run();
        assert!(h.with_state(|c| c.forms().model.as_ref().unwrap().input.ends_with('9')));
        h.get_by_label("やめる").click();
        h.run();
        assert!(h.query_by_label(&format!("{prefix} を編集")).is_none());
        h.get_all_by_label("編集").next().unwrap().click();
        h.run();
        h.get_by_label("単価を保存").click();
        h.run();
        assert!(
            h.with_state(|c| c.forms().model.is_none()),
            "保存すると閉じる"
        );
    }

    #[test]
    fn theme_choice_is_a_draft_until_saved() {
        let env = gui_env(now());
        let mut h = env.gpui_harness();
        open_settings(&mut h);
        h.get_by_label("ダーク").click();
        h.run();
        assert_eq!(
            h.with_state(|c| c.forms().settings.theme),
            claude_usage_monitor::models::domain::settings::Theme::Dark
        );
        assert_ne!(
            env.store.load().unwrap().theme,
            claude_usage_monitor::models::domain::settings::Theme::Dark,
            "保存するまで書かない"
        );
        h.get_by_label("保存").click();
        h.run();
        assert_eq!(
            env.store.load().unwrap().theme,
            claude_usage_monitor::models::domain::settings::Theme::Dark
        );
    }

    #[test]
    fn links_are_recorded_as_links() {
        let env = gui_env(now());
        let mut h = env.gpui_harness();
        open_settings(&mut h);
        h.get_by_role_and_label(Role::Link, "料金ページ");
        h.get_by_role_and_label(Role::Link, "モデル一覧");
    }
}
