//! `cps`コマンドの入口。CLIの解析と依存の組み立てだけを行う。
use clap::{Parser, Subcommand};
use claude_profile_switcher::controllers::cli::CliError;
use claude_profile_switcher::controllers::cli::profile::{ProfileCommand, execute};
use claude_profile_switcher::controllers::cli::run::run_claude;
use claude_profile_switcher::controllers::daemon::catalog::CatalogUpdater;
use claude_profile_switcher::controllers::daemon::collector::{CollectorDeps, UsageCollector};
use claude_profile_switcher::controllers::daemon::ingest::Ingestor;
use claude_profile_switcher::controllers::daemon::runner::{Daemon, DaemonParts, DaemonSettings};
use claude_profile_switcher::models::domain::profile::Profile;
use claude_profile_switcher::models::gateways::credentials::{SecurityCli, SystemCredentialStore};
use claude_profile_switcher::models::gateways::model_catalog::{
    DEFAULT_CATALOG_BASE, HttpModelCatalog,
};
use claude_profile_switcher::models::gateways::process::{SysProcessInfo, SystemClock};
use claude_profile_switcher::models::gateways::usage_api::{DEFAULT_USAGE_BASE, HttpUsageApi};
use claude_profile_switcher::models::ports::ProfileRepo;
use claude_profile_switcher::models::repositories::db::SqliteStore;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

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
    Daemon {
        /// 使用量の取得間隔（秒）。E2Eとリーク検査で短くするため
        #[arg(long, hide = true)]
        interval_secs: Option<u64>,
        /// 使用量の取得をこの回数行ったら終了する
        #[arg(long, hide = true)]
        max_ticks: Option<u64>,
    },
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
        Command::Daemon {
            interval_secs,
            max_ticks,
        } => run_daemon(&p, store, interval_secs, max_ticks),
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

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// デーモンを1つに限るためのロック。戻り値のファイルを持ち続ける間だけロックが有効になる。
fn acquire_lock(data_dir: &Path) -> Result<Option<std::fs::File>, CliError> {
    let lock = std::fs::File::create(data_dir.join("daemon.lock"))?;
    Ok(lock.try_lock().is_ok().then_some(lock))
}

/// モデル情報の更新を別スレッドで回す。公式ページの取得でトレイや使用量の取得を待たせないため。
fn spawn_catalog_thread(updater: CatalogUpdater) -> (Sender<()>, JoinHandle<()>) {
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let handle = std::thread::spawn(move || {
        loop {
            if let Err(e) = updater.run_once() {
                eprintln!("cps: モデル情報の更新に失敗しました: {e}");
            }
            if !matches!(
                stop_rx.recv_timeout(Duration::from_secs(24 * 60 * 60)),
                Err(RecvTimeoutError::Timeout)
            ) {
                break;
            }
        }
    });
    (stop_tx, handle)
}

/// 各プロファイルの設定ディレクトリを監視する。イベントは容量1024のチャネルに積むだけにし、
/// あふれた分はデーモンの定期的な全体走査で拾う。
fn start_watcher(
    profiles: &[Profile],
    home: &Path,
) -> Result<(RecommendedWatcher, Receiver<PathBuf>), CliError> {
    let (tx, rx): (SyncSender<PathBuf>, Receiver<PathBuf>) = sync_channel(1024);
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        for path in res.map(|ev| ev.paths).unwrap_or_default() {
            let _ = tx.try_send(path);
        }
    })
    .map_err(|e| other(e.to_string()))?;
    for profile in profiles {
        let dir = profile.resolved_config_dir(home);
        for sub in ["projects", "sessions", "jobs"]
            .map(|s| dir.join(s))
            .iter()
            .filter(|d| d.is_dir())
        {
            let _ = watcher.watch(sub, RecursiveMode::Recursive);
        }
    }
    Ok((watcher, rx))
}

/// 本番の具象型でデーモンの部品を組み立てる。
fn build_daemon(p: &Paths, store: Arc<SqliteStore>, settings: DaemonSettings) -> Daemon {
    let clock = Arc::new(SystemClock);
    let process = Arc::new(SysProcessInfo::new());
    let api = Arc::new(HttpUsageApi::new(
        &env_or("CPS_USAGE_API_BASE", DEFAULT_USAGE_BASE),
        Duration::from_secs(15),
    ));
    let creds = Arc::new(SystemCredentialStore::new(Arc::new(SecurityCli::default())));
    let collector = UsageCollector::new(
        CollectorDeps {
            profiles: store.clone(),
            creds,
            api,
            usage: store.clone(),
            log: store.clone(),
            clock: clock.clone(),
        },
        p.home.clone(),
    );
    let ingestor = Ingestor::new(
        store.clone(),
        store.clone(),
        process.clone(),
        clock.clone(),
        chrono::Duration::days(90),
    );
    Daemon::new(
        DaemonParts {
            collector,
            ingestor,
            profiles: store.clone(),
            settings: store.clone(),
            log: store.clone(),
            maintenance: store,
            process,
            clock,
            home: p.home.clone(),
        },
        settings,
    )
}

/// 依存を組み立ててデーモンを回す。具象型を知っているのは`main.rs`だけにする。
fn run_daemon(
    p: &Paths,
    store: SqliteStore,
    interval_secs: Option<u64>,
    max_ticks: Option<u64>,
) -> Result<ExitCode, CliError> {
    let Some(_lock) = acquire_lock(&p.data_dir)? else {
        eprintln!("cps: デーモンは既に起動しています");
        return Ok(ExitCode::from(1));
    };
    let store = Arc::new(store);
    // 初回起動で既定プロファイルの~/.claudeも監視対象に入れるため、監視の登録より前に作る。
    store.ensure_default()?;
    let catalog = CatalogUpdater::new(
        Arc::new(HttpModelCatalog::new(
            &env_or("CPS_CATALOG_BASE", DEFAULT_CATALOG_BASE),
            Duration::from_secs(30),
        )),
        store.clone(),
        store.clone(),
        Arc::new(SystemClock),
    );
    let (stop_tx, catalog_thread) = spawn_catalog_thread(catalog);
    let (_watcher, rx) = start_watcher(&store.list()?, &p.home)?;
    let settings = DaemonSettings {
        usage_interval: Duration::from_secs(interval_secs.unwrap_or(60)),
        follow_settings: interval_secs.is_none(),
        max_usage_ticks: max_ticks,
        ..DaemonSettings::default()
    };
    let result = build_daemon(p, store, settings).run(&rx, &AtomicBool::new(false));
    drop(stop_tx);
    let _ = catalog_thread.join();
    result?;
    Ok(ExitCode::SUCCESS)
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
