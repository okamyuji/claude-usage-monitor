//! 利用者の~/.claudeにある実際のJSONLを解析し、行数、解析失敗数、モデル別のTokenとコストを表示する。
//! 単体テストのフィクスチャでは分からない、実データの形式の揺れを確かめるための実動作確認用。
#![forbid(unsafe_code)]
use claude_usage_monitor::models::domain::pricing::{TokenUsage, cost_for, seed_models};
use claude_usage_monitor::models::domain::transcript::{Event, parse_line};
use claude_usage_monitor::models::gateways::jsonl::{read_new_lines, transcript_files};
use std::collections::{BTreeMap, HashSet};

fn main() {
    let home = std::env::var("HOME").expect("HOME");
    let dir = std::path::Path::new(&home).join(".claude");
    let files = transcript_files(&dir).expect("JSONLの列挙");
    let (mut lines, mut bad) = (0usize, 0usize);
    let mut seen = HashSet::new();
    let mut by_model: BTreeMap<String, TokenUsage> = BTreeMap::new();
    for f in &files {
        read_new_lines(&f.path, 0, |l| {
            lines += 1;
            match parse_line(l) {
                Ok(p) => {
                    if let Event::Assistant {
                        message_id,
                        model,
                        usage,
                        ..
                    } = p.event
                        && seen.insert(message_id)
                    {
                        let e = by_model
                            .entry(model.unwrap_or_else(|| "(none)".into()))
                            .or_default();
                        *e = e.plus(&usage);
                    }
                }
                Err(_) => bad += 1,
            }
        })
        .expect("読み取り");
    }
    let models = seed_models();
    let total: u64 = by_model.values().map(|u| u.output).sum();
    println!(
        "files={} lines={} malformed={} messages={} output_total={}",
        files.len(),
        lines,
        bad,
        seen.len(),
        total
    );
    for (m, u) in &by_model {
        let cost =
            cost_for(&models, m, u).map_or("単価未登録".to_string(), |c| format!("${c:.2}"));
        println!(
            "{m}\toutput={}\tinput={}\tcache_read={}\t{cost}",
            u.output, u.input, u.cache_read
        );
    }
}
