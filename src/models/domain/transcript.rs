//! Claude CodeのセッションJSONLの1行を解析する。
//!
//! JSONLの形式は公開仕様ではなく変わりうる。そのため`serde_json::Value`で必要な項目だけを拾い、
//! 項目の欠落や未知の型で取り込み全体が止まらないようにする。
use crate::models::domain::pricing::TokenUsage;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// ツール入力や本文の要約の最大文字数。画面1行に収め、DBの肥大を防ぐため。
pub const SUMMARY_CHARS: usize = 200;

/// 実行の種別。バックグラウンドの実行を対話と見分けて表示するために区別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    /// 端末で利用者が対話しているセッション。
    Interactive,
    /// SDKや`claude -p`によるヘッドレス実行。
    Headless,
    /// `~/.claude/jobs`に記録されるバックグラウンドジョブ。
    BackgroundJob,
}

impl SessionKind {
    /// JSONLの`entrypoint`から種別を決める。`sdk-`で始まるものはSDK経由の実行なのでヘッドレスとする。
    pub fn from_entrypoint(entrypoint: Option<&str>) -> Self {
        match entrypoint {
            Some(e) if e.starts_with("sdk-") => SessionKind::Headless,
            _ => SessionKind::Interactive,
        }
    }

    /// DB保存用の文字列。
    pub fn as_str(self) -> &'static str {
        match self {
            SessionKind::Interactive => "interactive",
            SessionKind::Headless => "headless",
            SessionKind::BackgroundJob => "background_job",
        }
    }

    /// DBの文字列から戻す。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "interactive" => Some(SessionKind::Interactive),
            "headless" => Some(SessionKind::Headless),
            "background_job" => Some(SessionKind::BackgroundJob),
            _ => None,
        }
    }
}

/// 要約の最大文字数。実測の最大は263字で、200字の`SUMMARY_CHARS`では切れるため別にする。
pub const NOTE_CHARS: usize = 2000;

const RECAP_HINT: &str = "(disable recaps in /config)";

/// `system`行のうち保存するもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// 離席中にClaude Codeが書く作業の要約（`away_summary`）。
    Recap,
    /// 会話の圧縮（`compact_boundary`）。
    Compact,
}

impl NoteKind {
    /// DB保存用の文字列。
    pub fn as_str(self) -> &'static str {
        match self {
            NoteKind::Recap => "recap",
            NoteKind::Compact => "compact",
        }
    }

    /// DBの文字列から戻す。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "recap" => Some(NoteKind::Recap),
            "compact" => Some(NoteKind::Compact),
            _ => None,
        }
    }
}

/// 行の種類によらず共通する項目。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LineMeta {
    /// 行のID。ユーザー発話をターンとして保存するときの一意キーに使う。
    pub uuid: Option<String>,
    /// セッションID。サブエージェントの行でも親セッションのIDが入る。
    pub session_id: Option<String>,
    /// 行の時刻。読めない場合は`None`。
    pub timestamp: Option<DateTime<Utc>>,
    /// 作業ディレクトリ。プロジェクト別集計に使う。
    pub cwd: Option<String>,
    /// gitブランチ。ブランチ別集計に使う。
    pub git_branch: Option<String>,
    /// 起動経路（`cli`、`sdk-py`など）。種別の判定に使う。
    pub entrypoint: Option<String>,
    /// サブエージェントの行ならそのID。
    pub agent_id: Option<String>,
}

/// アシスタント応答の内容ブロック。ライブログとターン表の要約に使う。
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// 思考。本文は保存しない。
    Thinking,
    /// 本文（1行に要約済み）。
    Text(String),
    /// ツール呼び出し。
    ToolUse {
        /// 呼び出しID。結果の行と結び付けるために使う。
        id: String,
        /// ツール名。
        name: String,
        /// 入力の要約。
        summary: String,
    },
}

/// ツール結果1件。エラー率の集計に使う。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    /// 対応する呼び出しID。
    pub tool_use_id: String,
    /// エラーだったか。
    pub is_error: bool,
}

/// 行が表す出来事。
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// アシスタント応答。同じ`message_id`が内容ブロックごとに複数行へ分かれて現れる。
    Assistant {
        /// API応答のID。Tokenを二重に数えないための一意キー。
        message_id: String,
        /// モデルID。
        model: Option<String>,
        /// Token数。同じ`message_id`の行では同じ値が重複して入る。
        usage: TokenUsage,
        /// この行の内容ブロック。
        blocks: Vec<Block>,
    },
    /// 利用者の発話（1行に要約済み）。
    UserPrompt(String),
    /// ツール結果。
    ToolResults(Vec<ToolResult>),
    /// Claude Codeが書いた要約、または圧縮の印。
    Note {
        /// 種類。
        kind: NoteKind,
        /// 本文（1行に要約済み）。圧縮の印では空のこともある。
        text: String,
    },
    /// 集計に使わない行。
    Other,
}

/// 解析済みの1行。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLine {
    /// 共通項目。
    pub meta: LineMeta,
    /// 出来事。
    pub event: Event,
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn n(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// 空白と改行を1つの空白にまとめ、`max_chars`文字で切る。切った場合は末尾に`…`を付ける。
pub fn one_line(text: &str, max_chars: usize) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= max_chars {
        return joined;
    }
    let mut out: String = joined.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// ツール入力から、利用者が「何をしているか」を読み取れる1項目を選んで要約する。
pub fn summarize_tool_input(name: &str, input: &Value) -> String {
    let pick = |k: &str| input.get(k).and_then(Value::as_str);
    let text = match name {
        "Bash" => pick("command"),
        "Read" | "Write" | "Edit" | "NotebookEdit" => pick("file_path"),
        "Grep" | "Glob" => pick("pattern"),
        "WebFetch" => pick("url"),
        "WebSearch" => pick("query"),
        "Agent" | "Task" => {
            let t = format!(
                "{}「{}」",
                pick("subagent_type").unwrap_or("general-purpose"),
                pick("description").unwrap_or("")
            );
            return one_line(&t, SUMMARY_CHARS);
        }
        _ => None,
    };
    one_line(text.unwrap_or(""), SUMMARY_CHARS)
}

fn parse_usage(u: &Value) -> TokenUsage {
    let (w5, w1) = match u.get("cache_creation") {
        Some(cc) if cc.is_object() => (
            n(cc, "ephemeral_5m_input_tokens"),
            n(cc, "ephemeral_1h_input_tokens"),
        ),
        _ => (n(u, "cache_creation_input_tokens"), 0),
    };
    TokenUsage {
        input: n(u, "input_tokens"),
        output: n(u, "output_tokens"),
        cache_read: n(u, "cache_read_input_tokens"),
        cache_write_5m: w5,
        cache_write_1h: w1,
    }
}

fn parse_block(b: &Value) -> Option<Block> {
    match b.get("type")?.as_str()? {
        "thinking" | "redacted_thinking" => Some(Block::Thinking),
        "text" => Some(Block::Text(one_line(
            b.get("text")?.as_str()?,
            SUMMARY_CHARS,
        ))),
        "tool_use" => {
            let name = s(b, "name")?;
            let summary = summarize_tool_input(&name, b.get("input").unwrap_or(&Value::Null));
            Some(Block::ToolUse {
                id: s(b, "id")?,
                name,
                summary,
            })
        }
        _ => None,
    }
}

fn parse_assistant(v: &Value) -> Event {
    let m = &v["message"];
    let Some(message_id) = s(m, "id") else {
        return Event::Other;
    };
    let blocks = m
        .get("content")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(parse_block).collect())
        .unwrap_or_default();
    Event::Assistant {
        message_id,
        model: s(m, "model"),
        usage: parse_usage(&m["usage"]),
        blocks,
    }
}

fn is_type(b: &Value, t: &str) -> bool {
    b.get("type").and_then(Value::as_str) == Some(t)
}

fn parse_user_blocks(items: &[Value]) -> Event {
    let results: Vec<ToolResult> = items
        .iter()
        .filter(|b| is_type(b, "tool_result"))
        .filter_map(|b| {
            Some(ToolResult {
                tool_use_id: s(b, "tool_use_id")?,
                is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect();
    if !results.is_empty() {
        return Event::ToolResults(results);
    }
    let texts: Vec<&str> = items
        .iter()
        .filter(|b| is_type(b, "text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect();
    if texts.is_empty() {
        Event::Other
    } else {
        Event::UserPrompt(one_line(&texts.join(" "), SUMMARY_CHARS))
    }
}

fn parse_user(v: &Value) -> Event {
    if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return Event::Other;
    }
    match &v["message"]["content"] {
        Value::String(t) => Event::UserPrompt(one_line(t, SUMMARY_CHARS)),
        Value::Array(items) => parse_user_blocks(items),
        _ => Event::Other,
    }
}

fn parse_system(v: &Value) -> Event {
    let text = v.get("content").and_then(Value::as_str);
    match (v.get("subtype").and_then(Value::as_str), text) {
        (Some("away_summary"), Some(t)) => Event::Note {
            kind: NoteKind::Recap,
            // Claude Codeは要約の末尾に設定の案内を足すことがある。作業の要約ではないので外す。
            text: one_line(
                t.trim_end().trim_end_matches(RECAP_HINT).trim_end(),
                NOTE_CHARS,
            ),
        },
        (Some("compact_boundary"), t) => Event::Note {
            kind: NoteKind::Compact,
            text: t.map(|t| one_line(t, NOTE_CHARS)).unwrap_or_default(),
        },
        _ => Event::Other,
    }
}

/// 1行を解析する。JSONとして読めない行だけをエラーにし、項目の欠落は許容する。
pub fn parse_line(line: &str) -> Result<ParsedLine, serde_json::Error> {
    let v: Value = serde_json::from_str(line)?;
    let meta = LineMeta {
        uuid: s(&v, "uuid"),
        session_id: s(&v, "sessionId"),
        timestamp: s(&v, "timestamp").and_then(|t| t.parse().ok()),
        cwd: s(&v, "cwd"),
        git_branch: s(&v, "gitBranch"),
        entrypoint: s(&v, "entrypoint"),
        agent_id: s(&v, "agentId"),
    };
    let event = match v.get("type").and_then(Value::as_str) {
        Some("assistant") => parse_assistant(&v),
        Some("user") => parse_user(&v),
        Some("system") => parse_system(&v),
        _ => Event::Other,
    };
    Ok(ParsedLine { meta, event })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ASSISTANT: &str = r#"{"type":"assistant","uuid":"u1","sessionId":"s1","timestamp":"2026-09-26T01:02:03.000Z","cwd":"/w","gitBranch":"main","entrypoint":"sdk-cli","agentId":"a9","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":2,"output_tokens":273,"cache_read_input_tokens":24775,"cache_creation_input_tokens":100,"cache_creation":{"ephemeral_5m_input_tokens":40,"ephemeral_1h_input_tokens":60}},"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"git status\n--short"}}]}}"#;

    #[test]
    fn parses_assistant_usage_and_tool_use() {
        let p = parse_line(ASSISTANT).unwrap();
        assert_eq!(p.meta.uuid.as_deref(), Some("u1"));
        assert_eq!(p.meta.session_id.as_deref(), Some("s1"));
        assert_eq!(p.meta.cwd.as_deref(), Some("/w"));
        assert_eq!(p.meta.git_branch.as_deref(), Some("main"));
        assert_eq!(p.meta.entrypoint.as_deref(), Some("sdk-cli"));
        assert_eq!(p.meta.agent_id.as_deref(), Some("a9"));
        assert_eq!(
            p.meta.timestamp.unwrap().to_rfc3339(),
            "2026-09-26T01:02:03+00:00"
        );
        let Event::Assistant {
            message_id,
            model,
            usage,
            blocks,
        } = p.event
        else {
            panic!()
        };
        assert_eq!(message_id, "msg_1");
        assert_eq!(model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(
            usage,
            TokenUsage {
                input: 2,
                output: 273,
                cache_read: 24775,
                cache_write_5m: 40,
                cache_write_1h: 60
            }
        );
        assert_eq!(
            blocks,
            vec![Block::ToolUse {
                id: "toolu_1".into(),
                name: "Bash".into(),
                summary: "git status --short".into()
            }]
        );
    }

    #[test]
    fn cache_creation_without_breakdown_counts_as_5m() {
        let line = r#"{"type":"assistant","message":{"id":"m","usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":9},"content":[{"type":"thinking","thinking":"x"},{"type":"redacted_thinking"},{"type":"text","text":"hi"},{"type":"image"}]}}"#;
        let Event::Assistant { usage, blocks, .. } = parse_line(line).unwrap().event else {
            panic!()
        };
        assert_eq!((usage.cache_write_5m, usage.cache_write_1h), (9, 0));
        assert_eq!(
            blocks,
            vec![Block::Thinking, Block::Thinking, Block::Text("hi".into())]
        );
    }

    #[test]
    fn null_cache_creation_falls_back_to_total() {
        let line = r#"{"type":"assistant","message":{"id":"m","usage":{"cache_creation_input_tokens":9,"cache_creation":null},"content":[]}}"#;
        let Event::Assistant { usage, .. } = parse_line(line).unwrap().event else {
            panic!()
        };
        assert_eq!((usage.cache_write_5m, usage.cache_write_1h), (9, 0));
    }

    #[test]
    fn only_text_blocks_form_the_prompt() {
        let line = r#"{"type":"user","message":{"content":[{"type":"document","text":"ignored","tool_use_id":"x"},{"type":"text","text":"a"}]}}"#;
        assert_eq!(
            parse_line(line).unwrap().event,
            Event::UserPrompt("a".into())
        );
    }

    #[test]
    fn assistant_without_message_id_is_other() {
        assert_eq!(
            parse_line(r#"{"type":"assistant","message":{"content":[]}}"#)
                .unwrap()
                .event,
            Event::Other
        );
    }

    #[test]
    fn user_string_content_is_prompt() {
        let line = r#"{"type":"user","sessionId":"s1","message":{"role":"user","content":"コミットしてください"}}"#;
        assert_eq!(
            parse_line(line).unwrap().event,
            Event::UserPrompt("コミットしてください".into())
        );
    }

    #[test]
    fn user_text_blocks_are_joined_prompt() {
        let line = r#"{"type":"user","message":{"content":[{"type":"text","text":"a"},{"type":"image"},{"type":"text","text":"b"}]}}"#;
        assert_eq!(
            parse_line(line).unwrap().event,
            Event::UserPrompt("a b".into())
        );
    }

    #[test]
    fn user_array_without_text_is_other() {
        let line = r#"{"type":"user","message":{"content":[{"type":"image"}]}}"#;
        assert_eq!(parse_line(line).unwrap().event, Event::Other);
        assert_eq!(
            parse_line(r#"{"type":"user","message":{"content":3}}"#)
                .unwrap()
                .event,
            Event::Other
        );
    }

    #[test]
    fn meta_user_lines_are_other() {
        let line = r#"{"type":"user","isMeta":true,"message":{"content":"<system>"}}"#;
        assert_eq!(parse_line(line).unwrap().event, Event::Other);
        let not_meta = r#"{"type":"user","isMeta":false,"message":{"content":"x"}}"#;
        assert_eq!(
            parse_line(not_meta).unwrap().event,
            Event::UserPrompt("x".into())
        );
    }

    #[test]
    fn tool_results_carry_error_flag() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":"x"},{"type":"tool_result","tool_use_id":"toolu_2","content":"ok"},{"type":"tool_result"}]}}"#;
        assert_eq!(
            parse_line(line).unwrap().event,
            Event::ToolResults(vec![
                ToolResult {
                    tool_use_id: "toolu_1".into(),
                    is_error: true
                },
                ToolResult {
                    tool_use_id: "toolu_2".into(),
                    is_error: false
                },
            ])
        );
    }

    #[test]
    fn other_types_and_bad_timestamp_are_tolerated() {
        let p = parse_line(r#"{"type":"attachment","timestamp":"bad"}"#).unwrap();
        assert_eq!(p.event, Event::Other);
        assert!(p.meta.timestamp.is_none());
    }

    #[test]
    fn malformed_content_blocks_are_dropped() {
        let line = json!({"type": "assistant", "message": {"id": "m1", "content": [
            {"text": "型なし"},
            {"type": 1},
            {"type": "text"},
            {"type": "text", "text": 1},
            {"type": "tool_use", "id": "t1"},
            {"type": "tool_use", "name": "Bash"},
            {"type": "image"}
        ]}});
        let Event::Assistant { blocks, .. } = parse_line(&line.to_string()).unwrap().event else {
            panic!("assistant行として解析されること");
        };
        assert!(blocks.is_empty(), "{blocks:?}");
    }

    #[test]
    fn invalid_json_is_error() {
        assert!(parse_line("{").is_err());
    }

    #[test]
    fn summarize_picks_meaningful_field_per_tool() {
        assert_eq!(summarize_tool_input("Bash", &json!({"command":"ls"})), "ls");
        for tool in ["Read", "Write", "Edit", "NotebookEdit"] {
            assert_eq!(
                summarize_tool_input(tool, &json!({"file_path":"/a.rs"})),
                "/a.rs"
            );
        }
        for tool in ["Grep", "Glob"] {
            assert_eq!(
                summarize_tool_input(tool, &json!({"pattern":"mutex"})),
                "mutex"
            );
        }
        assert_eq!(
            summarize_tool_input("WebFetch", &json!({"url":"https://x"})),
            "https://x"
        );
        assert_eq!(
            summarize_tool_input("WebSearch", &json!({"query":"q"})),
            "q"
        );
        for tool in ["Agent", "Task"] {
            assert_eq!(
                summarize_tool_input(
                    tool,
                    &json!({"subagent_type":"go-reviewer","description":"レビュー"})
                ),
                "go-reviewer「レビュー」"
            );
        }
        assert_eq!(
            summarize_tool_input("Agent", &json!({})),
            "general-purpose「」"
        );
        assert_eq!(summarize_tool_input("Unknown", &json!({"x":1})), "");
        assert_eq!(summarize_tool_input("Bash", &json!({})), "");
    }

    #[test]
    fn one_line_collapses_whitespace_and_truncates_by_chars() {
        assert_eq!(one_line("a\n  b", 10), "a b");
        assert_eq!(one_line("あいうえお", 3), "あいう…");
        assert_eq!(one_line("abc", 3), "abc");
        assert_eq!(
            one_line(&"x".repeat(SUMMARY_CHARS + 1), SUMMARY_CHARS)
                .chars()
                .count(),
            SUMMARY_CHARS + 1
        );
    }

    #[test]
    fn session_kind_from_entrypoint() {
        assert_eq!(
            SessionKind::from_entrypoint(Some("sdk-py")),
            SessionKind::Headless
        );
        assert_eq!(
            SessionKind::from_entrypoint(Some("cli")),
            SessionKind::Interactive
        );
        assert_eq!(SessionKind::from_entrypoint(None), SessionKind::Interactive);
        for k in [
            SessionKind::Interactive,
            SessionKind::Headless,
            SessionKind::BackgroundJob,
        ] {
            assert_eq!(SessionKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(SessionKind::Headless.as_str(), "headless");
        assert_eq!(SessionKind::parse("x"), None);
    }
    #[test]
    fn system_away_summary_becomes_recap_note() {
        let line = r#"{"type":"system","subtype":"away_summary","content":"作業は\n終わりました","uuid":"n1","sessionId":"s1","timestamp":"2026-10-04T03:27:16.359Z"}"#;
        let pl = parse_line(line).unwrap();
        assert_eq!(pl.meta.uuid.as_deref(), Some("n1"));
        assert_eq!(
            pl.event,
            Event::Note {
                kind: NoteKind::Recap,
                text: "作業は 終わりました".into()
            }
        );
    }

    #[test]
    fn system_compact_boundary_becomes_compact_note() {
        let line = r#"{"type":"system","subtype":"compact_boundary","content":"Conversation compacted","uuid":"n2","sessionId":"s1"}"#;
        assert_eq!(
            parse_line(line).unwrap().event,
            Event::Note {
                kind: NoteKind::Compact,
                text: "Conversation compacted".into()
            }
        );
    }

    #[test]
    fn compact_boundary_without_content_still_counts() {
        let line = r#"{"type":"system","subtype":"compact_boundary","uuid":"n3"}"#;
        assert_eq!(
            parse_line(line).unwrap().event,
            Event::Note {
                kind: NoteKind::Compact,
                text: String::new()
            }
        );
    }

    #[test]
    fn other_system_lines_and_broken_recaps_are_ignored() {
        for line in [
            r#"{"type":"system","subtype":"turn_duration","content":"x"}"#,
            r#"{"type":"system","subtype":"away_summary","content":3}"#,
            r#"{"type":"system","subtype":"away_summary"}"#,
            r#"{"type":"system"}"#,
        ] {
            assert_eq!(parse_line(line).unwrap().event, Event::Other, "{line}");
        }
    }

    #[test]
    fn long_recap_is_cut_at_note_chars() {
        let body = "あ".repeat(NOTE_CHARS + 5);
        let line = json!({"type":"system","subtype":"away_summary","content":body}).to_string();
        let Event::Note { text, .. } = parse_line(&line).unwrap().event else {
            panic!("要約になっていない")
        };
        assert_eq!(text.chars().count(), NOTE_CHARS + 1);
        let exact =
            json!({"type":"system","subtype":"away_summary","content":"あ".repeat(NOTE_CHARS)})
                .to_string();
        let Event::Note { text, .. } = parse_line(&exact).unwrap().event else {
            panic!("要約になっていない")
        };
        assert_eq!(text.chars().count(), NOTE_CHARS);
    }

    #[test]
    fn note_kind_round_trips() {
        for k in [NoteKind::Recap, NoteKind::Compact] {
            assert_eq!(NoteKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(NoteKind::Recap.as_str(), "recap");
        assert_eq!(NoteKind::Compact.as_str(), "compact");
        assert_eq!(NoteKind::parse("x"), None);
    }
    #[test]
    fn recap_drops_trailing_config_hint() {
        let line = r#"{"type":"system","subtype":"away_summary","content":"作業は終わりました。 (disable recaps in /config)"}"#;
        assert_eq!(
            parse_line(line).unwrap().event,
            Event::Note {
                kind: NoteKind::Recap,
                text: "作業は終わりました。".into()
            }
        );
    }
}
