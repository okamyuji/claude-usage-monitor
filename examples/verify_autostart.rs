//! ログイン時の自動起動を実際に登録し、plistを確かめてから解除する。計画3のTask 5の実動作確認用。
#![forbid(unsafe_code)]
use claude_profile_switcher::models::gateways::autostart::SystemAutostart;
use claude_profile_switcher::models::ports::Autostart;

fn main() {
    let exe = std::env::args().nth(1).expect("登録する実行ファイルのパス");
    let a = SystemAutostart::new(std::path::Path::new(&exe)).expect("組み立て");
    println!("登録前: {}", a.is_enabled().expect("状態"));
    a.enable().expect("登録");
    println!("登録後: {}", a.is_enabled().expect("状態"));
    let plist = directories::BaseDirs::new()
        .expect("ホーム")
        .home_dir()
        .join("Library/LaunchAgents/claude-profile-switcher.plist");
    println!("{}", std::fs::read_to_string(&plist).expect("plist"));
    a.disable().expect("解除");
    println!(
        "解除後: {} / plistの有無: {}",
        a.is_enabled().expect("状態"),
        plist.exists()
    );
}
