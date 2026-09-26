//! ライブログ。表示中だけ、選んだセッションのJSONLとジョブの`timeline.jsonl`をtailする（spec 7.4節）。
use crate::controllers::gui::app::GuiDeps;
use crate::models::domain::live_log::{
    LIVE_LOG_CAPACITY, LiveFilter, LiveKind, LiveLine, RingBuffer, SpawnInfo, line_from_timeline,
    lines_from_transcript,
};
use crate::models::gateways::jsonl::read_new_lines;
use crate::models::gateways::session_files::{find_session_files, tail_offset};
use crate::models::ports::RepoError;
use chrono::FixedOffset;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

/// ライブログの操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveAction {
    /// フィルタが変わった。フィルタは`Forms.live_filter`にある。
    FilterChanged,
    /// 最新の行へ移る。
    JumpToLatest,
    /// 最新の行へ移ったことを描画側が知らせる。1回だけ移動させるため。
    JumpDone,
}

/// 表示用の1行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveLineView {
    /// 時刻（`HH:MM:SS`）。
    pub time: String,
    /// 発生元。
    pub source: String,
    /// 字下げ。
    pub depth: u8,
    /// 種類。
    pub kind: LiveKind,
    /// ツール名。
    pub tool: Option<String>,
    /// 文。
    pub text: String,
}

/// ライブログのViewModel。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveLogVm {
    /// 絞り込み後の行。
    pub lines: Vec<LiveLineView>,
    /// 発生元の一覧。フィルタの選択肢に使う。
    pub sources: Vec<String>,
    /// 保持している全行数。
    pub total: usize,
    /// 読めないときの説明。
    pub note: Option<String>,
    /// 最新の行へ移るか。
    pub jump_to_bottom: bool,
}

/// tail中のファイル。
#[derive(Debug, Clone)]
struct Tail {
    path: PathBuf,
    offset: u64,
    source: String,
    depth: u8,
    timeline: bool,
}

/// ライブログの状態。別のセッションを選ぶか、詳細をライブログから切り替えたら捨てる（spec 7.4節）。
#[derive(Debug)]
pub struct LiveLogState {
    session_id: String,
    job_id: Option<String>,
    config_dir: PathBuf,
    tails: Vec<Tail>,
    buf: RingBuffer<LiveLine>,
    spawns: HashMap<String, SpawnInfo>,
    names: HashMap<String, String>,
    note: Option<String>,
}

impl LiveLogState {
    /// セッションのファイルを探し、末尾から最大2,000行を読んだ状態で作る。
    pub fn open(deps: &GuiDeps, session_id: &str) -> Result<Self, RepoError> {
        let session = deps
            .sessions
            .session(session_id)?
            .ok_or_else(|| RepoError::NotFound(format!("セッション {session_id}")))?;
        let profile = deps
            .profiles
            .list()?
            .into_iter()
            .find(|p| p.id == session.profile_id)
            .ok_or_else(|| RepoError::NotFound(format!("プロファイル {}", session.profile_id)))?;
        let ids = [session_id.to_string()];
        let subs = deps.sessions.subagents(&ids)?;
        let spawns = subs
            .iter()
            .filter_map(|a| {
                Some((
                    a.parent_tool_use_id.clone()?,
                    SpawnInfo {
                        agent_type: a
                            .agent_type
                            .clone()
                            .unwrap_or_else(|| "サブエージェント".into()),
                        description: a.description.clone().unwrap_or_default(),
                    },
                ))
            })
            .collect();
        let names = subs
            .into_iter()
            .map(|a| {
                (
                    a.agent_id,
                    a.agent_type.unwrap_or_else(|| "サブエージェント".into()),
                )
            })
            .collect();
        let mut st = Self {
            session_id: session_id.to_string(),
            job_id: deps.sessions.job_id(session_id)?,
            config_dir: profile.resolved_config_dir(&deps.home),
            tails: vec![],
            buf: RingBuffer::new(LIVE_LOG_CAPACITY),
            spawns,
            names,
            note: None,
        };
        st.poll();
        Ok(st)
    }

    /// 表示中のセッションID。
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 追記された行を読む。新しく現れたサブエージェントのファイルも拾う。行が増えたら`true`を返し、
    /// 呼び出し側が変化のあったときだけViewModelを作り直せるようにする。
    pub fn poll(&mut self) -> bool {
        self.discover();
        let mut batch: Vec<LiveLine> = vec![];
        for t in &mut self.tails {
            let (spawns, source, depth, timeline) =
                (&self.spawns, t.source.clone(), t.depth, t.timeline);
            if let Ok(out) = read_new_lines(&t.path, t.offset, |raw| {
                if timeline {
                    batch.extend(line_from_timeline(raw));
                } else {
                    batch.extend(lines_from_transcript(raw, &source, depth, spawns));
                }
            }) {
                t.offset = out.new_offset;
            }
        }
        // 複数のファイルの行を時刻順に混ぜる。時刻のない行は読んだ順のまま後ろに置く。
        batch.sort_by_key(|l| (l.ts.is_none(), l.ts));
        let changed = !batch.is_empty();
        self.buf.extend(batch);
        changed
    }

    fn discover(&mut self) {
        let first = self.tails.is_empty();
        let files = find_session_files(&self.config_dir, &self.session_id, self.job_id.as_deref());
        self.note = files.main.is_none().then(|| {
            "このセッションのJSONLが見つかりません。ファイルが削除された可能性があります"
                .to_string()
        });
        let mut found: Vec<Tail> = files
            .main
            .into_iter()
            .map(|path| Tail {
                path,
                offset: 0,
                source: "本体".into(),
                depth: 0,
                timeline: false,
            })
            .collect();
        found.extend(files.subagents.into_iter().map(|(id, path)| {
            Tail {
                source: self
                    .names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| "サブエージェント".into()),
                path,
                offset: 0,
                depth: 1,
                timeline: false,
            }
        }));
        found.extend(files.timeline.into_iter().map(|path| Tail {
            path,
            offset: 0,
            source: "ジョブ".into(),
            depth: 0,
            timeline: true,
        }));
        let fresh: Vec<Tail> = found
            .into_iter()
            .filter(|t| !self.tails.iter().any(|x| x.path == t.path))
            .collect();
        for mut t in fresh {
            // 開いた時点にあるファイルは末尾の2,000行から読む。開いた後に現れたファイルは全行が新しいので先頭から読む。
            if first {
                t.offset = tail_offset(&t.path, LIVE_LOG_CAPACITY).unwrap_or(0);
            }
            self.tails.push(t);
        }
    }

    /// ViewModelを作る。
    pub fn vm(&self, filter: &LiveFilter, tz: FixedOffset) -> LiveLogVm {
        let sources: BTreeSet<String> = self.buf.iter().map(|l| l.source.clone()).collect();
        LiveLogVm {
            lines: self
                .buf
                .iter()
                .filter(|l| filter.matches(l))
                .map(|l| LiveLineView {
                    time: l
                        .ts
                        .map(|t| t.with_timezone(&tz).format("%H:%M:%S").to_string())
                        .unwrap_or_default(),
                    source: l.source.clone(),
                    depth: l.depth,
                    kind: l.kind,
                    tool: l.tool.clone(),
                    text: l.text.clone(),
                })
                .collect(),
            sources: sources.into_iter().collect(),
            total: self.buf.len(),
            note: self.note.clone(),
            jump_to_bottom: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::live_log::{LiveFilter, LiveKind};
    use crate::models::domain::records::{JobRecord, SubagentRecord};
    use crate::models::domain::transcript::SessionKind;
    use crate::models::ports::{IngestRepo, ProfileRepo};
    use crate::models::repositories::db::SqliteStore;
    use crate::test_support::{
        FakeCreds, FakeDaemon, FixedClock, gui_deps, seed_session, temp_store,
    };
    use chrono::{DateTime, TimeZone, Utc};
    use std::collections::HashMap;
    use std::io::Write;
    use std::sync::Arc;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 1, 5, 0).unwrap()
    }

    const MAIN: &str = r#"{"type":"assistant","timestamp":"2026-09-26T01:02:14.000Z","sessionId":"s1","message":{"id":"m1","content":[{"type":"tool_use","id":"toolu_1","name":"Agent","input":{}}]}}"#;
    const SUB: &str = r#"{"type":"assistant","timestamp":"2026-09-26T01:02:15.000Z","sessionId":"s1","message":{"id":"m2","content":[{"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"/w/a.rs"}}]}}"#;

    fn append(p: &std::path::Path, l: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .unwrap();
        writeln!(f, "{l}").unwrap();
    }

    fn setup() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        GuiDeps,
        std::path::PathBuf,
        Arc<SqliteStore>,
    ) {
        let (d, s) = temp_store();
        let s = Arc::new(s);
        seed_session(&s, "s1", SessionKind::Interactive, Some("busy"), now());
        s.upsert_subagent(&SubagentRecord {
            agent_id: "a1".into(),
            session_id: "s1".into(),
            agent_type: Some("go-reviewer".into()),
            description: Some("レビュー".into()),
            parent_tool_use_id: Some("toolu_1".into()),
            spawn_depth: Some(1),
        })
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let proj = home.path().join(".claude/projects/-w");
        append(&proj.join("s1.jsonl"), MAIN);
        let deps = gui_deps(
            s.clone(),
            Arc::new(FixedClock::at(now())),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        (d, home, deps, proj, s)
    }

    #[test]
    fn open_reads_main_and_marks_agent_spawn() {
        let (_d, _h, deps, _p, _s) = setup();
        let st = LiveLogState::open(&deps, "s1").unwrap();
        let vm = st.vm(&LiveFilter::default(), deps.tz);
        assert_eq!(vm.lines.len(), 1);
        assert_eq!(
            (vm.lines[0].kind, vm.lines[0].text.as_str()),
            (LiveKind::AgentSpawn, "go-reviewer「レビュー」を起動")
        );
        assert_eq!(vm.lines[0].time, "10:02:14");
        assert_eq!(st.session_id(), "s1");
        assert!(LiveLogState::open(&deps, "gone").is_err());
    }

    #[test]
    fn poll_picks_up_appended_lines_and_new_subagent_files() {
        let (_d, _h, deps, proj, _s) = setup();
        let mut st = LiveLogState::open(&deps, "s1").unwrap();
        assert!(!st.poll(), "追記がなければ変化なし");
        append(&proj.join("s1/subagents/agent-a1.jsonl"), SUB);
        assert!(st.poll());
        let vm = st.vm(&LiveFilter::default(), deps.tz);
        assert_eq!(vm.lines.len(), 2);
        assert_eq!(
            (
                vm.lines[1].source.as_str(),
                vm.lines[1].depth,
                vm.lines[1].text.as_str()
            ),
            ("go-reviewer", 1, "/w/a.rs")
        );
        assert_eq!(vm.sources, ["go-reviewer", "本体"]);
        assert!(!st.poll());
        assert_eq!(st.vm(&LiveFilter::default(), deps.tz).total, 2);
        let only_sub = LiveFilter {
            source: Some("go-reviewer".into()),
            ..LiveFilter::default()
        };
        assert_eq!(st.vm(&only_sub, deps.tz).lines.len(), 1);
    }

    #[test]
    fn missing_transcript_shows_note() {
        let (_d, _h, deps, proj, _s) = setup();
        std::fs::remove_file(proj.join("s1.jsonl")).unwrap();
        let st = LiveLogState::open(&deps, "s1").unwrap();
        assert!(
            st.vm(&LiveFilter::default(), deps.tz)
                .note
                .unwrap()
                .contains("JSONLが見つかりません")
        );
    }

    #[test]
    fn job_timeline_is_merged() {
        let (_d, h, deps, _p, s) = setup();
        let p = s.ensure_default().unwrap();
        s.upsert_job(&JobRecord {
            job_id: "j1".into(),
            profile_id: p.id,
            session_id: Some("s1".into()),
            name: None,
            state: "working".into(),
            detail: None,
            in_flight_tasks: 0,
            tokens: None,
            created_at: None,
            updated_at: now(),
        })
        .unwrap();
        append(
            &h.path().join(".claude/jobs/j1/timeline.jsonl"),
            r#"{"at":"2026-09-26T01:03:00.000Z","state":"working","detail":"2/8件目"}"#,
        );
        let st = LiveLogState::open(&deps, "s1").unwrap();
        assert_eq!(
            st.vm(&LiveFilter::default(), deps.tz)
                .lines
                .last()
                .unwrap()
                .text,
            "ジョブ: working（2/8件目）"
        );
    }
}
