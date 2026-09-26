//! ログイン時の自動起動。
//!
//! macOSはLaunchAgent、WindowsはRunレジストリ、LinuxはXDG autostartに、`cps daemon`を登録する。3 OSの違いはauto-launchが吸収する。
use crate::models::ports::Autostart;
use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode};
use std::path::Path;

const APP_NAME: &str = "claude-profile-switcher";

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
        assert_eq!(a.app_name(), "claude-profile-switcher");
        assert_eq!(a.args(), ["daemon"]);
    }
}
