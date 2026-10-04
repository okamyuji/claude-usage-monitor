//! 実データのDBで今週のカレンダーの元データを読み、ターン数、セッション数、日ごとの帯の数と最大の列数を表示する。
#![forbid(unsafe_code)]
use chrono::{Duration, FixedOffset, Local, Utc};
use claude_usage_monitor::models::domain::calendar::{
    assign_lanes, local_midnight, segments, split_by_day, week_start,
};
use claude_usage_monitor::models::ports::SessionQueryRepo;
use claude_usage_monitor::models::repositories::db::{SqliteStore, ts};

fn main() {
    let db = std::env::args()
        .nth(1)
        .expect("引数にDBのパスを渡してください");
    let s = SqliteStore::open(std::path::Path::new(&db)).expect("DB");
    let tz: FixedOffset = *Local::now().offset();
    let start = week_start(Utc::now().with_timezone(&tz).date_naive(), 0);
    let from = local_midnight(start, tz);
    let to = local_midnight(start + Duration::days(7), tz);
    let turns = s.calendar_turns(from, to).expect("calendar_turns");
    let mut days: Vec<Vec<(u32, u32)>> = vec![vec![]; 7];
    let mut sessions = 0;
    for chunk in turns.chunk_by(|a, b| a.session_id == b.session_id) {
        sessions += 1;
        let pts: Vec<_> = chunk.iter().map(|t| (t.ts, t.tokens)).collect();
        for seg in segments(&pts) {
            for d in split_by_day(seg.start, seg.end, from) {
                days[d.day].push((d.start, d.end));
            }
        }
    }
    println!(
        "from={} to={} turns={} sessions={sessions}",
        ts(from),
        ts(to),
        turns.len()
    );
    for (i, spans) in days.iter().enumerate() {
        let lanes = assign_lanes(spans).iter().map(|l| l.1).max().unwrap_or(0);
        println!(
            "  day{i} {} bands={} max_lanes={lanes}",
            start + Duration::days(i as i64),
            spans.len()
        );
    }
}
