//! 取り込み結果をDBへ渡すための行の型。
//!
//! controllersがSQLの列を知らずに済むよう、repositoriesとの受け渡しはこの型で行う。
use crate::models::domain::pricing::TokenUsage;
use crate::models::domain::transcript::{NoteKind, SessionKind};
use chrono::{DateTime, Utc};
use std::path::PathBuf;

/// JSONLの読み取り位置。差分だけを読むために保存する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOffset {
    /// ファイルの絶対パス。
    pub path: PathBuf,
    /// 次に読む位置（バイト）。
    pub offset: u64,
    /// 前回読んだときのサイズ。変化がなければ読まずに済ませるため。
    pub size: u64,
    /// 前回読んだときの更新時刻（ミリ秒）。
    pub mtime_ms: i64,
}

/// セッション行の追加または更新。既存の値との合わせ方はrepositories側のSQLで決める。
#[derive(Debug, Clone, PartialEq)]
pub struct SessionUpsert {
    /// セッションID。
    pub session_id: String,
    /// プロファイルID。
    pub profile_id: i64,
    /// 種別。より具体的な種別（ジョブ > ヘッドレス > 対話）が優先される。
    pub kind: SessionKind,
    /// 起動経路。
    pub entrypoint: Option<String>,
    /// 作業ディレクトリ。
    pub cwd: Option<String>,
    /// gitブランチ。
    pub git_branch: Option<String>,
    /// 表示名。
    pub name: Option<String>,
    /// 最初の発話。一覧で内容を思い出せるようにするため。
    pub first_prompt: Option<String>,
    /// 開始時刻。
    pub started_at: DateTime<Utc>,
    /// 最終活動時刻。稼働中かどうかの判定に使う。
    pub last_activity_at: DateTime<Utc>,
    /// 状態（`busy`、`working`、`ended`など、記録元の値のまま）。
    pub status: Option<String>,
}

/// ターン1件。`(session_id, agent_id, message_id)`で一意。
#[derive(Debug, Clone, PartialEq)]
pub struct TurnRecord {
    /// セッションID。
    pub session_id: String,
    /// サブエージェントID。本体は空文字（一意制約でNULLを避けるため）。
    pub agent_id: String,
    /// API応答ID、または発話行のuuid。
    pub message_id: String,
    /// 時刻。
    pub ts: DateTime<Utc>,
    /// モデルID。
    pub model: Option<String>,
    /// 種類（`thinking`、`text`、`tool_use`、`prompt`）。
    pub kind: String,
    /// 要約。
    pub summary: String,
    /// Token数。
    pub usage: TokenUsage,
}

/// Claude Codeが書いた要約、または圧縮の印1件。`uuid`で一意。
#[derive(Debug, Clone, PartialEq)]
pub struct NoteRecord {
    /// 行のuuid。
    pub uuid: String,
    /// セッションID。サブエージェントの行でも親のID。
    pub session_id: String,
    /// 時刻。
    pub ts: DateTime<Utc>,
    /// 種類。
    pub kind: NoteKind,
    /// 本文。
    pub text: String,
}

/// ツール呼び出し1件。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallRecord {
    /// 呼び出しID。
    pub tool_use_id: String,
    /// セッションID。
    pub session_id: String,
    /// サブエージェントID（本体は空文字）。
    pub agent_id: String,
    /// 時刻。
    pub ts: DateTime<Utc>,
    /// ツール名。
    pub tool_name: String,
}

/// サブエージェント1件。`.meta.json`の内容。
#[derive(Debug, Clone, PartialEq)]
pub struct SubagentRecord {
    /// サブエージェントID。
    pub agent_id: String,
    /// 親セッションID。
    pub session_id: String,
    /// 種類（`go-reviewer`など）。
    pub agent_type: Option<String>,
    /// 依頼内容の説明。
    pub description: Option<String>,
    /// 親の`Agent`呼び出しID。ライブログで親子を結ぶため。
    pub parent_tool_use_id: Option<String>,
    /// 入れ子の深さ。
    pub spawn_depth: Option<i64>,
}

/// バックグラウンドジョブ1件。`state.json`の内容。
#[derive(Debug, Clone, PartialEq)]
pub struct JobRecord {
    /// ジョブID（ディレクトリ名）。
    pub job_id: String,
    /// プロファイルID。
    pub profile_id: i64,
    /// 結び付くセッションID。
    pub session_id: Option<String>,
    /// 表示名。
    pub name: Option<String>,
    /// 状態（`working`など）。
    pub state: String,
    /// 人が読める進捗説明。
    pub detail: Option<String>,
    /// 実行中タスク数。
    pub in_flight_tasks: i64,
    /// ジョブが報告するToken数。
    pub tokens: Option<i64>,
    /// 作成時刻。
    pub created_at: Option<DateTime<Utc>>,
    /// 更新時刻。
    pub updated_at: DateTime<Utc>,
}

/// 外部取得の結果区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchResult {
    /// 成功。
    Ok,
    /// 失敗。
    Failed,
}

impl FetchResult {
    /// DB保存用の文字列。
    pub fn as_str(self) -> &'static str {
        match self {
            FetchResult::Ok => "ok",
            FetchResult::Failed => "failed",
        }
    }
}

/// 取得ログ1件。診断画面で失敗理由を利用者に見せるために残す。
#[derive(Debug, Clone, PartialEq)]
pub struct FetchLogEntry {
    /// 対象（`usage:<profile_id>`、`catalog`、`ingest`）。
    pub target: String,
    /// 時刻。
    pub at: DateTime<Utc>,
    /// 結果。
    pub result: FetchResult,
    /// HTTPステータス。
    pub http_status: Option<u16>,
    /// 説明。トークンを含めてはならない。
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_result_strings_are_stable() {
        assert_eq!(FetchResult::Ok.as_str(), "ok");
        assert_eq!(FetchResult::Failed.as_str(), "failed");
    }
}
