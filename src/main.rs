//! `cps`コマンドの入口。CLIの解析と依存の組み立てだけを行う。
#![forbid(unsafe_code)]
use clap::{Parser, Subcommand};
use claude_profile_switcher::controllers::cli::CliError;
use claude_profile_switcher::controllers::cli::profile::{ProfileCommand, execute};
use claude_profile_switcher::controllers::cli::run::run_claude;
use claude_profile_switcher::controllers::daemon::alert::{AlertDeps, Alerter};
use claude_profile_switcher::controllers::daemon::catalog::CatalogUpdater;
use claude_profile_switcher::controllers::daemon::collector::{CollectorDeps, UsageCollector};
use claude_profile_switcher::controllers::daemon::ingest::Ingestor;
use claude_profile_switcher::controllers::daemon::runner::{Daemon, DaemonParts, DaemonSettings};
use claude_profile_switcher::controllers::daemon::tray::{TrayController, TrayDeps};
use claude_profile_switcher::controllers::gui::app::{GuiController, GuiDeps};
use claude_profile_switcher::models::domain::profile::Profile;
use claude_profile_switcher::models::gateways::autostart::SystemAutostart;
use claude_profile_switcher::models::gateways::credentials::{SecurityCli, SystemCredentialStore};
use claude_profile_switcher::models::gateways::daemon_control::LockFileDaemon;
use claude_profile_switcher::models::gateways::launcher::ExeGuiLauncher;
use claude_profile_switcher::models::gateways::model_catalog::{
    DEFAULT_CATALOG_BASE, HttpModelCatalog,
};
use claude_profile_switcher::models::gateways::notifier::notifier_from_env;
use claude_profile_switcher::models::gateways::process::{SysProcessInfo, SystemClock};
use claude_profile_switcher::models::gateways::usage_api::{DEFAULT_USAGE_BASE, HttpUsageApi};
use claude_profile_switcher::models::ports::ProfileRepo;
use claude_profile_switcher::models::repositories::db::SqliteStore;
use claude_profile_switcher::views::app::{CpsApp, apply_theme, install_fonts};
use claude_profile_switcher::views::theme;
use claude_profile_switcher::views::tray::run_with_tray;
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
        /// メニューバーのトレイを作らない。デスクトップのない環境とE2Eで使う
        #[arg(long)]
        no_tray: bool,
    },
    /// 使用中プロファイルの設定ディレクトリでclaudeを起動する
    Run {
        /// claudeにそのまま渡す引数
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// 画面を開く
    Gui,
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
            no_tray,
        } => run_daemon(&p, store, interval_secs, max_ticks, no_tray),
        Command::Run { args } => Ok(ExitCode::from(
            run_claude(&store, &args)?.clamp(0, 255) as u8
        )),
        Command::Gui => run_gui(&p, store),
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

/// GUIの依存を本番の具象型で組み立てる。
fn build_gui(p: &Paths, store: SqliteStore) -> Result<GuiController, CliError> {
    let store = Arc::new(store);
    let clock: Arc<SystemClock> = Arc::new(SystemClock);
    let catalog = Arc::new(CatalogUpdater::new(
        Arc::new(HttpModelCatalog::new(
            &env_or("CPS_CATALOG_BASE", DEFAULT_CATALOG_BASE),
            Duration::from_secs(30),
        )),
        store.clone(),
        store.clone(),
        clock.clone(),
    ));
    let exe = std::env::current_exe()?;
    Ok(GuiController::new(GuiDeps {
        clock,
        tz: *chrono::Local::now().offset(),
        home: p.home.clone(),
        profiles: store.clone(),
        usage: store.clone(),
        dashboard: store.clone(),
        sessions: store.clone(),
        analytics: store.clone(),
        diagnostics: store.clone(),
        logs: store.clone(),
        models: store.clone(),
        settings: store.clone(),
        creds: Arc::new(SystemCredentialStore::new(Arc::new(SecurityCli::default()))),
        autostart: Arc::new(SystemAutostart::new(&exe).map_err(other)?),
        daemon: Arc::new(LockFileDaemon::new(p.data_dir.join("daemon.lock"), exe)),
        catalog,
    }))
}

/// GUIを開く。閉じるとプロセスごと終わり、描画のメモリをOSが回収する（spec 3章）。
fn run_gui(p: &Paths, store: SqliteStore) -> Result<ExitCode, CliError> {
    store.ensure_default()?;
    let ctl = build_gui(p, store)?;
    let options = eframe::NativeOptions {
        // テストのビルドで開発用の依存がwgpuを有効にしても、本番は実測で省メモリだったglowで描く（spec 3章の決定表）。
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_title("Claude Profile Switcher"),
        ..Default::default()
    };
    eframe::run_native(
        "claude-profile-switcher",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            apply_theme(&cc.egui_ctx, ctl.vm().theme);
            Ok(Box::new(CpsApp::new(ctl)))
        }),
    )
    .map_err(|e| other(format!("GUIを起動できません: {e}")))?;
    Ok(ExitCode::SUCCESS)
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
            maintenance: store.clone(),
            process,
            clock: clock.clone(),
            home: p.home.clone(),
            alerter: Some(Alerter::new(AlertDeps {
                profiles: store.clone(),
                dashboard: store.clone(),
                settings: store,
                notifier: notifier_from_env(),
                clock,
                tz: *chrono::Local::now().offset(),
            })),
            paused: Arc::new(AtomicBool::new(false)),
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
    no_tray: bool,
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
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut daemon = build_daemon(p, store.clone(), settings);
    let result = if no_tray {
        daemon.run(&rx, &shutdown)
    } else {
        let tray = TrayController::new(TrayDeps {
            profiles: store.clone(),
            dashboard: store.clone(),
            launcher: Arc::new(ExeGuiLauncher::new(std::env::current_exe()?)),
            paused: daemon.paused().clone(),
            shutdown: shutdown.clone(),
            home: p.home.clone(),
        });
        let (tx, done) = std::sync::mpsc::channel();
        let stop = shutdown.clone();
        // 周期処理は別スレッドへ出し、メインスレッドでトレイのイベントループを回す（macOSはトレイの操作をメインスレッドに限るため）。
        run_with_tray(tray, notifier_from_env(), move || {
            let _ = tx.send(daemon.run(&rx, &stop));
        })
        .map_err(other)?;
        done.recv().unwrap_or(Ok(()))
    };
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
