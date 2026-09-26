//! プロファイル（Claude Codeの設定ディレクトリ1つ分）。
use std::path::{Path, PathBuf};

/// 既定プロファイルの名前。初回起動時に`~/.claude`を指す行として作る。
pub const DEFAULT_PROFILE: &str = "default";

/// プロファイル1件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// DB上のID。
    pub id: i64,
    /// 利用者が付けた名前。
    pub name: String,
    /// 設定ディレクトリ。`None`は`~/.claude`を使う既定の状態で、`CLAUDE_CONFIG_DIR`を設定しない。
    pub config_dir: Option<PathBuf>,
    /// 使用中か。`cps run`はこのプロファイルで起動する。
    pub is_active: bool,
}

impl Profile {
    /// 実際に読む設定ディレクトリ。既定のときに`home/.claude`へ解決するのは、Claude Code本体と同じ場所を見るため。
    pub fn resolved_config_dir(&self, home: &Path) -> PathBuf {
        self.config_dir
            .clone()
            .unwrap_or_else(|| home.join(".claude"))
    }
}

/// プロファイル名の検証エラー。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("プロファイル名は英数字、`-`、`_`の1〜32文字にしてください: {0}")]
pub struct ProfileNameError(pub String);

/// プロファイル名を検証する。名前は既定の設定ディレクトリ名（`~/.claude-<name>`）に使うため、
/// パス区切りや`..`を含む名前を拒否してディレクトリの外へ出る操作を防ぐ。
pub fn validate_profile_name(name: &str) -> Result<(), ProfileNameError> {
    let ok = (1..=32).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(ProfileNameError(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_uses_home_dot_claude() {
        let p = Profile {
            id: 1,
            name: DEFAULT_PROFILE.into(),
            config_dir: None,
            is_active: true,
        };
        assert_eq!(
            p.resolved_config_dir(Path::new("/h")),
            PathBuf::from("/h/.claude")
        );
    }

    #[test]
    fn explicit_config_dir_wins() {
        let p = Profile {
            id: 2,
            name: "sub".into(),
            config_dir: Some("/c".into()),
            is_active: false,
        };
        assert_eq!(p.resolved_config_dir(Path::new("/h")), PathBuf::from("/c"));
    }

    #[test]
    fn profile_name_validation() {
        assert!(validate_profile_name("sub_1-a").is_ok());
        assert!(validate_profile_name("a").is_ok());
        assert!(validate_profile_name(&"a".repeat(32)).is_ok());
        assert!(validate_profile_name("").is_err());
        assert!(validate_profile_name(&"a".repeat(33)).is_err());
        assert!(validate_profile_name("../x").is_err());
        assert!(validate_profile_name("a b").is_err());
        assert_eq!(
            validate_profile_name("日本"),
            Err(ProfileNameError("日本".into()))
        );
    }
}
