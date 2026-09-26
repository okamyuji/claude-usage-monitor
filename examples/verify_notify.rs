//! 実際のOS通知を1件出す。Task 1とTask 2の実動作確認用。
#![forbid(unsafe_code)]
use claude_profile_switcher::models::gateways::notifier::SystemNotifier;
use claude_profile_switcher::models::ports::Notifier;

fn main() {
    SystemNotifier
        .notify(
            "defaultの5時間枠が85%です",
            "リセットは14:00です（動作確認の通知）",
        )
        .expect("通知");
    println!("通知を出しました");
}
