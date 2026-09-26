//! `cps run`。使用中プロファイルの設定ディレクトリで`claude`を起動する。
use crate::controllers::cli::CliError;
use crate::models::domain::profile::Profile;
use crate::models::ports::{ProfileRepo, RepoError};
use std::ffi::OsStr;
use std::process::Command;

/// 起動コマンドを組み立てる。既定プロファイルでは`CLAUDE_CONFIG_DIR`を消す。
/// 親シェルから別プロファイルの値を引き継いで、意図しない設定で動くのを防ぐため。
pub fn build_command(program: &OsStr, profile: &Profile, args: &[String]) -> Command {
    let mut c = Command::new(program);
    c.args(args);
    match &profile.config_dir {
        Some(dir) => c.env("CLAUDE_CONFIG_DIR", dir),
        None => c.env_remove("CLAUDE_CONFIG_DIR"),
    };
    c
}

/// 使用中プロファイルで`claude`を実行し、その終了コードを返す。
///
/// Unixでは`exec`でプロセスを置き換える。シグナルや端末制御を`claude`に直接届けるため。
pub fn run_claude(profiles: &dyn ProfileRepo, args: &[String]) -> Result<i32, CliError> {
    profiles.ensure_default()?;
    let profile = profiles
        .active()?
        .ok_or_else(|| RepoError::NotFound("使用中のプロファイル".into()))?;
    let mut cmd = build_command(OsStr::new("claude"), &profile, args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(map_spawn_error(err))
    }
    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(map_spawn_error)?;
        Ok(status.code().unwrap_or(1))
    }
}

fn map_spawn_error(e: std::io::Error) -> CliError {
    if e.kind() == std::io::ErrorKind::NotFound {
        CliError::ClaudeNotFound
    } else {
        CliError::Io(e)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn p(dir: Option<&str>) -> Profile {
        Profile {
            id: 1,
            name: "p".into(),
            config_dir: dir.map(Into::into),
            is_active: true,
        }
    }

    fn env_of(c: &Command, key: &str) -> Option<Option<String>> {
        c.get_envs()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    #[test]
    fn explicit_profile_sets_config_dir() {
        let c = build_command(
            OsStr::new("claude"),
            &p(Some("/c/sub")),
            &["-p".into(), "hi".into()],
        );
        assert_eq!(env_of(&c, "CLAUDE_CONFIG_DIR"), Some(Some("/c/sub".into())));
        assert_eq!(c.get_args().collect::<Vec<_>>(), ["-p", "hi"]);
    }

    #[test]
    fn default_profile_removes_inherited_config_dir() {
        let c = build_command(OsStr::new("claude"), &p(None), &[]);
        assert_eq!(env_of(&c, "CLAUDE_CONFIG_DIR"), Some(None));
    }
}
