//! README用の画像に使う架空データを作る。実在の利用者のデータを写さず、同じ手順で何度でも作り直せるようにするため。
//!
//! Claude Codeと同じ形のログをHOMEに書き、デーモンと同じ取り込み処理でDBに入れる。
//! 稼働中の判定は最終更新からの経過時間で決まるので、作った直後に画像を書き出す。
//! `cargo run --example demo_data -- <DBを置くディレクトリ> <HOME>`
use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Utc};
use claude_usage_monitor::controllers::daemon::ingest::Ingestor;
use claude_usage_monitor::models::domain::usage::parse_usage;
use claude_usage_monitor::models::gateways::process::{SysProcessInfo, SystemClock};
use claude_usage_monitor::models::ports::{ProfileRepo, UsageRepo};
use claude_usage_monitor::models::repositories::db::{SqliteStore, ts};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MODEL: &str = "claude-opus-5-5";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, data, home] = args.as_slice() else {
        eprintln!("usage: demo_data <DBを置くディレクトリ> <HOME>");
        std::process::exit(2);
    };
    let (data, home) = (PathBuf::from(data), PathBuf::from(home));
    std::fs::create_dir_all(&data).unwrap();
    let cfg = home.join(".claude");
    let now = Utc::now();

    let store = Arc::new(SqliteStore::open(&data.join("cumon.db")).unwrap());
    let profile = store.ensure_default().unwrap();
    usage(&store, profile.id, now);
    week(&cfg, now);
    active(&cfg, now);

    let ing = Ingestor::new(
        store.clone(),
        store.clone(),
        Arc::new(SysProcessInfo::new()),
        Arc::new(SystemClock),
        Duration::days(90),
    );
    ing.scan_all(&profile, &cfg).unwrap();
    ing.ingest_live_state(&profile, &cfg).unwrap();
    println!("{}\n{}", data.display(), home.display());
}

/// 5時間枠と週間枠の推移。10分ごとに7日分の取得結果を入れる。
fn usage(store: &SqliteStore, profile_id: i64, now: DateTime<Utc>) {
    let five_reset = now + Duration::minutes(111);
    let week_reset = now + Duration::hours(50);
    for i in (0..7 * 24 * 6).rev() {
        let at = now - Duration::minutes(10 * i);
        // 5時間枠は枠の切り替わりで0に戻るので、切り替わりからの経過に比例させる。
        let into = 300 - (five_reset - at).num_minutes().rem_euclid(300);
        let five = 42.0 * into as f64 / 189.0;
        let weekly = 67.0 - 0.07 * (now - at).num_minutes() as f64 / 10.0;
        let body = format!(
            r#"{{"five_hour":{{"utilization":{five:.1},"resets_at":"{fr}"}},
 "limits":[
  {{"kind":"session","group":"session","percent":{five:.0},"severity":"normal","resets_at":"{fr}","scope":null,"is_active":false}},
  {{"kind":"weekly_all","group":"weekly","percent":{weekly:.0},"severity":"normal","resets_at":"{wr}","scope":null,"is_active":false}},
  {{"kind":"weekly_scoped","group":"weekly","percent":{fable:.0},"severity":"warning","resets_at":"{wr}","scope":{{"model":{{"id":null,"display_name":"Fable"}},"surface":null}},"is_active":true}}],
 "spend":{{"used":{{"amount_minor":1850,"currency":"USD","exponent":2}},"limit":{{"amount_minor":10000,"currency":"USD","exponent":2}},"percent":18,"severity":"normal","enabled":true}},
 "seven_day_breakdown":{{"as_of":"{fr}","rows":[
  {{"key":"claude_code","display_name":"Claude Code","percent":97}},
  {{"key":"chat","display_name":"Chats","percent":3}}]}}}}"#,
            fr = five_reset.to_rfc3339(),
            wr = week_reset.to_rfc3339(),
            fable = weekly + 14.0,
        );
        store
            .record_snapshot(profile_id, at, &parse_usage(&body).unwrap())
            .unwrap();
    }
}

/// カレンダー用に、前の週の過去のセッションを入れる。
fn week(cfg: &Path, now: DateTime<Utc>) {
    let projects = [
        ("todo-api", "認証まわりの整理"),
        ("web", "トップページの表示を速くする"),
        ("docs-site", "リリースノートを書く"),
        ("mobile-app", "通知の設定画面を足す"),
    ];
    // 曜日によって写る日数が変わらないよう、前の週の日曜から土曜までに入れ、画像では前週を開く。
    let today = now.with_timezone(&Local).date_naive();
    let sunday = today - Duration::days(i64::from(today.weekday().num_days_from_sunday()) + 7);
    for back in 0..7 {
        let day = sunday + Duration::days(back);
        let base = Local
            .from_local_datetime(&day.and_hms_opt(9, 0, 0).unwrap())
            .unwrap()
            .with_timezone(&Utc);
        for (i, (proj, prompt)) in projects.iter().enumerate() {
            // 日と案件で始まりをずらし、同じ時間帯に帯が重なる日も作る。最初に見える15時までに収める。
            let start = base + Duration::minutes(100 * i as i64 + 20 * (back % 3));
            let sid = format!("w{back}-{i}");
            let turns = 6 + (back as usize + i) % 5;
            let lines = session(&sid, proj, "cli", prompt, start, turns, 3);
            write(
                &cfg.join(format!("projects/-home-you-src-{proj}/{sid}.jsonl")),
                &lines,
            );
            if (back + i as i64) % 2 == 0 {
                let later = start + Duration::minutes(150);
                let sid2 = format!("{sid}b");
                let lines = session(&sid2, proj, "cli", "続きの作業", later, turns, 4);
                write(
                    &cfg.join(format!("projects/-home-you-src-{proj}/{sid2}.jsonl")),
                    &lines,
                );
            }
        }
    }
}

/// ダッシュボード用の稼働中のセッション。対話とそのサブエージェント、ヘッドレスの3つ。
fn active(cfg: &Path, now: DateTime<Utc>) {
    let sid = "demo-reset";
    let start = now - Duration::minutes(18);
    let proj = cfg.join("projects/-home-you-src-todo-api");
    let mut lines = vec![user(
        sid,
        "todo-api",
        "cli",
        start,
        "パスワードの再設定を追加して",
    )];
    lines.push(tool(
        sid,
        "todo-api",
        "cli",
        start + Duration::seconds(40),
        "m1",
        "Read",
        r#"{"file_path":"src/auth/mod.rs"}"#,
    ));
    lines.push(tool(
        sid,
        "todo-api",
        "cli",
        now - Duration::seconds(100),
        "m2",
        "Edit",
        r#"{"file_path":"src/auth/reset.rs"}"#,
    ));
    lines.push(tool(
        sid,
        "todo-api",
        "cli",
        now - Duration::seconds(88),
        "m3",
        "Bash",
        r#"{"command":"cargo test auth::"}"#,
    ));
    lines.push(tool(
        sid,
        "todo-api",
        "cli",
        now - Duration::seconds(60),
        "m4",
        "Agent",
        r#"{"subagent_type":"code-reviewer","description":"変更のレビュー"}"#,
    ));
    lines.push(text(
        sid,
        "todo-api",
        "cli",
        now - Duration::seconds(8),
        "m5",
        "パスワードの再設定を追加し、テストが通りました",
    ));
    write(&proj.join(format!("{sid}.jsonl")), &lines);

    let sub = proj.join(format!("{sid}/subagents/agent-r1.jsonl"));
    write(
        &sub,
        &[
            tool(
                sid,
                "todo-api",
                "cli",
                now - Duration::seconds(50),
                "s1",
                "Read",
                r#"{"file_path":"src/auth/reset.rs"}"#,
            ),
            text(
                sid,
                "todo-api",
                "cli",
                now - Duration::seconds(30),
                "s2",
                "指摘は2件です",
            ),
        ],
    );
    std::fs::write(
        sub.with_extension("meta.json"),
        r#"{"agentType":"code-reviewer","description":"変更のレビュー","spawnDepth":1}"#,
    )
    .unwrap();

    // 稼働中かどうかはプロセスの生存で決まるので、このプログラムのPIDを書く。取り込みの時点で生きていれば足りる。
    std::fs::create_dir_all(cfg.join("sessions")).unwrap();
    std::fs::write(
        cfg.join("sessions/demo.json"),
        format!(
            r#"{{"pid":{pid},"sessionId":"{sid}","cwd":"/home/you/src/todo-api","name":"パスワード再設定","status":"busy","startedAt":{s},"updatedAt":{u}}}"#,
            pid = std::process::id(),
            s = start.timestamp_millis(),
            u = now.timestamp_millis(),
        ),
    )
    .unwrap();

    let hl = "demo-ci";
    let t = now - Duration::minutes(3);
    write(
        &cfg.join(format!("projects/-home-you-src-web/{hl}.jsonl")),
        &[
            user(hl, "web", "sdk-cli", t, "CIの失敗を調べて直して"),
            tool(
                hl,
                "web",
                "sdk-cli",
                now - Duration::seconds(20),
                "h1",
                "Bash",
                r#"{"command":"npm test"}"#,
            ),
        ],
    );
}

/// 過去のセッション1つ分。`gap`分ごとに、ツールと本文を交互に並べる。
fn session(
    sid: &str,
    proj: &str,
    entry: &str,
    prompt: &str,
    start: DateTime<Utc>,
    turns: usize,
    gap: i64,
) -> Vec<String> {
    let mut lines = vec![user(sid, proj, entry, start, prompt)];
    for k in 0..turns {
        let at = start + Duration::minutes(gap * (k as i64 + 1));
        let id = format!("{sid}-{k}");
        lines.push(if k % 2 == 0 {
            tool(
                sid,
                proj,
                entry,
                at,
                &id,
                "Read",
                r#"{"file_path":"src/lib.rs"}"#,
            )
        } else {
            text(sid, proj, entry, at, &id, "確認しました")
        });
    }
    lines
}

fn user(sid: &str, proj: &str, entry: &str, at: DateTime<Utc>, prompt: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{sid}-u","sessionId":"{sid}","cwd":"/home/you/src/{proj}","gitBranch":"main","entrypoint":"{entry}","timestamp":"{t}","message":{{"role":"user","content":"{prompt}"}}}}"#,
        t = ts(at)
    )
}

fn tool(
    sid: &str,
    proj: &str,
    entry: &str,
    at: DateTime<Utc>,
    id: &str,
    name: &str,
    input: &str,
) -> String {
    assistant(
        sid,
        proj,
        entry,
        at,
        id,
        &format!(r#"{{"type":"tool_use","id":"toolu_{id}","name":"{name}","input":{input}}}"#),
    )
}

fn text(sid: &str, proj: &str, entry: &str, at: DateTime<Utc>, id: &str, body: &str) -> String {
    assistant(
        sid,
        proj,
        entry,
        at,
        id,
        &format!(r#"{{"type":"text","text":"{body}"}}"#),
    )
}

fn assistant(
    sid: &str,
    proj: &str,
    entry: &str,
    at: DateTime<Utc>,
    id: &str,
    content: &str,
) -> String {
    format!(
        r#"{{"type":"assistant","sessionId":"{sid}","cwd":"/home/you/src/{proj}","gitBranch":"main","entrypoint":"{entry}","timestamp":"{t}","message":{{"id":"{id}","model":"{MODEL}","content":[{content}],"usage":{{"input_tokens":1800,"output_tokens":160,"cache_read_input_tokens":24000,"cache_creation_input_tokens":1500}}}}}}"#,
        t = ts(at)
    )
}

fn write(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::File::create(path).unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
}
