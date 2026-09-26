//! 診断タブ。取得の結果とエラー、読み飛ばした行、デーモンのメモリの推移、DBサイズ。
use crate::controllers::gui::app::GuiDeps;
use crate::models::domain::display::{fmt_bytes, fmt_clock};
use crate::models::domain::records::{FetchLogEntry, FetchResult};
use crate::models::ports::RepoError;
use chrono::Duration;

/// 取得1行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRow {
    /// 対象。
    pub target: String,
    /// 時刻。
    pub at: String,
    /// 結果。
    pub result: String,
    /// HTTPステータスと説明。
    pub detail: String,
    /// 失敗か。赤で出す。
    pub failed: bool,
}

/// 診断タブのViewModel。
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticsVm {
    /// 取得の最新結果。
    pub fetches: Vec<FetchRow>,
    /// 読み飛ばした行の説明。
    pub ingest: String,
    /// デーモンのRSSの推移（UNIX秒、MB）。
    pub rss_points: Vec<[f64; 2]>,
    /// 最新のRSS。
    pub rss_latest: String,
    /// DBサイズ。
    pub db_size: String,
    /// 時差（秒）。
    pub tz_offset_secs: i32,
}

fn row(deps: &GuiDeps, label: String, last: Option<FetchLogEntry>) -> FetchRow {
    let now = deps.clock.now();
    match last {
        None => FetchRow {
            target: label,
            at: "—".into(),
            result: "未実行".into(),
            detail: String::new(),
            failed: false,
        },
        Some(e) => FetchRow {
            target: label,
            at: fmt_clock(e.at, now, deps.tz),
            failed: e.result == FetchResult::Failed,
            result: if e.result == FetchResult::Ok {
                "成功".into()
            } else {
                "失敗".into()
            },
            detail: match e.http_status {
                Some(s) => format!("HTTP {s} {}", e.message),
                None => e.message,
            },
        },
    }
}

/// `files=3 lines=40 malformed=2`を読む。
fn ingest_text(last: Option<FetchLogEntry>) -> String {
    let Some(e) = last else {
        return "まだ取り込んでいません".into();
    };
    let num = |k: &str| -> String {
        e.message
            .split_whitespace()
            .find_map(|kv| kv.strip_prefix(k))
            .unwrap_or("?")
            .to_string()
    };
    format!(
        "読み飛ばした行 {}件（直近の全体走査: ファイル{}件、{}行）",
        num("malformed="),
        num("files="),
        num("lines=")
    )
}

/// 診断タブを作る。
pub fn build(deps: &GuiDeps) -> Result<DiagnosticsVm, RepoError> {
    let now = deps.clock.now();
    let mut fetches = vec![];
    for p in deps.profiles.list()? {
        let last = deps
            .logs
            .recent(&format!("usage:{}", p.id), 1)?
            .into_iter()
            .next();
        fetches.push(row(deps, format!("使用量（{}）", p.name), last));
    }
    fetches.push(row(
        deps,
        "モデル情報".into(),
        deps.logs.recent("catalog", 1)?.into_iter().next(),
    ));
    let ingest = deps.logs.recent("ingest", 1)?.into_iter().next();
    fetches.push(row(deps, "JSONLの取り込み".into(), ingest.clone()));
    let stats = deps.diagnostics.daemon_stats(now - Duration::days(7))?;
    Ok(DiagnosticsVm {
        fetches,
        ingest: ingest_text(ingest),
        rss_points: stats
            .iter()
            .map(|s| {
                [
                    s.at.timestamp() as f64,
                    s.rss_bytes as f64 / (1024.0 * 1024.0),
                ]
            })
            .collect(),
        rss_latest: stats
            .last()
            .map(|s| fmt_bytes(s.rss_bytes))
            .unwrap_or_else(|| "記録なし".into()),
        db_size: fmt_bytes(deps.diagnostics.db_size()),
        tz_offset_secs: deps.tz.local_minus_utc(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::records::{FetchLogEntry, FetchResult};
    use crate::models::ports::{FetchLogRepo, MaintenanceRepo, ProfileRepo};
    use crate::test_support::{FakeCreds, FakeDaemon, FixedClock, gui_deps, temp_store};
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn rows_for_each_target_and_stats() {
        let now = Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap();
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        let log = |target: String, result, status, message: &str| {
            s.log(&FetchLogEntry {
                target,
                at: now - Duration::minutes(1),
                result,
                http_status: status,
                message: message.into(),
            })
            .unwrap()
        };
        log(
            format!("usage:{}", p.id),
            FetchResult::Failed,
            Some(429),
            "取得回数の上限に達しました",
        );
        log("catalog".into(), FetchResult::Ok, None, "12件を更新");
        log(
            "ingest".into(),
            FetchResult::Ok,
            None,
            "files=3 lines=40 malformed=2",
        );
        s.record_daemon_stats(now - Duration::minutes(10), 14 * 1024 * 1024)
            .unwrap();
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            Arc::new(s),
            Arc::new(FixedClock::at(now)),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        let vm = build(&deps).unwrap();
        assert_eq!(
            vm.fetches
                .iter()
                .map(|f| f.target.as_str())
                .collect::<Vec<_>>(),
            ["使用量（default）", "モデル情報", "JSONLの取り込み"]
        );
        assert_eq!(
            (
                vm.fetches[0].result.as_str(),
                vm.fetches[0].detail.as_str(),
                vm.fetches[0].failed
            ),
            ("失敗", "HTTP 429 取得回数の上限に達しました", true)
        );
        assert_eq!(
            (vm.fetches[1].result.as_str(), vm.fetches[1].failed),
            ("成功", false)
        );
        assert_eq!(
            vm.ingest,
            "読み飛ばした行 2件（直近の全体走査: ファイル3件、40行）"
        );
        assert_eq!((vm.rss_points.len(), vm.rss_latest.as_str()), (1, "14.0MB"));
        assert_eq!(vm.rss_points[0][1], 14.0, "グラフの縦軸はMB");
    }

    #[test]
    fn never_fetched_targets_are_listed() {
        let (_d, s) = temp_store();
        s.ensure_default().unwrap();
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            Arc::new(s),
            Arc::new(FixedClock::at(Utc::now())),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        let vm = build(&deps).unwrap();
        assert!(vm.fetches.iter().all(|f| f.result == "未実行"));
        assert_eq!(vm.ingest, "まだ取り込んでいません");
        assert_eq!(vm.rss_latest, "記録なし");
    }

    #[test]
    fn ingest_text_tolerates_missing_keys() {
        let e = FetchLogEntry {
            target: "ingest".into(),
            at: Utc::now(),
            result: FetchResult::Ok,
            http_status: None,
            message: "files=1".into(),
        };
        assert_eq!(
            ingest_text(Some(e)),
            "読み飛ばした行 ?件（直近の全体走査: ファイル1件、?行）"
        );
    }
}
