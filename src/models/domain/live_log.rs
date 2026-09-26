//! ライブログの行とリングバッファ。
//!
//! 本文はDBに保存せず、表示中だけJSONLから読む（spec 2章）。メモリは最新2,000行に限る（spec 11章）。

use crate::models::domain::transcript::{Block, Event, SUMMARY_CHARS, one_line, parse_line};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};

/// リングバッファの行数（spec 7.4節）。
pub const LIVE_LOG_CAPACITY: usize = 2000;

/// 行の種類。フィルタと色分けに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveKind {
    /// 利用者の入力。
    Prompt,
    /// 本文。
    Text,
    /// 思考。
    Thinking,
    /// ツール呼び出し。
    ToolUse,
    /// エラーになったツール結果。
    ToolError,
    /// サブエージェントの起動。
    AgentSpawn,
    /// ジョブの状態変化。
    JobState,
}

/// ライブログの1行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveLine {
    /// 時刻。行に時刻がなければ`None`。
    pub ts: Option<DateTime<Utc>>,
    /// 発生元（`本体`、サブエージェントの`agentType`、`ジョブ`）。
    pub source: String,
    /// 字下げの深さ。本体は0、サブエージェントは1。
    pub depth: u8,
    /// 種類。
    pub kind: LiveKind,
    /// ツール名。
    pub tool: Option<String>,
    /// 表示する文。200字まで。
    pub text: String,
}

/// サブエージェントの起動情報。親の`Agent`呼び出しの行に説明を付けるため。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnInfo {
    /// 種類。
    pub agent_type: String,
    /// 説明。
    pub description: String,
}

fn line(
    ts: Option<DateTime<Utc>>,
    source: &str,
    depth: u8,
    kind: LiveKind,
    tool: Option<String>,
    text: String,
) -> LiveLine {
    LiveLine {
        ts,
        source: source.to_string(),
        depth,
        kind,
        tool,
        text,
    }
}

fn block_line(
    b: &Block,
    spawns: &HashMap<String, SpawnInfo>,
) -> (LiveKind, Option<String>, String) {
    match b {
        Block::Thinking => (LiveKind::Thinking, None, "（思考中）".to_string()),
        Block::Text(t) => (LiveKind::Text, None, t.clone()),
        Block::ToolUse { id, name, summary } => match spawns.get(id) {
            Some(s) => (
                LiveKind::AgentSpawn,
                Some(name.clone()),
                format!("{}「{}」を起動", s.agent_type, s.description),
            ),
            None => (LiveKind::ToolUse, Some(name.clone()), summary.clone()),
        },
    }
}

/// JSONLの1行をライブログの行に変える。読めない行や表示しない種類の行は空にする。
pub fn lines_from_transcript(
    raw: &str,
    source: &str,
    depth: u8,
    spawns: &HashMap<String, SpawnInfo>,
) -> Vec<LiveLine> {
    let Ok(p) = parse_line(raw) else {
        return vec![];
    };
    let ts = p.meta.timestamp;
    match p.event {
        Event::Assistant { blocks, .. } => blocks
            .iter()
            .map(|b| {
                let (kind, tool, text) = block_line(b, spawns);
                line(ts, source, depth, kind, tool, text)
            })
            .collect(),
        Event::UserPrompt(t) => vec![line(ts, source, depth, LiveKind::Prompt, None, t)],
        Event::ToolResults(rs) => rs
            .iter()
            .filter(|r| r.is_error)
            .map(|r| {
                line(
                    ts,
                    source,
                    depth,
                    LiveKind::ToolError,
                    None,
                    format!("ツールがエラーを返しました（{}）", r.tool_use_id),
                )
            })
            .collect(),
        Event::Other => vec![],
    }
}

#[derive(Deserialize)]
struct TimelineRaw {
    at: Option<String>,
    state: Option<String>,
    detail: Option<String>,
}

/// ジョブの`timeline.jsonl`の1行を行に変える。`state`がない行は状態変化ではないので出さない。
pub fn line_from_timeline(raw: &str) -> Option<LiveLine> {
    let t: TimelineRaw = serde_json::from_str(raw).ok()?;
    let state = t.state?;
    let text = match t.detail {
        Some(d) => format!("ジョブ: {state}（{}）", one_line(&d, SUMMARY_CHARS)),
        None => format!("ジョブ: {state}"),
    };
    let ts =
        t.at.and_then(|a| DateTime::parse_from_rfc3339(&a).ok())
            .map(|d| d.with_timezone(&Utc));
    Some(line(ts, "ジョブ", 0, LiveKind::JobState, None, text))
}

/// 容量を超えたら古い方から捨てるバッファ。
#[derive(Debug, Clone)]
pub struct RingBuffer<T> {
    cap: usize,
    items: VecDeque<T>,
}

impl<T> RingBuffer<T> {
    /// 容量を決めて作る。
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            items: VecDeque::with_capacity(cap),
        }
    }

    /// 1件足す。
    pub fn push(&mut self, v: T) {
        if self.items.len() == self.cap {
            self.items.pop_front();
        }
        self.items.push_back(v);
    }

    /// まとめて足す。
    pub fn extend(&mut self, it: impl IntoIterator<Item = T>) {
        for v in it {
            self.push(v);
        }
    }

    /// 古い順に返す。
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    /// 件数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空か。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// 表示の絞り込み（spec 7.4節の4種類）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveFilter {
    /// ツール呼び出しとサブエージェントの起動を出す。
    pub tools: bool,
    /// 入力、本文、思考を出す。
    pub text: bool,
    /// エラーだけを出す。
    pub errors_only: bool,
    /// 発生元。`None`ならすべて。
    pub source: Option<String>,
}

impl Default for LiveFilter {
    fn default() -> Self {
        Self {
            tools: true,
            text: true,
            errors_only: false,
            source: None,
        }
    }
}

impl LiveFilter {
    /// 行を表示するか。
    pub fn matches(&self, l: &LiveLine) -> bool {
        if self.source.as_ref().is_some_and(|s| *s != l.source) {
            return false;
        }
        if self.errors_only {
            return l.kind == LiveKind::ToolError;
        }
        match l.kind {
            LiveKind::ToolUse | LiveKind::AgentSpawn => self.tools,
            LiveKind::Prompt | LiveKind::Text | LiveKind::Thinking => self.text,
            LiveKind::ToolError | LiveKind::JobState => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TOOL: &str = r#"{"type":"assistant","timestamp":"2026-09-26T01:02:14.000Z","sessionId":"s1","message":{"id":"m1","model":"claude-opus-5-5","content":[{"type":"text","text":"確認します"},{"type":"tool_use","id":"toolu_1","name":"Agent","input":{"subagent_type":"go-reviewer","description":"レビュー"}},{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"git status"}}],"usage":{"input_tokens":1,"output_tokens":1}}}"#;
    const PROMPT: &str = r#"{"type":"user","timestamp":"2026-09-26T01:02:10.000Z","sessionId":"s1","message":{"content":"直して"}}"#;
    const ERR: &str = r#"{"type":"user","timestamp":"2026-09-26T01:02:30.000Z","sessionId":"s1","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_2","is_error":true}]}}"#;

    fn spawns() -> HashMap<String, SpawnInfo> {
        HashMap::from([(
            "toolu_1".to_string(),
            SpawnInfo {
                agent_type: "go-reviewer".into(),
                description: "メモリ機能のレビュー".into(),
            },
        )])
    }

    #[test]
    fn assistant_blocks_become_lines_with_agent_spawn() {
        let lines = lines_from_transcript(TOOL, "本体", 0, &spawns());
        let kinds: Vec<LiveKind> = lines.iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            [LiveKind::Text, LiveKind::AgentSpawn, LiveKind::ToolUse]
        );
        assert_eq!(lines[1].text, "go-reviewer「メモリ機能のレビュー」を起動");
        assert_eq!(
            (lines[2].tool.as_deref(), lines[2].text.as_str()),
            (Some("Bash"), "git status")
        );
        assert!(
            lines
                .iter()
                .all(|l| l.source == "本体" && l.depth == 0 && l.ts.is_some())
        );
    }

    #[test]
    fn agent_call_without_meta_is_plain_tool_use() {
        let lines = lines_from_transcript(TOOL, "本体", 0, &HashMap::new());
        assert_eq!(lines[1].kind, LiveKind::ToolUse);
        assert_eq!(lines[1].tool.as_deref(), Some("Agent"));
    }

    #[test]
    fn prompt_error_other_and_broken_lines() {
        assert_eq!(
            lines_from_transcript(PROMPT, "go-reviewer", 1, &HashMap::new())[0].kind,
            LiveKind::Prompt
        );
        let e = lines_from_transcript(ERR, "本体", 0, &HashMap::new());
        assert_eq!(
            (e[0].kind, e[0].text.as_str()),
            (LiveKind::ToolError, "ツールがエラーを返しました（toolu_2）")
        );
        assert!(
            lines_from_transcript(r#"{"type":"attachment"}"#, "本体", 0, &HashMap::new())
                .is_empty()
        );
        assert!(lines_from_transcript("{broken", "本体", 0, &HashMap::new()).is_empty());
        let thinking = r#"{"type":"assistant","message":{"id":"m","content":[{"type":"thinking","thinking":"x"}]}}"#;
        assert_eq!(
            lines_from_transcript(thinking, "本体", 0, &HashMap::new())[0].text,
            "（思考中）"
        );
    }

    #[test]
    fn timeline_line_shows_state_and_detail() {
        let l = line_from_timeline(
            r#"{"at":"2026-07-26T02:45:13.081Z","state":"working","detail":"1/8件目","text":"x"}"#,
        )
        .unwrap();
        assert_eq!(
            (l.kind, l.text.as_str(), l.source.as_str()),
            (LiveKind::JobState, "ジョブ: working（1/8件目）", "ジョブ")
        );
        assert_eq!(
            line_from_timeline(r#"{"at":"bad","state":"done"}"#)
                .unwrap()
                .text,
            "ジョブ: done"
        );
        assert!(line_from_timeline(r#"{"detail":"no state"}"#).is_none());
        assert!(line_from_timeline("{broken").is_none());
    }

    #[test]
    fn ring_buffer_keeps_latest_capacity_lines() {
        let mut b = RingBuffer::new(3);
        assert!(b.is_empty());
        b.extend(1..=5);
        assert!(!b.is_empty());
        b.push(6);
        assert_eq!(b.iter().copied().collect::<Vec<_>>(), [4, 5, 6]);
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn filter_combinations() {
        let lines = lines_from_transcript(TOOL, "本体", 0, &spawns());
        let err = lines_from_transcript(ERR, "本体", 0, &HashMap::new()).remove(0);
        let all = LiveFilter::default();
        assert!(lines.iter().all(|l| all.matches(l)));
        let no_tools = LiveFilter {
            tools: false,
            ..LiveFilter::default()
        };
        assert_eq!(lines.iter().filter(|l| no_tools.matches(l)).count(), 1);
        let no_text = LiveFilter {
            text: false,
            ..LiveFilter::default()
        };
        assert_eq!(lines.iter().filter(|l| no_text.matches(l)).count(), 2);
        let errors = LiveFilter {
            errors_only: true,
            ..LiveFilter::default()
        };
        assert!(!lines.iter().any(|l| errors.matches(l)));
        assert!(errors.matches(&err));
        let src = LiveFilter {
            source: Some("go-reviewer".into()),
            ..LiveFilter::default()
        };
        assert!(!src.matches(&lines[0]));
    }
}
