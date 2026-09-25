//! `cps`コマンドの入口。CLIの解析と依存の組み立てだけを行う。
use clap::{Parser, Subcommand};
use std::process::ExitCode;

/// Claude CodeのToken使用量とセッションを常駐監視し、プロファイルを切り替えるツール。
#[derive(Parser)]
#[command(name = "cps", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 常駐して使用量とセッションを収集する
    Daemon,
    /// 使用中プロファイルの設定ディレクトリでclaudeを起動する
    Run {
        /// claudeにそのまま渡す引数
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// プロファイルを管理する
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    /// 登録済みのプロファイルを表示する
    List,
    /// プロファイルを追加する
    Add {
        /// プロファイル名（英数字、`-`、`_`で1〜32文字）
        name: String,
        /// 設定ディレクトリ。省略時は`~/.claude-<name>`
        #[arg(long)]
        config_dir: Option<std::path::PathBuf>,
    },
    /// 使用中のプロファイルを切り替える
    Use {
        /// プロファイル名
        name: String,
    },
    /// プロファイルを削除する
    Remove {
        /// プロファイル名
        name: String,
    },
}

fn main() -> ExitCode {
    let _cli = Cli::parse();
    ExitCode::SUCCESS
}
