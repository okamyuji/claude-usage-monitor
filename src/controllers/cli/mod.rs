//! `cumon run`と`cumon profile`の処理。
pub mod profile;
pub mod run;

use crate::models::ports::RepoError;

/// CLIの失敗。終了コードを決めるため、原因ごとに分ける。
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// DBの失敗、または入力不正。
    #[error(transparent)]
    Repo(#[from] RepoError),
    /// `claude`が見つからない。シェルの慣習に合わせて終了コード127を返す。
    #[error("claudeコマンドが見つかりません。PATHにclaudeがあるか確認してください")]
    ClaudeNotFound,
    /// ファイル操作や起動の失敗。
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl CliError {
    /// 終了コード。
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::ClaudeNotFound => 127,
            _ => 1,
        }
    }
}
