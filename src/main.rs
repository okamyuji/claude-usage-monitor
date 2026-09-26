//! `cps`コマンドの入口。CLIの解析と依存の組み立てだけを行う。
use clap::{Parser, Subcommand};
use claude_profile_switcher::controllers::cli::CliError;
use claude_profile_switcher::controllers::cli::profile::{ProfileCommand, execute};
use claude_profile_switcher::controllers::cli::run::run_claude;
use claude_profile_switcher::models::repositories::db::SqliteStore;
use std::path::PathBuf;
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
        config_dir: Option<PathBuf>,
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

struct Paths {
    data_dir: PathBuf,
    home: PathBuf,
}

fn other(msg: impl Into<String>) -> CliError {
    CliError::Io(std::io::Error::other(msg.into()))
}

/// データとホームの場所。`CPS_DATA_DIR`はテストと複数環境の切り分けのために上書きを許す。
fn paths() -> Result<Paths, CliError> {
    let home = directories::BaseDirs::new()
        .ok_or_else(|| other("ホームディレクトリが見つかりません"))?
        .home_dir()
        .to_path_buf();
    let data_dir = match std::env::var_os("CPS_DATA_DIR") {
        Some(d) => PathBuf::from(d),
        None => directories::ProjectDirs::from("work", "okamyuji", "cps")
            .ok_or_else(|| other("データディレクトリが見つかりません"))?
            .data_dir()
            .to_path_buf(),
    };
    Ok(Paths { data_dir, home })
}

fn open_store(p: &Paths) -> Result<SqliteStore, CliError> {
    SqliteStore::open(&p.data_dir.join("cps.db")).map_err(|e| other(format!("DBを開けません: {e}")))
}

fn profile_command(action: ProfileAction) -> ProfileCommand {
    match action {
        ProfileAction::List => ProfileCommand::List,
        ProfileAction::Add { name, config_dir } => ProfileCommand::Add { name, config_dir },
        ProfileAction::Use { name } => ProfileCommand::Use { name },
        ProfileAction::Remove { name } => ProfileCommand::Remove { name },
    }
}

fn run(cli: Cli) -> Result<ExitCode, CliError> {
    let p = paths()?;
    let store = open_store(&p)?;
    match cli.command {
        Command::Daemon => Ok(ExitCode::SUCCESS),
        Command::Run { args } => Ok(ExitCode::from(
            run_claude(&store, &args)?.clamp(0, 255) as u8
        )),
        Command::Profile { action } => {
            execute(
                &store,
                &p.home,
                profile_command(action),
                &mut std::io::stdout().lock(),
            )?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("cps: {e}");
            ExitCode::from(e.exit_code())
        }
    }
}
