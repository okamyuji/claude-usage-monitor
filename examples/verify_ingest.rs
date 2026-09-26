//! 利用者の~/.claudeを一時DBへ取り込み、件数と所要時間を表示する。2回実行して冪等性も確かめる。
//! Task 8、Task 10、Task 16の実動作確認用。
#![forbid(unsafe_code)]
use claude_usage_monitor::controllers::daemon::ingest::Ingestor;
use claude_usage_monitor::models::gateways::process::{SysProcessInfo, SystemClock};
use claude_usage_monitor::models::ports::ProfileRepo;
use claude_usage_monitor::models::repositories::db::SqliteStore;
use std::sync::Arc;

fn main() {
    let home = std::path::PathBuf::from(std::env::var("HOME").expect("HOME"));
    let db = std::env::args()
        .nth(1)
        .expect("引数に一時DBのパスを渡してください");
    let store = Arc::new(SqliteStore::open(std::path::Path::new(&db)).expect("DB"));
    let profile = store.ensure_default().expect("既定プロファイル");
    let ingestor = Ingestor::new(
        store.clone(),
        store.clone(),
        Arc::new(SysProcessInfo::new()),
        Arc::new(SystemClock),
        chrono::Duration::days(3650),
    );
    let started = std::time::Instant::now();
    let r = ingestor
        .scan_all(&profile, &profile.resolved_config_dir(&home))
        .expect("取り込み");
    println!("{r:?} elapsed={:?}", started.elapsed());
}
