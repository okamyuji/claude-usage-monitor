//! `cumon gui`の起動。

use crate::models::ports::GuiLauncher;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// 実行ファイルを`gui`付きで子プロセスとして起動する。GUIはデーモンと別プロセスにし、閉じたら描画のメモリをOSに返すため（spec 3章）。
pub struct ExeGuiLauncher {
    exe: PathBuf,
}

impl ExeGuiLauncher {
    /// 起動する実行ファイル（通常は自分自身）を指定する。
    pub fn new(exe: PathBuf) -> Self {
        Self { exe }
    }
}

impl GuiLauncher for ExeGuiLauncher {
    fn open(&self) -> Result<(), String> {
        Command::new(&self.exe)
            .arg("gui")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("GUIを起動できません: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_reports_spawn_failure_and_success() {
        assert!(
            ExeGuiLauncher::new("/nonexistent/cumon".into())
                .open()
                .unwrap_err()
                .contains("GUIを起動できません")
        );
        assert!(ExeGuiLauncher::new("/usr/bin/true".into()).open().is_ok());
    }
}
