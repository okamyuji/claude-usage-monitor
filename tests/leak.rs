//! リーク検査。デーモンを1秒周期で1,000回回し、100周期目と1,000周期目のRSSの差が5MB以下であることを確かめる（spec 12.4節）。
//!
//! 17分ほどかかるため`#[ignore]`を付け、`cargo test --release --test leak -- --ignored`で手元から実行する。
//! 常駐時に実際に動くのはトレイありのデーモンなので、macOSではトレイありの版も検査する。
#![forbid(unsafe_code)]
mod common;

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use sysinfo::{Pid, ProcessesToUpdate, System};

const LIMIT_BYTES: u64 = 5 * 1024 * 1024;

fn usage_rows(db: &std::path::Path) -> i64 {
    rusqlite::Connection::open(db)
        .and_then(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM fetch_log WHERE target LIKE 'usage:%'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap_or(0)
}

fn rss(sys: &mut System, pid: u32) -> u64 {
    sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    sys.process(Pid::from_u32(pid))
        .map(|p| p.memory())
        .unwrap_or(0)
}

/// `ticks`回の取得が記録されるまで待つ。JSONLへの追記も同時に続け、取り込みの経路にも負荷をかける。
fn wait_ticks(db: &std::path::Path, jsonl: &std::path::Path, ticks: i64, child: &mut Child) {
    let mut n = 0u64;
    while usage_rows(db) < ticks {
        assert!(
            child.try_wait().unwrap().is_none(),
            "デーモンが途中で終了しました"
        );
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(jsonl)
            .unwrap();
        writeln!(f, r#"{{"type":"user","uuid":"u{n}","sessionId":"leak","timestamp":"2026-09-26T00:00:00.000Z","message":{{"content":"x"}}}}"#).unwrap();
        n += 1;
        std::thread::park_timeout(Duration::from_millis(500));
    }
}

/// デーモンを`extra`付きで1,000周期回し、100周期目からのRSSの増加を返す。
fn rss_growth(extra: &[&str]) -> u64 {
    let wm = common::WireMock::start();
    common::stub_usage_ok(&wm);
    let data = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    common::write_credentials(home.path());
    let jsonl = home.path().join(".claude/projects/-leak/leak.jsonl");
    std::fs::create_dir_all(jsonl.parent().unwrap()).unwrap();
    std::fs::write(&jsonl, "").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cumon"))
        .env("CUMON_DATA_DIR", data.path())
        .env("HOME", home.path())
        .env("CUMON_USAGE_API_BASE", &wm.base_url)
        .env("CUMON_CATALOG_BASE", &wm.base_url)
        .env("CUMON_NOTIFY_LOG", data.path().join("n.log"))
        .args(["daemon", "--interval-secs", "1", "--max-ticks", "1000"])
        .args(extra)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let db = data.path().join("cumon.db");
    let mut sys = System::new();
    wait_ticks(&db, &jsonl, 100, &mut child);
    let at100 = rss(&mut sys, child.id());
    wait_ticks(&db, &jsonl, 999, &mut child);
    let at1000 = rss(&mut sys, child.id());
    let _ = child.wait();
    println!(
        "{extra:?} rss@100={at100} rss@1000={at1000} diff={}",
        at1000.saturating_sub(at100)
    );
    at1000.saturating_sub(at100)
}

#[test]
#[ignore]
fn headless_rss_growth_over_1000_ticks_is_within_5mb() {
    let diff = rss_growth(&["--no-tray"]);
    assert!(diff <= LIMIT_BYTES, "RSSが{diff}バイト増えました");
}

#[cfg(target_os = "macos")]
#[test]
#[ignore]
fn tray_rss_growth_over_1000_ticks_is_within_5mb() {
    let diff = rss_growth(&[]);
    assert!(diff <= LIMIT_BYTES, "RSSが{diff}バイト増えました");
}
