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

// 取り込み位置を消すのは、V1までのJSONLにある要約を次の全体走査で読み直すため。取り込みは冪等なので重複しない。
const SCHEMA_V2: &str = r#"
CREATE TABLE session_notes(uuid TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE, ts TEXT NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL);
CREATE INDEX session_notes_session ON session_notes(session_id, ts);
DELETE FROM ingest_offsets;
"#;

// 再開したセッションのJSONLには、前のセッションの要約が同じuuidで写される。uuidだけの主キーでは片方が捨てられるので、
// 主キーに session_id を含めて表を作り直す。取り込み位置を消し、捨てられた要約を次の全体走査で読み直す。
const SCHEMA_V3: &str = r#"
CREATE TABLE session_notes_v3(session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE, uuid TEXT NOT NULL, ts TEXT NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL, PRIMARY KEY(session_id, uuid));
INSERT INTO session_notes_v3(session_id, uuid, ts, kind, text) SELECT session_id, uuid, ts, kind, text FROM session_notes;
DROP TABLE session_notes;
ALTER TABLE session_notes_v3 RENAME TO session_notes;
CREATE INDEX session_notes_session ON session_notes(session_id, ts);
DELETE FROM ingest_offsets;
"#;

// 時刻のない行（`ai-title`など）を読んだ時刻で、V3までは最終活動時刻が全体走査の時刻へ進んでいた。
// 残っている記録の最後の時刻（記録がなければ開始時刻）まで戻す。取り込み位置を消し、次の全体走査で
// JSONLの実際の最後の時刻まで上げ直すとともに題名を読み直す。
const SCHEMA_V4: &str = r#"
ALTER TABLE sessions ADD COLUMN ai_title TEXT;
WITH last(session_id, ts) AS (
  SELECT session_id, MAX(ts) FROM (
    SELECT session_id, ts FROM turns
    UNION ALL SELECT session_id, ts FROM tool_calls
    UNION ALL SELECT session_id, ts FROM session_notes)
  GROUP BY session_id),
floor(session_id, ts) AS (
  SELECT s.session_id, MAX(s.started_at, COALESCE(l.ts, s.started_at))
  FROM sessions s LEFT JOIN last l ON l.session_id = s.session_id)
UPDATE sessions SET last_activity_at = floor.ts FROM floor
WHERE floor.session_id = sessions.session_id AND sessions.last_activity_at > floor.ts;
DELETE FROM ingest_offsets;
"#;

const MIGRATIONS_SLICE: &[M<'_>] = &[
    M::up(SCHEMA_V1),
    M::up(SCHEMA_V2),
    M::up(SCHEMA_V3),
    M::up(SCHEMA_V4),
];
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
        // DBにはプロンプトや作業ディレクトリが入るため、本人だけが読めるようにする。
        // SQLiteはWALと共有メモリのファイルをDB本体と同じ権限で作るので、WALを有効にする前に絞る。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(storage)?;
        }
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

/// V1のスキーマだけを当てたDBを作る。V2への移行を、取り込みと組み合わせて確かめるため。
#[cfg(test)]
pub(crate) fn create_v1(path: &Path) -> Connection {
    let mut c = Connection::open(path).unwrap();
    Migrations::from_slice(&[M::up(SCHEMA_V1)])
        .to_latest(&mut c)
        .unwrap();
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[cfg(unix)]
    #[test]
    fn database_files_are_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("cumon.db");
        let _store = SqliteStore::open(&path).unwrap();
        for f in [path.clone(), d.path().join("cumon.db-wal")] {
            let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", f.display());
        }
    }

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
        let path = d.path().join("cumon.db");
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
        let path = d.path().join("a/b/cumon.db");
        SqliteStore::open(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn open_fails_when_parent_is_a_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("f"), "").unwrap();
        assert!(matches!(
            SqliteStore::open(&d.path().join("f/cumon.db")),
            Err(RepoError::Storage(_))
        ));
    }
    #[test]
    fn v2_adds_notes_and_clears_offsets_without_touching_sessions() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("cumon.db");
        {
            let mut c = Connection::open(&path).unwrap();
            Migrations::from_slice(&[M::up(SCHEMA_V1)])
                .to_latest(&mut c)
                .unwrap();
            c.execute_batch(
                "INSERT INTO ingest_offsets(path, offset, size, mtime) VALUES('/a', 1, 1, 1);
                 INSERT INTO profiles(id, name, created_at) VALUES(1, 'p', 'x');
                 INSERT INTO sessions(session_id, profile_id, kind, name, started_at, last_activity_at) VALUES('s', 1, 'interactive', 'n', 'x', 'x');",
            )
            .unwrap();
        }
        let s = SqliteStore::open(&path).unwrap();
        let got: (i64, String, i64) = s
            .with(|c| {
                c.query_row(
                    "SELECT (SELECT COUNT(*) FROM ingest_offsets), (SELECT name FROM sessions), (SELECT COUNT(*) FROM session_notes)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(got, (0, "n".to_string(), 0));
    }

    #[test]
    fn v3_keys_notes_by_session_and_uuid_keeping_rows_and_rereading_files() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("cumon.db");
        {
            let mut c = Connection::open(&path).unwrap();
            Migrations::from_slice(&[M::up(SCHEMA_V1), M::up(SCHEMA_V2)])
                .to_latest(&mut c)
                .unwrap();
            c.execute_batch(
                "INSERT INTO profiles(id, name, created_at) VALUES(1, 'p', 'x');
                 INSERT INTO sessions(session_id, profile_id, kind, started_at, last_activity_at) VALUES('a', 1, 'interactive', 'x', 'x'), ('b', 1, 'interactive', 'x', 'x');
                 INSERT INTO session_notes(uuid, session_id, ts, kind, text) VALUES('n1', 'a', 't', 'recap', 'V2の要約');
                 INSERT INTO ingest_offsets(path, offset, size, mtime) VALUES('/a', 1, 1, 1);",
            )
            .unwrap();
        }
        let s = SqliteStore::open(&path).unwrap();
        let got: (i64, String, i64) = s
            .with(|c| {
                // V3の後は、別のセッションが同じuuidの要約を持てる。
                c.execute(
                    "INSERT INTO session_notes(session_id, uuid, ts, kind, text) VALUES('b', 'n1', 't', 'recap', '写された要約')",
                    [],
                )?;
                c.query_row(
                    "SELECT (SELECT COUNT(*) FROM ingest_offsets), (SELECT text FROM session_notes WHERE session_id = 'a'), (SELECT user_version FROM pragma_user_version)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(
            got,
            (0, "V2の要約".to_string(), MIGRATIONS_SLICE.len() as i64)
        );
        let index: i64 = s
            .with(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'session_notes_session'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(index, 1);
    }

    #[test]
    fn v4_adds_ai_title_restores_last_activity_from_records_and_rereads_files() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("cumon.db");
        {
            let mut c = Connection::open(&path).unwrap();
            Migrations::from_slice(&MIGRATIONS_SLICE[..3])
                .to_latest(&mut c)
                .unwrap();
            c.execute_batch(
                "INSERT INTO profiles(id, name, created_at) VALUES(1, 'p', 'x');
                 INSERT INTO sessions(session_id, profile_id, kind, started_at, last_activity_at) VALUES
                   ('turn', 1, 'interactive', '2026-09-01T00:00:00Z', '2026-10-04T18:49:17Z'),
                   ('tool', 1, 'interactive', '2026-09-01T00:00:00Z', '2026-10-04T18:49:17Z'),
                   ('note', 1, 'interactive', '2026-09-01T00:00:00Z', '2026-10-04T18:49:17Z'),
                   ('fine', 1, 'interactive', '2026-09-01T00:00:00Z', '2026-09-02T00:00:00Z'),
                   ('none', 1, 'interactive', '2026-09-01T00:00:00Z', '2026-10-04T18:49:17Z');
                 INSERT INTO turns(session_id, message_id, ts, kind, summary, input, output, cache_read, cache_write_5m, cache_write_1h) VALUES
                   ('turn', 'm1', '2026-09-02T00:00:00Z', 'prompt', '', 0, 0, 0, 0, 0),
                   ('turn', 'm2', '2026-09-03T00:00:00Z', 'prompt', '', 0, 0, 0, 0, 0),
                   ('tool', 'm3', '2026-09-02T00:00:00Z', 'prompt', '', 0, 0, 0, 0, 0),
                   ('fine', 'm4', '2026-09-02T00:00:00Z', 'prompt', '', 0, 0, 0, 0, 0);
                 INSERT INTO tool_calls(tool_use_id, session_id, ts, tool_name) VALUES('t1', 'tool', '2026-09-04T00:00:00Z', 'Bash');
                 INSERT INTO session_notes(session_id, uuid, ts, kind, text) VALUES('note', 'n1', '2026-09-05T00:00:00Z', 'recap', '要約');
                 INSERT INTO ingest_offsets(path, offset, size, mtime) VALUES('/a', 1, 1, 1);",
            )
            .unwrap();
        }
        let s = SqliteStore::open(&path).unwrap();
        let got: Vec<(String, String)> = s
            .with(|c| {
                c.prepare("SELECT session_id, last_activity_at FROM sessions ORDER BY session_id")?
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect()
            })
            .unwrap();
        let want = [
            ("fine", "2026-09-02T00:00:00Z"),
            ("none", "2026-09-01T00:00:00Z"),
            ("note", "2026-09-05T00:00:00Z"),
            ("tool", "2026-09-04T00:00:00Z"),
            ("turn", "2026-09-03T00:00:00Z"),
        ]
        .map(|(a, b)| (a.to_string(), b.to_string()));
        assert_eq!(got, want);
        let after: (i64, Option<String>, i64) = s
            .with(|c| {
                c.query_row(
                    "SELECT (SELECT COUNT(*) FROM ingest_offsets), (SELECT ai_title FROM sessions WHERE session_id = 'turn'), (SELECT user_version FROM pragma_user_version)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(after, (0, None, 4));
    }

    /// PRの途中の版は、V2で主キーを(session_id, uuid)にしていた。その版で作ったDBもV3で移ることを確かめる。
    #[test]
    fn v3_also_migrates_v2_tables_keyed_by_session_and_uuid() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("cumon.db");
        {
            let mut c = Connection::open(&path).unwrap();
            Migrations::from_slice(&[
                M::up(SCHEMA_V1),
                M::up(
                    "CREATE TABLE session_notes(session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE, uuid TEXT NOT NULL, ts TEXT NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL, PRIMARY KEY(session_id, uuid));",
                ),
            ])
            .to_latest(&mut c)
            .unwrap();
            c.execute_batch(
                "INSERT INTO profiles(id, name, created_at) VALUES(1, 'p', 'x');
                 INSERT INTO sessions(session_id, profile_id, kind, started_at, last_activity_at) VALUES('a', 1, 'interactive', 'x', 'x'), ('b', 1, 'interactive', 'x', 'x');
                 INSERT INTO session_notes(session_id, uuid, ts, kind, text) VALUES('a', 'n1', 't', 'recap', '元'), ('b', 'n1', 't', 'recap', '写し');",
            )
            .unwrap();
        }
        let s = SqliteStore::open(&path).unwrap();
        let rows: i64 = s
            .with(|c| c.query_row("SELECT COUNT(*) FROM session_notes", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(rows, 2);
    }
}
