//! メモリの表示と、終了の要求の確認。
//!
//! 全プロセスの取得は数ミリ秒から20ミリ秒かかるため、`refresh`のたびではなく`REFRESH_SECS`ごとに取り、結果を持っておく。
use crate::controllers::gui::app::{GuiDeps, REFRESH_SECS};
use crate::controllers::gui::dashboard::runs::RunItem;
use crate::models::domain::memory::{
    self, Bound, ExitError, LiveProc, ProcEntry, Target, Tree, Victim,
};
use crate::models::domain::memory_text::{fmt_bytes, resume_command};
use crate::models::gateways::live_sessions::read_live_sessions;
use chrono::{DateTime, Duration, Utc};
use std::sync::mpsc::{Receiver, TryRecvError};

/// 終了を要求してから、まだ動いているかを確かめるまでの秒数。Claude Codeが終了処理を済ませる時間を見込む。
pub const EXIT_GRACE_SECS: i64 = 5;

/// 確認ダイアログの中身。
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmExit {
    /// 開いた時点で固定した対象。
    pub victim: Victim,
    /// 見出し。
    pub title: String,
    /// メモリの表示。
    pub memory: String,
    /// 端末のセッションの再開コマンド。
    pub resume: Option<String>,
}

/// 行に付けるメモリ。
#[derive(Debug, Clone, PartialEq)]
pub struct SessionMemory {
    /// 表示（`412MB`）。
    pub text: String,
    /// Codeタブのセッションか。バッジに使う。
    pub desktop: bool,
    /// 終了を選んだときのダイアログ。
    pub exit: ConfirmExit,
}

/// Desktopのカード。
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopMemoryVm {
    /// 表示。
    pub text: String,
    /// 終了を選んだときのダイアログ。
    pub exit: ConfirmExit,
}

/// メモリまわりの操作。
#[derive(Debug, Clone, PartialEq)]
pub enum MemAction {
    /// 確認ダイアログを開く。
    Ask(ConfirmExit),
    /// ダイアログのOK。
    Confirm,
    /// ダイアログのキャンセル。
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
struct PendingExit {
    victim: Victim,
    title: String,
    at: DateTime<Utc>,
}

/// スナップショットと、確認中や要求済みの対象。
#[derive(Debug, Default)]
pub struct MemoryState {
    snapshot: Vec<ProcEntry>,
    taken_at: Option<DateTime<Utc>>,
    confirm: Option<ConfirmExit>,
    pending: Vec<PendingExit>,
    jobs: Vec<ExitJob>,
}

/// 別スレッドで送っている終了の要求。
#[derive(Debug)]
struct ExitJob {
    exit: ConfirmExit,
    at: DateTime<Utc>,
    rx: Receiver<Result<(), ExitError>>,
}

fn interrupted(c: &ConfirmExit) -> String {
    format!("{} の終了の要求が中断しました", c.title)
}

impl MemoryState {
    /// 前回から`REFRESH_SECS`以上たっていれば取り直し、猶予を過ぎた要求を判定する。まだ動いている対象の知らせを返す。
    pub fn refresh_if_due(&mut self, deps: &GuiDeps, now: DateTime<Utc>) -> Option<String> {
        if self
            .taken_at
            .is_some_and(|t| now - t < Duration::seconds(REFRESH_SECS))
        {
            return None;
        }
        self.snapshot = deps.process_tree.snapshot();
        self.taken_at = Some(now);
        let grace = Duration::seconds(EXIT_GRACE_SECS);
        let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|x| now - x.at >= grace);
        self.pending = wait;
        let alive: Vec<String> = due
            .into_iter()
            .filter(|x| {
                self.snapshot
                    .iter()
                    .any(|e| e.pid == x.victim.pid && e.start_time == x.victim.start_time)
            })
            .map(|x| format!("{} に終了を要求しましたが、まだ動いています", x.title))
            .collect();
        (!alive.is_empty()).then(|| alive.join(" / "))
    }

    /// 操作を処理し、画面下部に出す知らせを返す。
    pub fn handle(&mut self, deps: &GuiDeps, a: MemAction, now: DateTime<Utc>) -> Option<String> {
        match a {
            MemAction::Ask(c) => {
                self.confirm = Some(c);
                None
            }
            MemAction::Cancel => {
                self.confirm = None;
                None
            }
            MemAction::Confirm => {
                let c = self.confirm.take()?;
                let (tx, rx) = std::sync::mpsc::channel();
                let tree = deps.process_tree.clone();
                let v = c.victim;
                // Desktopの終了はosascriptの応答を待つ。許可のダイアログが出ると数十秒かかるため、描画を止めないよう別スレッドで送る。
                std::thread::spawn(move || {
                    let _ = tx.send(tree.request_exit(&v));
                });
                let msg = format!("{} に終了を要求しています", c.title);
                self.jobs.push(ExitJob {
                    exit: c,
                    at: now,
                    rx,
                });
                Some(msg)
            }
        }
    }

    /// 終わった要求の結果を受け取り、知らせを返す。
    pub fn poll(&mut self) -> Option<String> {
        let mut msgs = vec![];
        for j in std::mem::take(&mut self.jobs) {
            match j.rx.try_recv() {
                Ok(r) => msgs.push(self.finish(j.exit, j.at, r)),
                Err(TryRecvError::Empty) => self.jobs.push(j),
                Err(TryRecvError::Disconnected) => msgs.push(interrupted(&j.exit)),
            }
        }
        (!msgs.is_empty()).then(|| msgs.join(" / "))
    }

    /// 要求がすべて終わるまで待って結果を受け取る。テストで別スレッドの終了を待つため。
    #[cfg(test)]
    pub(crate) fn wait_jobs(&mut self) -> Option<String> {
        let mut msgs = vec![];
        for j in std::mem::take(&mut self.jobs) {
            msgs.push(match j.rx.recv() {
                Ok(r) => self.finish(j.exit, j.at, r),
                Err(_) => interrupted(&j.exit),
            });
        }
        (!msgs.is_empty()).then(|| msgs.join(" / "))
    }

    fn finish(&mut self, c: ConfirmExit, at: DateTime<Utc>, r: Result<(), ExitError>) -> String {
        match r {
            Ok(()) => {
                self.pending.push(PendingExit {
                    victim: c.victim,
                    title: c.title.clone(),
                    at,
                });
                match &c.resume {
                    Some(cmd) => format!(
                        "{} に終了を要求しました。再開するには次を実行します: {cmd}",
                        c.title
                    ),
                    None => format!("{} に終了を要求しました", c.title),
                }
            }
            Err(e) => e.to_string(),
        }
    }

    /// 確認中のダイアログ。
    pub fn confirm(&self) -> Option<&ConfirmExit> {
        self.confirm.as_ref()
    }
}

/// 全プロファイルの`sessions/<pid>.json`を読む。読めないディレクトリは空として扱い、一覧全体を失敗させない。
pub fn lives(deps: &GuiDeps) -> Vec<LiveProc> {
    let profiles = deps.profiles.list().unwrap_or_default();
    profiles
        .iter()
        .flat_map(|p| {
            let dir = p.resolved_config_dir(&deps.home);
            read_live_sessions(&dir)
                .unwrap_or_default()
                .into_iter()
                .map(move |f| LiveProc {
                    session_id: f.session_id,
                    pid: f.pid,
                    started_at: f.started_at,
                    config_dir: p.config_dir.clone(),
                })
        })
        .collect()
}

/// 行にメモリを付ける。サブエージェントの行は親と同じIDを持つので付けない。
pub fn attach(items: &mut [RunItem], st: &MemoryState, lives: &[LiveProc]) {
    let tree = Tree::new(&st.snapshot);
    let bound = memory::bind(&tree, lives);
    for it in items.iter_mut().filter(|i| i.depth == 0) {
        if let Some(b) = bound.iter().find(|b| b.session_id == it.session_id) {
            it.memory = Some(session_memory(it, b));
        }
    }
}

/// Desktopのカード。Desktopが動いていなければ`None`。
pub fn desktop(st: &MemoryState, lives: &[LiveProc]) -> Option<DesktopMemoryVm> {
    let tree = Tree::new(&st.snapshot);
    let bound = memory::bind(&tree, lives);
    let (victim, rss) = memory::desktop(&tree, &bound)?;
    let text = fmt_bytes(rss);
    Some(DesktopMemoryVm {
        exit: ConfirmExit {
            victim,
            title: "Claude Desktop".into(),
            memory: text.clone(),
            resume: None,
        },
        text,
    })
}

fn session_memory(it: &RunItem, b: &Bound) -> SessionMemory {
    let text = fmt_bytes(b.rss);
    let resume = (b.victim.target == Target::TerminalSession)
        .then(|| resume_command(&b.session_id, b.config_dir.as_deref(), cfg!(windows)))
        .flatten();
    SessionMemory {
        desktop: b.victim.target == Target::DesktopSession,
        exit: ConfirmExit {
            victim: b.victim,
            title: it.title.clone(),
            memory: text.clone(),
            resume,
        },
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::activity::RunKind;
    use crate::test_support::{FakeProcessTree, gui_deps_with_procs};
    use std::path::PathBuf;
    use std::sync::Arc;

    const MIB: u64 = 1 << 20;
    const TERM: &str = "/Users/u/.local/bin/claude";
    const DESK: &str = "/Applications/Claude.app/Contents/MacOS/Claude";

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_000_000, 0).unwrap()
    }

    fn p(pid: u32, parent: u32, mib: u64, start: i64, exe: &str) -> ProcEntry {
        ProcEntry {
            pid,
            parent: Some(parent),
            rss: mib * MIB,
            start_time: start as u64,
            exe: Some(PathBuf::from(exe)),
        }
    }

    fn item(sid: &str, depth: u8) -> RunItem {
        RunItem {
            session_id: sid.into(),
            kind: RunKind::Interactive,
            state: String::new(),
            title: format!("t-{sid}"),
            project: String::new(),
            context: String::new(),
            elapsed: String::new(),
            tokens: String::new(),
            cost: String::new(),
            detail: None,
            depth,
            memory: None,
        }
    }

    fn live(sid: &str, pid: u32, started: i64) -> LiveProc {
        LiveProc {
            session_id: sid.into(),
            pid,
            started_at: DateTime::from_timestamp(started, 0),
            config_dir: None,
        }
    }

    fn state_with(procs: Vec<ProcEntry>) -> MemoryState {
        MemoryState {
            snapshot: procs,
            taken_at: Some(t0()),
            ..MemoryState::default()
        }
    }

    fn ask(v: Victim, title: &str, resume: Option<&str>) -> MemAction {
        MemAction::Ask(ConfirmExit {
            victim: v,
            title: title.into(),
            memory: "1MB".into(),
            resume: resume.map(str::to_string),
        })
    }

    #[test]
    fn attaches_memory_only_to_bound_top_level_rows() {
        let s = t0().timestamp() - 600;
        let st = state_with(vec![p(10, 1, 400, s, TERM), p(11, 10, 100, s, "/bin/zsh")]);
        let mut items = vec![item("s1", 0), item("s1", 1), item("s2", 0)];
        attach(&mut items, &st, &[live("s1", 10, s + 2)]);
        let m = items[0].memory.as_ref().unwrap();
        assert_eq!(m.text, "500MB");
        assert!(!m.desktop);
        assert_eq!(
            m.exit.victim,
            Victim {
                pid: 10,
                start_time: s as u64,
                target: Target::TerminalSession
            }
        );
        assert_eq!(m.exit.title, "t-s1");
        assert_eq!(m.exit.memory, "500MB");
        assert_eq!(m.exit.resume.as_deref(), Some("claude --resume s1"));
        assert!(items[1].memory.is_none(), "サブエージェント");
        assert!(items[2].memory.is_none(), "結び付けなし");
    }

    #[test]
    fn code_tab_session_is_marked_desktop_without_resume_command() {
        let s = t0().timestamp() - 60;
        let exe = "/U/Library/Application Support/Claude/claude-code/2.1.284/claude.app/Contents/MacOS/claude";
        let st = state_with(vec![p(20, 1, 300, s, exe)]);
        let mut items = vec![item("c", 0)];
        attach(&mut items, &st, &[live("c", 20, s)]);
        let m = items[0].memory.as_ref().unwrap();
        assert!(m.desktop);
        assert_eq!(m.exit.resume, None);
    }

    #[test]
    fn builds_desktop_card() {
        let st = state_with(vec![p(100, 1, 1229, 50, DESK)]);
        let d = desktop(&st, &[]).unwrap();
        assert_eq!(d.text, "1.2GB");
        assert_eq!(d.exit.title, "Claude Desktop");
        assert_eq!(d.exit.memory, "1.2GB");
        assert_eq!(d.exit.resume, None);
        assert_eq!(
            d.exit.victim,
            Victim {
                pid: 100,
                start_time: 50,
                target: Target::Desktop
            }
        );
        assert!(desktop(&state_with(vec![]), &[]).is_none());
    }

    #[test]
    fn takes_snapshot_only_once_per_refresh_period() {
        let procs = Arc::new(FakeProcessTree::default());
        let (deps, _db, _home) = gui_deps_with_procs(procs.clone());
        let mut st = MemoryState::default();
        *procs.procs.lock().unwrap() = vec![p(1, 0, 1, 0, TERM)];
        assert_eq!(st.refresh_if_due(&deps, t0()), None);
        assert_eq!(st.snapshot.len(), 1);
        procs.procs.lock().unwrap().clear();
        st.refresh_if_due(&deps, t0() + Duration::seconds(REFRESH_SECS - 1));
        assert_eq!(st.snapshot.len(), 1, "まだ取り直さない");
        st.refresh_if_due(&deps, t0() + Duration::seconds(REFRESH_SECS));
        assert!(st.snapshot.is_empty());
    }

    #[test]
    fn requests_exit_after_confirm_and_not_after_cancel() {
        let procs = Arc::new(FakeProcessTree::default());
        let (deps, _db, _home) = gui_deps_with_procs(procs.clone());
        let mut st = MemoryState::default();
        let v = Victim {
            pid: 10,
            start_time: 5,
            target: Target::TerminalSession,
        };
        let a = ask(v, "設計", Some("claude --resume x"));
        assert_eq!(st.handle(&deps, a.clone(), t0()), None);
        assert_eq!(st.confirm().map(|c| c.victim), Some(v));
        assert_eq!(st.handle(&deps, MemAction::Cancel, t0()), None);
        assert!(st.confirm().is_none());
        assert!(procs.exits.lock().unwrap().is_empty());
        assert_eq!(
            st.handle(&deps, MemAction::Confirm, t0()),
            None,
            "確認中でなければ何もしない"
        );
        st.handle(&deps, a, t0());
        assert_eq!(st.poll(), None, "要求の前は受け取るものがない");
        assert_eq!(
            st.handle(&deps, MemAction::Confirm, t0()).as_deref(),
            Some("設計 に終了を要求しています")
        );
        assert_eq!(
            st.wait_jobs().as_deref(),
            Some("設計 に終了を要求しました。再開するには次を実行します: claude --resume x")
        );
        assert_eq!(*procs.exits.lock().unwrap(), [v]);
        assert!(st.confirm().is_none());
        st.handle(&deps, ask(v, "設計", None), t0());
        st.handle(&deps, MemAction::Confirm, t0());
        assert_eq!(st.wait_jobs().as_deref(), Some("設計 に終了を要求しました"));
        *procs.fail.lock().unwrap() = Some(ExitError::Gone);
        st.handle(&deps, ask(v, "設計", None), t0());
        st.handle(&deps, MemAction::Confirm, t0());
        assert_eq!(st.wait_jobs().as_deref(), Some("すでに終了しています"));
        assert_eq!(st.wait_jobs(), None, "受け取り済みの要求は残らない");
        assert_eq!(st.pending.len(), 2, "失敗した要求は猶予の判定に入れない");
    }

    #[test]
    fn reports_still_running_after_grace_period() {
        let procs = Arc::new(FakeProcessTree::default());
        let (deps, _db, _home) = gui_deps_with_procs(procs.clone());
        let mut st = MemoryState::default();
        let a = Victim {
            pid: 10,
            start_time: 5,
            target: Target::TerminalSession,
        };
        let b = Victim {
            pid: 20,
            start_time: 6,
            target: Target::TerminalSession,
        };
        *procs.procs.lock().unwrap() = vec![p(10, 1, 1, 5, TERM), p(20, 1, 1, 6, TERM)];
        for (v, t) in [(a, "A"), (b, "B")] {
            st.handle(&deps, ask(v, t, None), t0());
            st.handle(&deps, MemAction::Confirm, t0());
            st.wait_jobs();
        }
        let early = t0() + Duration::seconds(EXIT_GRACE_SECS - 1);
        assert_eq!(st.refresh_if_due(&deps, early), None, "猶予の中");
        assert_eq!(st.pending.len(), 2);
        procs.procs.lock().unwrap().retain(|e| e.pid != 20);
        let later = early + Duration::seconds(REFRESH_SECS);
        assert_eq!(
            st.refresh_if_due(&deps, later).as_deref(),
            Some("A に終了を要求しましたが、まだ動いています")
        );
        assert!(st.pending.is_empty(), "判定したものは消す");
    }

    #[test]
    fn reports_every_still_running_target() {
        let procs = Arc::new(FakeProcessTree::default());
        let (deps, _db, _home) = gui_deps_with_procs(procs.clone());
        let mut st = MemoryState::default();
        *procs.procs.lock().unwrap() = vec![p(10, 1, 1, 5, TERM), p(20, 1, 1, 6, TERM)];
        for (pid, start, t) in [(10, 5, "A"), (20, 6, "B")] {
            let v = Victim {
                pid,
                start_time: start,
                target: Target::TerminalSession,
            };
            st.handle(&deps, ask(v, t, None), t0());
            st.handle(&deps, MemAction::Confirm, t0());
            st.wait_jobs();
        }
        let at = t0() + Duration::seconds(EXIT_GRACE_SECS);
        let msg = st.refresh_if_due(&deps, at).unwrap();
        assert!(msg.contains("A に") && msg.contains("B に"), "{msg}");
    }

    #[test]
    fn reused_pid_is_not_reported_as_still_running() {
        let procs = Arc::new(FakeProcessTree::default());
        let (deps, _db, _home) = gui_deps_with_procs(procs.clone());
        let mut st = MemoryState::default();
        let v = Victim {
            pid: 30,
            start_time: 7,
            target: Target::TerminalSession,
        };
        st.handle(&deps, ask(v, "C", None), t0());
        st.handle(&deps, MemAction::Confirm, t0());
        st.wait_jobs();
        *procs.procs.lock().unwrap() = vec![p(30, 1, 1, 99, TERM)];
        let at = t0() + Duration::seconds(EXIT_GRACE_SECS);
        assert_eq!(st.refresh_if_due(&deps, at), None);
    }

    #[test]
    fn poll_returns_result_once_the_request_finishes() {
        let procs = Arc::new(FakeProcessTree::default());
        let (deps, _db, _home) = gui_deps_with_procs(procs.clone());
        let mut st = MemoryState::default();
        let v = Victim {
            pid: 10,
            start_time: 5,
            target: Target::TerminalSession,
        };
        st.handle(&deps, ask(v, "D", None), t0());
        st.handle(&deps, MemAction::Confirm, t0());
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let msg = loop {
            if let Some(m) = st.poll() {
                break m;
            }
            assert!(std::time::Instant::now() < until, "要求が終わりません");
            std::thread::yield_now();
        };
        assert_eq!(msg, "D に終了を要求しました");
        assert_eq!(*procs.exits.lock().unwrap(), [v]);
        assert_eq!(st.pending.len(), 1);
    }

    #[test]
    fn reports_interrupted_request_when_worker_dies() {
        let procs = Arc::new(FakeProcessTree::default());
        procs.panic.store(true, std::sync::atomic::Ordering::SeqCst);
        let (deps, _db, _home) = gui_deps_with_procs(procs);
        let mut st = MemoryState::default();
        let v = Victim {
            pid: 10,
            start_time: 5,
            target: Target::TerminalSession,
        };
        st.handle(&deps, ask(v, "E", None), t0());
        st.handle(&deps, MemAction::Confirm, t0());
        assert_eq!(
            st.wait_jobs().as_deref(),
            Some("E の終了の要求が中断しました")
        );
        assert!(st.pending.is_empty());
    }

    #[test]
    fn reads_live_sessions_of_all_profiles() {
        let procs = Arc::new(FakeProcessTree::default());
        let (deps, _db, _home) = gui_deps_with_procs(procs);
        deps.profiles.ensure_default().unwrap();
        let dir = deps.home.join(".claude/sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("10.json"),
            r#"{"pid":10,"sessionId":"s1","startedAt":1790000000000}"#,
        )
        .unwrap();
        let l = lives(&deps);
        assert_eq!(l.len(), 1);
        assert_eq!(
            (l[0].pid, l[0].session_id.as_str(), l[0].config_dir.clone()),
            (10, "s1", None)
        );
        assert_eq!(l[0].started_at, DateTime::from_timestamp(1_790_000_000, 0));
    }
}
