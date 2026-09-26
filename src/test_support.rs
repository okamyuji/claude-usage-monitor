//! テスト専用の補助。実SQLiteの一時DBを作る。DBをモックしない方針をテスト全体で守るため。
use crate::models::repositories::db::SqliteStore;

/// 一時ディレクトリに実DBを作る。`TempDir`を返すのは、呼び出し側が持っている間だけファイルを残すため。
pub(crate) fn temp_store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().expect("一時ディレクトリ");
    let store = SqliteStore::open(&dir.path().join("cps.db")).expect("DBを開く");
    (dir, store)
}

use testcontainers::core::IntoContainerPort;
use testcontainers::runners::SyncRunner;
use testcontainers::{Container, GenericImage};

/// WireMockコンテナ。本番と同じHTTP経路で外部APIのふるまいを再現するために使う。
pub(crate) struct WireMock {
    _container: Container<GenericImage>,
    pub(crate) base_url: String,
}

impl WireMock {
    /// コンテナを起動し、管理APIが応答するまで待つ。
    pub(crate) fn start() -> Self {
        let container = GenericImage::new("wiremock/wiremock", "3.13.2-alpine")
            .with_exposed_port(8080.tcp())
            .start()
            .expect("WireMockの起動（DOCKER_HOSTとTESTCONTAINERS_DOCKER_SOCKET_OVERRIDEを確認）");
        let port = container.get_host_port_ipv4(8080.tcp()).expect("ポート");
        let base_url = format!("http://127.0.0.1:{port}");
        let health = format!("{base_url}/__admin/health");
        let ready = (0..60).any(|_| {
            let ok = ureq::get(&health).call().is_ok();
            if !ok {
                std::thread::park_timeout(std::time::Duration::from_millis(500));
            }
            ok
        });
        assert!(ready, "WireMockが30秒以内に起動しませんでした");
        Self {
            _container: container,
            base_url,
        }
    }

    /// スタブを1件登録する。
    pub(crate) fn stub(&self, mapping: serde_json::Value) {
        ureq::post(&format!("{}/__admin/mappings", self.base_url))
            .send_json(mapping)
            .expect("スタブ登録");
    }

    /// 受け取ったリクエストの一覧。ヘッダーの検証に使う。
    pub(crate) fn requests(&self) -> serde_json::Value {
        ureq::get(&format!("{}/__admin/requests", self.base_url))
            .call()
            .expect("リクエスト一覧")
            .body_mut()
            .read_json()
            .expect("JSON")
    }
}

use crate::models::domain::profile::Profile;
use crate::models::domain::usage::UsageSnapshot;
use crate::models::ports::{
    Clock, Credential, CredentialError, CredentialStore, UsageApi, UsageApiError,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// 手で進める時計。
pub(crate) struct FixedClock(pub(crate) Mutex<DateTime<Utc>>);

impl FixedClock {
    pub(crate) fn at(t: DateTime<Utc>) -> Self {
        Self(Mutex::new(t))
    }
    pub(crate) fn advance(&self, d: chrono::Duration) {
        let mut g = self.0.lock().unwrap();
        *g += d;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

/// トークンごとに応答を決めるAPI。呼び出し回数も数える。
pub(crate) struct FakeApi {
    pub(crate) responses: HashMap<String, Result<UsageSnapshot, UsageApiError>>,
    pub(crate) calls: Mutex<Vec<String>>,
}

impl UsageApi for FakeApi {
    fn fetch(&self, token: &str) -> Result<UsageSnapshot, UsageApiError> {
        self.calls.lock().unwrap().push(token.to_string());
        self.responses
            .get(token)
            .cloned()
            .unwrap_or(Err(UsageApiError::Http(404)))
    }
}

/// プロファイル名ごとに認証情報を返す保存先。
pub(crate) struct FakeCreds(pub(crate) HashMap<String, Result<Credential, CredentialError>>);

impl CredentialStore for FakeCreds {
    fn load(&self, profile: &Profile, _dir: &Path) -> Result<Credential, CredentialError> {
        self.0
            .get(&profile.name)
            .cloned()
            .unwrap_or(Err(CredentialError::Missing))
    }
}

use crate::models::domain::pricing::TokenUsage;
use crate::models::domain::records::{SessionUpsert, TurnRecord};
use crate::models::domain::transcript::SessionKind;
use crate::models::ports::{IngestRepo, ProfileRepo};

/// セッション行を1つ作る。既定プロファイルに属させる。
pub(crate) fn seed_session(
    s: &SqliteStore,
    id: &str,
    kind: SessionKind,
    status: Option<&str>,
    last: DateTime<Utc>,
) {
    let p = s.ensure_default().unwrap();
    s.upsert_session(&SessionUpsert {
        session_id: id.into(),
        profile_id: p.id,
        kind,
        entrypoint: None,
        cwd: Some(format!("/work/{id}")),
        git_branch: Some("main".into()),
        name: Some(format!("name-{id}")),
        first_prompt: Some(format!("prompt-{id}")),
        started_at: last - chrono::Duration::minutes(5),
        last_activity_at: last,
        status: status.map(str::to_string),
    })
    .unwrap();
}

/// ターン行を1つ作る。
#[allow(clippy::too_many_arguments)]
pub(crate) fn seed_turn(
    s: &SqliteStore,
    session: &str,
    agent: &str,
    message: &str,
    ts: DateTime<Utc>,
    model: Option<&str>,
    kind: &str,
    usage: TokenUsage,
) {
    s.upsert_turn(&TurnRecord {
        session_id: session.into(),
        agent_id: agent.into(),
        message_id: message.into(),
        ts,
        model: model.map(str::to_string),
        kind: kind.into(),
        summary: format!("summary-{message}"),
        usage,
    })
    .unwrap();
}

/// 入力と出力だけを持つToken数。
pub(crate) fn tokens(input: u64, output: u64) -> TokenUsage {
    TokenUsage {
        input,
        output,
        ..TokenUsage::default()
    }
}
