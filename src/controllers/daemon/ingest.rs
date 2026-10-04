//! JSONL、稼働中セッション、ジョブの取り込み。
//!
//! 取り込みは冪等にしてある（repositories側のUPSERT）。そのため、ファイルが縮んだときや
//! 監視イベントの取りこぼしを全体走査で拾い直したときも、同じ行を安全に再処理できる。
use crate::models::domain::pricing::TokenUsage;
use crate::models::domain::profile::Profile;
use crate::models::domain::records::{
    FetchLogEntry, FetchResult, FileOffset, JobRecord, NoteRecord, SessionUpsert, SubagentRecord,
    ToolCallRecord, TurnRecord,
};
use crate::models::domain::transcript::{Block, Event, ParsedLine, SessionKind, parse_line};
use crate::models::gateways::jobs::{read_jobs, read_subagent_meta};
use crate::models::gateways::jsonl::{
    ReadOutcome, TranscriptFile, file_signature, read_new_lines, transcript_files,
};
use crate::models::gateways::live_sessions::read_live_sessions;
use crate::models::ports::{Clock, FetchLogRepo, IngestRepo, ProcessInfo, RepoError};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// 取り込み結果の件数。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IngestReport {
    /// 読んだファイル数（変化がなく飛ばしたものは含まない）。
    pub files_read: usize,
    /// 読んだ行数。
    pub lines: usize,
    /// JSONとして読めなかった行数。診断画面に出す。
    pub malformed: usize,
}

impl IngestReport {
    /// 2つの合計。全プロファイル分を1行の取得ログにまとめるため。
    pub fn plus(self, o: IngestReport) -> IngestReport {
        IngestReport {
            files_read: self.files_read + o.files_read,
            lines: self.lines + o.lines,
            malformed: self.malformed + o.malformed,
        }
    }
}

/// 取り込み役。
pub struct Ingestor {
    repo: Arc<dyn IngestRepo>,
    log: Arc<dyn FetchLogRepo>,
    process: Arc<dyn ProcessInfo>,
    clock: Arc<dyn Clock>,
    retention: chrono::Duration,
}

struct FileState<'a> {
    profile_id: i64,
    agent_id: String,
    sessions: HashMap<String, SessionUpsert>,
    repo: &'a dyn IngestRepo,
    cutoff: DateTime<Utc>,
    now: DateTime<Utc>,
}

impl FileState<'_> {
    fn apply(&mut self, pl: &ParsedLine) -> Result<(), RepoError> {
        let Some(sid) = pl.meta.session_id.clone() else {
            return Ok(());
        };
        let at = pl.meta.timestamp.unwrap_or(self.now);
        if at < self.cutoff {
            return Ok(());
        }
        self.track_session(&sid, pl, at)?;
        match &pl.event {
            Event::Assistant {
                message_id,
                model,
                usage,
                blocks,
            } => self.apply_assistant(&sid, at, message_id, model, *usage, blocks)?,
            Event::UserPrompt(text) => self.apply_prompt(&sid, pl, at, text)?,
            Event::ToolResults(rs) => {
                for r in rs.iter().filter(|r| r.is_error) {
                    self.repo.mark_tool_error(&r.tool_use_id)?;
                }
            }
            Event::Note { kind, text } => {
                // uuidがない行は一意キーを作れず、読み直しで重複するため保存しない。
                if let Some(uuid) = &pl.meta.uuid {
                    self.repo.upsert_note(&NoteRecord {
                        uuid: uuid.clone(),
                        session_id: sid.clone(),
                        ts: at,
                        kind: *kind,
                        text: text.clone(),
                    })?;
                }
            }
            Event::Other => {}
        }
        Ok(())
    }

    /// セッション行を用意し、種別と期間を更新する。
    fn track_session(
        &mut self,
        sid: &str,
        pl: &ParsedLine,
        at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        if !self.sessions.contains_key(sid) {
            let s = SessionUpsert {
                session_id: sid.to_string(),
                profile_id: self.profile_id,
                kind: SessionKind::from_entrypoint(pl.meta.entrypoint.as_deref()),
                entrypoint: pl.meta.entrypoint.clone(),
                cwd: pl.meta.cwd.clone(),
                git_branch: pl.meta.git_branch.clone(),
                name: None,
                first_prompt: None,
                started_at: at,
                last_activity_at: at,
                status: None,
            };
            // ターンの外部キーを満たすため、最初に見た時点でセッション行を作る。
            self.repo.upsert_session(&s)?;
            self.sessions.insert(sid.to_string(), s);
        }
        let s = self.sessions.get_mut(sid).expect("直前に挿入済み");
        // ファイル先頭の行（queue-operationやpermission-modeなど）はentrypointやcwdを持たないことがあるため、後の行で埋める。
        if s.entrypoint.is_none() && pl.meta.entrypoint.is_some() {
            s.entrypoint = pl.meta.entrypoint.clone();
            s.kind = SessionKind::from_entrypoint(s.entrypoint.as_deref());
        }
        if s.cwd.is_none() {
            s.cwd = pl.meta.cwd.clone();
        }
        if s.git_branch.is_none() {
            s.git_branch = pl.meta.git_branch.clone();
        }
        s.last_activity_at = s.last_activity_at.max(at);
        s.started_at = s.started_at.min(at);
        Ok(())
    }

    /// 利用者の入力をターンとして記録する。本体セッションの最初の入力は一覧の見出しに使うため残す。
    fn apply_prompt(
        &mut self,
        sid: &str,
        pl: &ParsedLine,
        at: DateTime<Utc>,
        text: &str,
    ) -> Result<(), RepoError> {
        let s = self.sessions.get_mut(sid).expect("track_sessionで挿入済み");
        if self.agent_id.is_empty() && s.first_prompt.is_none() {
            s.first_prompt = Some(text.to_string());
        }
        let id = pl
            .meta
            .uuid
            .clone()
            .unwrap_or_else(|| format!("prompt-{}", at.timestamp_millis()));
        self.repo.upsert_turn(&TurnRecord {
            session_id: sid.to_string(),
            agent_id: self.agent_id.clone(),
            message_id: id,
            ts: at,
            model: None,
            kind: "prompt".into(),
            summary: text.to_string(),
            usage: TokenUsage::default(),
        })
    }

    fn apply_assistant(
        &self,
        sid: &str,
        at: DateTime<Utc>,
        message_id: &str,
        model: &Option<String>,
        usage: TokenUsage,
        blocks: &[Block],
    ) -> Result<(), RepoError> {
        let turn = |kind: &str, summary: String| TurnRecord {
            session_id: sid.to_string(),
            agent_id: self.agent_id.clone(),
            message_id: message_id.to_string(),
            ts: at,
            model: model.clone(),
            kind: kind.to_string(),
            summary,
            usage,
        };
        if blocks.is_empty() {
            return self.repo.upsert_turn(&turn("text", String::new()));
        }
        for b in blocks {
            let record = match b {
                Block::Thinking => turn("thinking", String::new()),
                Block::Text(t) => turn("text", t.clone()),
                Block::ToolUse { id, name, summary } => {
                    self.repo.upsert_tool_call(&ToolCallRecord {
                        tool_use_id: id.clone(),
                        session_id: sid.to_string(),
                        agent_id: self.agent_id.clone(),
                        ts: at,
                        tool_name: name.clone(),
                    })?;
                    turn("tool_use", format!("{name}: {summary}"))
                }
            };
            self.repo.upsert_turn(&record)?;
        }
        Ok(())
    }
}

impl Ingestor {
    /// 依存と保持期間を受け取って作る。保持期間より古い行は取り込まず、初回の過去分取り込みでDBを膨らませない。
    pub fn new(
        repo: Arc<dyn IngestRepo>,
        log: Arc<dyn FetchLogRepo>,
        process: Arc<dyn ProcessInfo>,
        clock: Arc<dyn Clock>,
        retention: chrono::Duration,
    ) -> Self {
        Self {
            repo,
            log,
            process,
            clock,
            retention,
        }
    }

    /// 設定ディレクトリ全体を走査する。監視イベントの取りこぼしを拾うため定期的に呼ぶ。
    pub fn scan_all(
        &self,
        profile: &Profile,
        config_dir: &Path,
    ) -> Result<IngestReport, RepoError> {
        let mut report = IngestReport::default();
        let files = transcript_files(config_dir).map_err(|e| RepoError::Storage(e.to_string()))?;
        for f in &files {
            self.ingest_file(profile, f, &mut report)?;
        }
        self.ingest_live_state(profile, config_dir)?;
        Ok(report)
    }

    /// JSONL1つを前回の位置から取り込む。サイズと更新時刻が前回と同じなら開かない。
    pub fn ingest_file(
        &self,
        profile: &Profile,
        file: &TranscriptFile,
        report: &mut IngestReport,
    ) -> Result<(), RepoError> {
        let Ok((size, mtime_ms)) = file_signature(&file.path) else {
            return Ok(());
        };
        let prev = self.repo.offset(&file.path)?;
        if prev
            .as_ref()
            .is_some_and(|o| o.size == size && o.mtime_ms == mtime_ms)
        {
            return Ok(());
        }
        let now = self.clock.now();
        let (sessions, outcome, malformed) =
            self.read_lines(profile, file, prev.map_or(0, |o| o.offset), now)?;
        self.finish_file(file, &sessions, malformed, now)?;
        self.repo.save_offset(&FileOffset {
            path: file.path.clone(),
            offset: outcome.new_offset,
            size,
            mtime_ms,
        })?;
        report.files_read += 1;
        report.lines += outcome.lines;
        report.malformed += malformed;
        Ok(())
    }

    /// 行を読み、1行ずつDBへ反映する。DBの失敗は最初の1件で止め、読み取り位置を進めない。
    fn read_lines(
        &self,
        profile: &Profile,
        file: &TranscriptFile,
        offset: u64,
        now: DateTime<Utc>,
    ) -> Result<(HashMap<String, SessionUpsert>, ReadOutcome, usize), RepoError> {
        let mut state = FileState {
            profile_id: profile.id,
            agent_id: file.agent_id.clone().unwrap_or_default(),
            sessions: HashMap::new(),
            repo: self.repo.as_ref(),
            cutoff: now - self.retention,
            now,
        };
        let mut first_err: Option<RepoError> = None;
        let mut malformed = 0;
        let outcome = read_new_lines(&file.path, offset, |line| {
            if first_err.is_some() {
                return;
            }
            match parse_line(line) {
                Ok(pl) => first_err = state.apply(&pl).err(),
                Err(_) => malformed += 1,
            }
        })
        .map_err(|e| RepoError::Storage(e.to_string()))?;
        match first_err {
            Some(e) => Err(e),
            None => Ok((state.sessions, outcome, malformed)),
        }
    }

    /// セッションの最終状態、サブエージェントのメタ情報、読めなかった行数を反映する。
    fn finish_file(
        &self,
        file: &TranscriptFile,
        sessions: &HashMap<String, SessionUpsert>,
        malformed: usize,
        now: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        for s in sessions.values() {
            self.repo.upsert_session(s)?;
        }
        if let (Some(agent_id), Some(meta), Some(sid)) = (
            &file.agent_id,
            read_subagent_meta(&file.path),
            sessions.keys().next(),
        ) {
            self.repo.upsert_subagent(&SubagentRecord {
                agent_id: agent_id.clone(),
                session_id: sid.clone(),
                agent_type: meta.agent_type,
                description: meta.description,
                parent_tool_use_id: meta.tool_use_id,
                spawn_depth: meta.spawn_depth,
            })?;
        }
        if malformed > 0 {
            self.log.log(&FetchLogEntry {
                target: "ingest".into(),
                at: now,
                result: FetchResult::Failed,
                http_status: None,
                message: format!(
                    "{}: 読めない行が{malformed}行ありました",
                    file.path.display()
                ),
            })?;
        }
        Ok(())
    }

    /// 稼働中セッションとジョブを取り込む。
    pub fn ingest_live_state(&self, profile: &Profile, config_dir: &Path) -> Result<(), RepoError> {
        let now = self.clock.now();
        let live = read_live_sessions(config_dir).unwrap_or_default();
        let alive: Vec<String> = live.iter().map(|s| s.session_id.clone()).collect();
        for s in live {
            let status = if self.process.is_alive(s.pid) {
                s.status
            } else {
                Some("ended".into())
            };
            self.repo.upsert_session(&SessionUpsert {
                session_id: s.session_id,
                profile_id: profile.id,
                kind: SessionKind::Interactive,
                entrypoint: None,
                cwd: s.cwd,
                git_branch: None,
                name: s.name,
                first_prompt: None,
                started_at: s.started_at.unwrap_or(now),
                last_activity_at: s.updated_at.unwrap_or(now),
                status,
            })?;
        }
        self.repo.end_missing_interactive(profile.id, &alive)?;
        for j in read_jobs(config_dir).unwrap_or_default() {
            let updated = j.updated_at.unwrap_or(now);
            if let Some(sid) = &j.session_id {
                self.repo.upsert_session(&SessionUpsert {
                    session_id: sid.clone(),
                    profile_id: profile.id,
                    kind: SessionKind::BackgroundJob,
                    entrypoint: None,
                    cwd: j.cwd.clone(),
                    git_branch: None,
                    name: j.name.clone(),
                    first_prompt: None,
                    started_at: j.created_at.unwrap_or(updated),
                    last_activity_at: updated,
                    status: Some(j.state.clone()),
                })?;
            }
            self.repo.upsert_job(&JobRecord {
                job_id: j.job_id,
                profile_id: profile.id,
                session_id: j.session_id,
                name: j.name,
                state: j.state,
                detail: j.detail,
                in_flight_tasks: j.in_flight_tasks,
                tokens: j.tokens,
                created_at: j.created_at,
                updated_at: updated,
            })?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::profile::Profile;
    use crate::models::domain::transcript::NoteKind;
    use crate::models::ports::{ProfileRepo, SessionQueryRepo};
    use crate::test_support::{FixedClock, temp_store};
    use chrono::{TimeZone, Utc};
    use std::io::Write;

    struct AliveOnly(u32);
    impl ProcessInfo for AliveOnly {
        fn is_alive(&self, pid: u32) -> bool {
            pid == self.0
        }
        fn self_rss_bytes(&self) -> u64 {
            0
        }
    }

    const A1: &str = r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-09-26T00:00:01.000Z","entrypoint":"sdk-cli","cwd":"/w","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":2,"output_tokens":273,"cache_read_input_tokens":100},"content":[{"type":"thinking","thinking":"x"}]}}"#;
    const A2: &str = r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-09-26T00:00:02.000Z","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":2,"output_tokens":273,"cache_read_input_tokens":100},"content":[{"type":"tool_use","id":"toolu_1","name":"Agent","input":{"subagent_type":"go-reviewer","description":"レビュー"}}]}}"#;
    const U1: &str = r#"{"type":"user","uuid":"u1","sessionId":"s1","timestamp":"2026-09-26T00:00:00.000Z","entrypoint":"sdk-cli","message":{"content":"コミットしてください"}}"#;
    const R1: &str = r#"{"type":"user","sessionId":"s1","timestamp":"2026-09-26T00:00:03.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":true}]}}"#;
    const OLD: &str = r#"{"type":"user","uuid":"u0","sessionId":"s0","timestamp":"2025-01-01T00:00:00.000Z","message":{"content":"古い"}}"#;

    struct Env {
        _d: tempfile::TempDir,
        home: tempfile::TempDir,
        store: Arc<crate::models::repositories::db::SqliteStore>,
        profile: Profile,
        ingestor: Ingestor,
    }

    fn env() -> Env {
        let (d, store) = temp_store();
        let store = Arc::new(store);
        let profile = store.ensure_default().unwrap();
        let clock = Arc::new(FixedClock::at(
            Utc.with_ymd_and_hms(2026, 9, 26, 0, 1, 0).unwrap(),
        ));
        let ingestor = Ingestor::new(
            store.clone(),
            store.clone(),
            Arc::new(AliveOnly(42)),
            clock,
            chrono::Duration::days(90),
        );
        Env {
            _d: d,
            home: tempfile::tempdir().unwrap(),
            store,
            profile,
            ingestor,
        }
    }

    fn write(path: &Path, lines: &[&str]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    fn count(store: &crate::models::repositories::db::SqliteStore, sql: &str) -> i64 {
        store.with(|c| c.query_row(sql, [], |r| r.get(0))).unwrap()
    }

    #[test]
    fn ingest_counts_split_assistant_message_once() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        write(
            &cfg.join("projects/-w/s1.jsonl"),
            &[U1, A1, A2, R1, "broken", OLD],
        );
        let r = e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!((r.files_read, r.lines, r.malformed), (1, 6, 1));
        assert_eq!(
            count(
                &e.store,
                "SELECT SUM(output) FROM turns WHERE kind != 'prompt'"
            ),
            273
        );
        assert_eq!(count(&e.store, "SELECT COUNT(*) FROM turns"), 2);
        assert_eq!(count(&e.store, "SELECT SUM(is_error) FROM tool_calls"), 1);
        let (kind, prompt): (String, String) = e
            .store
            .with(|c| {
                c.query_row(
                    "SELECT kind, first_prompt FROM sessions WHERE session_id = 's1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap();
        assert_eq!(
            (kind.as_str(), prompt.as_str()),
            ("headless", "コミットしてください")
        );
        assert_eq!(
            count(
                &e.store,
                "SELECT COUNT(*) FROM sessions WHERE session_id = 's0'"
            ),
            0
        );
    }

    #[test]
    fn entrypoint_on_later_line_sets_headless_kind() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        let first =
            r#"{"type":"queue-operation","sessionId":"s1","timestamp":"2026-09-26T00:00:00.000Z"}"#;
        let queued = r#"{"type":"user","uuid":"q1","sessionId":"s1","timestamp":"2026-09-26T00:00:00.500Z","message":{"content":"hi"}}"#;
        write(&cfg.join("projects/-w/s1.jsonl"), &[first, queued, A1]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        let (kind, ep): (String, String) = e
            .store
            .with(|c| {
                c.query_row("SELECT kind, entrypoint FROM sessions", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
            })
            .unwrap();
        assert_eq!((kind.as_str(), ep.as_str()), ("headless", "sdk-cli"));
    }

    #[test]
    fn cwd_and_branch_on_later_line_fill_the_session() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        // 今のClaude Codeは、cwdを持たないmodeやpermission-modeの行をファイルの先頭に書く。
        let first =
            r#"{"type":"permission-mode","sessionId":"s1","timestamp":"2026-09-26T00:00:00.000Z"}"#;
        let user = r#"{"type":"user","uuid":"q1","sessionId":"s1","timestamp":"2026-09-26T00:00:00.500Z","cwd":"/w/app","gitBranch":"main","message":{"content":"hi"}}"#;
        let moved = r#"{"type":"user","uuid":"q2","sessionId":"s1","timestamp":"2026-09-26T00:00:01.000Z","cwd":"/w/other","gitBranch":"dev","message":{"content":"next"}}"#;
        write(&cfg.join("projects/-w/s1.jsonl"), &[first, user, moved]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(session_col(&e.store, "cwd"), "/w/app");
        assert_eq!(session_col(&e.store, "git_branch"), "main");
    }

    #[test]
    fn cwd_and_branch_seen_first_survive_a_later_scan() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        let f = cfg.join("projects/-w/s1.jsonl");
        let user = r#"{"type":"user","uuid":"q1","sessionId":"s1","timestamp":"2026-09-26T00:00:00.500Z","cwd":"/w/app","gitBranch":"main","message":{"content":"hi"}}"#;
        write(&f, &[user]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        // 追記分だけを読む2回目のスキャンでも、最初に見えた値を残す。
        let mode =
            r#"{"type":"permission-mode","sessionId":"s1","timestamp":"2026-09-26T00:00:02.000Z"}"#;
        let moved = r#"{"type":"user","uuid":"q2","sessionId":"s1","timestamp":"2026-09-26T00:00:03.000Z","cwd":"/w/other","gitBranch":"dev","message":{"content":"next"}}"#;
        write(&f, &[mode, moved]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(session_col(&e.store, "cwd"), "/w/app");
        assert_eq!(session_col(&e.store, "git_branch"), "main");
    }

    #[test]
    fn storage_error_is_returned_for_each_event_kind() {
        let prompt = r#"{"type":"user","uuid":"u9","sessionId":"s1","timestamp":"2026-09-26T00:00:00.000Z","message":{"content":"hi"}}"#;
        let tool_error = r#"{"type":"user","uuid":"u8","sessionId":"s1","timestamp":"2026-09-26T00:00:00.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","is_error":true}]}}"#;
        for (line, table) in [
            (A1, "turns"),
            (prompt, "turns"),
            (tool_error, "tool_calls"),
            (prompt, "sessions"),
        ] {
            let e = env();
            let cfg = e.home.path().join(".claude");
            write(&cfg.join("projects/-w/s1.jsonl"), &[line]);
            e.store
                .with(|c| c.execute_batch(&format!("PRAGMA foreign_keys=OFF; DROP TABLE {table};")))
                .unwrap();
            assert!(
                e.ingestor.scan_all(&e.profile, &cfg).is_err(),
                "{table}: {line}"
            );
        }
    }

    #[test]
    fn rescan_reads_only_new_lines_and_skips_unchanged_files() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        let f = cfg.join("projects/-w/s1.jsonl");
        write(&f, &[U1, A1]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(e.ingestor.scan_all(&e.profile, &cfg).unwrap().files_read, 0);
        write(&f, &[A2]);
        let r = e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!((r.files_read, r.lines), (1, 1));
        assert_eq!(count(&e.store, "SELECT SUM(output) FROM turns"), 273);
    }

    #[test]
    fn truncated_file_is_reingested_without_double_count() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        let f = cfg.join("projects/-w/s1.jsonl");
        write(&f, &[U1, A1, A2]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        std::fs::write(&f, format!("{A1}\n")).unwrap();
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(count(&e.store, "SELECT SUM(output) FROM turns"), 273);
    }

    #[test]
    fn subagent_log_is_linked_with_meta() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        write(&cfg.join("projects/-w/s1.jsonl"), &[U1]);
        let sub = cfg.join("projects/-w/s1/subagents/agent-a1.jsonl");
        write(
            &sub,
            &[
                r#"{"type":"user","uuid":"u9","sessionId":"s1","agentId":"a1","timestamp":"2026-09-26T00:00:05.000Z","message":{"content":"サブの依頼"}}"#,
            ],
        );
        std::fs::write(sub.with_extension("meta.json"), r#"{"agentType":"go-reviewer","description":"レビュー","toolUseId":"toolu_1","spawnDepth":1}"#).unwrap();
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(
            count(
                &e.store,
                "SELECT COUNT(*) FROM subagents WHERE agent_type = 'go-reviewer'"
            ),
            1
        );
        assert_eq!(
            count(&e.store, "SELECT COUNT(*) FROM turns WHERE agent_id = 'a1'"),
            1
        );
        let prompt: String = e
            .store
            .with(|c| c.query_row("SELECT first_prompt FROM sessions", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(prompt, "コミットしてください");
    }

    #[test]
    fn live_sessions_and_jobs_update_sessions() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        write(&cfg.join("projects/-w/s1.jsonl"), &[U1]);
        std::fs::create_dir_all(cfg.join("sessions")).unwrap();
        std::fs::write(cfg.join("sessions/42.json"), r#"{"pid":42,"sessionId":"s1","name":"設計","status":"busy","startedAt":1790000000000,"updatedAt":1790000060000}"#).unwrap();
        std::fs::write(cfg.join("sessions/43.json"), r#"{"pid":43,"sessionId":"s2","status":"busy","startedAt":1790000000000,"updatedAt":1790000060000}"#).unwrap();
        std::fs::create_dir_all(cfg.join("jobs/j1")).unwrap();
        std::fs::write(cfg.join("jobs/j1/state.json"), r#"{"state":"working","detail":"1/8","inFlight":{"tasks":1},"sessionId":"s3","updatedAt":"2026-09-26T00:00:30.000Z","createdAt":"2026-09-26T00:00:00.000Z"}"#).unwrap();
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        let rows: Vec<(String, String, Option<String>)> = e
            .store
            .with(|c| {
                let mut st =
                    c.prepare("SELECT session_id, kind, status FROM sessions ORDER BY session_id")?;
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect()
            })
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("s1".into(), "headless".into(), Some("busy".into())),
                ("s2".into(), "interactive".into(), Some("ended".into())),
                ("s3".into(), "background_job".into(), Some("working".into())),
            ]
        );
        assert_eq!(count(&e.store, "SELECT in_flight_tasks FROM jobs"), 1);
    }

    #[test]
    fn vanished_live_file_marks_session_ended() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        let live = cfg.join("sessions/4242.json");
        std::fs::create_dir_all(live.parent().unwrap()).unwrap();
        std::fs::write(
            &live,
            r#"{"pid":42,"sessionId":"s-live","status":"busy","startedAt":1790000000000,"updatedAt":1790000000000}"#,
        )
        .unwrap();
        e.ingestor.ingest_live_state(&e.profile, &cfg).unwrap();
        std::fs::remove_file(&live).unwrap();
        e.ingestor.ingest_live_state(&e.profile, &cfg).unwrap();
        let status: String = e
            .store
            .with(|c| {
                c.query_row(
                    "SELECT status FROM sessions WHERE session_id='s-live'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(status, "ended");
    }

    fn session_col(store: &crate::models::repositories::db::SqliteStore, col: &str) -> String {
        store
            .with(|c| {
                c.query_row(
                    &format!("SELECT {col} FROM sessions WHERE session_id = 's1'"),
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap()
    }

    #[test]
    fn line_exactly_at_retention_cutoff_is_kept() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        // 現在時刻 2026-09-26T00:01:00Z から保持期間90日を引いた時刻ちょうど。
        let edge = r#"{"type":"user","uuid":"ue","sessionId":"se","timestamp":"2026-06-28T00:01:00.000Z","message":{"content":"境界"}}"#;
        write(&cfg.join("projects/-w/se.jsonl"), &[edge]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(
            count(
                &e.store,
                "SELECT COUNT(*) FROM sessions WHERE session_id = 'se'"
            ),
            1
        );
    }

    #[test]
    fn later_line_without_entrypoint_keeps_kind() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        let plain = r#"{"type":"user","uuid":"q2","sessionId":"s1","timestamp":"2026-09-26T00:00:05.000Z","message":{"content":"続き"}}"#;
        write(&cfg.join("projects/-w/s1.jsonl"), &[A1, plain]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(session_col(&e.store, "kind"), "headless");
        assert_eq!(session_col(&e.store, "entrypoint"), "sdk-cli");
    }

    #[test]
    fn first_seen_entrypoint_is_not_overwritten() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        // DBは種別を上位にだけ更新するため、対話→ヘッドレスの順で後の行が種別を変えないことを確かめる。
        let first = r#"{"type":"user","uuid":"q4","sessionId":"s1","timestamp":"2026-09-26T00:00:00.000Z","entrypoint":"cli","message":{"content":"はじめ"}}"#;
        let later = r#"{"type":"user","uuid":"q5","sessionId":"s1","timestamp":"2026-09-26T00:00:05.000Z","entrypoint":"sdk-cli","message":{"content":"続き"}}"#;
        write(&cfg.join("projects/-w/s1.jsonl"), &[first, later]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(session_col(&e.store, "entrypoint"), "cli");
        assert_eq!(session_col(&e.store, "kind"), "interactive");
    }

    #[test]
    fn first_prompt_is_not_overwritten_by_later_prompts() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        let second = r#"{"type":"user","uuid":"u2","sessionId":"s1","timestamp":"2026-09-26T00:00:05.000Z","message":{"content":"二つ目"}}"#;
        write(&cfg.join("projects/-w/s1.jsonl"), &[U1, second]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(
            session_col(&e.store, "first_prompt"),
            "コミットしてください"
        );
    }

    #[test]
    fn grown_file_with_same_mtime_is_reread() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        let f = cfg.join("projects/-w/s1.jsonl");
        write(&f, &[U1, A1]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        let mtime = std::fs::metadata(&f).unwrap().modified().unwrap();
        write(&f, &[A2]);
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        let r = e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(
            (r.files_read, r.lines),
            (1, 1),
            "サイズが変われば更新時刻が同じでも読む"
        );
    }

    #[test]
    fn malformed_lines_are_logged_only_when_present() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        let ingest_logs = |e: &Env| {
            count(
                &e.store,
                "SELECT COUNT(*) FROM fetch_log WHERE target = 'ingest'",
            )
        };
        write(&cfg.join("projects/-w/s1.jsonl"), &[U1, A1]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(ingest_logs(&e), 0);
        write(&cfg.join("projects/-w/s2.jsonl"), &["{broken"]);
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!(ingest_logs(&e), 1);
    }

    #[test]
    fn scan_all_reports_malformed_lines() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        write(&cfg.join("projects/-w/s1.jsonl"), &[A1, "{broken"]);
        let r = e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        assert_eq!((r.files_read, r.malformed), (1, 1));
    }
    #[test]
    fn ingest_saves_recaps_and_compacts_once_even_from_subagent_files() {
        let e = env();
        let cfg = e.home.path().join(".claude");
        let main = cfg.join("projects/-w/s1.jsonl");
        write(
            &main,
            &[
                U1,
                r#"{"type":"system","subtype":"away_summary","content":"本体の要約","uuid":"n1","sessionId":"s1","timestamp":"2026-09-26T00:00:10.000Z"}"#,
                r#"{"type":"system","subtype":"away_summary","content":"uuidなし","sessionId":"s1","timestamp":"2026-09-26T00:00:11.000Z"}"#,
                r#"{"type":"system","subtype":"compact_boundary","uuid":"n2","sessionId":"s1","timestamp":"2026-09-26T00:00:20.000Z"}"#,
            ],
        );
        write(
            &cfg.join("projects/-w/s1/subagents/agent-a1.jsonl"),
            &[
                r#"{"type":"system","subtype":"away_summary","content":"サブの要約","uuid":"n3","sessionId":"s1","agentId":"a1","timestamp":"2026-09-26T00:00:30.000Z"}"#,
            ],
        );
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        e.store
            .with(|c| c.execute("DELETE FROM ingest_offsets", []))
            .unwrap();
        e.ingestor.scan_all(&e.profile, &cfg).unwrap();
        let got: Vec<_> = e
            .store
            .notes("s1")
            .unwrap()
            .into_iter()
            .map(|n| (n.kind, n.text))
            .collect();
        assert_eq!(
            got,
            [
                (NoteKind::Recap, "本体の要約".to_string()),
                (NoteKind::Compact, String::new()),
                (NoteKind::Recap, "サブの要約".into())
            ]
        );
    }
}
