//! 実際のセッションのJSONLを末尾から読み、ライブログの行を表示する。Task 8の実動作確認用。
#![forbid(unsafe_code)]
use claude_profile_switcher::models::domain::live_log::{
    LIVE_LOG_CAPACITY, RingBuffer, lines_from_transcript,
};
use claude_profile_switcher::models::gateways::jsonl::read_new_lines;
use claude_profile_switcher::models::gateways::session_files::{find_session_files, tail_offset};
use std::collections::HashMap;

fn main() {
    let sid = std::env::args()
        .nth(1)
        .expect("引数にセッションIDを渡してください");
    let home = std::env::var("HOME").expect("HOME");
    let files = find_session_files(&std::path::Path::new(&home).join(".claude"), &sid, None);
    let main = files.main.expect("本体のJSONLが見つかりません");
    let mut buf = RingBuffer::new(LIVE_LOG_CAPACITY);
    let off = tail_offset(&main, LIVE_LOG_CAPACITY).expect("tail");
    read_new_lines(&main, off, |l| {
        buf.extend(lines_from_transcript(l, "本体", 0, &HashMap::new()))
    })
    .expect("read");
    println!(
        "file={} offset={off} lines={} subagents={}",
        main.display(),
        buf.len(),
        files.subagents.len()
    );
    for l in buf.iter().skip(buf.len().saturating_sub(8)) {
        println!("{:?} {:?} {:?} {}", l.ts, l.kind, l.tool, l.text);
    }
}
