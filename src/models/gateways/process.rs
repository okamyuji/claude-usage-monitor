//! OSの時計とプロセス情報。
use crate::models::ports::{Clock, ProcessInfo};
use chrono::{DateTime, Utc};
use std::sync::Mutex;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// システム時計。
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// sysinfoによるプロセス情報。全プロセスの一覧を持たず、問い合わせたpidだけを更新してメモリを抑える。
pub struct SysProcessInfo {
    sys: Mutex<System>,
}

impl SysProcessInfo {
    /// 空の状態で作る。
    pub fn new() -> Self {
        Self {
            sys: Mutex::new(System::new()),
        }
    }

    fn refresh(&self, pid: Pid) -> Option<u64> {
        let mut sys = self.sys.lock().ok()?;
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );
        sys.process(pid).map(|p| p.memory())
    }
}

impl Default for SysProcessInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessInfo for SysProcessInfo {
    fn is_alive(&self, pid: u32) -> bool {
        self.refresh(Pid::from_u32(pid)).is_some()
    }

    fn self_rss_bytes(&self) -> u64 {
        self.refresh(Pid::from_u32(std::process::id())).unwrap_or(0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ports::{Clock, ProcessInfo};

    #[test]
    fn own_process_is_alive_and_has_rss() {
        let p = SysProcessInfo::new();
        assert!(p.is_alive(std::process::id()));
        assert!(!p.is_alive(u32::MAX - 1));
        assert!(
            p.self_rss_bytes() > 1024 * 1024,
            "テストの実行中のプロセスは1MB以上使う"
        );
    }

    #[test]
    fn system_clock_is_close_to_now() {
        let d = SystemClock.now() - chrono::Utc::now();
        assert!(d.num_seconds().abs() < 5);
    }
}
