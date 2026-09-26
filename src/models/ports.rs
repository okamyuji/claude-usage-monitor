//! controllersが依存する外部依存の抽象。
//!
//! 具象型（SQLite、ureq、Keychain）をcontrollersから隠し、テストでHTTPや時計を差し替えられるようにする。
//! DBはテストでも実SQLiteを使うため、リポジトリのtraitはテスト用の差し替えではなく層の境界として置く。
use crate::models::domain::pricing::ModelInfo;
use crate::models::domain::profile::Profile;
use crate::models::domain::projection::Sample;
use crate::models::domain::read_models::{
    DaemonStat, DailyUsage, GroupBy, GroupUsage, LatestUsage, RunRow, SessionFilter,
    SessionModelUsage, SessionRow, SubagentRow, ToolStat, TurnRow,
};
use crate::models::domain::records::{
    FetchLogEntry, FileOffset, JobRecord, SessionUpsert, SubagentRecord, ToolCallRecord, TurnRecord,
};
use crate::models::domain::settings::Settings;
use crate::models::domain::usage::UsageSnapshot;
use chrono::{DateTime, Utc};
use std::path::Path;

/// 永続化の失敗。SQLiteの型をcontrollersへ漏らさないよう文字列で包む。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RepoError {
    /// DBの読み書きに失敗した。
    #[error("DB操作に失敗しました: {0}")]
    Storage(String),
    /// 指定した行がない。
    #[error("見つかりません: {0}")]
    NotFound(String),
    /// 入力が不正。
    #[error("入力が不正です: {0}")]
    Invalid(String),
}

/// 現在時刻。テストで時刻を固定するために注入する。
pub trait Clock: Send + Sync {
    /// 現在時刻（UTC）。
    fn now(&self) -> DateTime<Utc>;
}

/// 使用量APIの失敗区分。区分ごとに再試行の方針と画面の文言が変わるため分ける。
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum UsageApiError {
    /// 401または403。トークン期限切れ。
    #[error("トークンが無効です（期限切れの可能性があります）")]
    Unauthorized,
    /// 429。
    #[error("取得回数の上限に達しました")]
    RateLimited,
    /// その他のHTTPステータス。
    #[error("使用量APIがHTTP {0}を返しました")]
    Http(u16),
    /// 接続やタイムアウト。
    #[error("使用量APIに接続できません: {0}")]
    Transport(String),
    /// 応答を解析できない。
    #[error("使用量APIの応答を解析できません: {0}")]
    Parse(String),
}

/// 使用量API。
pub trait UsageApi: Send + Sync {
    /// アクセストークンで現在の使用量を取得する。
    fn fetch(&self, access_token: &str) -> Result<UsageSnapshot, UsageApiError>;
}

/// 認証情報。`Debug`を手書きしてトークンを伏せるのは、ログやテストの失敗表示にトークンが出る事故を防ぐため。
#[derive(Clone, PartialEq)]
pub struct Credential {
    /// アクセストークン。
    pub access_token: String,
    /// 期限。過ぎていればAPIを呼ばずに期限切れと表示する。
    pub expires_at: Option<DateTime<Utc>>,
    /// 契約種別（`max`など）。プロファイル画面に表示する。
    pub subscription_type: Option<String>,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("access_token", &"<伏せ字>")
            .field("expires_at", &self.expires_at)
            .field("subscription_type", &self.subscription_type)
            .finish()
    }
}

/// 認証情報の読み出し失敗。
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CredentialError {
    /// 保存先に認証情報がない。未ログイン。
    #[error("認証情報がありません。このプロファイルでclaudeにログインしてください")]
    Missing,
    /// 読めたが形式が違う。
    #[error("認証情報の形式を読めません: {0}")]
    Malformed(String),
    /// 保存先へのアクセスに失敗した。
    #[error("認証情報の保存先を読めません: {0}")]
    Io(String),
}

/// 認証情報の保存先。
pub trait CredentialStore: Send + Sync {
    /// プロファイルの認証情報を読む。`config_dir`は解決済みの設定ディレクトリ。
    fn load(&self, profile: &Profile, config_dir: &Path) -> Result<Credential, CredentialError>;
}

/// 公式ページからのモデル情報取得の失敗。
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CatalogError {
    /// 取得に失敗した。
    #[error("公式ページを取得できません: {0}")]
    Fetch(String),
    /// 表が見つからない、または1件も読めない。ページ構成の変更を疑う。
    #[error("公式ページから単価表を読み取れません: {0}")]
    Parse(String),
}

/// 公式のモデル情報の取得元。
pub trait ModelCatalogSource: Send + Sync {
    /// 最新のモデル情報を取得する。
    fn fetch(&self) -> Result<Vec<ModelInfo>, CatalogError>;
}

/// OSのプロセス情報。
pub trait ProcessInfo: Send + Sync {
    /// pidのプロセスが生きているか。稼働中セッションの判定に使う。
    fn is_alive(&self, pid: u32) -> bool;
    /// 自プロセスのRSS（バイト）。リークを目視で確かめるために記録する。
    fn self_rss_bytes(&self) -> u64;
}

/// プロファイルの永続化。
pub trait ProfileRepo: Send + Sync {
    /// 全件を名前順で返す。
    fn list(&self) -> Result<Vec<Profile>, RepoError>;
    /// 追加する。名前が不正または重複なら`Invalid`。
    fn add(&self, name: &str, config_dir: Option<&Path>) -> Result<Profile, RepoError>;
    /// 削除する。使用中のプロファイルは削除できない。
    fn remove(&self, name: &str) -> Result<(), RepoError>;
    /// 使用中にする。他のプロファイルは使用中でなくなる。
    fn set_active(&self, name: &str) -> Result<(), RepoError>;
    /// 使用中のプロファイル。
    fn active(&self) -> Result<Option<Profile>, RepoError>;
    /// 1件もなければ`~/.claude`を指す既定プロファイルを使用中として作る。初回起動で設定なしに動かすため。
    fn ensure_default(&self) -> Result<Profile, RepoError>;
}

/// 使用率の永続化。
pub trait UsageRepo: Send + Sync {
    /// 1回の取得結果を保存する。
    fn record_snapshot(
        &self,
        profile_id: i64,
        at: DateTime<Utc>,
        snap: &UsageSnapshot,
    ) -> Result<(), RepoError>;
    /// 指定の枠の`since`以降の使用率を時刻順で返す。予測に使う。
    fn samples(
        &self,
        profile_id: i64,
        kind: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<Sample>, RepoError>;
}

/// 取得ログの永続化。
pub trait FetchLogRepo: Send + Sync {
    /// 1件記録する。
    fn log(&self, e: &FetchLogEntry) -> Result<(), RepoError>;
    /// 対象の新しい順に`limit`件返す。
    fn recent(&self, target: &str, limit: usize) -> Result<Vec<FetchLogEntry>, RepoError>;
}

/// モデル情報の永続化。
pub trait ModelRepo: Send + Sync {
    /// 全件。
    fn all(&self) -> Result<Vec<ModelInfo>, RepoError>;
    /// 空なら初期値を入れる。
    fn seed_if_empty(&self, models: &[ModelInfo]) -> Result<(), RepoError>;
    /// 公式の値で追加または更新する。`source=user`の行は変えない。更新した行数を返す。
    fn upsert_official(&self, models: &[ModelInfo], at: DateTime<Utc>) -> Result<usize, RepoError>;
    /// 利用者の編集として追加または更新する。以後の公式情報の自動取得で上書きされない。
    fn set_user(&self, m: &ModelInfo) -> Result<(), RepoError>;
}

/// 設定の永続化。
pub trait SettingsRepo: Send + Sync {
    /// 保存済みの設定。未保存の項目は既定値になる。
    fn load(&self) -> Result<Settings, RepoError>;
    /// 検証してから保存する。範囲外なら`Invalid`で、何も保存しない。
    fn save(&self, s: &Settings) -> Result<(), RepoError>;
}

/// JSONL取り込みの永続化。
pub trait IngestRepo: Send + Sync {
    /// 保存済みの読み取り位置。
    fn offset(&self, path: &Path) -> Result<Option<FileOffset>, RepoError>;
    /// 読み取り位置を保存する。
    fn save_offset(&self, o: &FileOffset) -> Result<(), RepoError>;
    /// セッションを追加または更新する。
    fn upsert_session(&self, s: &SessionUpsert) -> Result<(), RepoError>;
    /// ターンを追加する。同じ`message_id`が再来してもTokenを加算しない。
    fn upsert_turn(&self, t: &TurnRecord) -> Result<(), RepoError>;
    /// ツール呼び出しを追加する。
    fn upsert_tool_call(&self, c: &ToolCallRecord) -> Result<(), RepoError>;
    /// ツール呼び出しをエラーとして記録する。
    fn mark_tool_error(&self, tool_use_id: &str) -> Result<(), RepoError>;
    /// サブエージェントを追加または更新する。
    fn upsert_subagent(&self, s: &SubagentRecord) -> Result<(), RepoError>;
    /// ジョブを追加または更新する。
    fn upsert_job(&self, j: &JobRecord) -> Result<(), RepoError>;
    /// 稼働中ファイルに現れない対話セッションを`ended`にし、件数を返す。
    /// Claude Codeは終了時に稼働中ファイルを消すため、消えたことを終了の根拠にする。
    fn end_missing_interactive(
        &self,
        profile_id: i64,
        alive_session_ids: &[String],
    ) -> Result<usize, RepoError>;
}

/// 概要画面の読み取り。
pub trait DashboardRepo: Send + Sync {
    /// プロファイルの最新の取得結果。まだ取得していなければ`None`。
    fn latest_usage(&self, profile_id: i64) -> Result<Option<LatestUsage>, RepoError>;
    /// 稼働中の候補。`since`以降に動いたセッションと、状態が終了でない対話セッションを、新しい順に最大200件返す。
    /// 稼働中の最終判定は設定の時間を使うcontrollerで行う。
    fn recent_runs(&self, since: DateTime<Utc>) -> Result<Vec<RunRow>, RepoError>;
}

/// セッション画面とログ再生の読み取り。
pub trait SessionQueryRepo: Send + Sync {
    /// 絞り込んだセッションを新しい順に返す。
    fn list_sessions(&self, f: &SessionFilter) -> Result<Vec<SessionRow>, RepoError>;
    /// 1件。
    fn session(&self, session_id: &str) -> Result<Option<SessionRow>, RepoError>;
    /// ターンを時刻順に返す。長いセッションでは新しい方から`limit`件だけを返す。
    fn turns(&self, session_id: &str, limit: usize) -> Result<Vec<TurnRow>, RepoError>;
    /// セッション、エージェント、モデルごとの合計。
    fn model_usage(&self, session_ids: &[String]) -> Result<Vec<SessionModelUsage>, RepoError>;
    /// サブエージェント。
    fn subagents(&self, session_ids: &[String]) -> Result<Vec<SubagentRow>, RepoError>;
    /// セッションに結び付くジョブのID。ライブログで`timeline.jsonl`を探すのに使う。
    fn job_id(&self, session_id: &str) -> Result<Option<String>, RepoError>;
}

/// 分析画面と推移画面の読み取り。
pub trait AnalyticsRepo: Send + Sync {
    /// `since`以降のターンを軸とモデルで集計する。
    fn usage_by(&self, group: GroupBy, since: DateTime<Utc>) -> Result<Vec<GroupUsage>, RepoError>;
    /// 現地日付ごとの合計。`tz_offset_secs`はUTCからの時差（秒）。
    fn daily_usage(
        &self,
        since: DateTime<Utc>,
        tz_offset_secs: i32,
    ) -> Result<Vec<DailyUsage>, RepoError>;
    /// ツールごとの呼び出し回数とエラー回数を、回数の多い順に返す。
    fn tool_stats(&self, since: DateTime<Utc>) -> Result<Vec<ToolStat>, RepoError>;
    /// 枠のリセット時刻を分単位でまとめて返す。APIはリセット時刻をミリ秒の揺れ付きで返すため。
    fn reset_times(
        &self,
        profile_id: i64,
        kind: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<DateTime<Utc>>, RepoError>;
}

/// 診断画面の読み取り。
pub trait DiagnosticsRepo: Send + Sync {
    /// `since`以降のデーモン統計を時刻順に返す。
    fn daemon_stats(&self, since: DateTime<Utc>) -> Result<Vec<DaemonStat>, RepoError>;
    /// DBファイルとWALの合計サイズ。
    fn db_size(&self) -> u64;
}

/// デーモンの稼働確認と起動。GUIの「デーモン停止中」表示と起動ボタンに使う。
pub trait DaemonControl: Send + Sync {
    /// 稼働中か。
    fn is_running(&self) -> bool;
    /// 起動する。起動を待たずに戻る。
    fn start(&self) -> Result<(), String>;
}

/// モデル情報を今すぐ公式ページから取り直す。設定画面の「今すぐ更新」に使う。
pub trait CatalogRefresh: Send + Sync {
    /// 取り直し、更新した行数を返す。
    fn refresh(&self) -> Result<usize, RepoError>;
}

/// 保守処理の永続化。
pub trait MaintenanceRepo: Send + Sync {
    /// `cutoff`より古い時系列の行を削除し、削除行数を返す。
    fn purge_before(&self, cutoff: DateTime<Utc>) -> Result<usize, RepoError>;
    /// デーモンのRSSとDBサイズを記録する。
    fn record_daemon_stats(&self, at: DateTime<Utc>, rss_bytes: u64) -> Result<(), RepoError>;
}

/// OS通知。
pub trait Notifier: Send + Sync {
    /// 通知を1件出す。
    fn notify(&self, title: &str, body: &str) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_debug_hides_token() {
        let c = Credential {
            access_token: "secret-token".into(),
            expires_at: None,
            subscription_type: Some("max".into()),
        };
        let shown = format!("{c:?}");
        assert!(!shown.contains("secret-token"));
        assert!(shown.contains("max"));
    }
}
