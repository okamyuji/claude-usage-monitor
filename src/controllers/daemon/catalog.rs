//! 公式ページからのモデル情報の更新。取得に失敗しても既存の値を残し、コスト表示を0にしない。
use crate::models::domain::pricing::seed_models;
use crate::models::domain::records::{FetchLogEntry, FetchResult};
use crate::models::ports::{Clock, FetchLogRepo, ModelCatalogSource, ModelRepo, RepoError};
use std::sync::Arc;

/// モデル情報の更新役。
pub struct CatalogUpdater {
    source: Arc<dyn ModelCatalogSource>,
    models: Arc<dyn ModelRepo>,
    log: Arc<dyn FetchLogRepo>,
    clock: Arc<dyn Clock>,
}

impl CatalogUpdater {
    /// 依存を受け取って作る。
    pub fn new(
        source: Arc<dyn ModelCatalogSource>,
        models: Arc<dyn ModelRepo>,
        log: Arc<dyn FetchLogRepo>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            source,
            models,
            log,
            clock,
        }
    }

    /// 初期値を入れてから公式の値で更新する。更新した行数を返す。
    pub fn run_once(&self) -> Result<usize, RepoError> {
        self.models.seed_if_empty(&seed_models())?;
        let now = self.clock.now();
        let (n, result, message) = match self.source.fetch() {
            Ok(models) => (
                self.models.upsert_official(&models, now)?,
                FetchResult::Ok,
                String::new(),
            ),
            Err(e) => (0, FetchResult::Failed, e.to_string()),
        };
        self.log.log(&FetchLogEntry {
            target: "catalog".into(),
            at: now,
            result,
            http_status: None,
            message,
        })?;
        Ok(n)
    }
}

impl crate::models::ports::CatalogRefresh for CatalogUpdater {
    fn refresh(&self) -> Result<usize, RepoError> {
        self.run_once()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::pricing::ModelInfo;
    use crate::models::domain::pricing::{ModelSource, seed_models};
    use crate::models::ports::{CatalogError, FetchLogRepo, ModelRepo};
    use crate::test_support::{FixedClock, temp_store};
    use chrono::Utc;

    struct Source(Result<Vec<ModelInfo>, CatalogError>);
    impl ModelCatalogSource for Source {
        fn fetch(&self) -> Result<Vec<ModelInfo>, CatalogError> {
            self.0.clone()
        }
    }

    fn updater(
        src: Result<Vec<ModelInfo>, CatalogError>,
    ) -> (
        tempfile::TempDir,
        Arc<crate::models::repositories::db::SqliteStore>,
        CatalogUpdater,
    ) {
        let (d, s) = temp_store();
        let s = Arc::new(s);
        let u = CatalogUpdater::new(
            Arc::new(Source(src)),
            s.clone(),
            s.clone(),
            Arc::new(FixedClock::at(Utc::now())),
        );
        (d, s, u)
    }

    #[test]
    fn seeds_then_applies_official_update() {
        let mut latest = seed_models();
        latest[0].input = 11.0;
        let (_d, s, u) = updater(Ok(latest));
        assert_eq!(u.run_once().unwrap(), seed_models().len());
        assert_eq!(
            s.all()
                .unwrap()
                .iter()
                .find(|m| m.model_prefix == "claude-fable-5-1")
                .unwrap()
                .input,
            11.0
        );
        assert_eq!(
            s.recent("catalog", 1).unwrap()[0].result,
            crate::models::domain::records::FetchResult::Ok
        );
    }

    #[test]
    fn unparseable_catalog_keeps_existing_models() {
        let (_d, s, u) = updater(Err(CatalogError::Parse("表がありません".into())));
        assert_eq!(u.run_once().unwrap(), 0);
        assert_eq!(s.all().unwrap().len(), seed_models().len());
        assert!(
            s.all()
                .unwrap()
                .iter()
                .all(|m| m.source == ModelSource::Official)
        );
        let log = s.recent("catalog", 1).unwrap();
        assert!(log[0].message.contains("表がありません"));
    }

    #[test]
    fn catalog_refresh_runs_once() {
        let mut latest = seed_models();
        latest[0].input = 12.0;
        let (_d, s, u) = updater(Ok(latest));
        let r: &dyn crate::models::ports::CatalogRefresh = &u;
        assert_eq!(r.refresh().unwrap(), seed_models().len());
        assert_eq!(
            s.all()
                .unwrap()
                .iter()
                .find(|m| m.model_prefix == "claude-fable-5-1")
                .unwrap()
                .input,
            12.0
        );
    }
}
