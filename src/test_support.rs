//! テスト専用の補助。実SQLiteの一時DBを作る。DBをモックしない方針をテスト全体で守るため。
use crate::models::domain::memory::{ExitError, ProcEntry, Victim};
use crate::models::ports::ProcessTree;
use crate::models::repositories::db::SqliteStore;

/// 一時ディレクトリに実DBを作る。`TempDir`を返すのは、呼び出し側が持っている間だけファイルを残すため。
pub(crate) fn temp_store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().expect("一時ディレクトリ");
    let store = SqliteStore::open(&dir.path().join("cumon.db")).expect("DBを開く");
    (dir, store)
}

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

/// WireMockコンテナ。本番と同じHTTP経路で外部APIのふるまいを再現するために使う。
pub(crate) struct WireMock {
    _container: Container<GenericImage>,
    pub(crate) base_url: String,
    // コンテナを止めてから枠を返すため、`_container`より後に置く（フィールドは宣言順に破棄される）。
    _slot: Slot,
}

impl WireMock {
    /// コンテナを起動し、管理APIが応答するまで待つ。
    pub(crate) fn start() -> Self {
        let slot = Slot::acquire();
        let container = GenericImage::new("wiremock/wiremock", "3.13.2-alpine")
            .with_exposed_port(8080.tcp())
            .start()
            .expect("WireMockの起動（DOCKER_HOSTとTESTCONTAINERS_DOCKER_SOCKET_OVERRIDEを確認）");
        let port = container.get_host_port_ipv4(8080.tcp()).expect("ポート");
        let base_url = format!("http://127.0.0.1:{port}");
        let health = format!("{base_url}/__admin/health");
        // 品質ゲートのmutation testingと並行して動くと起動が遅れるため、最大120秒待つ。
        let ready = (0..240).any(|_| {
            let ok = ureq::get(&health).call().is_ok();
            if !ok {
                std::thread::park_timeout(std::time::Duration::from_millis(500));
            }
            ok
        });
        assert!(ready, "WireMockが120秒以内に起動しませんでした");
        Self {
            _container: container,
            base_url,
            _slot: slot,
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

use crate::models::domain::profile::Profile;
use crate::models::domain::usage::UsageSnapshot;
use crate::models::ports::{
    Clock, Credential, CredentialError, CredentialStore, UsageApi, UsageApiError,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// 手で進める時計。
pub(crate) struct FixedClock(pub(crate) Mutex<DateTime<Utc>>);

impl FixedClock {
    pub(crate) fn at(t: DateTime<Utc>) -> Self {
        Self(Mutex::new(t))
    }
    pub(crate) fn advance(&self, d: chrono::Duration) {
        let mut g = self.0.lock().unwrap();
        *g += d;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

/// トークンごとに応答を決めるAPI。呼び出し回数も数える。
pub(crate) struct FakeApi {
    pub(crate) responses: HashMap<String, Result<UsageSnapshot, UsageApiError>>,
    pub(crate) calls: Mutex<Vec<String>>,
}

impl UsageApi for FakeApi {
    fn fetch(&self, token: &str) -> Result<UsageSnapshot, UsageApiError> {
        self.calls.lock().unwrap().push(token.to_string());
        self.responses
            .get(token)
            .cloned()
            .unwrap_or(Err(UsageApiError::Http(404)))
    }
}

/// プロファイル名ごとに認証情報を返す保存先。
pub(crate) struct FakeCreds(pub(crate) HashMap<String, Result<Credential, CredentialError>>);

impl CredentialStore for FakeCreds {
    fn load(&self, profile: &Profile, _dir: &Path) -> Result<Credential, CredentialError> {
        self.0
            .get(&profile.name)
            .cloned()
            .unwrap_or(Err(CredentialError::Missing))
    }
}

use crate::models::domain::pricing::TokenUsage;
use crate::models::domain::records::{SessionUpsert, TurnRecord};
use crate::models::domain::transcript::SessionKind;
use crate::models::ports::{IngestRepo, ProfileRepo};

/// セッション行を1つ作る。既定プロファイルに属させる。
pub(crate) fn seed_session(
    s: &SqliteStore,
    id: &str,
    kind: SessionKind,
    status: Option<&str>,
    last: DateTime<Utc>,
) {
    let p = s.ensure_default().unwrap();
    s.upsert_session(&SessionUpsert {
        session_id: id.into(),
        profile_id: p.id,
        kind,
        entrypoint: None,
        cwd: Some(format!("/work/{id}")),
        git_branch: Some("main".into()),
        name: Some(format!("name-{id}")),
        first_prompt: Some(format!("prompt-{id}")),
        started_at: last - chrono::Duration::minutes(5),
        last_activity_at: last,
        status: status.map(str::to_string),
    })
    .unwrap();
}

/// ターン行を1つ作る。
#[allow(clippy::too_many_arguments)]
pub(crate) fn seed_turn(
    s: &SqliteStore,
    session: &str,
    agent: &str,
    message: &str,
    ts: DateTime<Utc>,
    model: Option<&str>,
    kind: &str,
    usage: TokenUsage,
) {
    s.upsert_turn(&TurnRecord {
        session_id: session.into(),
        agent_id: agent.into(),
        message_id: message.into(),
        ts,
        model: model.map(str::to_string),
        kind: kind.into(),
        summary: format!("summary-{message}"),
        usage,
    })
    .unwrap();
}

/// 入力と出力だけを持つToken数。
pub(crate) fn tokens(input: u64, output: u64) -> TokenUsage {
    TokenUsage {
        input,
        output,
        ..TokenUsage::default()
    }
}

use crate::controllers::gui::app::GuiDeps;
use crate::models::ports::RepoError;
use crate::models::ports::{CatalogRefresh, DaemonControl};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 稼働状態を切り替えられるデーモン制御。
#[derive(Default)]
pub(crate) struct FakeDaemon {
    pub(crate) running: AtomicBool,
    pub(crate) fail_start: bool,
}

impl DaemonControl for FakeDaemon {
    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    fn start(&self) -> Result<(), String> {
        if self.fail_start {
            return Err("デーモンを起動できません: テスト".into());
        }
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// 呼ばれた回数を数えるモデル情報の更新。
#[derive(Default)]
pub(crate) struct CountingCatalog(pub(crate) std::sync::atomic::AtomicUsize);

impl CatalogRefresh for CountingCatalog {
    fn refresh(&self) -> Result<usize, RepoError> {
        Ok(self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

/// GUIのcontrollerのテスト用の依存。DBは実SQLite、時計は固定、認証情報は`creds`で与える。
pub(crate) fn gui_deps(
    store: Arc<SqliteStore>,
    clock: Arc<FixedClock>,
    home: &std::path::Path,
    creds: Arc<dyn CredentialStore>,
    daemon: Arc<FakeDaemon>,
) -> GuiDeps {
    GuiDeps {
        clock,
        tz: chrono::FixedOffset::east_opt(9 * 3600).unwrap(),
        home: home.to_path_buf(),
        profiles: store.clone(),
        usage: store.clone(),
        dashboard: store.clone(),
        sessions: store.clone(),
        analytics: store.clone(),
        diagnostics: store.clone(),
        logs: store.clone(),
        models: store.clone(),
        settings: store.clone(),
        creds,
        daemon,
        catalog: Arc::new(CountingCatalog::default()),
        autostart: Arc::new(FakeAutostart::default()),
        process_tree: Arc::new(FakeProcessTree::default()),
    }
}
/// 固定の一覧を返し、終了の要求を記録するだけのプロセス一覧。利用者のプロセスに触れずにGUIを確かめるため。
#[derive(Default)]
pub(crate) struct FakeProcessTree {
    pub(crate) procs: Mutex<Vec<ProcEntry>>,
    pub(crate) exits: Mutex<Vec<Victim>>,
    pub(crate) fail: Mutex<Option<ExitError>>,
}

impl ProcessTree for FakeProcessTree {
    fn snapshot(&self) -> Vec<ProcEntry> {
        self.procs.lock().unwrap().clone()
    }
    fn request_exit(&self, v: &Victim) -> Result<(), ExitError> {
        if let Some(e) = self.fail.lock().unwrap().clone() {
            return Err(e);
        }
        self.exits.lock().unwrap().push(*v);
        Ok(())
    }
}

/// メモリの状態のテスト用の依存。一時ディレクトリはテストの間だけ残す必要があるので、一緒に返す。
pub(crate) fn gui_deps_with_procs(
    procs: Arc<FakeProcessTree>,
) -> (GuiDeps, tempfile::TempDir, tempfile::TempDir) {
    let (db, store) = temp_store();
    let home = tempfile::tempdir().unwrap();
    let mut d = gui_deps(
        Arc::new(store),
        Arc::new(FixedClock::at(Utc::now())),
        home.path(),
        Arc::new(FakeCreds(HashMap::new())),
        Arc::new(FakeDaemon::default()),
    );
    d.process_tree = procs;
    (d, db, home)
}

/// 通知を記録するだけのフェイク。
#[derive(Default)]
pub(crate) struct RecordingNotifier(pub(crate) Mutex<Vec<(String, String)>>);

impl crate::models::ports::Notifier for RecordingNotifier {
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .push((title.to_string(), body.to_string()));
        Ok(())
    }
}

/// 登録状態を覚えるだけの自動起動。
#[derive(Default)]
pub(crate) struct FakeAutostart(pub(crate) Mutex<bool>);

impl crate::models::ports::Autostart for FakeAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        Ok(*self.0.lock().unwrap())
    }
    fn enable(&self) -> Result<(), String> {
        *self.0.lock().unwrap() = true;
        Ok(())
    }
    fn disable(&self) -> Result<(), String> {
        *self.0.lock().unwrap() = false;
        Ok(())
    }
}
