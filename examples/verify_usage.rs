//! 既定プロファイル（またはCLAUDE_CONFIG_DIR）の認証情報で実際の使用量APIを呼び、枠ごとの使用率を表示する。
//! トークンは表示しない。Task 3、Task 11、Task 12の実動作確認用。
#![forbid(unsafe_code)]
use claude_profile_switcher::models::domain::profile::Profile;
use claude_profile_switcher::models::gateways::credentials::{SecurityCli, SystemCredentialStore};
use claude_profile_switcher::models::gateways::usage_api::{DEFAULT_USAGE_BASE, HttpUsageApi};
use claude_profile_switcher::models::ports::{CredentialStore, UsageApi};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let home = PathBuf::from(std::env::var("HOME").expect("HOME"));
    let profile = Profile {
        id: 0,
        name: "default".into(),
        config_dir: std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from),
        is_active: true,
    };
    let cred = SystemCredentialStore::new(Arc::new(SecurityCli::default()))
        .load(&profile, &profile.resolved_config_dir(&home))
        .expect("認証情報");
    println!(
        "subscription={:?} expires_at={:?}",
        cred.subscription_type, cred.expires_at
    );
    let snap = HttpUsageApi::new(DEFAULT_USAGE_BASE, Duration::from_secs(15))
        .fetch(&cred.access_token)
        .expect("使用量API");
    for l in &snap.limits {
        println!(
            "{}\t{:?}\t{}%\t{}\t{:?}",
            l.kind, l.scope_label, l.percent, l.severity, l.resets_at
        );
    }
    for b in &snap.breakdown {
        println!("breakdown\t{}\t{}%", b.display_name, b.percent);
    }
    if let Some(s) = &snap.spend {
        println!(
            "spend\t{}/{:?}\t10^-{}\t{}",
            s.used_minor, s.limit_minor, s.exponent, s.currency
        );
    }
}
