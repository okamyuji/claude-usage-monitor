//! ログイン時の自動起動。
//!
//! macOSはLaunchAgent、WindowsはRunレジストリ、LinuxはXDG autostartに、`cumon daemon`を登録する。3 OSの違いはauto-launchが吸収する。
use crate::models::ports::Autostart;
use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode};
use std::path::Path;

const APP_NAME: &str = "claude-usage-monitor";

/// auto-launchによる実装。
pub struct SystemAutostart {
    inner: AutoLaunch,
}

impl SystemAutostart {
    /// 登録する実行ファイルを指定して作る。
    pub fn new(exe: &Path) -> Result<Self, String> {
        let inner = AutoLaunchBuilder::new()
            .set_app_name(APP_NAME)
            .set_app_path(&exe.to_string_lossy())
            .set_macos_launch_mode(MacOSLaunchMode::LaunchAgent)
            .set_args(&["daemon"])
            // 異常終了だけ再起動する。トレイの「終了」は0で終わるので、利用者が止めたデーモンは起こし直さない。
            .set_agent_extra_config(
                "<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>",
            )
            .build()
            .map_err(|e| format!("自動起動を設定できません: {e}"))?;
        Ok(Self { inner })
    }

    /// 登録名。
    pub fn app_name(&self) -> &str {
        self.inner.get_app_name()
    }

    /// 起動時の引数。
    pub fn args(&self) -> &[String] {
        self.inner.get_args()
    }
}

impl Autostart for SystemAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        self.inner.is_enabled().map_err(|e| e.to_string())
    }
    fn enable(&self) -> Result<(), String> {
        self.inner
            .enable()
            .map_err(|e| format!("自動起動を登録できません: {e}"))
    }
    fn disable(&self) -> Result<(), String> {
        self.inner
            .disable()
            .map_err(|e| format!("自動起動を解除できません: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_for_existing_executable() {
        let a = SystemAutostart::new(std::path::Path::new("/usr/bin/true")).unwrap();
        assert_eq!(a.app_name(), "claude-usage-monitor");
        assert_eq!(a.args(), ["daemon"]);
    }

    /// 子プロセスの中で実際に登録と解除を行う。`HOME`を差し替えた子でだけ動き、利用者の自動起動の設定には触れない。
    #[test]
    #[ignore = "enable_and_disable_round_trip_in_temp_home から子プロセスとして呼ぶ"]
    fn child_enable_disable() {
        if std::env::var_os("CUMON_AUTOSTART_CHILD").is_none() {
            return;
        }
        let a = SystemAutostart::new(std::path::Path::new("/usr/bin/true")).unwrap();
        assert!(!a.is_enabled().unwrap());
        a.enable().unwrap();
        assert!(a.is_enabled().unwrap());
        #[cfg(target_os = "macos")]
        {
            let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
            let plist = std::fs::read_to_string(
                home.join("Library/LaunchAgents/claude-usage-monitor.plist"),
            )
            .unwrap();
            assert!(
                plist
                    .contains("<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>"),
                "{plist}"
            );
        }
        a.disable().unwrap();
        assert!(!a.is_enabled().unwrap());
    }

    /// auto-launchは登録先をホームから決める。環境変数の書き換えはプロセス全体に効き並列のテストと衝突するため、子プロセスで`HOME`を差し替える。
    #[cfg(not(windows))]
    #[test]
    fn enable_and_disable_round_trip_in_temp_home() {
        let home = tempfile::tempdir().unwrap();
        // 実際のmacOSのホームには必ずある。auto-launchは`LaunchAgents`だけを作り、親は作らない。
        std::fs::create_dir(home.path().join("Library")).unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "models::gateways::autostart::tests::child_enable_disable",
                "--ignored",
                "--nocapture",
            ])
            .env("CUMON_AUTOSTART_CHILD", "1")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "{stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
