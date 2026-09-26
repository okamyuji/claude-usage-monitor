//! 実デーモンが溜めたDBから、プロファイルごとに上限到達の予測を表示する。Task 5とTask 19の実動作確認用。
#![forbid(unsafe_code)]
use chrono::{Duration, Utc};
use claude_profile_switcher::models::domain::projection::{WINDOW_MINUTES, project};
use claude_profile_switcher::models::ports::{ProfileRepo, UsageRepo};
use claude_profile_switcher::models::repositories::db::SqliteStore;

fn main() {
    let db = std::env::args()
        .nth(1)
        .expect("引数にDBのパスを渡してください");
    let store = SqliteStore::open(std::path::Path::new(&db)).expect("DB");
    let now = Utc::now();
    for p in store.list().expect("プロファイル") {
        for kind in ["session", "weekly_all"] {
            let s = store
                .samples(p.id, kind, now - Duration::minutes(WINDOW_MINUTES))
                .expect("samples");
            let last = s.last().map(|x| x.percent);
            println!(
                "{}\t{kind}\tsamples={}\tlatest={last:?}\t{:?}",
                p.name,
                s.len(),
                project(&s, now, None)
            );
        }
    }
}
