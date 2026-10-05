//! 設定の永続化。

use crate::models::domain::settings::Settings;
use crate::models::ports::{RepoError, SettingsRepo};
use crate::models::repositories::db::SqliteStore;
use rusqlite::params;

impl SettingsRepo for SqliteStore {
    fn load(&self) -> Result<Settings, RepoError> {
        let pairs: Vec<(String, String)> = self.with(|c| {
            let mut st = c.prepare("SELECT key, value FROM settings")?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
        })?;
        Ok(Settings::from_pairs(&pairs))
    }

    fn save(&self, s: &Settings) -> Result<(), RepoError> {
        s.validate()
            .map_err(|e| RepoError::Invalid(e.to_string()))?;
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            for (k, v) in s.to_pairs() {
                tx.execute(
                    "INSERT INTO settings(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![k, v],
                )?;
            }
            tx.commit()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::models::domain::settings::{Settings, Theme};
    use crate::models::ports::{RepoError, SettingsRepo};
    use crate::test_support::temp_store;

    #[test]
    fn empty_table_loads_defaults() {
        let (_d, s) = temp_store();
        assert_eq!(s.load().unwrap(), Settings::default());
    }

    #[test]
    fn saved_values_load_back_and_overwrite() {
        let (_d, s) = temp_store();
        let a = Settings {
            usage_interval_secs: 150,
            theme: Theme::Dark,
            ..Settings::default()
        };
        s.save(&a).unwrap();
        let b = Settings {
            usage_interval_secs: 120,
            ..a
        };
        s.save(&b).unwrap();
        assert_eq!(s.load().unwrap(), b);
    }

    #[test]
    fn out_of_range_is_rejected_and_not_saved() {
        let (_d, s) = temp_store();
        let bad = Settings {
            usage_interval_secs: 5,
            ..Settings::default()
        };
        assert!(matches!(s.save(&bad), Err(RepoError::Invalid(m)) if m.contains("取得間隔")));
        assert_eq!(s.load().unwrap(), Settings::default());
    }
}
