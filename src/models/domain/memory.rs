//! プロセスのメモリの集計と、終了を要求してよい対象かの判定。OSには触らない。
use chrono::{DateTime, Utc};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// セッションのファイルの`startedAt`が、プロセスの開始から何秒後までなら同じセッションとみなすか。
/// 実測の差は1秒から4秒だった。pidが再利用されたプロセスは元のセッションより後に始まるので、下限で弾ける。
pub const BIND_WINDOW_SECS: i64 = 30;

/// スナップショットのプロセス1つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcEntry {
    /// プロセスID。
    pub pid: u32,
    /// 親のプロセスID。子孫をたどるのに使う。
    pub parent: Option<u32>,
    /// 常駐メモリ（バイト）。
    pub rss: u64,
    /// 開始時刻（UNIX秒）。pidの再利用を見分けるのに使う。
    pub start_time: u64,
    /// 実行ファイルのパス。種別の判定に使う。
    pub exe: Option<PathBuf>,
}

/// 実行パスから決めたプロセスの種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcKind {
    /// 端末で動くClaude Code。
    TerminalSession,
    /// DesktopのCodeタブが起動したClaude Code。
    DesktopSession,
    /// Claude Desktopの本体。
    Desktop,
    /// 上のどれでもない。終了を要求しない。
    Other,
}

/// 終了を要求できる対象。`Other`を型として持たないので、無関係なプロセスを指定できない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// 端末のセッション。
    TerminalSession,
    /// Codeタブのセッション。
    DesktopSession,
    /// Desktop全体。
    Desktop,
}

/// 確認ダイアログを開いた時点で固定した対象。行の並びが変わっても同じプロセスを指すため、位置ではなくpidと開始時刻を持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Victim {
    /// プロセスID。
    pub pid: u32,
    /// 開始時刻（UNIX秒）。
    pub start_time: u64,
    /// 対象の種別。
    pub target: Target,
}

/// 終了の要求の失敗。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExitError {
    /// 送る直前に照合したら、別のプロセスに入れ替わっていた。
    #[error("対象のプロセスが入れ替わったため、終了を要求しませんでした")]
    Mismatch,
    /// 送る前に終わっていた。
    #[error("すでに終了しています")]
    Gone,
    /// OSが要求を受け付けなかった。`sysinfo`は権限不足と他の失敗を区別できないため1つにまとめる。
    #[error("終了を要求できませんでした: {0}")]
    Failed(String),
}

/// `sessions/<pid>.json`の1件。
#[derive(Debug, Clone, PartialEq)]
pub struct LiveProc {
    /// セッションID。
    pub session_id: String,
    /// プロセスID。
    pub pid: u32,
    /// セッションの開始時刻。
    pub started_at: Option<DateTime<Utc>>,
    /// プロファイルの設定ディレクトリ。既定のプロファイルは`None`。再開コマンドに使う。
    pub config_dir: Option<PathBuf>,
}

/// プロセスと結び付いたセッション。
#[derive(Debug, Clone, PartialEq)]
pub struct Bound {
    /// セッションID。
    pub session_id: String,
    /// 終了を要求するときの対象。
    pub victim: Victim,
    /// 子孫を含むRSSの合計。同じセッションのプロセスが複数あるときは、全員の分を足す。
    pub rss: u64,
    /// このセッションに属するプロセス。Desktopの合計から除くのに使う。
    pub members: Vec<u32>,
    /// プロファイルの設定ディレクトリ。
    pub config_dir: Option<PathBuf>,
}

/// パスを要素に分ける。`Path::components`は実行中のOSの区切りしか扱わず、macOSのテストでWindowsのパスを分けられないため、両方の区切りで分ける。
fn parts(exe: &Path) -> Vec<&str> {
    exe.to_str()
        .map(|s| s.split(['/', '\\']).filter(|x| !x.is_empty()).collect())
        .unwrap_or_default()
}

/// 実行パスから種別を決める。
pub fn classify(exe: Option<&Path>) -> ProcKind {
    let v = exe.map(parts).unwrap_or_default();
    let Some((name, dirs)) = v.split_last() else {
        return ProcKind::Other;
    };
    let adjacent = |a: &str, b: &str| dirs.windows(2).any(|w| w[0] == a && w[1] == b);
    match *name {
        "claude" | "claude.exe" if adjacent("Claude", "claude-code") => ProcKind::DesktopSession,
        "claude" | "claude.exe" => ProcKind::TerminalSession,
        "Claude" if dirs.ends_with(&["Claude.app", "Contents", "MacOS"]) => ProcKind::Desktop,
        "Claude.exe" if dirs.contains(&"AnthropicClaude") => ProcKind::Desktop,
        _ if dirs.ends_with(&["claude", "versions"]) => ProcKind::TerminalSession,
        _ => ProcKind::Other,
    }
}

fn target_of(k: ProcKind) -> Option<Target> {
    match k {
        ProcKind::TerminalSession => Some(Target::TerminalSession),
        ProcKind::DesktopSession => Some(Target::DesktopSession),
        ProcKind::Desktop => Some(Target::Desktop),
        ProcKind::Other => None,
    }
}

fn is_desktop_crashpad(exe: Option<&Path>) -> bool {
    let v = exe.map(parts).unwrap_or_default();
    v.last() == Some(&"chrome_crashpad_handler") && v.contains(&"Claude.app")
}

/// 送る直前の照合。種別と開始時刻がどちらも一致するときだけ通す。
pub fn verify(entry: &ProcEntry, v: &Victim) -> Result<(), ExitError> {
    let same = target_of(classify(entry.exe.as_deref())) == Some(v.target)
        && entry.start_time == v.start_time;
    if same {
        Ok(())
    } else {
        Err(ExitError::Mismatch)
    }
}

/// 親子の表。
pub struct Tree<'a> {
    by_pid: HashMap<u32, &'a ProcEntry>,
    kids: HashMap<u32, Vec<u32>>,
}

impl<'a> Tree<'a> {
    /// スナップショットから作る。
    pub fn new(snap: &'a [ProcEntry]) -> Self {
        let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
        for e in snap {
            if let Some(pp) = e.parent {
                kids.entry(pp).or_default().push(e.pid);
            }
        }
        Self {
            by_pid: snap.iter().map(|e| (e.pid, e)).collect(),
            kids,
        }
    }

    /// pidのプロセス。
    pub fn get(&self, pid: u32) -> Option<&'a ProcEntry> {
        self.by_pid.get(&pid).copied()
    }

    /// `pid`の祖先に`root`があるか。循環した表でも止まるよう、訪れたpidを覚える。
    pub fn is_under(&self, pid: u32, root: u32) -> bool {
        let mut seen = HashSet::new();
        let mut cur = self.get(pid).and_then(|e| e.parent);
        while let Some(p) = cur.filter(|p| seen.insert(*p)) {
            if p == root {
                return true;
            }
            cur = self.get(p).and_then(|e| e.parent);
        }
        false
    }

    /// `root`と子孫のRSSの合計。`stop`に入るpidの下へは進まない。
    /// 各プロセスを別々の時点で読むため親子の表が循環することがあり、訪れたpidを覚えて2回数えない。
    pub fn subtree_rss(&self, root: u32, stop: &HashSet<u32>) -> u64 {
        let mut seen = HashSet::new();
        let mut stack = vec![root];
        let mut sum = 0;
        while let Some(pid) = stack.pop() {
            if !seen.insert(pid) {
                continue;
            }
            let Some(e) = self.get(pid) else { continue };
            sum += e.rss;
            let next = self.kids.get(&pid).into_iter().flatten();
            stack.extend(next.filter(|k| !stop.contains(k)));
        }
        sum
    }
}

fn session_target(e: &ProcEntry) -> Option<Target> {
    target_of(classify(e.exe.as_deref())).filter(|t| *t != Target::Desktop)
}

fn is_bound(e: &ProcEntry, started_at: Option<DateTime<Utc>>) -> bool {
    let start = e.start_time as i64;
    session_target(e).is_some()
        && started_at.is_some_and(|t| (start..=start + BIND_WINDOW_SECS).contains(&t.timestamp()))
}

/// セッションとプロセスを結び付け、行ごとのRSSを出す。
/// 同じ`session_id`が複数あるときは`startedAt`が新しいものを使う。
/// 入れ子のセッションを二重に数えないよう、ほかの結び付いたセッションの下へは進まない。
pub fn bind(tree: &Tree, lives: &[LiveProc]) -> Vec<Bound> {
    let mut cands: Vec<(&LiveProc, &ProcEntry, Target)> = lives
        .iter()
        .filter_map(|l| {
            let e = tree.get(l.pid).filter(|e| is_bound(e, l.started_at))?;
            Some((l, e, session_target(e)?))
        })
        .collect();
    // 同じ時刻のファイルが2つあっても、読んだ順（`read_dir`の順は決まらない）に左右されないよう、pidまで含めて並べる。
    cands.sort_by_key(|(l, _, _)| (l.session_id.as_str(), Reverse((l.started_at, l.pid))));
    // 同じpidを別のセッションが名乗っていても、1つのセッションにだけ数える。
    // 先に各セッションの最新のプロセスに割り当ててから、残りを割り当てる。そうしないと、名前順で先のセッションが別のセッションの本体を取ってしまう。
    let mut owner: HashSet<u32> = HashSet::new();
    let groups: Vec<&[(&LiveProc, &ProcEntry, Target)]> = cands
        .chunk_by(|a, b| a.0.session_id == b.0.session_id)
        .filter(|g| owner.insert(g[0].1.pid))
        .collect();
    let members: Vec<Vec<u32>> = groups
        .iter()
        .map(|g| {
            let rest = g[1..].iter().map(|c| c.1.pid).filter(|p| owner.insert(*p));
            std::iter::once(g[0].1.pid)
                .chain(rest.collect::<Vec<_>>())
                .collect()
        })
        .collect();
    groups
        .into_iter()
        .zip(members)
        .map(|(g, members)| {
            let (l, e, target) = g[0];
            Bound {
                session_id: l.session_id.clone(),
                victim: Victim {
                    pid: e.pid,
                    start_time: e.start_time,
                    target,
                },
                rss: members.iter().map(|m| tree.subtree_rss(*m, &owner)).sum(),
                members,
                config_dir: l.config_dir.clone(),
            }
        })
        .collect()
}

/// Desktop全体の対象とRSS。結び付いたセッションの下へは進まない。crashpadは親がPID 1なので別に加える。
pub fn desktop(tree: &Tree, bound: &[Bound]) -> Option<(Victim, u64)> {
    let root = tree
        .by_pid
        .values()
        .filter(|e| classify(e.exe.as_deref()) == ProcKind::Desktop)
        .min_by_key(|e| e.pid)?;
    let stop: HashSet<u32> = bound
        .iter()
        .flat_map(|b| b.members.iter().copied())
        .collect();
    // 本体の子のcrashpadは木の中で数える。本体より前に起動したものは、前回のDesktopの残骸とみなして足さない。
    let crash: u64 = tree
        .by_pid
        .values()
        .filter(|e| is_desktop_crashpad(e.exe.as_deref()))
        .filter(|e| e.start_time >= root.start_time && !tree.is_under(e.pid, root.pid))
        .map(|e| e.rss)
        .sum();
    let v = Victim {
        pid: root.pid,
        start_time: root.start_time,
        target: Target::Desktop,
    };
    Some((v, tree.subtree_rss(root.pid, &stop) + crash))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1 << 20;
    const TERM: &str = "/Users/u/.local/bin/claude";
    const DESK_SESSION: &str = "/Users/u/Library/Application Support/Claude/claude-code/2.1.284/claude.app/Contents/MacOS/claude";
    const DESK: &str = "/Applications/Claude.app/Contents/MacOS/Claude";
    const HELPER: &str = "/Applications/Claude.app/Contents/Frameworks/Claude Helper.app/Contents/MacOS/Claude Helper";
    const CRASHPAD: &str = "/Applications/Claude.app/Contents/Frameworks/Electron Framework.framework/Helpers/chrome_crashpad_handler";

    fn p(pid: u32, parent: u32, mib: u64, start: u64, exe: &str) -> ProcEntry {
        ProcEntry {
            pid,
            parent: Some(parent),
            rss: mib * MIB,
            start_time: start,
            exe: Some(PathBuf::from(exe)),
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

    fn k(s: &str) -> ProcKind {
        classify(Some(Path::new(s)))
    }

    #[test]
    fn classifies_by_executable_path() {
        assert_eq!(k(TERM), ProcKind::TerminalSession);
        assert_eq!(
            k("/home/u/.local/share/claude/versions/2.1.284"),
            ProcKind::TerminalSession
        );
        assert_eq!(
            k(r"C:\Users\u\.local\bin\claude.exe"),
            ProcKind::TerminalSession
        );
        assert_eq!(k(DESK_SESSION), ProcKind::DesktopSession);
        assert_eq!(
            k(r"C:\Users\u\AppData\Roaming\Claude\claude-code\2.1.284\claude.exe"),
            ProcKind::DesktopSession
        );
        assert_eq!(k(DESK), ProcKind::Desktop);
        assert_eq!(
            k(r"C:\Users\u\AppData\Local\AnthropicClaude\Claude.exe"),
            ProcKind::Desktop
        );
        assert_eq!(k(HELPER), ProcKind::Other);
        assert_eq!(k("/usr/bin/claudex"), ProcKind::Other);
        assert_eq!(
            k("/opt/Claude/x/claude-code/claude"),
            ProcKind::TerminalSession,
            "Claudeとclaude-codeが隣り合わない"
        );
        assert_eq!(
            k("/opt/claude/versions/x/y"),
            ProcKind::Other,
            "versionsの直下ではない"
        );
        assert_eq!(
            k("/Applications/Claude.app/Contents/MacOS/claude"),
            ProcKind::TerminalSession,
            "大文字と小文字を区別する"
        );
        assert_eq!(
            k("/Applications/X.app/Contents/MacOS/Claude"),
            ProcKind::Other
        );
        assert_eq!(k(r"C:\x\Claude.exe"), ProcKind::Other);
        assert_eq!(k("/"), ProcKind::Other);
        assert_eq!(classify(None), ProcKind::Other);
    }

    #[test]
    fn verify_passes_only_when_kind_and_start_time_match() {
        let e = p(10, 1, 1, 100, TERM);
        let ok = Victim {
            pid: 10,
            start_time: 100,
            target: Target::TerminalSession,
        };
        assert_eq!(verify(&e, &ok), Ok(()));
        let later = Victim {
            start_time: 101,
            ..ok
        };
        assert_eq!(verify(&e, &later), Err(ExitError::Mismatch));
        for t in [Target::Desktop, Target::DesktopSession] {
            let v = Victim { target: t, ..ok };
            assert_eq!(verify(&e, &v), Err(ExitError::Mismatch));
        }
        let d = p(20, 1, 1, 5, DESK);
        let dv = Victim {
            pid: 20,
            start_time: 5,
            target: Target::Desktop,
        };
        assert_eq!(verify(&d, &dv), Ok(()));
        let s = p(30, 1, 1, 7, DESK_SESSION);
        let sv = Victim {
            pid: 30,
            start_time: 7,
            target: Target::DesktopSession,
        };
        assert_eq!(verify(&s, &sv), Ok(()));
        let other = p(40, 1, 1, 9, HELPER);
        for t in [
            Target::TerminalSession,
            Target::DesktopSession,
            Target::Desktop,
        ] {
            let v = Victim {
                pid: 40,
                start_time: 9,
                target: t,
            };
            assert_eq!(verify(&other, &v), Err(ExitError::Mismatch));
        }
        let no_exe = ProcEntry {
            exe: None,
            ..e.clone()
        };
        assert_eq!(verify(&no_exe, &ok), Err(ExitError::Mismatch));
    }

    #[test]
    fn sums_rss_of_descendants() {
        let snap = vec![
            p(10, 1, 100, 0, TERM),
            p(11, 10, 20, 0, "/bin/zsh"),
            p(12, 11, 3, 0, "/bin/sleep"),
            p(13, 10, 4, 0, "/usr/bin/node"),
            p(99, 1, 1000, 0, "/bin/other"),
        ];
        let t = Tree::new(&snap);
        let none = HashSet::new();
        assert_eq!(t.subtree_rss(12, &none), 3 * MIB, "親だけ");
        assert_eq!(t.subtree_rss(11, &none), 23 * MIB, "孫まで");
        assert_eq!(t.subtree_rss(10, &none), 127 * MIB, "枝分かれ");
        assert_eq!(
            t.subtree_rss(10, &HashSet::from([11])),
            104 * MIB,
            "stopの下へ進まない"
        );
        assert_eq!(
            t.subtree_rss(11, &HashSet::from([11])),
            23 * MIB,
            "起点はstopでも数える"
        );
        assert_eq!(t.subtree_rss(7, &none), 0, "スナップショットにない");
        assert_eq!(t.get(13).map(|e| e.rss), Some(4 * MIB));
        assert!(t.get(7).is_none());
    }

    #[test]
    fn stops_on_cyclic_parent_table() {
        let snap = vec![p(10, 11, 1, 0, TERM), p(11, 10, 2, 0, "/bin/zsh")];
        assert_eq!(Tree::new(&snap).subtree_rss(10, &HashSet::new()), 3 * MIB);
    }

    #[test]
    fn binds_only_inside_start_time_window() {
        let snap = vec![p(10, 1, 5, 1000, TERM)];
        let t = Tree::new(&snap);
        let ids = |lives: &[LiveProc]| {
            bind(&t, lives)
                .into_iter()
                .map(|b| b.session_id)
                .collect::<Vec<_>>()
        };
        assert!(ids(&[live("a", 10, 999)]).is_empty(), "プロセスより前");
        assert_eq!(ids(&[live("a", 10, 1000)]), ["a"], "ちょうど");
        assert_eq!(ids(&[live("a", 10, 1030)]), ["a"], "30秒後");
        assert!(ids(&[live("a", 10, 1031)]).is_empty(), "31秒後");
        let no_start = LiveProc {
            started_at: None,
            ..live("a", 10, 1000)
        };
        assert!(ids(&[no_start]).is_empty(), "開始時刻なし");
        assert!(
            ids(&[live("a", 11, 1000)]).is_empty(),
            "スナップショットにない"
        );
        let b = &bind(&t, &[live("a", 10, 1002)])[0];
        assert_eq!(
            b.victim,
            Victim {
                pid: 10,
                start_time: 1000,
                target: Target::TerminalSession
            }
        );
        assert_eq!(b.rss, 5 * MIB);
    }

    #[test]
    fn does_not_bind_other_or_desktop_pids() {
        let snap = vec![p(10, 1, 5, 1000, HELPER), p(20, 1, 5, 1000, DESK)];
        let t = Tree::new(&snap);
        assert!(bind(&t, &[live("a", 10, 1001), live("b", 20, 1001)]).is_empty());
    }

    #[test]
    fn keeps_newest_duplicate_and_does_not_double_count_nested_sessions() {
        let snap = vec![
            p(10, 1, 100, 1000, TERM),
            p(11, 10, 1, 1000, "/bin/zsh"),
            p(12, 11, 50, 2000, TERM),
            p(20, 1, 7, 3000, TERM),
        ];
        let t = Tree::new(&snap);
        let mut got = bind(
            &t,
            &[
                live("outer", 10, 1001),
                live("inner", 12, 2001),
                live("dup", 10, 1002),
                live("dup", 20, 3001),
            ],
        );
        got.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        let r: Vec<(&str, u32, u64)> = got
            .iter()
            .map(|b| (b.session_id.as_str(), b.victim.pid, b.rss / MIB))
            .collect();
        assert_eq!(r, [("dup", 20, 7), ("inner", 12, 50), ("outer", 10, 101)]);
    }

    #[test]
    fn duplicate_with_same_start_picks_larger_pid_regardless_of_order() {
        let snap = vec![p(10, 1, 1, 1000, TERM), p(20, 1, 2, 1000, TERM)];
        let t = Tree::new(&snap);
        for lives in [
            [live("dup", 10, 1001), live("dup", 20, 1001)],
            [live("dup", 20, 1001), live("dup", 10, 1001)],
        ] {
            let got = bind(&t, &lives);
            assert_eq!(got.len(), 1);
            assert_eq!(got[0].victim.pid, 20);
        }
    }

    #[test]
    fn sums_every_process_of_same_session_into_newest_row() {
        let apart = vec![p(30, 1, 10, 100, TERM), p(40, 1, 20, 200, TERM)];
        let t = Tree::new(&apart);
        let b = bind(&t, &[live("s", 30, 101), live("s", 40, 201)]);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].victim.pid, 40, "終了の対象は新しいほう");
        assert_eq!(b[0].rss, 30 * MIB);
        let mut m = b[0].members.clone();
        m.sort();
        assert_eq!(m, [30, 40]);
        let nested = vec![
            p(30, 1, 10, 100, TERM),
            p(31, 30, 1, 100, "/bin/zsh"),
            p(40, 31, 20, 200, TERM),
        ];
        let t = Tree::new(&nested);
        let b = bind(&t, &[live("s", 30, 101), live("s", 40, 201)]);
        assert_eq!(b[0].rss, 31 * MIB, "入れ子でも二重に数えない");
    }

    #[test]
    fn desktop_total_skips_every_member_of_bound_sessions() {
        let snap = vec![
            p(100, 1, 200, 50, DESK),
            p(103, 100, 300, 60, DESK_SESSION),
            p(104, 100, 70, 65, DESK_SESSION),
        ];
        let t = Tree::new(&snap);
        let bound = bind(&t, &[live("code", 103, 61), live("code", 104, 66)]);
        assert_eq!(desktop(&t, &bound).unwrap().1, 200 * MIB);
    }

    #[test]
    fn crashpad_inside_tree_or_older_than_desktop_is_not_added() {
        let snap = vec![
            p(100, 1, 200, 50, DESK),
            p(105, 100, 9, 50, CRASHPAD),
            p(106, 1, 8, 40, CRASHPAD),
            p(107, 1, 7, 55, CRASHPAD),
        ];
        let t = Tree::new(&snap);
        assert_eq!(
            desktop(&t, &[]).unwrap().1,
            (200 + 9 + 7) * MIB,
            "子のcrashpadは木の中で1回だけ、Desktopより古い残骸は足さない"
        );
    }

    #[test]
    fn desktop_total_excludes_bound_sessions_and_adds_crashpad() {
        let snap = vec![
            p(100, 1, 200, 50, DESK),
            p(101, 100, 60, 50, HELPER),
            p(
                102,
                100,
                1,
                50,
                "/Applications/Claude.app/Contents/Helpers/disclaimer",
            ),
            p(103, 102, 300, 60, DESK_SESSION),
            p(104, 103, 40, 60, "/usr/bin/node"),
            p(105, 1, 9, 50, CRASHPAD),
            p(106, 1, 500, 70, DESK_SESSION),
            p(107, 100, 30, 80, DESK_SESSION),
            p(108, 1, 8, 50, "/opt/x/chrome_crashpad_handler"),
        ];
        let t = Tree::new(&snap);
        let bound = bind(&t, &[live("code", 103, 61), live("orphan", 106, 71)]);
        let (v, rss) = desktop(&t, &bound).unwrap();
        assert_eq!(
            v,
            Victim {
                pid: 100,
                start_time: 50,
                target: Target::Desktop
            }
        );
        assert_eq!(
            rss,
            (200 + 60 + 1 + 9 + 30) * MIB,
            "結び付いた103と孤児の106を除き、結び付かない107を含める"
        );
        let only = vec![p(10, 1, 5, 1000, TERM)];
        assert!(desktop(&Tree::new(&only), &[]).is_none());
    }
}
