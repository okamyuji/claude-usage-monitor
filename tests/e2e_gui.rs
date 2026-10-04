//! GUIの主要導線（spec 12.2節）。一時ディレクトリの実SQLiteを使い、egui_kittestで操作する。
#![forbid(unsafe_code)]
mod common;

use chrono::{TimeZone, Utc};
use claude_usage_monitor::controllers::gui::app::Tab;
use claude_usage_monitor::controllers::gui::dashboard::ListMode;
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
fn help_icons_on_dashboard_show_their_text_on_hover() {
    let env = gui_env(now());
    let mut h = env.harness();
    let n = h.get_all_by_label("ⓘ").count();
    assert!(n > 0);
    let labels = |h: &egui_kittest::Harness<'_, _>| {
        h.query_all_by_role(egui::accesskit::Role::Label).count()
    };
    let mut missing = Vec::new();
    for i in 0..n {
        h.get_all_by_label("ⓘ").nth(i).unwrap().hover();
        h.step();
        let before = labels(&h);
        for _ in 0..60 {
            h.step();
        }
        if labels(&h) <= before {
            missing.push(i);
        }
    }
    assert!(missing.is_empty(), "説明が出ないⓘ: {missing:?} / {n}");
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
        claude_usage_monitor::views::theme::apply(&h.ctx);
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

use claude_usage_monitor::models::domain::records::{FetchLogEntry, FetchResult};
use claude_usage_monitor::models::domain::usage::parse_usage;
use claude_usage_monitor::models::ports::{FetchLogRepo, ProfileRepo, UsageRepo};

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

#[test]
fn cards_show_limits_remaining_and_projection() {
    let env = gui_env(now());
    record_rising_usage(&env);
    let h = env.harness();
    h.get_by_label("5時間枠");
    h.get_by_label("30%");
    h.get_by_label_contains("残り2時間（14:00）");
    h.get_by_label_contains("このペースだと 13:10 に上限");
    h.get_by_label("週間枠（Fable）");
    h.get_by_label("使用中");
}

#[test]
fn token_expired_is_shown_on_401() {
    let env = gui_env(now());
    record_rising_usage(&env);
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
    let h = env.harness();
    h.get_by_label_contains("トークン期限切れ");
}

#[test]
fn empty_db_shows_not_fetched_and_daemon_stopped() {
    let env = gui_env(now());
    env.store.ensure_default().unwrap();
    let h = env.harness();
    h.get_by_label_contains("まだ取得していません");
    h.get_by_label_contains("デーモン停止中");
    h.get_by_label("稼働中の実行はありません");
}

use claude_usage_monitor::controllers::daemon::ingest::Ingestor;
use claude_usage_monitor::models::gateways::process::SysProcessInfo;
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
    let mut h = env.harness();
    h.get_by_label("ヘッドレス");
    h.get_by_label("ツール実行中");
    h.get_by_label("稼働中 1");
    env.advance(chrono::Duration::seconds(130));
    h.state_mut().refresh();
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
    let h = env.harness();
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
    let h = env.harness();
    h.get_by_label("今日");
    h.get_by_label("今週");
    assert_eq!(h.get_all_by_label("1.01k <$0.01").count(), 2);
}

use claude_usage_monitor::models::domain::pricing::TokenUsage;
use claude_usage_monitor::models::domain::records::{SessionUpsert, TurnRecord};
use claude_usage_monitor::models::domain::transcript::SessionKind;
use claude_usage_monitor::models::ports::IngestRepo;

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
fn history_row_opens_turns_beside_the_list() {
    let env = gui_env(now());
    seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
    let mut h = env.harness();
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
fn history_search_and_kind_filter_keep_cards_and_trend() {
    use egui::accesskit::Role;
    let env = gui_env(now());
    seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
    seed_session_with_turns(&env, "s2", "夜間の集計", SessionKind::Headless, None);
    let mut h = env.harness();
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

fn live_depth(
    h: &egui_kittest::Harness<'static, claude_usage_monitor::controllers::gui::app::GuiController>,
) -> u8 {
    match &h.state().vm().body {
        claude_usage_monitor::controllers::gui::app::TabVm::Dashboard(d) => {
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
    }
}

#[test]
fn active_session_opens_live_log_and_streams_subagent_lines() {
    let env = gui_env(now());
    let proj = seed_live_session(&env);
    let mut h = env.harness();
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
    assert_eq!(live_depth(&h), 1);
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
    let mut h = env.harness();
    h.get_by_label("設計の相談").click();
    h.run();
    h.get_by_label_contains("ジョブ: working（2/8件目）");
}

#[test]
fn replay_next_button_changes_displayed_turn() {
    let env = gui_env(now());
    seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
    let mut h = env.harness();
    h.get_by_label("履歴").click();
    h.run();
    h.get_by_label("設計の相談").click();
    h.run();
    h.get_by_label("再生").click();
    h.run();
    h.get_by_label_contains("1 / 3");
    h.get_by_label_contains("次のターン").click();
    h.run();
    h.get_by_label_contains("2 / 3");
    h.get_by_label_contains("現在: Read: src/main.rs");
    h.get_by_label("推移");
}

#[test]
fn trend_range_switches_and_analytics_tab_renders() {
    use claude_usage_monitor::controllers::gui::app::TabVm;
    use claude_usage_monitor::controllers::gui::dashboard::trend::TrendRange;
    let env = gui_env(now());
    record_rising_usage(&env);
    seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
    let mut h = env.harness();
    h.get_by_label("5時間枠のグラフ");
    h.get_by_label("24時間").click();
    h.run();
    assert!(
        matches!(&h.state().vm().body, TabVm::Dashboard(d) if d.trend.as_ref().unwrap().range == TrendRange::Hours24)
    );
    h.get_by_label("分析").click();
    h.run();
    h.get_by_label("プロジェクト別");
    h.get_by_label("/work/app");
}

#[cfg(unix)]
#[test]
fn profile_added_in_tab_and_activated_from_card_is_used_by_cumon_run() {
    use egui::accesskit::Role;
    let env = gui_env(now());
    env.store.ensure_default().unwrap();
    let dir = env.home.path().join("work-config");
    let mut h = env.harness();
    h.get_by_label("プロファイル").click();
    h.run();
    for (name, text) in [
        ("プロファイル名", "work"),
        ("設定ディレクトリ", dir.to_str().unwrap()),
    ] {
        h.get_by_role_and_label(Role::TextInput, name).click();
        h.run();
        h.get_by_role_and_label(Role::TextInput, name)
            .type_text(text);
        h.run();
    }
    h.get_by_label("追加").click();
    h.run();
    h.get_by_label("ダッシュボード").click();
    h.run();
    h.get_by_label("使用中にする").click();
    h.run();
    h.get_by_label_contains("work に切り替えました");
    assert!(matches!(&h.state().vm().body,
        claude_usage_monitor::controllers::gui::app::TabVm::Dashboard(d) if d.cards.iter().any(|k| k.name == "work" && k.is_active)));
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
fn settings_threshold_is_saved_for_daemon() {
    use claude_usage_monitor::models::ports::SettingsRepo;
    use egui::accesskit::Role;
    let env = gui_env(now());
    let mut h = env.harness();
    h.get_by_label("設定").click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "通知閾値（%）")
        .click();
    h.state_mut().forms_mut().settings.threshold = "95".into();
    h.run();
    h.get_by_label("保存").click();
    h.run();
    assert_eq!(env.store.load().unwrap().notify_threshold_percent, 95.0);
    h.get_by_label_contains("5分以内");
    h.get_by_label("診断").click();
    h.run();
    h.get_by_label("JSONLの取り込み");
}

mod memory_e2e {
    use super::*;
    use claude_usage_monitor::controllers::gui::app::{GuiController, TabVm};
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

    fn notice(h: &egui_kittest::Harness<'static, GuiController>) -> Option<String> {
        h.state().vm().notice.clone()
    }

    /// 「終了する」を押し、別スレッドの要求の結果が知らせに出るまでフレームを回す。
    fn confirm_exit(h: &mut egui_kittest::Harness<'static, GuiController>) {
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

    fn open_exit_dialog(h: &mut egui_kittest::Harness<'static, GuiController>) {
        h.get_by_label("設計の相談").click_secondary();
        h.run();
        h.get_by_label("終了してメモリを解放").click();
        h.run();
    }

    #[test]
    fn row_shows_memory_and_right_click_requests_exit_after_confirm() {
        let env = setup();
        let mut h = env.harness();
        h.get_by_label_contains("500MB");
        open_exit_dialog(&mut h);
        h.get_by_label_contains("実行中の編集が失われることがあります");
        confirm_exit(&mut h);
        assert_eq!(*env.procs.exits.lock().unwrap(), [victim()]);
        assert!(notice(&h).unwrap().contains("claude --resume s1"));
        assert!(h.query_by_label("終了する").is_none(), "ダイアログが閉じる");
    }

    #[test]
    fn cancel_does_not_request_exit() {
        let env = setup();
        let mut h = env.harness();
        open_exit_dialog(&mut h);
        h.get_by_label("やめる").click();
        h.run();
        assert!(env.procs.exits.lock().unwrap().is_empty());
        assert!(h.query_by_label("終了する").is_none());
    }

    #[test]
    fn refresh_while_dialog_is_open_keeps_the_same_victim() {
        let env = setup();
        let mut h = env.harness();
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
    fn desktop_card_requests_exit_after_confirm() {
        let env = setup();
        env.procs.procs.lock().unwrap().push(proc(
            100,
            1,
            1229,
            50,
            "/Applications/Claude.app/Contents/MacOS/Claude",
        ));
        let mut h = env.harness();
        h.get_by_label("Claude Desktop");
        h.get_by_label_contains("1.2GB");
        h.get_by_label("Desktopを終了").click();
        h.run();
        confirm_exit(&mut h);
        assert_eq!(
            *env.procs.exits.lock().unwrap(),
            [Victim {
                pid: 100,
                start_time: 50,
                target: Target::Desktop
            }]
        );
    }

    #[test]
    fn still_running_target_is_reported_after_grace_period() {
        let env = setup();
        let mut h = env.harness();
        open_exit_dialog(&mut h);
        confirm_exit(&mut h);
        env.advance(chrono::Duration::seconds(6));
        h.run();
        assert_eq!(
            notice(&h).as_deref(),
            Some("設計の相談 に終了を要求しましたが、まだ動いています")
        );
    }

    #[test]
    fn subagent_row_has_no_exit_menu() {
        use claude_usage_monitor::models::domain::pricing::TokenUsage;
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
        let mut h = env.harness();
        let sub = "go-reviewer「メモリ機能のレビュー」";
        h.get_by_label(sub).click_secondary();
        h.run();
        assert!(h.query_by_label("終了してメモリを解放").is_none());
        match &h.state().vm().body {
            TabVm::Dashboard(d) => {
                let row = d.active.iter().find(|r| r.title == sub).unwrap();
                assert_eq!(row.depth, 1);
                assert!(row.memory.is_none());
            }
            other => panic!("{other:?}"),
        }
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
        let h = env.harness();
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
        let h = env.harness();
        h.get_by_label("Desktop");
        match &h.state().vm().body {
            TabVm::Dashboard(d) => assert!(d.active[0].memory.as_ref().unwrap().desktop),
            other => panic!("{other:?}"),
        }
    }
}

mod calendar_e2e {
    use super::*;

    fn open_calendar(
        env: &common::gui::GuiEnv,
    ) -> egui_kittest::Harness<'static, claude_usage_monitor::controllers::gui::app::GuiController>
    {
        let mut h = env.harness();
        h.get_by_label("カレンダー").click();
        h.run();
        assert_eq!(h.state().vm().tab, Tab::Calendar);
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
        assert_eq!(h.state().calendar_selected(), Some("s1"));
        h.get_by_label_contains("のターン");
        h.get_by_label("ダッシュボード").click();
        h.run();
        assert_eq!(h.state().dash_selected(), None);
        h.get_by_label("左の一覧からセッションを選んでください");
        h.get_by_label("カレンダー").click();
        h.run();
        h.get_by_label("Read: src/main.rs");
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
        assert_eq!(h.state().calendar_selected(), None);
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
        let env = gui_env(now());
        seed_session_with_turns(&env, "s1", "設計の相談", SessionKind::Interactive, None);
        for pref in [egui::ThemePreference::Light, egui::ThemePreference::Dark] {
            let mut h = env.harness();
            claude_usage_monitor::views::theme::apply(&h.ctx);
            h.ctx.set_theme(pref);
            h.get_by_label("カレンダー").click();
            h.run();
            assert_eq!(
                h.ctx.global_style().visuals.dark_mode,
                pref == egui::ThemePreference::Dark
            );
            h.get_by_label_contains("設計の相談");
            h.get_by_label("2026年9/20〜9/26");
        }
    }
}
