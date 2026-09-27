//! 使用量API（`GET /api/oauth/usage`）のHTTPクライアント。
//!
//! ureqの同期APIを使うのは、非同期ランタイムを常駐させずにメモリを小さく保つため。
use crate::models::domain::usage::{UsageSnapshot, parse_usage};
use crate::models::ports::{UsageApi, UsageApiError};
use std::time::Duration;
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig};

/// 本番の接続先。テストではWireMockのURLに差し替える。
pub const DEFAULT_USAGE_BASE: &str = "https://api.anthropic.com";

/// 接続先の上書きを許してよいか。上書き先にはOAuthトークンが送られるため、公式のホストと、テスト用のローカルのポートだけを許す。
pub fn is_allowed_usage_base(base: &str) -> bool {
    let b = base.trim_end_matches('/');
    b == DEFAULT_USAGE_BASE
        || ["http://127.0.0.1:", "http://localhost:"].iter().any(|p| {
            b.strip_prefix(p)
                .is_some_and(|port| !port.is_empty() && port.bytes().all(|c| c.is_ascii_digit()))
        })
}

/// 使用量APIのクライアント。`Agent`を使い回すのは、周期取得のたびに接続を作り直さないため。
pub struct HttpUsageApi {
    agent: Agent,
    url: String,
}

impl HttpUsageApi {
    /// 接続先と全体のタイムアウトを指定して作る。
    /// 証明書はOSの証明書ストアで検証する。同梱のwebpki-rootsでは、社内プロキシが差し込むCAを信頼できず、TLSが`invalid data`で失敗するため。
    pub fn new(base_url: &str, timeout: Duration) -> Self {
        let agent: Agent = Agent::config_builder()
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            agent,
            url: format!("{}/api/oauth/usage", base_url.trim_end_matches('/')),
        }
    }
}

impl UsageApi for HttpUsageApi {
    fn fetch(&self, access_token: &str) -> Result<UsageSnapshot, UsageApiError> {
        let mut resp = self
            .agent
            .get(&self.url)
            .header("Authorization", &format!("Bearer {access_token}"))
            .header("anthropic-beta", "oauth-2025-04-20")
            .call()
            // ureqのエラー表示にはURLしか含まれずトークンは出ないが、念のため種別名だけを残す。
            .map_err(|e| UsageApiError::Transport(error_kind(&e)))?;
        match resp.status().as_u16() {
            200 => {
                let body = resp
                    .body_mut()
                    .read_to_string()
                    .map_err(|e| UsageApiError::Transport(error_kind(&e)))?;
                parse_usage(&body).map_err(|e| UsageApiError::Parse(e.to_string()))
            }
            401 | 403 => Err(UsageApiError::Unauthorized),
            429 => Err(UsageApiError::RateLimited),
            s => Err(UsageApiError::Http(s)),
        }
    }
}

fn error_kind(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "タイムアウト".to_string(),
        ureq::Error::Io(io) => format!("通信エラー（{}）", io.kind()),
        ureq::Error::ConnectionFailed => "接続できません".to_string(),
        ureq::Error::HostNotFound => "ホストが見つかりません".to_string(),
        _ => "通信エラー".to_string(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_official_or_local_bases_may_receive_the_token() {
        for ok in [
            "https://api.anthropic.com",
            "https://api.anthropic.com/",
            "http://127.0.0.1:8080",
            "http://localhost:32768/",
        ] {
            assert!(is_allowed_usage_base(ok), "{ok}");
        }
        for ng in [
            "http://api.anthropic.com",
            "https://api.anthropic.com.evil.example",
            "https://evil.example",
            "http://127.0.0.1",
            "http://127.0.0.1:",
            "http://127.0.0.1:80@evil.example",
            "http://localhost:80/x",
            "",
        ] {
            assert!(!is_allowed_usage_base(ng), "{ng}");
        }
    }
    use crate::test_support::WireMock;
    use serde_json::json;

    fn stub_status(wm: &WireMock, status: u16, body: &str) {
        wm.stub(json!({
            "request": {"method": "GET", "url": "/api/oauth/usage"},
            "response": {"status": status, "body": body}
        }));
    }

    #[test]
    fn maps_statuses_and_sends_headers() {
        let wm = WireMock::start();
        let api = HttpUsageApi::new(&wm.base_url, Duration::from_secs(5));

        stub_status(
            &wm,
            200,
            include_str!("../../../tests/fixtures/usage_ok.json"),
        );
        assert_eq!(api.fetch("tok").unwrap().limits.len(), 3);
        let reqs = wm.requests();
        let headers = &reqs["requests"][0]["request"]["headers"];
        assert_eq!(headers["Authorization"], "Bearer tok");
        assert_eq!(headers["anthropic-beta"], "oauth-2025-04-20");

        stub_status(&wm, 401, "{}");
        assert_eq!(api.fetch("tok").unwrap_err(), UsageApiError::Unauthorized);
        stub_status(&wm, 403, "{}");
        assert_eq!(api.fetch("tok").unwrap_err(), UsageApiError::Unauthorized);
        stub_status(&wm, 429, "{}");
        assert_eq!(api.fetch("tok").unwrap_err(), UsageApiError::RateLimited);
        stub_status(&wm, 500, "{}");
        assert_eq!(api.fetch("tok").unwrap_err(), UsageApiError::Http(500));
        stub_status(&wm, 200, "not json");
        assert!(matches!(
            api.fetch("tok").unwrap_err(),
            UsageApiError::Parse(_)
        ));
    }

    #[test]
    fn error_kind_names_each_transport_failure() {
        assert_eq!(
            error_kind(&ureq::Error::Timeout(ureq::Timeout::Global)),
            "タイムアウト"
        );
        let io = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
        assert_eq!(
            error_kind(&ureq::Error::Io(io)),
            "通信エラー（connection reset）"
        );
        assert_eq!(error_kind(&ureq::Error::ConnectionFailed), "接続できません");
        assert_eq!(
            error_kind(&ureq::Error::HostNotFound),
            "ホストが見つかりません"
        );
        assert_eq!(error_kind(&ureq::Error::BadUri("x".into())), "通信エラー");
    }

    #[test]
    fn slow_response_times_out() {
        let wm = WireMock::start();
        wm.stub(json!({
            "request": {"method": "GET", "url": "/api/oauth/usage"},
            "response": {"status": 200, "body": "{}", "fixedDelayMilliseconds": 3000}
        }));
        let api = HttpUsageApi::new(&wm.base_url, Duration::from_millis(500));
        assert!(matches!(
            api.fetch("tok").unwrap_err(),
            UsageApiError::Transport(_)
        ));
    }

    #[test]
    fn unreachable_host_is_transport_error_without_token() {
        let api = HttpUsageApi::new("http://127.0.0.1:1", Duration::from_millis(500));
        let err = api.fetch("secret-token").unwrap_err();
        assert!(matches!(err, UsageApiError::Transport(_)));
        assert!(!err.to_string().contains("secret-token"));
    }
}
