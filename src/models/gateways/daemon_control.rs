//! デーモンの稼働確認と起動。
//!
//! デーモンはロックファイルを持ち続けるため、GUIはロックを取れるかどうかで稼働を判定する。PIDファイルと違い、異常終了してもロックはOSが外す。

use crate::models::ports::DaemonControl;
use std::fs::{File, TryLockError};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// ロックファイルでデーモンを確かめ、実行ファイルを`daemon`付きで起動する。
pub struct LockFileDaemon {
    lock_path: PathBuf,
    exe: PathBuf,
}

impl LockFileDaemon {
    /// ロックファイルと、起動する実行ファイル（通常は自分自身）を指定する。
    pub fn new(lock_path: PathBuf, exe: PathBuf) -> Self {
        Self { lock_path, exe }
    }
}

impl DaemonControl for LockFileDaemon {
    fn is_running(&self) -> bool {
        // 開けない（ディレクトリがない）なら、デーモンはまだ一度も起動していない。
        let Ok(f) = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lock_path)
        else {
            return false;
        };
        // ロックを取れたら誰も持っていない。取ったロックはfの破棄で外れる。
        matches!(f.try_lock(), Err(TryLockError::WouldBlock))
    }

    fn start(&self) -> Result<(), String> {
        Command::new(&self.exe)
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("デーモンを起動できません: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ports::DaemonControl;
    use std::sync::Mutex;

    /// 子プロセスの起動と、ロックの確認を同時に走らせないための鍵。
    /// 起動の瞬間（forkからexecまで）は子がロックファイルのディスクリプタを共有するため、
    /// 並行して走るとロックを外した直後でも保持中に見える。
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn running_only_while_another_handle_holds_the_lock() {
        let _g = SERIAL.lock().unwrap();
        let d = tempfile::tempdir().unwrap();
        let lock = d.path().join("daemon.lock");
        let c = LockFileDaemon::new(lock.clone(), PathBuf::from("/nonexistent/cps"));
        assert!(!c.is_running());
        let held = std::fs::File::create(&lock).unwrap();
        held.try_lock().unwrap();
        assert!(c.is_running());
        drop(held);
        assert!(!c.is_running());
    }

    #[test]
    fn missing_directory_is_not_running() {
        let c = LockFileDaemon::new(
            PathBuf::from("/nonexistent/dir/daemon.lock"),
            PathBuf::from("/bin/true"),
        );
        assert!(!c.is_running());
    }

    #[test]
    fn start_reports_spawn_failure() {
        let _g = SERIAL.lock().unwrap();
        let d = tempfile::tempdir().unwrap();
        let c = LockFileDaemon::new(
            d.path().join("daemon.lock"),
            PathBuf::from("/nonexistent/cps"),
        );
        assert!(c.start().unwrap_err().contains("デーモンを起動できません"));
        let ok = LockFileDaemon::new(d.path().join("daemon.lock"), PathBuf::from("/usr/bin/true"));
        assert!(ok.start().is_ok());
    }
}
