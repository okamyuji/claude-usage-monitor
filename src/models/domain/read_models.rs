//! 画面用の読み取り結果の型。
//!
//! DBの行の形をcontrollersへ渡すための型で、書き込み用の`records.rs`とは分ける。読み取りの列を増やしても取り込み側に影響させないため。
use crate::models::domain::pricing::TokenUsage;
use crate::models::domain::transcript::SessionKind;
use crate::models::domain::usage::{BreakdownRow, LimitWindow, Spend};
use chrono::{DateTime, Utc};

/// プロファイルの最新の取得結果。
#[derive(Debug, Clone, PartialEq)]
pub struct LatestUsage {
    /// 取得時刻。「N分前の値」の表示に使う。
    pub fetched_at: DateTime<Utc>,
    /// 枠。DBは`group`を持たないため、`group`は空文字になる。
    pub limits: Vec<LimitWindow>,
    /// 用途の内訳。
    pub breakdown: Vec<BreakdownRow>,
    /// 追加課金。
    pub spend: Option<Spend>,
}

/// セッション1件。
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    /// セッションID。
    pub session_id: String,
    /// プロファイルID。
    pub profile_id: i64,
    /// 種別。
    pub kind: SessionKind,
    /// 名前。
    pub name: Option<String>,
    /// 作業ディレクトリ。
    pub cwd: Option<String>,
    /// ブランチ。
    pub git_branch: Option<String>,
    /// 最初の入力。名前がないセッションの見出しに使う。
    pub first_prompt: Option<String>,
    /// 稼働中ファイルやジョブの状態。
    pub status: Option<String>,
    /// 開始時刻。
    pub started_at: DateTime<Utc>,
    /// 最終更新。
    pub last_activity_at: DateTime<Utc>,
    /// 本体の最後のターンの種類。状態の判定に使う。
    pub last_turn_kind: Option<String>,
    /// 本体の最後の応答のモデル。コンテキスト長の参照に使う。
    pub last_model: Option<String>,
    /// 本体の最後の応答が読んだ入力の量。コンテキスト使用率の分子。
    pub last_context_tokens: u64,
    /// サブエージェントを含むターン数。
    pub turn_count: i64,
}

/// ジョブの進捗。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobInfo {
    /// ジョブID。ライブログで`timeline.jsonl`を探すのに使う。
    pub job_id: String,
    /// 状態。
    pub state: String,
    /// 人が読める進捗説明。
    pub detail: Option<String>,
    /// 実行中タスク数。
    pub in_flight_tasks: i64,
}

/// 稼働中の候補1件。
#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    /// セッション。
    pub session: SessionRow,
    /// ジョブならその進捗。
    pub job: Option<JobInfo>,
}

/// サブエージェント1件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentRow {
    /// エージェントID。
    pub agent_id: String,
    /// 親セッションID。
    pub session_id: String,
    /// 種類（`go-reviewer`など）。
    pub agent_type: Option<String>,
    /// 起動時の説明。
    pub description: Option<String>,
    /// 親の`Agent`呼び出しのID。ライブログで起動行に説明を付けるのに使う。
    pub parent_tool_use_id: Option<String>,
    /// 最初のターンの時刻。
    pub first_ts: Option<DateTime<Utc>>,
    /// 最後のターンの時刻。稼働中の判定に使う。
    pub last_ts: Option<DateTime<Utc>>,
    /// 最後のターンの種類。
    pub last_turn_kind: Option<String>,
}

/// セッション、エージェント、モデルごとのToken合計。コストはモデルごとの単価で出すため、モデル単位で返す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionModelUsage {
    /// セッションID。
    pub session_id: String,
    /// エージェントID（本体は空文字）。
    pub agent_id: String,
    /// モデル。
    pub model: Option<String>,
    /// 合計。
    pub usage: TokenUsage,
}

/// セッション一覧の絞り込み。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFilter {
    /// 名前、最初の入力、作業ディレクトリ、IDに含まれる文字列。空なら絞らない。
    pub query: String,
    /// プロファイル。
    pub profile_id: Option<i64>,
    /// 種別。
    pub kind: Option<SessionKind>,
    /// 最大件数。spec 11章の1,000件を既定にする。
    pub limit: usize,
}

impl Default for SessionFilter {
    fn default() -> Self {
        Self {
            query: String::new(),
            profile_id: None,
            kind: None,
            limit: 1000,
        }
    }
}

/// ターン1件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRow {
    /// エージェントID（本体は空文字）。
    pub agent_id: String,
    /// メッセージID。
    pub message_id: String,
    /// 時刻。
    pub ts: DateTime<Utc>,
    /// モデル。
    pub model: Option<String>,
    /// 種類（`prompt`、`text`、`tool_use`、`thinking`）。
    pub kind: String,
    /// 要約（200字まで）。
    pub summary: String,
    /// Token数。
    pub usage: TokenUsage,
}

/// 分析画面の集計の軸。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    /// 作業ディレクトリ。
    Project,
    /// モデル。
    Model,
    /// ブランチ。
    Branch,
    /// 本体（`main`）とサブエージェント（`subagent`）。
    Agent,
}

/// 集計1行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupUsage {
    /// 軸の値。
    pub key: String,
    /// モデル。コスト計算のため軸とは別に持つ。
    pub model: Option<String>,
    /// 合計。
    pub usage: TokenUsage,
}

/// 日ごとの合計。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyUsage {
    /// 現地日付（`YYYY-MM-DD`）。
    pub day: String,
    /// 合計。
    pub usage: TokenUsage,
}

/// ツールごとの呼び出し回数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolStat {
    /// ツール名。
    pub tool_name: String,
    /// 呼び出し回数。
    pub calls: i64,
    /// エラー回数。
    pub errors: i64,
}

/// デーモンの統計1点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonStat {
    /// 時刻。
    pub at: DateTime<Utc>,
    /// RSS。
    pub rss_bytes: u64,
    /// DBサイズ。
    pub db_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_filter_default_is_unfiltered_1000() {
        let f = SessionFilter::default();
        assert_eq!(
            (f.query.as_str(), f.profile_id, f.kind, f.limit),
            ("", None, None, 1000)
        );
    }
}
