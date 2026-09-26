//! 公式ページ（Markdown版）の取得。
use crate::models::domain::catalog::{merge_catalog, parse_overview_md, parse_pricing_md};
use crate::models::domain::pricing::ModelInfo;
use crate::models::ports::{CatalogError, ModelCatalogSource};
use std::time::Duration;
use ureq::Agent;

/// 本番の取得元。
pub const DEFAULT_CATALOG_BASE: &str = "https://platform.claude.com";
const PRICING_PATH: &str = "/docs/en/about-claude/pricing.md";
const OVERVIEW_PATH: &str = "/docs/en/models/overview.md";

/// 公式ページの取得元。
pub struct HttpModelCatalog {
    agent: Agent,
    base: String,
}

impl HttpModelCatalog {
    /// 取得元と全体のタイムアウトを指定して作る。
    pub fn new(base_url: &str, timeout: Duration) -> Self {
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(timeout))
            .build()
            .into();
        Self {
            agent,
            base: base_url.trim_end_matches('/').to_string(),
        }
    }

    fn get(&self, path: &str) -> Result<String, CatalogError> {
        self.agent
            .get(&format!("{}{path}", self.base))
            .call()
            .map_err(|e| CatalogError::Fetch(format!("{path}: {e}")))?
            .body_mut()
            .read_to_string()
            .map_err(|e| CatalogError::Fetch(format!("{path}: {e}")))
    }
}

impl ModelCatalogSource for HttpModelCatalog {
    fn fetch(&self) -> Result<Vec<ModelInfo>, CatalogError> {
        let prices = parse_pricing_md(&self.get(PRICING_PATH)?);
        if prices.is_empty() {
            return Err(CatalogError::Parse(
                "料金ページに単価表が見つかりません".into(),
            ));
        }
        let overview = parse_overview_md(&self.get(OVERVIEW_PATH)?);
        Ok(merge_catalog(&prices, &overview))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::WireMock;
    use serde_json::json;

    fn stub(wm: &WireMock, path: &str, status: u16, body: &str) {
        wm.stub(json!({"request": {"method": "GET", "url": path}, "response": {"status": status, "body": body}}));
    }

    #[test]
    fn fetches_and_merges_official_pages() {
        let wm = WireMock::start();
        stub(
            &wm,
            PRICING_PATH,
            200,
            include_str!("../../../tests/fixtures/pricing.md"),
        );
        stub(
            &wm,
            OVERVIEW_PATH,
            200,
            include_str!("../../../tests/fixtures/overview.md"),
        );
        let models = HttpModelCatalog::new(&wm.base_url, Duration::from_secs(5))
            .fetch()
            .unwrap();
        assert!(
            models
                .iter()
                .any(|m| m.model_prefix == "claude-opus-5-5" && m.input == 4.0)
        );
    }

    #[test]
    fn changed_page_structure_is_parse_error() {
        let wm = WireMock::start();
        stub(&wm, PRICING_PATH, 200, "# ページ構成が変わった");
        let err = HttpModelCatalog::new(&wm.base_url, Duration::from_secs(5))
            .fetch()
            .unwrap_err();
        assert!(matches!(err, CatalogError::Parse(_)));
    }

    #[test]
    fn http_error_is_fetch_error() {
        let wm = WireMock::start();
        stub(&wm, PRICING_PATH, 503, "");
        let err = HttpModelCatalog::new(&wm.base_url, Duration::from_secs(5))
            .fetch()
            .unwrap_err();
        assert!(matches!(err, CatalogError::Fetch(_)));
    }
}
