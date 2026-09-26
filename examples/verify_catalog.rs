//! Anthropic公式の料金ページとモデル一覧を実際に取得し、読み取ったモデル情報を表示する。
//! ページ構成が変わったときに解析が壊れていないかを実サイトで確かめるための実動作確認用。
#![forbid(unsafe_code)]
use claude_profile_switcher::models::gateways::model_catalog::{
    DEFAULT_CATALOG_BASE, HttpModelCatalog,
};
use claude_profile_switcher::models::ports::ModelCatalogSource;
use std::time::Duration;

fn main() {
    let models = HttpModelCatalog::new(DEFAULT_CATALOG_BASE, Duration::from_secs(30))
        .fetch()
        .expect("公式ページ");
    println!("models={}", models.len());
    for m in &models {
        println!(
            "{}\t{}\tin={} out={} read={} w5m={} w1h={} ctx={:?}",
            m.model_prefix,
            m.display_name,
            m.input,
            m.output,
            m.cache_read,
            m.cache_write_5m,
            m.cache_write_1h,
            m.context_window
        );
    }
}
