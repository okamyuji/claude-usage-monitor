//! 実行の種別と状態、稼働中かどうかの判定。
//!
//! 対話、ヘッドレス、ジョブ、サブエージェントで「稼働中」の根拠が違うため、判定をここに集めて画面ごとのずれを防ぐ。

use crate::models::domain::transcript::SessionKind;
use chrono::{DateTime, Duration, Utc};

/// 画面に出す実行の種別。DBの`SessionKind`にサブエージェントを加えたもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    /// 画面で操作中の対話セッション。
    Interactive,
    /// SDKや`claude -p`による実行。
    Headless,
    /// バックグラウンドジョブ。
    Job,
    /// サブエージェント。
    Subagent,
}

impl RunKind {
    /// DBの種別から変換する。
    pub fn from_session(k: SessionKind) -> Self {
        match k {
            SessionKind::Interactive => RunKind::Interactive,
            SessionKind::Headless => RunKind::Headless,
            SessionKind::BackgroundJob => RunKind::Job,
        }
    }

    /// バッジの文言。
    pub fn label(self) -> &'static str {
        match self {
            RunKind::Interactive => "対話",
            RunKind::Headless => "ヘッドレス",
            RunKind::Job => "ジョブ",
            RunKind::Subagent => "サブエージェント",
        }
    }
}

/// 実行の状態。spec 7.1節の4つの名前に、未知の状態を名前のまま出す`Other`を加える。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    /// 応答生成中。
    Generating,
    /// ツール実行中。
    RunningTool,
    /// 入力待ち。
    WaitingInput,
    /// 終了。
    Ended,
    /// 未知の状態。Claude Codeが新しい状態を足しても表示から落とさないため。
    Other(String),
}

impl RunState {
    /// バッジの文言。
    pub fn label(&self) -> String {
        match self {
            RunState::Generating => "応答生成中".into(),
            RunState::RunningTool => "ツール実行中".into(),
            RunState::WaitingInput => "入力待ち".into(),
            RunState::Ended => "終了".into(),
            RunState::Other(s) => s.clone(),
        }
    }
}

/// 状態を決める。`busy`だけでは生成中かツール実行中か分からないため、最後のターンの種類で分ける。
pub fn run_state(status: Option<&str>, last_turn_kind: Option<&str>) -> RunState {
    match status {
        Some("ended") => RunState::Ended,
        Some("idle") => RunState::WaitingInput,
        None | Some("busy") | Some("working") => match last_turn_kind {
            Some("tool_use") => RunState::RunningTool,
            _ => RunState::Generating,
        },
        Some(other) => RunState::Other(other.to_string()),
    }
}

/// 稼働中と見なす時間。設定画面で変えられる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityRules {
    /// ヘッドレスとサブエージェントを、最終更新からこの時間まで稼働中とする。
    pub headless_active: Duration,
    /// ジョブを、`working`かつ最終更新からこの時間まで稼働中とする。
    pub job_active: Duration,
}

/// 稼働中かどうか。対話は稼働中ファイルの状態、他は最終更新からの経過時間で決める（spec 5.5節）。
pub fn is_active(
    kind: SessionKind,
    status: Option<&str>,
    last_activity: DateTime<Utc>,
    now: DateTime<Utc>,
    rules: &ActivityRules,
) -> bool {
    match kind {
        SessionKind::Interactive => status.is_some_and(|s| s != "ended"),
        SessionKind::Headless => now - last_activity <= rules.headless_active,
        SessionKind::BackgroundJob => {
            status == Some("working") && now - last_activity <= rules.job_active
        }
    }
}

/// サブエージェントが稼働中か。記録はJSONLだけなので最終更新からの経過時間で決める。
pub fn subagent_active(last: DateTime<Utc>, now: DateTime<Utc>, rules: &ActivityRules) -> bool {
    now - last <= rules.headless_active
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::transcript::SessionKind;
    use chrono::{Duration, TimeZone, Utc};

    fn rules() -> ActivityRules {
        ActivityRules {
            headless_active: Duration::seconds(120),
            job_active: Duration::minutes(10),
        }
    }

    #[test]
    fn kinds_have_labels() {
        assert_eq!(
            RunKind::from_session(SessionKind::Interactive),
            RunKind::Interactive
        );
        assert_eq!(
            RunKind::from_session(SessionKind::Headless),
            RunKind::Headless
        );
        assert_eq!(
            RunKind::from_session(SessionKind::BackgroundJob),
            RunKind::Job
        );
        let labels: Vec<_> = [
            RunKind::Interactive,
            RunKind::Headless,
            RunKind::Job,
            RunKind::Subagent,
        ]
        .iter()
        .map(|k| k.label())
        .collect();
        assert_eq!(labels, ["対話", "ヘッドレス", "ジョブ", "サブエージェント"]);
    }

    #[test]
    fn state_comes_from_status_then_last_turn() {
        assert_eq!(run_state(Some("ended"), Some("tool_use")), RunState::Ended);
        assert_eq!(run_state(Some("idle"), None), RunState::WaitingInput);
        assert_eq!(
            run_state(Some("busy"), Some("tool_use")),
            RunState::RunningTool
        );
        assert_eq!(run_state(Some("busy"), Some("text")), RunState::Generating);
        assert_eq!(
            run_state(Some("working"), Some("tool_use")),
            RunState::RunningTool
        );
        assert_eq!(run_state(None, Some("prompt")), RunState::Generating);
        assert_eq!(
            run_state(Some("shell"), None),
            RunState::Other("shell".into())
        );
        assert_eq!(RunState::Other("shell".into()).label(), "shell");
        assert_eq!(RunState::Generating.label(), "応答生成中");
        assert_eq!(RunState::RunningTool.label(), "ツール実行中");
        assert_eq!(RunState::WaitingInput.label(), "入力待ち");
        assert_eq!(RunState::Ended.label(), "終了");
    }

    #[test]
    fn activity_rules_per_kind() {
        let now = Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap();
        let r = rules();
        assert!(is_active(
            SessionKind::Interactive,
            Some("idle"),
            now - Duration::hours(5),
            now,
            &r
        ));
        assert!(!is_active(
            SessionKind::Interactive,
            Some("ended"),
            now,
            now,
            &r
        ));
        assert!(!is_active(SessionKind::Interactive, None, now, now, &r));
        assert!(is_active(
            SessionKind::Headless,
            None,
            now - Duration::seconds(120),
            now,
            &r
        ));
        assert!(!is_active(
            SessionKind::Headless,
            None,
            now - Duration::seconds(121),
            now,
            &r
        ));
        assert!(is_active(
            SessionKind::BackgroundJob,
            Some("working"),
            now - Duration::minutes(10),
            now,
            &r
        ));
        assert!(!is_active(
            SessionKind::BackgroundJob,
            Some("working"),
            now - Duration::minutes(11),
            now,
            &r
        ));
        assert!(!is_active(
            SessionKind::BackgroundJob,
            Some("done"),
            now,
            now,
            &r
        ));
        assert!(subagent_active(now - Duration::seconds(120), now, &r));
        assert!(!subagent_active(now - Duration::seconds(121), now, &r));
    }
}
