//! プロファイルタブ。一覧、追加、使用中の切替、削除、トークン期限。
//!
//! 追加や切替は`cumon profile`と同じ関数を呼ぶ。GUIとCLIで結果がずれないようにするため。
use crate::controllers::cli::profile::{ProfileCommand, execute};
use crate::controllers::gui::app::GuiDeps;
use crate::models::domain::display::fmt_clock;
use crate::models::domain::profile::Profile;
use crate::models::ports::{CredentialError, RepoError};

/// ログインの手順（spec 9章）。
pub const LOGIN_HELP: &str = "追加したプロファイルは、「使用中にする」を押してからターミナルで cumon run を実行し、/login でログインすると使えます";

/// プロファイル1行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRow {
    /// 名前。
    pub name: String,
    /// 設定ディレクトリ。
    pub dir: String,
    /// 使用中か。
    pub active: bool,
    /// トークンの状態。トークンそのものは持たない。
    pub token: String,
    /// 契約種別。
    pub plan: String,
}

/// プロファイルタブのViewModel。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilesVm {
    /// 一覧。
    pub rows: Vec<ProfileRow>,
    /// 直前の操作の結果。
    pub message: Option<String>,
    /// 削除の確認中のプロファイル。削除すると履歴も消えるため、1回押しただけでは消さない。
    pub pending_remove: Option<String>,
    /// ログインの手順。
    pub login_help: &'static str,
}

/// プロファイルの操作。ダッシュボードのカードの「使用中にする」も`Use`を送る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfilesAction {
    /// 入力欄の名前とディレクトリで追加する。
    Add,
    /// 使用中にする。
    Use(String),
    /// 削除の確認を出す。
    Remove(String),
    /// 削除する。
    ConfirmRemove,
    /// 削除をやめる。
    CancelRemove,
}

fn token_text(deps: &GuiDeps, p: &Profile) -> (String, String) {
    let now = deps.clock.now();
    match deps.creds.load(p, &p.resolved_config_dir(&deps.home)) {
        Ok(c) => {
            let token = match c.expires_at {
                Some(t) if t <= now => format!("期限切れ（{}）", fmt_clock(t, now, deps.tz)),
                Some(t) => format!("期限 {}", fmt_clock(t, now, deps.tz)),
                None => "期限不明".into(),
            };
            (token, c.subscription_type.unwrap_or_else(|| "—".into()))
        }
        Err(CredentialError::Missing) => ("認証情報なし（ログインが必要です）".into(), "—".into()),
        Err(e) => (e.to_string(), "—".into()),
    }
}

/// プロファイルタブを作る。
pub fn build(
    deps: &GuiDeps,
    message: Option<String>,
    pending_remove: Option<String>,
) -> Result<ProfilesVm, RepoError> {
    let rows = deps
        .profiles
        .list()?
        .iter()
        .map(|p| {
            let (token, plan) = token_text(deps, p);
            ProfileRow {
                name: p.name.clone(),
                dir: p.resolved_config_dir(&deps.home).display().to_string(),
                active: p.is_active,
                token,
                plan,
            }
        })
        .collect();
    Ok(ProfilesVm {
        rows,
        message,
        pending_remove,
        login_help: LOGIN_HELP,
    })
}

/// `cumon profile`と同じ処理を実行し、表示する文を返す。失敗もその文で返す。
pub fn run_command(deps: &GuiDeps, cmd: ProfileCommand) -> String {
    let mut out = Vec::new();
    match execute(deps.profiles.as_ref(), &deps.home, cmd, &mut out) {
        Ok(()) => String::from_utf8_lossy(&out).trim().to_string(),
        Err(e) => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ports::{Credential, CredentialError, ProfileRepo};
    use crate::test_support::{FakeCreds, FakeDaemon, FixedClock, gui_deps, temp_store};
    use chrono::{Duration, TimeZone, Utc};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    fn make_deps(
        creds: HashMap<String, Result<Credential, CredentialError>>,
    ) -> (tempfile::TempDir, tempfile::TempDir, GuiDeps) {
        let (d, s) = temp_store();
        s.ensure_default().unwrap();
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            Arc::new(s),
            Arc::new(FixedClock::at(now())),
            home.path(),
            Arc::new(FakeCreds(creds)),
            Arc::new(FakeDaemon::default()),
        );
        (d, home, deps)
    }

    fn cred(expires: Option<chrono::DateTime<Utc>>) -> Result<Credential, CredentialError> {
        Ok(Credential {
            access_token: "secret".into(),
            expires_at: expires,
            subscription_type: Some("max".into()),
        })
    }

    #[test]
    fn rows_show_token_state_and_plan() {
        let (_d, _h, deps) = make_deps(HashMap::from([(
            "default".to_string(),
            cred(Some(now() + Duration::hours(3))),
        )]));
        run_command(
            &deps,
            ProfileCommand::Add {
                name: "sub".into(),
                config_dir: None,
            },
        );
        let vm = build(&deps, None, None).unwrap();
        assert_eq!(vm.rows.len(), 2);
        let d = vm.rows.iter().find(|r| r.name == "default").unwrap();
        assert_eq!(
            (d.active, d.token.as_str(), d.plan.as_str()),
            (true, "期限 15:00", "max")
        );
        let s = vm.rows.iter().find(|r| r.name == "sub").unwrap();
        assert_eq!(
            (s.token.as_str(), s.plan.as_str()),
            ("認証情報なし（ログインが必要です）", "—")
        );
        assert!(s.dir.ends_with(".claude-sub"));
        assert!(!format!("{vm:?}").contains("secret"));
    }

    #[test]
    fn expired_and_unknown_expiry() {
        let (_d, _h, deps) = make_deps(HashMap::from([(
            "default".to_string(),
            cred(Some(now() - Duration::minutes(1))),
        )]));
        assert_eq!(
            build(&deps, None, None).unwrap().rows[0].token,
            "期限切れ（11:59）"
        );
        let (_d2, _h2, deps2) = make_deps(HashMap::from([("default".to_string(), cred(None))]));
        assert_eq!(build(&deps2, None, None).unwrap().rows[0].token, "期限不明");
        let (_d3, _h3, deps3) = make_deps(HashMap::from([(
            "default".to_string(),
            Err(CredentialError::Io("権限がありません".into())),
        )]));
        assert!(
            build(&deps3, None, None).unwrap().rows[0]
                .token
                .contains("権限がありません")
        );
    }

    #[test]
    fn commands_return_cli_messages() {
        let (_d, _h, deps) = make_deps(HashMap::new());
        assert!(
            run_command(
                &deps,
                ProfileCommand::Add {
                    name: "work".into(),
                    config_dir: None
                }
            )
            .contains("work")
        );
        assert!(
            run_command(
                &deps,
                ProfileCommand::Use {
                    name: "work".into()
                }
            )
            .contains("work に切り替えました")
        );
        assert!(
            run_command(
                &deps,
                ProfileCommand::Add {
                    name: "bad name".into(),
                    config_dir: None
                }
            )
            .contains("プロファイル名")
        );
        assert!(
            run_command(
                &deps,
                ProfileCommand::Remove {
                    name: "work".into()
                }
            )
            .contains("使用中のプロファイルは削除できません")
        );
    }
}
