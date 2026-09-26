//! モデル情報の永続化。
use crate::models::domain::pricing::{ModelInfo, ModelSource};
use crate::models::ports::{ModelRepo, RepoError};
use crate::models::repositories::db::{SqliteStore, ts};
use chrono::{DateTime, Utc};
use rusqlite::params;

fn insert(
    tx: &rusqlite::Transaction<'_>,
    m: &ModelInfo,
    at: Option<String>,
    on_conflict: &str,
) -> rusqlite::Result<usize> {
    tx.execute(
        &format!(
            "INSERT INTO models(model_prefix, display_name, input, output, cache_read, cache_write_5m, cache_write_1h, context_window, source, fetched_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) {on_conflict}"
        ),
        params![
            m.model_prefix, m.display_name, m.input, m.output, m.cache_read, m.cache_write_5m, m.cache_write_1h,
            m.context_window.map(|v| v as i64), m.source.as_str(), at
        ],
    )
}

impl ModelRepo for SqliteStore {
    fn all(&self) -> Result<Vec<ModelInfo>, RepoError> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT model_prefix, display_name, input, output, cache_read, cache_write_5m, cache_write_1h, context_window, source FROM models ORDER BY model_prefix",
            )?;
            st.query_map([], |r| {
                Ok(ModelInfo {
                    model_prefix: r.get(0)?,
                    display_name: r.get(1)?,
                    input: r.get(2)?,
                    output: r.get(3)?,
                    cache_read: r.get(4)?,
                    cache_write_5m: r.get(5)?,
                    cache_write_1h: r.get(6)?,
                    context_window: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
                    source: ModelSource::parse(&r.get::<_, String>(8)?).unwrap_or(ModelSource::Official),
                })
            })?
            .collect()
        })
    }

    fn seed_if_empty(&self, models: &[ModelInfo]) -> Result<(), RepoError> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let count: i64 = tx.query_row("SELECT COUNT(*) FROM models", [], |r| r.get(0))?;
            if count == 0 {
                for m in models {
                    insert(&tx, m, None, "")?;
                }
            }
            tx.commit()
        })
    }

    fn upsert_official(&self, models: &[ModelInfo], at: DateTime<Utc>) -> Result<usize, RepoError> {
        let at = ts(at);
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let mut n = 0;
            for m in models {
                n += insert(
                    &tx,
                    m,
                    Some(at.clone()),
                    "ON CONFLICT(model_prefix) DO UPDATE SET display_name = excluded.display_name, input = excluded.input,
                     output = excluded.output, cache_read = excluded.cache_read, cache_write_5m = excluded.cache_write_5m,
                     cache_write_1h = excluded.cache_write_1h, context_window = excluded.context_window, fetched_at = excluded.fetched_at
                     WHERE models.source = 'official'",
                )?;
            }
            tx.commit()?;
            Ok(n)
        })
    }
}
#[cfg(test)]
mod tests {
    use crate::models::domain::pricing::{ModelSource, seed_models};
    use crate::models::ports::ModelRepo;
    use crate::test_support::temp_store;
    use chrono::Utc;

    #[test]
    fn seed_only_when_empty() {
        let (_d, s) = temp_store();
        s.seed_if_empty(&seed_models()).unwrap();
        s.seed_if_empty(&seed_models()[..1]).unwrap();
        assert_eq!(s.all().unwrap().len(), seed_models().len());
    }

    #[test]
    fn official_update_does_not_touch_user_rows() {
        let (_d, s) = temp_store();
        s.seed_if_empty(&seed_models()).unwrap();
        s.with(|c| c.execute("UPDATE models SET input = 99, source = 'user' WHERE model_prefix = 'claude-opus-5-5'", []))
            .unwrap();
        let mut update = seed_models();
        for m in &mut update {
            m.input = 1.5;
        }
        let n = s.upsert_official(&update, Utc::now()).unwrap();
        assert_eq!(n, seed_models().len() - 1);
        let all = s.all().unwrap();
        let opus = all
            .iter()
            .find(|m| m.model_prefix == "claude-opus-5-5")
            .unwrap();
        assert_eq!((opus.input, opus.source), (99.0, ModelSource::User));
        assert!(
            all.iter()
                .filter(|m| m.source == ModelSource::Official)
                .all(|m| m.input == 1.5)
        );
    }

    #[test]
    fn official_update_adds_new_models() {
        let (_d, s) = temp_store();
        let mut m = seed_models()[0].clone();
        m.model_prefix = "claude-new-9".into();
        assert_eq!(s.upsert_official(&[m], Utc::now()).unwrap(), 1);
        assert_eq!(s.all().unwrap().len(), 1);
    }
}
