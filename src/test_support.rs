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
