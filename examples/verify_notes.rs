//! 実データのDBで、要約と圧縮の件数、要約のあるセッション数、最新の要約3件を表示する。
#![forbid(unsafe_code)]
use claude_usage_monitor::models::domain::read_models::SessionFilter;
use claude_usage_monitor::models::domain::transcript::NoteKind;
use claude_usage_monitor::models::ports::SessionQueryRepo;
use claude_usage_monitor::models::repositories::db::{SqliteStore, ts};

fn main() {
    let db = std::env::args()
        .nth(1)
        .expect("引数にDBのパスを渡してください");
    let s = SqliteStore::open(std::path::Path::new(&db)).expect("DB");
    let sessions = s
        .list_sessions(&SessionFilter {
            limit: usize::MAX,
            ..SessionFilter::default()
        })
        .expect("list_sessions");
    let (mut recaps, mut compacts, mut with_recap) = (0, 0, 0);
    let mut latest = Vec::new();
    for row in &sessions {
        let notes = s.notes(&row.session_id).expect("notes");
        let r: Vec<_> = notes.iter().filter(|n| n.kind == NoteKind::Recap).collect();
        recaps += r.len();
        compacts += notes.len() - r.len();
        with_recap += usize::from(!r.is_empty());
        latest.extend(r.into_iter().map(|n| (n.ts, n.text.clone())));
    }
    latest.sort_by_key(|n| std::cmp::Reverse(n.0));
    println!(
        "sessions={} recaps={recaps} compacts={compacts} sessions_with_recap={with_recap}",
        sessions.len()
    );
    for (t, text) in latest.iter().take(3) {
        println!("  {} {text}", ts(*t));
    }
}
