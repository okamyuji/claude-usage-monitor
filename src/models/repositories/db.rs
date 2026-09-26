//! SQLiteの接続、PRAGMA、スキーマ。
//!
//! デーモン（書き込み）とGUI（読み取り）が同じファイルを同時に使うため、WALモードにする。
use crate::models::domain::pricing::TokenUsage;
use crate::models::ports::RepoError;
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const SCHEMA_V1: &str = r#"
CREATE TABLE profiles(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, config_dir TEXT, is_active INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL);
CREATE TABLE usage_samples(id INTEGER PRIMARY KEY, profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, fetched_at TEXT NOT NULL, kind TEXT NOT NULL, scope_label TEXT, percent REAL NOT NULL, resets_at TEXT, severity TEXT NOT NULL);
CREATE INDEX usage_samples_profile_time ON usage_samples(profile_id, kind, fetched_at);
CREATE TABLE usage_breakdown(profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, key TEXT NOT NULL, display_name TEXT NOT NULL, percent REAL NOT NULL, fetched_at TEXT NOT NULL, PRIMARY KEY(profile_id, key));
CREATE TABLE spend_samples(id INTEGER PRIMARY KEY, profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, fetched_at TEXT NOT NULL, used_minor INTEGER NOT NULL, limit_minor INTEGER, exponent INTEGER NOT NULL, currency TEXT NOT NULL);
CREATE TABLE sessions(session_id TEXT PRIMARY KEY, profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, kind TEXT NOT NULL, entrypoint TEXT, cwd TEXT, git_branch TEXT, name TEXT, first_prompt TEXT, started_at TEXT NOT NULL, last_activity_at TEXT NOT NULL, ended_at TEXT, status TEXT);
CREATE INDEX sessions_activity ON sessions(last_activity_at);
CREATE TABLE turns(id INTEGER PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE, agent_id TEXT NOT NULL DEFAULT '', message_id TEXT NOT NULL, ts TEXT NOT NULL, model TEXT, kind TEXT NOT NULL, summary TEXT NOT NULL, input INTEGER NOT NULL, output INTEGER NOT NULL, cache_read INTEGER NOT NULL, cache_write_5m INTEGER NOT NULL, cache_write_1h INTEGER NOT NULL, UNIQUE(session_id, agent_id, message_id));
CREATE INDEX turns_ts ON turns(ts);
CREATE TABLE tool_calls(tool_use_id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE, agent_id TEXT NOT NULL DEFAULT '', ts TEXT NOT NULL, tool_name TEXT NOT NULL, is_error INTEGER NOT NULL DEFAULT 0);
CREATE TABLE subagents(agent_id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE, agent_type TEXT, description TEXT, parent_tool_use_id TEXT, spawn_depth INTEGER);
CREATE TABLE jobs(job_id TEXT NOT NULL, profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, session_id TEXT, name TEXT, state TEXT NOT NULL, detail TEXT, in_flight_tasks INTEGER NOT NULL DEFAULT 0, tokens INTEGER, created_at TEXT, updated_at TEXT NOT NULL, PRIMARY KEY(profile_id, job_id));
CREATE TABLE ingest_offsets(path TEXT PRIMARY KEY, offset INTEGER NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL);
CREATE TABLE fetch_log(id INTEGER PRIMARY KEY, target TEXT NOT NULL, at TEXT NOT NULL, result TEXT NOT NULL, http_status INTEGER, message TEXT NOT NULL);
CREATE INDEX fetch_log_target_at ON fetch_log(target, at);
CREATE TABLE daemon_stats(at TEXT PRIMARY KEY, rss_bytes INTEGER NOT NULL, db_bytes INTEGER NOT NULL);
CREATE TABLE models(model_prefix TEXT PRIMARY KEY, display_name TEXT NOT NULL, input REAL NOT NULL, output REAL NOT NULL, cache_read REAL NOT NULL, cache_write_5m REAL NOT NULL, cache_write_1h REAL NOT NULL, context_window INTEGER, source TEXT NOT NULL, fetched_at TEXT);
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

const MIGRATIONS_SLICE: &[M<'_>] = &[M::up(SCHEMA_V1)];
const MIGRATIONS: Migrations<'_> = Migrations::from_slice(MIGRATIONS_SLICE);

/// SQLiteの保存先。`rusqlite::Connection`はスレッド間で共有できないため`Mutex`で包む。
pub struct SqliteStore {
    conn: Mutex<Connection>,
    path: PathBuf,
}

fn storage<E: std::fmt::Display>(e: E) -> RepoError {
    RepoError::Storage(e.to_string())
}

/// 時刻をDB保存用の文字列にする。ミリ秒精度で末尾`Z`に揃え、文字列比較で時刻順になるようにする。
pub fn ts(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// DBの時刻文字列を戻す。
pub fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

impl SqliteStore {
    /// ファイルを開き、スキーマを最新にする。親ディレクトリがなければ作る。
    pub fn open(path: &Path) -> Result<Self, RepoError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(storage)?;
        }
        let mut conn = Connection::open(path).map_err(storage)?;
        // auto_vacuumは表を作る前にしか効かないため、マイグレーションより先に設定する。
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")
            .map_err(storage)?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))
            .map_err(storage)?;
        // WALではNORMALでも電源断以外でデータを失わず、書き込みごとのfsyncを省けるため取り込みが速くなる。
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(storage)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(storage)?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(storage)?;
        MIGRATIONS.to_latest(&mut conn).map_err(storage)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        })
    }

    /// 接続を借りてSQLを実行する。ロックの失敗とSQLの失敗を`RepoError`へまとめる。
    pub(crate) fn with<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, RepoError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RepoError::Storage("DB接続のロックが壊れています".into()))?;
        f(&conn).map_err(storage)
    }

    /// DBファイルとWALの合計サイズ。診断画面でDBの肥大を確かめるために使う。
    pub fn db_bytes(&self) -> u64 {
        let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let wal = PathBuf::from(format!("{}-wal", self.path.display()));
        size(&self.path) + size(&wal)
    }
}

/// SQLの結果を、1行ずつタプルへ変換して返す。
///
/// 列ごとに`r.get(i)?`を書くと、分岐が増えてCRAP値が上がり、壊れた行のテストも列の数だけ要る。
/// rusqliteのタプル変換を使うと、型の不一致は1か所で`Err`になる。
pub(crate) fn query_rows<T>(
    c: &Connection,
    sql: &str,
    p: impl rusqlite::Params,
) -> rusqlite::Result<Vec<T>>
where
    T: for<'r> TryFrom<&'r rusqlite::Row<'r>, Error = rusqlite::Error>,
{
    let mut st = c.prepare(sql)?;
    st.query_map(p, |r| T::try_from(r))?.collect()
}

/// Token数の5列から作る。
pub(crate) fn usage_of(input: i64, output: i64, cache_read: i64, w5: i64, w1: i64) -> TokenUsage {
    TokenUsage {
        input: input as u64,
        output: output as u64,
        cache_read: cache_read as u64,
        cache_write_5m: w5 as u64,
        cache_write_1h: w1 as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn timestamps_round_trip_and_sort_lexically() {
        let a = Utc.with_ymd_and_hms(2026, 9, 26, 1, 2, 3).unwrap();
        let b = a + chrono::Duration::milliseconds(5);
        assert_eq!(ts(a), "2026-09-26T01:02:03.000Z");
        assert!(ts(a) < ts(b));
        assert_eq!(parse_ts(&ts(b)), Some(b));
        assert_eq!(parse_ts("2026-09-26T10:02:03+09:00"), Some(a));
        assert_eq!(parse_ts("bad"), None);
    }

    #[test]
    fn open_applies_pragmas_and_reports_size() {
        let (_d, s) = crate::test_support::temp_store();
        let mode: String = s
            .with(|c| c.query_row("PRAGMA journal_mode", [], |r| r.get(0)))
            .unwrap();
        let fk: i64 = s
            .with(|c| c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)))
            .unwrap();
        let vacuum: i64 = s
            .with(|c| c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0)))
            .unwrap();
        let sync: i64 = s
            .with(|c| c.query_row("PRAGMA synchronous", [], |r| r.get(0)))
            .unwrap();
        let tables: i64 = s
            .with(|c| c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('profiles','turns','models')", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(
            (mode.as_str(), fk, vacuum, sync, tables),
            ("wal", 1, 2, 1, 3)
        );
        assert!(s.db_bytes() > 0);
    }

    #[test]
    fn db_bytes_is_sum_of_db_and_wal() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("cps.db");
        let s = SqliteStore::open(&path).unwrap();
        s.with(|c| c.execute("INSERT INTO settings(key, value) VALUES('k', 'v')", []))
            .unwrap();
        let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        assert!(size(&wal) > 0);
        assert_eq!(s.db_bytes(), size(&path) + size(&wal));
    }

    #[test]
    fn open_creates_missing_parent_dirs() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("a/b/cps.db");
        SqliteStore::open(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn open_fails_when_parent_is_a_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("f"), "").unwrap();
        assert!(matches!(
            SqliteStore::open(&d.path().join("f/cps.db")),
            Err(RepoError::Storage(_))
        ));
    }
}
