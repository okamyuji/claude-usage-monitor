//! 実データのDBに読み取り用のクエリを流し、件数と合計を表示する。Task 5〜7の実動作確認用。
use chrono::{Duration, Utc};
use claude_profile_switcher::models::domain::read_models::{GroupBy, SessionFilter};
use claude_profile_switcher::models::ports::{
    AnalyticsRepo, DashboardRepo, ProfileRepo, SessionQueryRepo,
};
use claude_profile_switcher::models::repositories::db::SqliteStore;

fn main() {
    let db = std::env::args()
        .nth(1)
        .expect("引数にDBのパスを渡してください");
    let s = SqliteStore::open(std::path::Path::new(&db)).expect("DB");
    let now = Utc::now();
    for p in s.list().expect("profiles") {
        let u = s.latest_usage(p.id).expect("usage");
        println!(
            "profile={} limits={:?}",
            p.name,
            u.map(|u| u
                .limits
                .iter()
                .map(|l| (l.kind.clone(), l.percent))
                .collect::<Vec<_>>())
        );
    }
    let runs = s.recent_runs(now - Duration::minutes(10)).expect("runs");
    println!("recent_runs={}", runs.len());
    for r in runs.iter().take(5) {
        println!(
            "  {} {:?} status={:?} last={:?}",
            r.session.session_id, r.session.kind, r.session.status, r.session.last_turn_kind
        );
    }
    let list = s.list_sessions(&SessionFilter::default()).expect("list");
    println!("sessions(limit1000)={}", list.len());
    let since = now - Duration::days(7);
    let total: u64 = s
        .usage_by(GroupBy::Model, since)
        .expect("by model")
        .iter()
        .map(|g| g.usage.output)
        .sum();
    println!("output_7d={total}");
    println!(
        "tools_top3={:?}",
        s.tool_stats(since)
            .expect("tools")
            .iter()
            .take(3)
            .map(|t| (t.tool_name.clone(), t.calls, t.errors))
            .collect::<Vec<_>>()
    );
}
