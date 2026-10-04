//! 1セッションのターンから、概要に出す数値を出す。
use crate::models::domain::calendar::segments;
use crate::models::domain::read_models::TurnRow;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

/// 概要の数値。
#[derive(Debug, Clone, PartialEq)]
pub struct SessionStats {
    /// 作業時間。カレンダーの帯と揃えるため、15分を超える空白で切った区間の長さの合計にする。
    pub active: Duration,
    /// API要求の数（モデルを持つターン）。
    pub requests: usize,
    /// モデルごとの要求数。多い順、同数は名前順。
    pub by_model: Vec<(String, usize)>,
}

/// Claude Codeが利用者の発話として記録する、機械の出力の先頭。依頼として数えない。
/// `<command-message>`は同じ操作の`<command-name>`と重なるので外す。
const MACHINE_INPUTS: [&str; 6] = [
    "<task-notification>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<command-message>",
];

fn is_request(summary: &str) -> bool {
    !MACHINE_INPUTS.iter().any(|m| summary.starts_with(m))
}

/// スラッシュコマンドは`<command-name>/x</command-name>`のように記録されるので、名前だけにする。
fn request_text(summary: &str) -> String {
    summary
        .strip_prefix("<command-name>")
        .and_then(|r| r.split_once("</command-name>"))
        .map_or_else(|| summary.to_string(), |(name, _)| name.to_string())
}

/// 本体セッションの利用者の依頼（時刻、本文）を時刻順に返す。
/// Claude Codeは同じ発話を同じ時刻で2行書くことがあるので、続けて同じものは1件にする。
pub fn requests(turns: &[TurnRow]) -> Vec<(DateTime<Utc>, String)> {
    let mut out: Vec<(DateTime<Utc>, String)> = turns
        .iter()
        .filter(|t| t.agent_id.is_empty() && t.kind == "prompt" && is_request(&t.summary))
        .map(|t| (t.ts, request_text(&t.summary)))
        .collect();
    out.dedup();
    out
}

/// ターンから数値を出す。
pub fn stats(turns: &[TurnRow]) -> SessionStats {
    // サブエージェントのターンは本体と時刻が入り混じって届くので、区間に切る前に並べ直す。
    let mut pts: Vec<_> = turns.iter().map(|t| (t.ts, 0u64)).collect();
    pts.sort_by_key(|p| p.0);
    let active = segments(&pts)
        .iter()
        .fold(Duration::zero(), |a, s| a + (s.end - s.start));
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for m in turns.iter().filter_map(|t| t.model.as_deref()) {
        *counts.entry(m).or_default() += 1;
    }
    let mut by_model: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(m, n)| (m.to_string(), n))
        .collect();
    by_model.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    SessionStats {
        active,
        requests: by_model.iter().map(|m| m.1).sum(),
        by_model,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::pricing::TokenUsage;
    use chrono::{TimeZone, Utc};

    fn turn(min: i64, agent: &str, model: Option<&str>, kind: &str) -> TurnRow {
        TurnRow {
            agent_id: agent.into(),
            message_id: format!("m{min}{agent}"),
            ts: Utc.with_ymd_and_hms(2026, 10, 4, 9, 0, 0).unwrap() + Duration::minutes(min),
            model: model.map(Into::into),
            kind: kind.into(),
            summary: String::new(),
            usage: TokenUsage::default(),
        }
    }

    #[test]
    fn empty_turns_give_zero() {
        let s = stats(&[]);
        assert_eq!(
            (s.active, s.requests, s.by_model.len()),
            (Duration::zero(), 0, 0)
        );
        assert!(requests(&[]).is_empty());
    }

    #[test]
    fn active_time_sums_segments_split_by_gap() {
        let t = [
            turn(0, "", None, "prompt"),
            turn(10, "", Some("opus"), "text"),
            turn(30, "", Some("opus"), "text"),
            turn(35, "", Some("opus"), "text"),
            turn(60, "", Some("opus"), "text"),
        ];
        assert_eq!(stats(&t).active, Duration::minutes(15));
    }

    #[test]
    fn gap_of_exactly_fifteen_minutes_is_not_split() {
        let t = [turn(0, "", None, "prompt"), turn(15, "", Some("x"), "text")];
        assert_eq!(stats(&t).active, Duration::minutes(15));
    }

    #[test]
    fn requests_list_only_main_agent_and_api_requests_need_model() {
        let t = [
            turn(0, "", None, "prompt"),
            turn(1, "a1", None, "prompt"),
            turn(2, "", Some("opus"), "tool_use"),
            turn(3, "a1", Some("haiku"), "text"),
            turn(4, "", None, "text"),
        ];
        assert_eq!(stats(&t).requests, 2);
        assert_eq!(requests(&t), [(t[0].ts, "".to_string())]);
    }

    #[test]
    fn machine_inputs_are_not_requests() {
        for text in [
            "<task-notification> <task-id>b1</task-id>",
            "<bash-stdout>ok</bash-stdout>",
            "<bash-stderr>x</bash-stderr>",
            "<local-command-stdout>x</local-command-stdout>",
            "<local-command-stderr>x</local-command-stderr>",
            "<command-message>compact</command-message>",
        ] {
            assert!(!is_request(text), "{text}");
        }
        for text in [
            "直してください",
            "<command-name>/compact</command-name>",
            "<bash-input>ls</bash-input>",
            "<pasted_content id=\"1\">x</pasted_content>",
        ] {
            assert!(is_request(text), "{text}");
        }
    }

    fn prompt(min: i64, text: &str) -> TurnRow {
        TurnRow {
            summary: text.into(),
            ..turn(min, "", None, "prompt")
        }
    }

    fn texts(t: &[TurnRow]) -> Vec<String> {
        requests(t).into_iter().map(|r| r.1).collect()
    }

    #[test]
    fn requests_skip_machine_inputs() {
        assert_eq!(
            texts(&[prompt(0, "<task-notification> x"), prompt(1, "直して")]),
            ["直して"]
        );
    }

    #[test]
    fn slash_command_shows_only_its_name() {
        assert_eq!(
            texts(&[
                prompt(
                    0,
                    "<command-name>/effort</command-name> <command-message>effort</command-message>"
                ),
                prompt(1, "<command-name>/x"),
            ]),
            ["/effort", "<command-name>/x"]
        );
    }

    #[test]
    fn same_request_at_same_time_is_listed_once() {
        assert_eq!(
            texts(&[
                prompt(0, "同じ"),
                prompt(0, "同じ"),
                prompt(1, "同じ"),
                prompt(1, "別")
            ]),
            ["同じ", "同じ", "別"]
        );
    }

    #[test]
    fn by_model_sorts_by_count_then_name() {
        let t = [
            turn(0, "", Some("b"), "text"),
            turn(1, "", Some("a"), "text"),
            turn(2, "", Some("c"), "text"),
            turn(3, "", Some("c"), "text"),
        ];
        assert_eq!(
            stats(&t).by_model,
            [("c".to_string(), 2), ("a".into(), 1), ("b".into(), 1)]
        );
    }

    #[test]
    fn unsorted_turns_are_sorted_before_segmenting() {
        let t = [turn(10, "", Some("x"), "text"), turn(0, "", None, "prompt")];
        assert_eq!(stats(&t).active, Duration::minutes(10));
    }
}
