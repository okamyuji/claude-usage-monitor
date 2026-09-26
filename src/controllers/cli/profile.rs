//! `cps profile`。プロファイルの一覧、追加、切替、削除。
use crate::controllers::cli::CliError;
use crate::models::domain::profile::validate_profile_name;
use crate::models::ports::{ProfileRepo, RepoError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// プロファイル操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileCommand {
    /// 一覧。
    List,
    /// 追加。
    Add {
        /// 名前。
        name: String,
        /// 設定ディレクトリ。`None`なら`~/.claude-<name>`を作る。
        config_dir: Option<PathBuf>,
    },
    /// 使用中にする。
    Use {
        /// 名前。
        name: String,
    },
    /// 削除。
    Remove {
        /// 名前。
        name: String,
    },
}

/// 操作を実行し、結果を`out`に書く。出力先を引数にするのは、テストで標準出力を使わずに検証するため。
pub fn execute(
    repo: &dyn ProfileRepo,
    home: &Path,
    cmd: ProfileCommand,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    repo.ensure_default()?;
    match cmd {
        ProfileCommand::List => list(repo, home, out),
        ProfileCommand::Add { name, config_dir } => add(repo, home, &name, config_dir, out),
        ProfileCommand::Use { name } => {
            repo.set_active(&name)?;
            writeln!(
                out,
                "使用中のプロファイルを {name} に切り替えました。以後に起動するセッションから有効です"
            )?;
            Ok(())
        }
        ProfileCommand::Remove { name } => {
            repo.remove(&name)?;
            writeln!(
                out,
                "削除しました: {name}（設定ディレクトリは残しています）"
            )?;
            Ok(())
        }
    }
}

fn list(repo: &dyn ProfileRepo, home: &Path, out: &mut dyn Write) -> Result<(), CliError> {
    for p in repo.list()? {
        let mark = if p.is_active { '*' } else { ' ' };
        writeln!(
            out,
            "{mark} {}\t{}",
            p.name,
            p.resolved_config_dir(home).display()
        )?;
    }
    Ok(())
}

fn add(
    repo: &dyn ProfileRepo,
    home: &Path,
    name: &str,
    config_dir: Option<PathBuf>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    validate_profile_name(name).map_err(|e| RepoError::Invalid(e.to_string()))?;
    let dir = config_dir.unwrap_or_else(|| home.join(format!(".claude-{name}")));
    std::fs::create_dir_all(&dir)?;
    repo.add(name, Some(&dir))?;
    writeln!(out, "追加しました: {name}（{}）", dir.display())?;
    writeln!(
        out,
        "このプロファイルでログインするには `cps profile use {name}` の後に `cps run` を実行し、/login してください"
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temp_store;

    fn run(repo: &dyn ProfileRepo, home: &Path, cmd: ProfileCommand) -> Result<String, CliError> {
        let mut out = Vec::new();
        execute(repo, home, cmd, &mut out)?;
        Ok(String::from_utf8(out).unwrap())
    }

    #[test]
    fn add_creates_default_dir_and_use_switches() {
        let (_d, s) = temp_store();
        let home = tempfile::tempdir().unwrap();
        run(
            &s,
            home.path(),
            ProfileCommand::Add {
                name: "sub".into(),
                config_dir: None,
            },
        )
        .unwrap();
        assert!(home.path().join(".claude-sub").is_dir());
        run(&s, home.path(), ProfileCommand::Use { name: "sub".into() }).unwrap();
        let list = run(&s, home.path(), ProfileCommand::List).unwrap();
        assert!(list.contains("* sub"));
        assert!(list.contains("  default"));
    }

    #[test]
    fn errors_are_reported() {
        let (_d, s) = temp_store();
        let home = tempfile::tempdir().unwrap();
        assert!(
            run(
                &s,
                home.path(),
                ProfileCommand::Use {
                    name: "none".into()
                }
            )
            .is_err()
        );
        assert!(
            run(
                &s,
                home.path(),
                ProfileCommand::Add {
                    name: "../x".into(),
                    config_dir: None
                }
            )
            .is_err()
        );
        assert!(
            run(
                &s,
                home.path(),
                ProfileCommand::Remove {
                    name: "default".into()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn remove_deletes_inactive_profile() {
        let (_d, s) = temp_store();
        let home = tempfile::tempdir().unwrap();
        run(
            &s,
            home.path(),
            ProfileCommand::Add {
                name: "sub".into(),
                config_dir: Some(home.path().join("x")),
            },
        )
        .unwrap();
        run(
            &s,
            home.path(),
            ProfileCommand::Remove { name: "sub".into() },
        )
        .unwrap();
        assert!(
            !run(&s, home.path(), ProfileCommand::List)
                .unwrap()
                .contains("sub")
        );
    }
}
