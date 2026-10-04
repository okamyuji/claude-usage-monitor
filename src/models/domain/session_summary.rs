//! 1セッションのターンから、概要に出す数値を出す。
use crate::models::domain::calendar::segments;
use crate::models::domain::read_models::TurnRow;
use chrono::Duration;
use std::collections::HashMap;

/// 概要の数値。
#[derive(Debug, Clone, PartialEq)]
pub struct SessionStats {
    /// 作業時間。カレンダーの帯と揃えるため、15分を超える空白で切った区間の長さの合計にする。
    pub active: Duration,
    /// 本体セッションの依頼の数。
    pub prompts: usize,
    /// API要求の数（モデルを持つターン）。
    pub requests: usize,
    /// モデルごとの要求数。多い順、同数は名前順。
    pub by_model: Vec<(String, usize)>,
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
        prompts: turns
            .iter()
            .filter(|t| t.agent_id.is_empty() && t.kind == "prompt")
            .count(),
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
            (s.active, s.prompts, s.requests, s.by_model.len()),
            (Duration::zero(), 0, 0, 0)
        );
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
    fn prompts_count_only_main_agent_and_requests_need_model() {
        let t = [
            turn(0, "", None, "prompt"),
            turn(1, "a1", None, "prompt"),
            turn(2, "", Some("opus"), "tool_use"),
            turn(3, "a1", Some("haiku"), "text"),
            turn(4, "", None, "text"),
        ];
        let s = stats(&t);
        assert_eq!((s.prompts, s.requests), (1, 2));
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
