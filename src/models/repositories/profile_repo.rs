//! プロファイルの永続化。
use crate::models::domain::profile::{DEFAULT_PROFILE, Profile, validate_profile_name};
use crate::models::ports::{ProfileRepo, RepoError};
use crate::models::repositories::db::{SqliteStore, ts};
use rusqlite::{OptionalExtension, Row, params};
use std::path::{Path, PathBuf};

fn row_to_profile(r: &Row<'_>) -> rusqlite::Result<Profile> {
    Ok(Profile {
        id: r.get(0)?,
        name: r.get(1)?,
        config_dir: r.get::<_, Option<String>>(2)?.map(PathBuf::from),
        is_active: r.get::<_, i64>(3)? == 1,
    })
}

const SELECT: &str = "SELECT id, name, config_dir, is_active FROM profiles";

impl ProfileRepo for SqliteStore {
    fn list(&self) -> Result<Vec<Profile>, RepoError> {
        self.with(|c| {
            let mut st = c.prepare(&format!("{SELECT} ORDER BY name"))?;
            st.query_map([], row_to_profile)?.collect()
        })
    }

    fn add(&self, name: &str, config_dir: Option<&Path>) -> Result<Profile, RepoError> {
        validate_profile_name(name).map_err(|e| RepoError::Invalid(e.to_string()))?;
        let dir = config_dir.map(|p| p.to_string_lossy().into_owned());
        let now = ts(chrono::Utc::now());
        let inserted = self.with(|c| {
            c.execute(
                "INSERT INTO profiles(name, config_dir, is_active, created_at) VALUES(?1, ?2, 0, ?3) ON CONFLICT(name) DO NOTHING",
                params![name, dir, now],
            )
        })?;
        if inserted == 0 {
            return Err(RepoError::Invalid(format!(
                "同じ名前のプロファイルがあります: {name}"
            )));
        }
        self.with(|c| c.query_row(&format!("{SELECT} WHERE name = ?1"), [name], row_to_profile))
    }

    fn remove(&self, name: &str) -> Result<(), RepoError> {
        let active: Option<i64> = self.with(|c| {
            c.query_row(
                "SELECT is_active FROM profiles WHERE name = ?1",
                [name],
                |r| r.get(0),
            )
            .optional()
        })?;
        match active {
            None => Err(RepoError::NotFound(format!("プロファイル {name}"))),
            Some(1) => Err(RepoError::Invalid(format!(
                "使用中のプロファイルは削除できません: {name}"
            ))),
            Some(_) => self
                .with(|c| c.execute("DELETE FROM profiles WHERE name = ?1", [name]))
                .map(|_| ()),
        }
    }

    fn set_active(&self, name: &str) -> Result<(), RepoError> {
        let changed = self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let n = tx.execute("UPDATE profiles SET is_active = (name = ?1)", [name])?;
            let hit: i64 = tx.query_row(
                "SELECT COUNT(*) FROM profiles WHERE name = ?1",
                [name],
                |r| r.get(0),
            )?;
            if hit == 0 {
                return Ok(0);
            }
            tx.commit()?;
            Ok(n)
        })?;
        if changed == 0 {
            Err(RepoError::NotFound(format!("プロファイル {name}")))
        } else {
            Ok(())
        }
    }

    fn active(&self) -> Result<Option<Profile>, RepoError> {
        self.with(|c| {
            c.query_row(&format!("{SELECT} WHERE is_active = 1"), [], row_to_profile)
                .optional()
        })
    }

    fn ensure_default(&self) -> Result<Profile, RepoError> {
        let count: i64 =
            self.with(|c| c.query_row("SELECT COUNT(*) FROM profiles", [], |r| r.get(0)))?;
        if count == 0 {
            self.add(DEFAULT_PROFILE, None)?;
            self.set_active(DEFAULT_PROFILE)?;
        }
        match self.active()? {
            Some(p) => Ok(p),
            None => Err(RepoError::NotFound("使用中のプロファイル".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::models::ports::{ProfileRepo, RepoError};
    use crate::test_support::temp_store;
    use std::path::Path;

    #[test]
    fn ensure_default_creates_active_default_once() {
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        assert_eq!(
            (p.name.as_str(), p.config_dir.clone(), p.is_active),
            ("default", None, true)
        );
        assert_eq!(s.ensure_default().unwrap().id, p.id);
        assert_eq!(s.list().unwrap().len(), 1);
    }

    #[test]
    fn ensure_default_keeps_existing_active_profile() {
        let (_d, s) = temp_store();
        s.add("sub", None).unwrap();
        s.set_active("sub").unwrap();
        assert_eq!(s.ensure_default().unwrap().name, "sub");
        assert_eq!(s.list().unwrap().len(), 1);
    }

    #[test]
    fn ensure_default_fails_when_profiles_exist_but_none_active() {
        let (_d, s) = temp_store();
        s.add("sub", None).unwrap();
        assert!(matches!(s.ensure_default(), Err(RepoError::NotFound(_))));
    }

    #[test]
    fn add_use_and_active_switch() {
        let (_d, s) = temp_store();
        s.ensure_default().unwrap();
        let sub = s.add("sub", Some(Path::new("/c/sub"))).unwrap();
        assert!(!sub.is_active);
        s.set_active("sub").unwrap();
        let active = s.active().unwrap().unwrap();
        assert_eq!(active.name, "sub");
        assert_eq!(active.config_dir.as_deref(), Some(Path::new("/c/sub")));
        assert_eq!(s.list().unwrap().iter().filter(|p| p.is_active).count(), 1);
    }

    #[test]
    fn list_is_sorted_by_name() {
        let (_d, s) = temp_store();
        s.add("zz", None).unwrap();
        s.add("aa", None).unwrap();
        let names: Vec<String> = s.list().unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["aa", "zz"]);
    }

    #[test]
    fn duplicate_or_invalid_names_are_rejected() {
        let (_d, s) = temp_store();
        s.add("sub", None).unwrap();
        assert!(matches!(s.add("sub", None), Err(RepoError::Invalid(_))));
        assert!(matches!(s.add("../x", None), Err(RepoError::Invalid(_))));
    }

    #[test]
    fn set_active_unknown_is_not_found_and_keeps_current() {
        let (_d, s) = temp_store();
        s.ensure_default().unwrap();
        assert!(matches!(s.set_active("nope"), Err(RepoError::NotFound(_))));
        assert_eq!(s.active().unwrap().unwrap().name, "default");
    }

    #[test]
    fn remove_rejects_active_and_unknown() {
        let (_d, s) = temp_store();
        s.ensure_default().unwrap();
        s.add("sub", None).unwrap();
        assert!(matches!(s.remove("default"), Err(RepoError::Invalid(_))));
        assert!(matches!(s.remove("nope"), Err(RepoError::NotFound(_))));
        s.remove("sub").unwrap();
        assert_eq!(s.list().unwrap().len(), 1);
    }

    #[test]
    fn active_is_none_when_empty() {
        let (_d, s) = temp_store();
        assert_eq!(s.active().unwrap(), None);
    }

    #[test]
    fn reopening_keeps_data_and_schema() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("cps.db");
        crate::models::repositories::db::SqliteStore::open(&path)
            .unwrap()
            .add("sub", None)
            .unwrap();
        let again = crate::models::repositories::db::SqliteStore::open(&path).unwrap();
        assert_eq!(again.list().unwrap()[0].name, "sub");
    }
}
