//! E2E用のWireMock起動。ライブラリの`test_support`は`cfg(test)`で外から使えないため、E2E側に同じ役割を置く。
pub mod gui;
use testcontainers::core::IntoContainerPort;
use testcontainers::runners::SyncRunner;
use testcontainers::{Container, GenericImage};

pub struct WireMock {
    _container: Container<GenericImage>,
    pub base_url: String,
}

impl WireMock {
    pub fn start() -> Self {
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
        }
    }

    pub fn stub(&self, mapping: serde_json::Value) {
        ureq::post(&format!("{}/__admin/mappings", self.base_url))
            .send_json(mapping)
            .unwrap();
    }
}
