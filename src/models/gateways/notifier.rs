//! OS通知の送信。
//!
//! E2Eで通知の回数を数えるため、`CPS_NOTIFY_LOG`を指定したときはOSに出さずファイルへ追記する。

use crate::models::ports::Notifier;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

/// notify-rustでOSの通知センターに出す。
pub struct SystemNotifier;

impl Notifier for SystemNotifier {
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        notify_rust::Notification::new()
            .appname("Claude Profile Switcher")
            .summary(title)
            .body(body)
            .show()
            .map(|_| ())
            .map_err(|e| format!("通知を出せません: {e}"))
    }
}

/// ファイルへ1行ずつ追記する。E2Eで通知の回数を数えるため。
pub struct FileNotifier {
    path: PathBuf,
}

impl FileNotifier {
    /// 追記先を指定する。
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Notifier for FileNotifier {
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        let line = format!(
            "{}\t{}\n",
            title.replace('\n', " "),
            body.replace('\n', " ")
        );
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(line.as_bytes()))
            .map_err(|e| format!("通知を書き込めません: {e}"))
    }
}

/// 環境変数の値から選ぶ。
pub fn notifier_from(log: Option<OsString>) -> Arc<dyn Notifier> {
    match log {
        Some(p) => Arc::new(FileNotifier::new(PathBuf::from(p))),
        None => Arc::new(SystemNotifier),
    }
}

/// `CPS_NOTIFY_LOG`があればファイルへ、なければOSへ出す。
pub fn notifier_from_env() -> Arc<dyn Notifier> {
    notifier_from(std::env::var_os("CPS_NOTIFY_LOG"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ports::Notifier;

    #[test]
    fn file_notifier_appends_one_line_per_notification() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("n.log");
        let n = FileNotifier::new(path.clone());
        n.notify("5時間枠 85%", "リセット 14:00").unwrap();
        n.notify("週間枠 90%", "改行を\n含む").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "5時間枠 85%\tリセット 14:00\n週間枠 90%\t改行を 含む\n"
        );
    }

    #[test]
    fn file_notifier_reports_io_error() {
        let n = FileNotifier::new("/nonexistent/dir/n.log".into());
        assert!(
            n.notify("a", "b")
                .unwrap_err()
                .contains("通知を書き込めません")
        );
    }

    #[test]
    fn env_selects_file_notifier() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("n.log");
        let n = notifier_from(Some(path.clone().into_os_string()));
        n.notify("t", "b").unwrap();
        assert!(path.exists());
    }
}
