//! `cps daemon`のE2E。実バイナリを一時HOMEで起動し、WireMockの応答とJSONLがDBに入ることを確かめる。
#![forbid(unsafe_code)]
mod common;

use assert_cmd::Command;
use common::WireMock;
use rusqlite::Connection;
use serde_json::json;
use std::path::Path;

fn write_home(home: &Path, token_expired: bool) {
    let cfg = home.join(".claude");
    std::fs::create_dir_all(cfg.join("projects/-w/s1/subagents")).unwrap();
    let expires = if token_expired {
        1
    } else {
        4_102_444_800_000i64
    };
    std::fs::write(
        cfg.join(".credentials.json"),
        format!(r#"{{"claudeAiOauth":{{"accessToken":"e2e","expiresAt":{expires}}}}}"#),
    )
    .unwrap();
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    std::fs::write(
        cfg.join("projects/-w/s1.jsonl"),
        format!(
            "{}\n{}\n",
            format_args!(r#"{{"type":"user","uuid":"u1","sessionId":"s1","timestamp":"{now}","entrypoint":"sdk-cli","message":{{"content":"hi"}}}}"#),
            format_args!(r#"{{"type":"assistant","sessionId":"s1","timestamp":"{now}","message":{{"id":"m1","model":"claude-opus-5-5","usage":{{"input_tokens":1,"output_tokens":50}},"content":[{{"type":"text","text":"ok"}}]}}}}"#),
        ),
    )
    .unwrap();
    std::fs::create_dir_all(cfg.join("jobs/j1")).unwrap();
    std::fs::write(
        cfg.join("jobs/j1/state.json"),
        format!(r#"{{"state":"working","detail":"1/8","inFlight":{{"tasks":1}},"sessionId":"s9","updatedAt":"{now}"}}"#),
    )
    .unwrap();
}

fn daemon(data: &Path, home: &Path, wm: &WireMock) -> Command {
    let mut c = Command::cargo_bin("cps").unwrap();
    c.env("CPS_DATA_DIR", data)
        .env("HOME", home)
        .env("CPS_USAGE_API_BASE", &wm.base_url)
        .env("CPS_CATALOG_BASE", &wm.base_url)
        .args([
            "daemon",
            "--no-tray",
            "--interval-secs",
            "1",
            "--max-ticks",
            "2",
        ])
        .timeout(std::time::Duration::from_secs(60));
    c
}

fn q(db: &Path, sql: &str) -> i64 {
    Connection::open(db)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

#[test]
fn daemon_collects_usage_sessions_headless_and_jobs() {
    let wm = WireMock::start();
    wm.stub(json!({"request": {"method": "GET", "url": "/api/oauth/usage", "headers": {"Authorization": {"equalTo": "Bearer e2e"}}},
                   "response": {"status": 200, "body": include_str!("fixtures/usage_ok.json")}}));
    let data = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    write_home(home.path(), false);
    daemon(data.path(), home.path(), &wm).assert().success();
    let db = data.path().join("cps.db");
    assert_eq!(
        q(
            &db,
            "SELECT COUNT(*) FROM usage_samples WHERE kind = 'session'"
        ),
        2
    );
    assert_eq!(
        q(
            &db,
            "SELECT COUNT(*) FROM sessions WHERE session_id = 's1' AND kind = 'headless'"
        ),
        1
    );
    assert_eq!(
        q(
            &db,
            "SELECT COUNT(*) FROM sessions WHERE session_id = 's9' AND kind = 'background_job'"
        ),
        1
    );
    assert_eq!(q(&db, "SELECT SUM(output) FROM turns"), 50);
    assert!(q(&db, "SELECT COUNT(*) FROM models") >= 6);
}

#[test]
fn expired_token_is_logged_as_401_without_calling_api() {
    let wm = WireMock::start();
    let data = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    write_home(home.path(), true);
    daemon(data.path(), home.path(), &wm).assert().success();
    let db = data.path().join("cps.db");
    assert_eq!(
        q(
            &db,
            "SELECT COUNT(*) FROM fetch_log WHERE target LIKE 'usage:%' AND http_status = 401"
        ),
        2
    );
    assert_eq!(q(&db, "SELECT COUNT(*) FROM usage_samples"), 0);
}

#[test]
fn second_daemon_refuses_to_start() {
    let wm = WireMock::start();
    let data = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    write_home(home.path(), false);
    let lock = std::fs::File::create(data.path().join("daemon.lock")).unwrap();
    lock.try_lock().unwrap();
    daemon(data.path(), home.path(), &wm)
        .assert()
        .failure()
        .stderr(predicates::str::contains("既に起動"));
}

#[test]
fn threshold_change_notifies_once_on_next_cycle() {
    let wm = WireMock::start();
    wm.stub(json!({"request": {"method": "GET", "url": "/api/oauth/usage", "headers": {"Authorization": {"equalTo": "Bearer e2e"}}},
                   "response": {"status": 200, "body": include_str!("fixtures/usage_ok.json")}}));
    let data = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    write_home(home.path(), false);
    let log = data.path().join("notify.log");
    // DBを作ってから閾値を10%に下げ、デーモンの最初の周期で5時間枠（13%）を通知させる。
    Command::cargo_bin("cps")
        .unwrap()
        .env("CPS_DATA_DIR", data.path())
        .env("HOME", home.path())
        .args(["profile", "list"])
        .assert()
        .success();
    Connection::open(data.path().join("cps.db"))
        .unwrap()
        .execute(
            "INSERT INTO settings(key, value) VALUES('notify_threshold_percent', '10') ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )
        .unwrap();
    daemon(data.path(), home.path(), &wm)
        .env("CPS_NOTIFY_LOG", &log)
        .assert()
        .success();
    let lines = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        lines
            .lines()
            .filter(|l| l.starts_with("defaultの5時間枠が13%です"))
            .count(),
        1,
        "2周期取得しても同じ枠の通知は1回だけ: {lines}"
    );
}
