//! `<config>/sessions/<pid>.json`（対話中セッションの記録）の読み取り。
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::io;
use std::path::Path;

/// 稼働中セッションのファイル1つ。
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSessionFile {
    /// プロセスID。生存確認に使う。
    pub pid: u32,
    /// セッションID。
    pub session_id: String,
    /// 作業ディレクトリ。
    pub cwd: Option<String>,
    /// セッション名。
    pub name: Option<String>,
    /// 状態（`busy`など）。
    pub status: Option<String>,
    /// 開始時刻。
    pub started_at: Option<DateTime<Utc>>,
    /// 更新時刻。
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    pid: u32,
    session_id: String,
    cwd: Option<String>,
    name: Option<String>,
    status: Option<String>,
    started_at: Option<i64>,
    updated_at: Option<i64>,
}

/// 全ファイルを読む。壊れたファイルは飛ばす。書き込み途中のファイルで全体を失敗させないため。
pub fn read_live_sessions(config_dir: &Path) -> io::Result<Vec<LiveSessionFile>> {
    let dir = config_dir.join("sessions");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(r) = serde_json::from_str::<Raw>(&text) else {
            continue;
        };
        out.push(LiveSessionFile {
            pid: r.pid,
            session_id: r.session_id,
            cwd: r.cwd,
            name: r.name,
            status: r.status,
            started_at: r
                .started_at
                .and_then(DateTime::<Utc>::from_timestamp_millis),
            updated_at: r
                .updated_at
                .and_then(DateTime::<Utc>::from_timestamp_millis),
        });
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_session_files_and_skips_broken() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("sessions");
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join("17342.json"), r#"{"pid":17342,"sessionId":"s1","cwd":"/w","name":"設計","status":"busy","startedAt":1790000000000,"updatedAt":1790000060000}"#).unwrap();
        std::fs::write(s.join("1.json"), "broken").unwrap();
        std::fs::write(s.join("17342.abc.key"), "x").unwrap();
        let v = read_live_sessions(d.path()).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(
            (v[0].pid, v[0].session_id.as_str(), v[0].status.as_deref()),
            (17342, "s1", Some("busy"))
        );
        assert_eq!(
            v[0].updated_at.unwrap().timestamp_millis(),
            1_790_000_060_000
        );
    }

    #[test]
    fn missing_dir_is_empty() {
        assert!(
            read_live_sessions(tempfile::tempdir().unwrap().path())
                .unwrap()
                .is_empty()
        );
    }
}
