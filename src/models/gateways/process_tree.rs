//! `sysinfo`による全プロセスの一覧と、終了の要求。
use crate::models::domain::memory::{ExitError, ProcEntry, Target, Victim, verify};
use crate::models::domain::memory_text::{QUIT_DESKTOP, quit_result};
use crate::models::ports::{ExitSender, ProcessTree};
use std::process::Command;
use std::sync::{Arc, Mutex};
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind};

/// 取る項目を最小にする。全プロセスを5秒ごとに取るため。
/// 実行パスは毎回取り直す。シェルがforkしてからexecするまでの間に一覧を取ると、子のパスがシェルのまま残り、claudeのセッションと結び付かなくなるため。
/// 811プロセスで取り直しの有無による時間の差はなかった（どちらも約41ms）。
fn kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_memory()
        .with_exe(UpdateKind::Always)
}

fn entry(pid: &Pid, p: &Process) -> ProcEntry {
    ProcEntry {
        pid: pid.as_u32(),
        parent: p.parent().map(|x| x.as_u32()),
        rss: p.memory(),
        start_time: p.start_time(),
        exe: p.exe().map(|x| x.to_path_buf()),
    }
}

/// 本番のプロセス一覧。
pub struct SysProcessTree {
    sys: Mutex<System>,
    sender: Arc<dyn ExitSender>,
}

impl SysProcessTree {
    /// 終了を送る役を受け取って作る。
    pub fn new(sender: Arc<dyn ExitSender>) -> Self {
        Self {
            sys: Mutex::new(System::new()),
            sender,
        }
    }
}

impl ProcessTree for SysProcessTree {
    fn snapshot(&self) -> Vec<ProcEntry> {
        let mut sys = self.sys.lock().unwrap_or_else(|e| e.into_inner());
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind());
        sys.processes()
            .iter()
            .map(|(pid, p)| entry(pid, p))
            .collect()
    }

    fn request_exit(&self, v: &Victim) -> Result<(), ExitError> {
        let e = {
            let mut sys = self.sys.lock().unwrap_or_else(|e| e.into_inner());
            let pid = Pid::from_u32(v.pid);
            sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind());
            sys.process(pid)
                .map(|p| entry(&pid, p))
                .ok_or(ExitError::Gone)?
        };
        verify(&e, v)?;
        self.sender.send(v.pid, v.target)
    }
}

/// OSに終了を要求する。テストから呼ぶと利用者のDesktopが終わるため、mutationの対象から外す（`.cargo/mutants.toml`）。
pub struct SystemExitSender;

impl ExitSender for SystemExitSender {
    fn send(&self, pid: u32, target: Target) -> Result<(), ExitError> {
        // macOSではアプリに通常の終了手順を踏ませる。`cfg!`にするのは、分岐をどのOSでもコンパイルしてカバレッジの計測から外さないため。
        if cfg!(target_os = "macos") && target == Target::Desktop {
            let out = Command::new("osascript")
                .args(["-e", QUIT_DESKTOP])
                .output()
                .map_err(|e| ExitError::Failed(e.to_string()))?;
            return quit_result(out.status.code(), &String::from_utf8_lossy(&out.stderr));
        }
        let mut sys = System::new();
        let p = Pid::from_u32(pid);
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[p]),
            true,
            ProcessRefreshKind::nothing(),
        );
        let proc = sys.process(p).ok_or(ExitError::Gone)?;
        // WindowsのsysinfoはSIGTERMに当たるものを持たず`None`を返すので、`kill`（taskkill /F）に落とす。
        let sent = proc.kill_with(Signal::Term).unwrap_or_else(|| proc.kill());
        if sent {
            Ok(())
        } else {
            Err(ExitError::Failed(
                "OSが終了の要求を受け付けませんでした".into(),
            ))
        }
    }
}
