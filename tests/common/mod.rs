//! E2E用のWireMock起動。ライブラリの`test_support`は`cfg(test)`で外から使えないため、E2E側に同じ役割を置く。
#![allow(dead_code)]
pub mod gui;
use testcontainers::core::IntoContainerPort;
use testcontainers::runners::SyncRunner;
use testcontainers::{Container, GenericImage};

/// 1つのテストプロセスで同時に動かすWireMockのコンテナの上限。
/// テストは並列に動くため、上限がないと数十個のコンテナ（中身はJava）が一斉に起動し、
/// Colimaの仮想マシンのCPUを食い合って起動待ちが時間切れになる。
const MAX_CONTAINERS: usize = 3;
static SLOTS: (std::sync::Mutex<usize>, std::sync::Condvar) =
    (std::sync::Mutex::new(0), std::sync::Condvar::new());

/// コンテナ1つ分の枠。持っている間だけ枠を使い、捨てると次の待ち手に渡す。
struct Slot;

impl Slot {
    fn acquire() -> Self {
        let (count, freed) = &SLOTS;
        let mut n = count.lock().unwrap_or_else(|e| e.into_inner());
        while *n >= MAX_CONTAINERS {
            n = freed.wait(n).unwrap_or_else(|e| e.into_inner());
        }
        *n += 1;
        Slot
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let (count, freed) = &SLOTS;
        *count.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        freed.notify_one();
    }
}

pub struct WireMock {
    _container: Container<GenericImage>,
    pub base_url: String,
    // コンテナを止めてから枠を返すため、`_container`より後に置く（フィールドは宣言順に破棄される）。
    _slot: Slot,
}

impl WireMock {
    pub fn start() -> Self {
        let slot = Slot::acquire();
        let container = GenericImage::new("wiremock/wiremock", "3.13.2-alpine")
            .with_exposed_port(8080.tcp())
            .start()
            .expect("WireMockの起動");
        let port = container.get_host_port_ipv4(8080.tcp()).unwrap();
        let base_url = format!("http://127.0.0.1:{port}");
        // 品質ゲートのmutation testingと並行して動くと起動が遅れるため、最大120秒待つ。
        let ready = (0..240).any(|_| {
            let ok = ureq::get(&format!("{base_url}/__admin/health"))
                .call()
                .is_ok();
            if !ok {
                std::thread::park_timeout(std::time::Duration::from_millis(500));
            }
            ok
        });
        assert!(ready);
        Self {
            _container: container,
            base_url,
            _slot: slot,
        }
    }

    pub fn stub(&self, mapping: serde_json::Value) {
        ureq::post(&format!("{}/__admin/mappings", self.base_url))
            .send_json(mapping)
            .unwrap();
    }
}

use std::path::Path;

/// 一時ディレクトリをデータとホームにして`cumon`を起動する。
pub fn cumon(data: &Path, home: &Path, path_prepend: Option<&Path>) -> assert_cmd::Command {
    let mut c = assert_cmd::Command::cargo_bin("cumon").unwrap();
    c.env("CUMON_DATA_DIR", data)
        .env("HOME", home)
        .env_remove("CLAUDE_CONFIG_DIR");
    if let Some(p) = path_prepend {
        c.env(
            "PATH",
            format!("{}:{}", p.display(), std::env::var("PATH").unwrap()),
        );
    }
    c
}

/// 受け取った`CLAUDE_CONFIG_DIR`と引数を表示するだけの偽の`claude`。
#[cfg(unix)]
pub fn fake_claude(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("claude");
    std::fs::write(
        &p,
        "#!/bin/sh\necho \"CONFIG=${CLAUDE_CONFIG_DIR:-none} ARGS=$*\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// 使用量APIに`usage_ok.json`を返すスタブを登録する。
pub fn stub_usage_ok(wm: &WireMock) {
    wm.stub(serde_json::json!({"request": {"method": "GET", "url": "/api/oauth/usage"},
                               "response": {"status": 200, "body": include_str!("../fixtures/usage_ok.json")}}));
}

/// 期限の先の認証情報を`~/.claude/.credentials.json`に置く。
pub fn write_credentials(home: &Path) {
    let cfg = home.join(".claude");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"e2e","expiresAt":4102444800000}}"#,
    )
    .unwrap();
}
