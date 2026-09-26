//! テスト専用の補助。実SQLiteの一時DBを作る。DBをモックしない方針をテスト全体で守るため。
use crate::models::repositories::db::SqliteStore;

/// 一時ディレクトリに実DBを作る。`TempDir`を返すのは、呼び出し側が持っている間だけファイルを残すため。
pub(crate) fn temp_store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().expect("一時ディレクトリ");
    let store = SqliteStore::open(&dir.path().join("cps.db")).expect("DBを開く");
    (dir, store)
}
