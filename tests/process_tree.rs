//! `SysProcessTree`と`SystemExitSender`を、テストが起動した子プロセスで確かめる。
//! 子にはこのテストのハーネス自身を`claude`という名前で複製して使う。macOSは`/bin/sleep`の複製を起動直後に止め、
//! Linuxはシンボリックリンクをrealpathで元の名前に戻すため、どちらも使えない。
use claude_usage_monitor::models::domain::memory::{ExitError, ProcEntry, Target, Victim};
use claude_usage_monitor::models::gateways::process_tree::{SysProcessTree, SystemExitSender};
use claude_usage_monitor::models::ports::{ExitSender, ProcessTree};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SLEEPER: &str = "sleeper";

/// 子として起動されたときだけ走る。通常の実行では`#[ignore]`で飛ばす。
#[test]
#[ignore]
fn sleeper() {
    std::thread::sleep(Duration::from_secs(60));
}

/// 複製と起動を直列にする。Linuxでは、別スレッドのforkが書き込み中のファイルを開いたままにすると`ETXTBSY`で起動に失敗するため。
static SPAWN: Mutex<()> = Mutex::new(());

/// `ETXTBSY`のerrno。LinuxとmacOSで同じ値。
const ETXTBSY: i32 = 26;

/// assertが失敗しても子を残さないよう、捨てるときに止める。
struct Sleeper {
    child: Child,
    _dir: tempfile::TempDir,
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_claude() -> Sleeper {
    let _g = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join(if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    });
    std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
    for _ in 0..20 {
        match Command::new(&exe)
            .args(["--ignored", "--exact", SLEEPER])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => return Sleeper { child, _dir: dir },
            Err(e) if e.raw_os_error() == Some(ETXTBSY) => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(e) => panic!("{e}"),
        }
    }
    panic!("起動できません");
}

#[derive(Default)]
struct Recorder(Mutex<Vec<(u32, Target)>>);

impl ExitSender for Recorder {
    fn send(&self, pid: u32, target: Target) -> Result<(), ExitError> {
        self.0.lock().unwrap().push((pid, target));
        Ok(())
    }
}

fn find(tree: &SysProcessTree, pid: u32) -> ProcEntry {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(e) = tree.snapshot().into_iter().find(|e| e.pid == pid) {
            return e;
        }
        assert!(Instant::now() < until, "スナップショットに子が現れません");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn snapshot_has_child_pid_parent_start_time_and_path() {
    let s = spawn_claude();
    let tree = SysProcessTree::new(Arc::new(Recorder::default()));
    let e = find(&tree, s.child.id());
    assert_eq!(e.parent, Some(std::process::id()));
    assert!(e.start_time > 0);
    assert!(e.rss > 0);
    assert_eq!(e.exe.unwrap().file_stem().unwrap(), "claude");
}

#[test]
fn sender_is_called_only_when_victim_matches() {
    let s = spawn_claude();
    let rec = Arc::new(Recorder::default());
    let tree = SysProcessTree::new(rec.clone());
    let e = find(&tree, s.child.id());
    let ok = Victim {
        pid: e.pid,
        start_time: e.start_time,
        target: Target::TerminalSession,
    };
    let later = Victim {
        start_time: e.start_time + 1,
        ..ok
    };
    assert_eq!(tree.request_exit(&later), Err(ExitError::Mismatch));
    let desk = Victim {
        target: Target::Desktop,
        ..ok
    };
    assert_eq!(tree.request_exit(&desk), Err(ExitError::Mismatch));
    assert!(rec.0.lock().unwrap().is_empty());
    assert_eq!(tree.request_exit(&ok), Ok(()));
    assert_eq!(*rec.0.lock().unwrap(), [(e.pid, Target::TerminalSession)]);
}

#[test]
fn missing_pid_is_gone() {
    let tree = SysProcessTree::new(Arc::new(Recorder::default()));
    let v = Victim {
        pid: u32::MAX - 1,
        start_time: 1,
        target: Target::TerminalSession,
    };
    assert_eq!(tree.request_exit(&v), Err(ExitError::Gone));
}

#[cfg(unix)]
#[test]
fn system_sender_terminates_child_with_sigterm() {
    use std::os::unix::process::ExitStatusExt;
    let mut s = spawn_claude();
    let tree = SysProcessTree::new(Arc::new(Recorder::default()));
    let pid = find(&tree, s.child.id()).pid;
    assert_eq!(SystemExitSender.send(pid, Target::TerminalSession), Ok(()));
    let status = s.child.wait().unwrap();
    assert_eq!(status.signal(), Some(15));
    assert_eq!(
        SystemExitSender.send(pid, Target::TerminalSession),
        Err(ExitError::Gone)
    );
}
