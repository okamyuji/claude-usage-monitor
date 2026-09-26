//! Claude Codeの認証情報を読む。
//!
//! 保存先はOSと設定ディレクトリで変わる。設定ディレクトリの`.credentials.json`を先に見て、
//! なければmacOSのKeychainを読む。トークンの更新（書き込み）は行わない。Claude Code本体の認証を壊さないため。
use crate::models::domain::profile::Profile;
use crate::models::ports::{Credential, CredentialError, CredentialStore};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Keychainの読み出し。テストで`security`コマンドを呼ばずに済むよう抽象化する。
pub trait KeychainReader: Send + Sync {
    /// サービス名の汎用パスワードを読む。存在しなければ`Ok(None)`。
    fn read(&self, service: &str) -> Result<Option<String>, CredentialError>;
}

/// macOS標準の`security`コマンドで読む。依存クレートを増やさず、OSの許可ダイアログもOS標準の挙動に任せるため。
pub struct SecurityCli {
    program: OsString,
}

impl SecurityCli {
    /// 呼び出すプログラムを指定して作る。テストで利用者のKeychainに触れずに呼び出し経路を確かめるため。
    pub fn with_program(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl Default for SecurityCli {
    fn default() -> Self {
        Self::with_program("security")
    }
}

impl KeychainReader for SecurityCli {
    fn read(&self, service: &str) -> Result<Option<String>, CredentialError> {
        if !cfg!(target_os = "macos") {
            return Ok(None);
        }
        let out = Command::new(&self.program)
            .args(["find-generic-password", "-s", service, "-w"])
            .output()
            .map_err(|e| CredentialError::Io(e.kind().to_string()))?;
        if !out.status.success() {
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        ))
    }
}

/// Keychainのサービス名。`CLAUDE_CONFIG_DIR`を設定したプロファイルは、パスのSHA-256の先頭8桁が付く。
pub fn keychain_service(config_dir: Option<&Path>) -> String {
    match config_dir {
        None => "Claude Code-credentials".to_string(),
        Some(dir) => {
            let h = Sha256::digest(dir.to_string_lossy().as_bytes());
            let hex: String = h.iter().take(4).map(|b| format!("{b:02x}")).collect();
            format!("Claude Code-credentials-{hex}")
        }
    }
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(rename = "claudeAiOauth")]
    oauth: RawOauth,
}

#[derive(Deserialize)]
struct RawOauth {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
}

/// 認証情報のJSONを読む。エラーメッセージに内容を含めないのは、トークンの一部が漏れるのを防ぐため。
pub fn parse_credentials_json(s: &str) -> Result<Credential, CredentialError> {
    let raw: RawFile = serde_json::from_str(s)
        .map_err(|e| CredentialError::Malformed(format!("{}行{}列", e.line(), e.column())))?;
    Ok(Credential {
        access_token: raw.oauth.access_token,
        expires_at: raw
            .oauth
            .expires_at
            .and_then(DateTime::<Utc>::from_timestamp_millis),
        subscription_type: raw.oauth.subscription_type,
    })
}

/// 設定ディレクトリのファイル、次にKeychainの順で読む保存先。
pub struct SystemCredentialStore {
    keychain: Arc<dyn KeychainReader>,
}

impl SystemCredentialStore {
    /// Keychainの読み出し方を注入して作る。
    pub fn new(keychain: Arc<dyn KeychainReader>) -> Self {
        Self { keychain }
    }
}

impl CredentialStore for SystemCredentialStore {
    fn load(&self, profile: &Profile, config_dir: &Path) -> Result<Credential, CredentialError> {
        let file = config_dir.join(".credentials.json");
        match std::fs::read_to_string(&file) {
            Ok(s) => return parse_credentials_json(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(CredentialError::Io(e.kind().to_string())),
        }
        let service = keychain_service(profile.config_dir.as_deref());
        match self.keychain.read(&service)? {
            Some(s) => parse_credentials_json(&s),
            None => Err(CredentialError::Missing),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeKeychain(Mutex<Vec<String>>, Option<String>);
    impl KeychainReader for FakeKeychain {
        fn read(&self, service: &str) -> Result<Option<String>, CredentialError> {
            self.0.lock().unwrap().push(service.to_string());
            Ok(self.1.clone())
        }
    }

    const JSON: &str = r#"{"claudeAiOauth":{"accessToken":"tok","expiresAt":1790000000000,"subscriptionType":"max","refreshToken":"r"}}"#;

    fn profile(dir: Option<&str>) -> Profile {
        Profile {
            id: 1,
            name: "p".into(),
            config_dir: dir.map(Into::into),
            is_active: true,
        }
    }

    #[cfg(target_os = "macos")]
    fn fake_security(dir: &Path) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("security");
        std::fs::write(&p, "#!/bin/sh\n[ \"$3\" = \"svc\" ] && [ \"$4\" = \"-w\" ] && { echo ' secret '; exit 0; }\nexit 44\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn security_cli_reads_trimmed_password_or_none() {
        let d = tempfile::tempdir().unwrap();
        let cli = SecurityCli::with_program(fake_security(d.path()));
        assert_eq!(cli.read("svc").unwrap().as_deref(), Some("secret"));
        assert_eq!(cli.read("other").unwrap(), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn security_cli_missing_program_is_io_error() {
        let cli = SecurityCli::with_program("/nonexistent/security");
        assert!(matches!(cli.read("svc"), Err(CredentialError::Io(_))));
    }

    #[test]
    fn keychain_service_default_and_hashed() {
        assert_eq!(keychain_service(None), "Claude Code-credentials");
    }

    #[test]
    fn keychain_service_hashes_explicit_dir() {
        // 2026-09-26に利用者の環境で、CLAUDE_CONFIG_DIR=/tmp/cps-kc のログインが作ったサービス名。
        assert_eq!(
            keychain_service(Some(Path::new("/tmp/cps-kc"))),
            "Claude Code-credentials-f360b1d6"
        );
    }

    #[test]
    fn parses_credentials_json() {
        let c = parse_credentials_json(JSON).unwrap();
        assert_eq!(c.access_token, "tok");
        assert_eq!(c.subscription_type.as_deref(), Some("max"));
        assert_eq!(c.expires_at.unwrap().timestamp_millis(), 1_790_000_000_000);
    }

    #[test]
    fn malformed_json_is_reported_without_content() {
        let e = parse_credentials_json(r#"{"claudeAiOauth":{}}"#).unwrap_err();
        assert!(matches!(e, CredentialError::Malformed(_)));
        assert!(matches!(
            parse_credentials_json("x"),
            Err(CredentialError::Malformed(_))
        ));
    }

    #[test]
    fn file_in_config_dir_takes_precedence() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".credentials.json"), JSON).unwrap();
        let kc = Arc::new(FakeKeychain(Mutex::new(vec![]), None));
        let store = SystemCredentialStore::new(kc.clone());
        assert_eq!(
            store.load(&profile(None), d.path()).unwrap().access_token,
            "tok"
        );
        assert!(kc.0.lock().unwrap().is_empty());
    }

    #[test]
    fn falls_back_to_keychain_with_profile_service() {
        let d = tempfile::tempdir().unwrap();
        let kc = Arc::new(FakeKeychain(Mutex::new(vec![]), Some(JSON.into())));
        let store = SystemCredentialStore::new(kc.clone());
        store.load(&profile(Some("/tmp/cps-kc")), d.path()).unwrap();
        assert_eq!(
            kc.0.lock().unwrap()[0],
            keychain_service(Some(Path::new("/tmp/cps-kc")))
        );
    }

    #[test]
    fn unreadable_credentials_file_is_io_error() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join(".credentials.json")).unwrap();
        let store = SystemCredentialStore::new(Arc::new(FakeKeychain(Mutex::new(vec![]), None)));
        assert!(matches!(
            store.load(&profile(None), d.path()),
            Err(CredentialError::Io(_))
        ));
    }

    #[test]
    fn missing_everywhere_is_missing() {
        let d = tempfile::tempdir().unwrap();
        let store = SystemCredentialStore::new(Arc::new(FakeKeychain(Mutex::new(vec![]), None)));
        assert_eq!(
            store.load(&profile(None), d.path()).unwrap_err(),
            CredentialError::Missing
        );
    }
}
