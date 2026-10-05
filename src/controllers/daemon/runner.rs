//! デーモンの周期処理。使用量の取得、監視イベントによる差分取り込み、定期的な全体走査、
//! RSSの記録、保持期間の削除を1本のループで回す。
//!
//! 待機は監視チャネルの`recv_timeout`で行う。イベントが来たらすぐ取り込み、来なければ周期で進むため。
use crate::controllers::daemon::alert::Alerter;
use crate::controllers::daemon::collector::UsageCollector;
use crate::controllers::daemon::ingest::{IngestReport, Ingestor};
use crate::controllers::daemon::retention::purge_expired;
use crate::models::domain::profile::Profile;
use crate::models::domain::records::{FetchLogEntry, FetchResult};
use crate::models::gateways::jsonl::TranscriptFile;
use crate::models::ports::{
    Clock, FetchLogRepo, MaintenanceRepo, ProcessInfo, ProfileRepo, RepoError, SettingsRepo,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// 周期の設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSettings {
    /// 使用量の取得間隔。
    pub usage_interval: Duration,
    /// 全体走査の間隔。監視イベントの取りこぼしを拾うため。
    pub full_scan_interval: Duration,
    /// RSS記録の間隔。
    pub stats_interval: Duration,
    /// 保持日数。
    pub retention_days: i64,
    /// 使用量の取得をこの回数行ったら終了する。E2Eとリーク検査で回数を決めて止めるため。
    pub max_usage_ticks: Option<u64>,
    /// `true`なら全体走査ごとに`settings`表から取得間隔と保持日数を読み直す。
    /// CLIで`--interval-secs`を指定したときとテストでは`false`にし、指定した間隔を設定で変えられないようにする。
    pub follow_settings: bool,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            usage_interval: Duration::from_secs(120),
            full_scan_interval: Duration::from_secs(300),
            stats_interval: Duration::from_secs(600),
            retention_days: 90,
            max_usage_ticks: None,
            follow_settings: false,
        }
    }
}

/// デーモンの部品。
pub struct DaemonParts {
    /// 使用量の収集役。
    pub collector: UsageCollector,
    /// 取り込み役。
    pub ingestor: Ingestor,
    /// プロファイル一覧。
    pub profiles: Arc<dyn ProfileRepo>,
    /// 設定。
    pub settings: Arc<dyn SettingsRepo>,
    /// 取得ログ。取り込み件数を診断画面へ渡す。
    pub log: Arc<dyn FetchLogRepo>,
    /// 保守。
    pub maintenance: Arc<dyn MaintenanceRepo>,
    /// プロセス情報。
    pub process: Arc<dyn ProcessInfo>,
    /// 時計。
    pub clock: Arc<dyn Clock>,
    /// ホームディレクトリ。
    pub home: PathBuf,
    /// 閾値の通知。`None`なら通知しない（テストと、通知を使わない環境）。
    pub alerter: Option<Alerter>,
    /// 一時停止の旗。トレイと共有する。
    pub paused: Arc<AtomicBool>,
}

/// 常駐デーモン。
pub struct Daemon {
    p: DaemonParts,
    s: DaemonSettings,
}

/// パスがどのプロファイルの設定ディレクトリ配下かを返す。監視イベントを正しいプロファイルに取り込むため。
pub fn profile_for_path<'a>(
    profiles: &'a [Profile],
    home: &Path,
    path: &Path,
) -> Option<&'a Profile> {
    profiles
        .iter()
        .find(|p| path.starts_with(p.resolved_config_dir(home)))
}

/// 監視イベントのパスをどう取り込むか。
enum Route<'a> {
    /// セッションまたはサブエージェントのJSONL。
    Transcript(&'a Profile, TranscriptFile),
    /// 稼働中セッションやジョブの状態ファイル。
    LiveState(&'a Profile),
    /// どのプロファイルにも属さないパス。
    Ignore,
}

/// パスを取り込み方へ振り分ける。`timeline.jsonl`はジョブの状態なので、会話ログとしては読まない。
fn route_path<'a>(profiles: &'a [Profile], home: &Path, path: PathBuf) -> Route<'a> {
    let Some(profile) = profile_for_path(profiles, home, &path) else {
        return Route::Ignore;
    };
    if path.extension().is_none_or(|e| e != "jsonl") || path.ends_with("timeline.jsonl") {
        return Route::LiveState(profile);
    }
    let agent_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("agent-"))
        .map(str::to_string);
    Route::Transcript(profile, TranscriptFile { path, agent_id })
}

/// 保持期間の削除を行う間隔。
const PURGE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// 各処理の次回予定時刻。時刻を引数で受け取り、実時間を待たずに周期の計算をテストできるようにする。
#[derive(Debug, Clone, Copy)]
struct Schedule {
    usage: Instant,
    scan: Instant,
    stats: Instant,
    purge: Instant,
}

/// ある時点で実行すべき処理。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Due {
    scan: bool,
    usage: bool,
    stats: bool,
    purge: bool,
}

fn advance(next: &mut Instant, now: Instant, every: Duration) -> bool {
    if now < *next {
        return false;
    }
    *next = now + every;
    true
}

impl Schedule {
    fn starting(now: Instant) -> Self {
        Self {
            usage: now,
            scan: now,
            stats: now,
            purge: now,
        }
    }

    /// 期限が来た処理を返し、その処理の次回時刻を進める。
    fn take_due(&mut self, now: Instant, s: &DaemonSettings) -> Due {
        Due {
            scan: advance(&mut self.scan, now, s.full_scan_interval),
            usage: advance(&mut self.usage, now, s.usage_interval),
            stats: advance(&mut self.stats, now, s.stats_interval),
            purge: advance(&mut self.purge, now, PURGE_INTERVAL),
        }
    }

    /// 次の使用量取得か全体走査までの待ち時間。監視イベントに素早く反応できるよう最大1秒にする。
    fn wait(&self, now: Instant) -> Duration {
        self.usage
            .min(self.scan)
            .saturating_duration_since(now)
            .min(Duration::from_secs(1))
    }
}

impl Daemon {
    /// 部品と設定から作る。
    pub fn new(parts: DaemonParts, settings: DaemonSettings) -> Self {
        Self {
            p: parts,
            s: settings,
        }
    }

    fn ingest_paths(&self, dirty: &mut HashSet<PathBuf>) -> Result<(), RepoError> {
        if dirty.is_empty() {
            return Ok(());
        }
        let profiles = self.p.profiles.list()?;
        let mut report = IngestReport::default();
        let mut live_refresh: HashSet<i64> = HashSet::new();
        for path in dirty.drain() {
            match route_path(&profiles, &self.p.home, path) {
                Route::Transcript(profile, file) => {
                    self.p.ingestor.ingest_file(profile, &file, &mut report)?
                }
                Route::LiveState(profile) => {
                    live_refresh.insert(profile.id);
                }
                Route::Ignore => {}
            }
        }
        for p in profiles.iter().filter(|p| live_refresh.contains(&p.id)) {
            self.p
                .ingestor
                .ingest_live_state(p, &p.resolved_config_dir(&self.p.home))?;
        }
        Ok(())
    }

    /// 現在の周期設定。設定の反映をテストで確かめるため公開する。
    pub fn settings(&self) -> &DaemonSettings {
        &self.s
    }

    fn full_scan(&mut self) -> Result<(), RepoError> {
        self.reload_settings();
        let mut total = IngestReport::default();
        for p in self.p.profiles.list()? {
            let report = self
                .p
                .ingestor
                .scan_all(&p, &p.resolved_config_dir(&self.p.home))?;
            total = total.plus(report);
        }
        self.p.log.log(&FetchLogEntry {
            target: "ingest".into(),
            at: self.p.clock.now(),
            result: FetchResult::Ok,
            http_status: None,
            message: format!(
                "files={} lines={} malformed={}",
                total.files_read, total.lines, total.malformed
            ),
        })
    }

    /// 設定画面の変更を、デーモンを再起動せずに次の全体走査から反映する。読めなければ今の値を使い続ける。
    fn reload_settings(&mut self) {
        if !self.s.follow_settings {
            return;
        }
        if let Ok(st) = self.p.settings.load() {
            self.s.usage_interval = Duration::from_secs(st.usage_interval_secs);
            self.s.retention_days = st.retention_days;
        }
    }

    /// 一時停止の旗。トレイに渡すため公開する。
    pub fn paused(&self) -> &Arc<AtomicBool> {
        &self.p.paused
    }

    /// 通知役を差し替える。
    pub fn set_alerter(&mut self, a: Alerter) {
        self.p.alerter = Some(a);
    }

    /// `shutdown`が立つか、`max_usage_ticks`に達するまで回す。DBの失敗だけを返して止まる。
    pub fn run(
        &mut self,
        events: &Receiver<PathBuf>,
        shutdown: &AtomicBool,
    ) -> Result<(), RepoError> {
        self.p.profiles.ensure_default()?;
        let mut sched = Schedule::starting(Instant::now());
        let mut ticks = 0u64;
        let mut dirty: HashSet<PathBuf> = HashSet::new();
        while !shutdown.load(Ordering::Relaxed) {
            let due = sched.take_due(Instant::now(), &self.s);
            if self.cycle(due, &mut dirty, &mut ticks)? {
                break;
            }
            self.maintain(due)?;
            wait_events(events, sched.wait(Instant::now()), &mut dirty);
        }
        Ok(())
    }

    /// 1周期ぶんの取り込みと取得を行い、`max_usage_ticks`に達したら`true`を返す。
    /// 一時停止中は取得と取り込みを止める。監視イベントは`dirty`に残し、再開後の最初の周期で取り込む。
    fn cycle(
        &mut self,
        due: Due,
        dirty: &mut HashSet<PathBuf>,
        ticks: &mut u64,
    ) -> Result<bool, RepoError> {
        if self.p.paused.load(Ordering::Relaxed) {
            return Ok(false);
        }
        if due.scan {
            self.full_scan()?;
            dirty.clear();
        }
        self.ingest_paths(dirty)?;
        if !due.usage {
            return Ok(false);
        }
        self.usage_tick()?;
        *ticks += 1;
        Ok(self.s.max_usage_ticks.is_some_and(|m| *ticks >= m))
    }

    fn usage_tick(&mut self) -> Result<(), RepoError> {
        self.p.collector.tick()?;
        if let Some(a) = self.p.alerter.as_mut() {
            a.check()?;
        }
        Ok(())
    }
}

/// 次の周期まで監視イベントを待ち、届いたパスを`dirty`に集める。
/// 監視役が止まって送信側が消えた後も周期を保つため、切断時は同じ時間だけ眠る。
fn wait_events(
    events: &Receiver<PathBuf>,
    wait: std::time::Duration,
    dirty: &mut HashSet<PathBuf>,
) {
    match events.recv_timeout(wait) {
        Ok(p) => {
            dirty.insert(p);
            dirty.extend(events.try_iter());
        }
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => std::thread::park_timeout(wait),
    }
}

impl Daemon {
    fn maintain(&self, due: Due) -> Result<(), RepoError> {
        if due.stats {
            self.p
                .maintenance
                .record_daemon_stats(self.p.clock.now(), self.p.process.self_rss_bytes())?;
        }
        if due.purge {
            purge_expired(
                self.p.maintenance.as_ref(),
                self.p.clock.now(),
                self.s.retention_days,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::daemon::collector::CollectorDeps;
    use crate::models::domain::settings::Settings;
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{Credential, FetchLogRepo, ProfileRepo, SettingsRepo};
    use crate::test_support::{FakeApi, FakeCreds, FixedClock, temp_store};
    use chrono::Utc;
    use std::collections::HashMap;
    use std::collections::HashSet;

    use std::io::Write;
    use std::sync::Mutex;
    use std::sync::mpsc::sync_channel;

    #[test]
    fn default_usage_interval_is_two_minutes() {
        assert_eq!(
            DaemonSettings::default().usage_interval,
            Duration::from_secs(120)
        );
    }

    #[test]
    fn runs_until_max_ticks_ingesting_and_collecting() {
        let (_d, store) = temp_store();
        let store = Arc::new(store);
        store.ensure_default().unwrap();
        let home = tempfile::tempdir().unwrap();
        let f = home.path().join(".claude/projects/-w/s1.jsonl");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        writeln!(std::fs::File::create(&f).unwrap(), r#"{{"type":"user","uuid":"u1","sessionId":"s1","timestamp":"{}","message":{{"content":"hi"}}}}"#, crate::models::repositories::db::ts(Utc::now())).unwrap();
        let clock = Arc::new(FixedClock::at(Utc::now()));
        let snap = parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap();
        let api = Arc::new(FakeApi {
            responses: HashMap::from([("t".to_string(), Ok(snap))]),
            calls: Mutex::new(vec![]),
        });
        let creds = Arc::new(FakeCreds(HashMap::from([(
            "default".to_string(),
            Ok(Credential {
                access_token: "t".into(),
                expires_at: None,
                subscription_type: None,
            }),
        )])));
        let process = Arc::new(crate::models::gateways::process::SysProcessInfo::new());
        let collector = UsageCollector::new(
            CollectorDeps {
                profiles: store.clone(),
                creds,
                api: api.clone(),
                usage: store.clone(),
                log: store.clone(),
                clock: clock.clone(),
            },
            home.path().to_path_buf(),
        );
        let ingestor = Ingestor::new(
            store.clone(),
            store.clone(),
            process.clone(),
            clock.clone(),
            chrono::Duration::days(90),
        );
        let mut daemon = Daemon::new(
            DaemonParts {
                collector,
                ingestor,
                profiles: store.clone(),
                settings: store.clone(),
                log: store.clone(),
                maintenance: store.clone(),
                process,
                clock,
                home: home.path().to_path_buf(),
                alerter: None,
                paused: Arc::new(AtomicBool::new(false)),
            },
            DaemonSettings {
                usage_interval: Duration::from_millis(10),
                max_usage_ticks: Some(3),
                ..DaemonSettings::default()
            },
        );
        let (tx, rx) = sync_channel(8);
        tx.send(f.clone()).unwrap();
        daemon.run(&rx, &AtomicBool::new(false)).unwrap();
        assert_eq!(api.calls.lock().unwrap().len(), 3);
        let (sessions, stats): (i64, i64) = store
            .with(|c| {
                Ok((
                    c.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?,
                    c.query_row("SELECT COUNT(*) FROM daemon_stats", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!((sessions, stats), (1, 1));
    }

    fn daemon_for(
        store: &Arc<crate::models::repositories::db::SqliteStore>,
        home: &std::path::Path,
        settings: DaemonSettings,
    ) -> Daemon {
        let api = Arc::new(FakeApi {
            responses: HashMap::new(),
            calls: Mutex::new(vec![]),
        });
        daemon_with(
            store,
            home,
            settings,
            api,
            Arc::new(FakeCreds(HashMap::new())),
        )
    }

    /// 使用量APIが`usage_ok.json`を返すデーモン。APIの呼び出し回数を数えるため、フェイクも返す。
    fn usage_daemon(
        store: &Arc<crate::models::repositories::db::SqliteStore>,
        home: &std::path::Path,
        settings: DaemonSettings,
    ) -> (Daemon, Arc<FakeApi>) {
        let snap = parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap();
        let api = Arc::new(FakeApi {
            responses: HashMap::from([("t".to_string(), Ok(snap))]),
            calls: Mutex::new(vec![]),
        });
        let creds = Arc::new(FakeCreds(HashMap::from([(
            "default".to_string(),
            Ok(Credential {
                access_token: "t".into(),
                expires_at: None,
                subscription_type: None,
            }),
        )])));
        (daemon_with(store, home, settings, api.clone(), creds), api)
    }

    fn daemon_with(
        store: &Arc<crate::models::repositories::db::SqliteStore>,
        home: &std::path::Path,
        settings: DaemonSettings,
        api: Arc<FakeApi>,
        creds: Arc<FakeCreds>,
    ) -> Daemon {
        let clock = Arc::new(FixedClock::at(Utc::now()));
        let process = Arc::new(crate::models::gateways::process::SysProcessInfo::new());
        let collector = UsageCollector::new(
            CollectorDeps {
                profiles: store.clone(),
                creds,
                api,
                usage: store.clone(),
                log: store.clone(),
                clock: clock.clone(),
            },
            home.to_path_buf(),
        );
        let ingestor = Ingestor::new(
            store.clone(),
            store.clone(),
            process.clone(),
            clock.clone(),
            chrono::Duration::days(90),
        );
        Daemon::new(
            DaemonParts {
                collector,
                ingestor,
                profiles: store.clone(),
                settings: store.clone(),
                log: store.clone(),
                maintenance: store.clone(),
                process,
                clock,
                home: home.to_path_buf(),
                alerter: None,
                paused: Arc::new(AtomicBool::new(false)),
            },
            settings,
        )
    }

    fn quick(ticks: u64) -> DaemonSettings {
        DaemonSettings {
            usage_interval: Duration::from_millis(10),
            max_usage_ticks: Some(ticks),
            ..DaemonSettings::default()
        }
    }

    #[test]
    fn paused_daemon_skips_collection() {
        let (_d, store) = temp_store();
        let store = Arc::new(store);
        store.ensure_default().unwrap();
        let home = tempfile::tempdir().unwrap();
        let (mut daemon, api) = usage_daemon(&store, home.path(), quick(1));
        daemon.paused().store(true, Ordering::SeqCst);
        let paused = daemon.paused().clone();
        let while_paused = Arc::new(Mutex::new(None));
        let (seen, api2) = (while_paused.clone(), api.clone());
        let h = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            *seen.lock().unwrap() = Some(api2.calls.lock().unwrap().len());
            paused.store(false, Ordering::SeqCst);
        });
        let (_tx, rx) = sync_channel(8);
        daemon.run(&rx, &AtomicBool::new(false)).unwrap();
        h.join().unwrap();
        assert_eq!(
            *while_paused.lock().unwrap(),
            Some(0),
            "一時停止中はAPIを呼ばない"
        );
        assert_eq!(
            api.calls.lock().unwrap().len(),
            1,
            "再開すると次の周期で取得する"
        );
    }

    #[test]
    fn alerter_runs_after_usage_tick() {
        use crate::controllers::daemon::alert::{AlertDeps, Alerter};
        use crate::test_support::RecordingNotifier;
        let (_d, store) = temp_store();
        let store = Arc::new(store);
        store.ensure_default().unwrap();
        store
            .save(&Settings {
                notify_threshold_percent: 10.0,
                ..Settings::default()
            })
            .unwrap();
        let home = tempfile::tempdir().unwrap();
        let (mut daemon, _api) = usage_daemon(&store, home.path(), quick(1));
        let notes = Arc::new(RecordingNotifier::default());
        daemon.set_alerter(Alerter::new(AlertDeps {
            profiles: store.clone(),
            dashboard: store.clone(),
            settings: store.clone(),
            notifier: notes.clone(),
            clock: Arc::new(FixedClock::at(Utc::now())),
            tz: chrono::FixedOffset::east_opt(0).unwrap(),
        }));
        let (_tx, rx) = sync_channel(8);
        daemon.run(&rx, &AtomicBool::new(false)).unwrap();
        assert!(!notes.0.lock().unwrap().is_empty());
    }

    #[test]
    fn dirty_paths_are_routed_by_kind() {
        let (_d, store) = temp_store();
        let store = Arc::new(store);
        store.ensure_default().unwrap();
        let home = tempfile::tempdir().unwrap();
        let cfg = home.path().join(".claude");
        let now = crate::models::repositories::db::ts(Utc::now());
        let main = cfg.join("projects/-w/s1.jsonl");
        let sub = cfg.join("projects/-w/s1/subagents/agent-a1.jsonl");
        let live = cfg.join("sessions/1.json");
        let timeline = cfg.join("jobs/j1/timeline.jsonl");
        for p in [&main, &sub, &live, &timeline] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        }
        let line = |extra: &str| {
            format!(
                r#"{{"type":"user","uuid":"u{extra}","sessionId":"s1","timestamp":"{now}","message":{{"content":"hi"}}}}"#
            )
        };
        writeln!(std::fs::File::create(&main).unwrap(), "{}", line("1")).unwrap();
        writeln!(std::fs::File::create(&sub).unwrap(), "{}", line("2")).unwrap();
        std::fs::write(&live, r#"{"pid":1,"sessionId":"s1","status":"busy"}"#).unwrap();
        std::fs::write(&timeline, "{}\n").unwrap();
        let daemon = daemon_for(&store, home.path(), DaemonSettings::default());
        let mut dirty: HashSet<PathBuf> = [
            main,
            sub,
            live,
            timeline,
            PathBuf::from("/elsewhere/x.jsonl"),
        ]
        .into_iter()
        .collect();
        daemon.ingest_paths(&mut dirty).unwrap();
        assert!(dirty.is_empty());
        let (turns, sub_turns, status): (i64, i64, String) = store
            .with(|c| {
                Ok((
                    c.query_row("SELECT COUNT(*) FROM turns", [], |r| r.get(0))?,
                    c.query_row(
                        "SELECT COUNT(*) FROM turns WHERE agent_id = 'a1'",
                        [],
                        |r| r.get(0),
                    )?,
                    c.query_row(
                        "SELECT status FROM sessions WHERE session_id = 's1'",
                        [],
                        |r| r.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!((turns, sub_turns), (2, 1));
        assert_eq!(status, "busy");
    }

    #[test]
    fn empty_dirty_set_does_nothing() {
        let (_d, store) = temp_store();
        let store = Arc::new(store);
        let daemon = daemon_for(
            &store,
            std::path::Path::new("/h"),
            DaemonSettings::default(),
        );
        daemon.ingest_paths(&mut HashSet::new()).unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    fn settings() -> DaemonSettings {
        DaemonSettings {
            usage_interval: Duration::from_secs(60),
            full_scan_interval: Duration::from_secs(300),
            stats_interval: Duration::from_secs(600),
            ..DaemonSettings::default()
        }
    }

    #[test]
    fn schedule_runs_everything_at_start_then_each_on_its_own_period() {
        let t0 = Instant::now();
        let s = settings();
        let mut sched = Schedule::starting(t0);
        assert_eq!(
            sched.take_due(t0, &s),
            Due {
                scan: true,
                usage: true,
                stats: true,
                purge: true
            }
        );
        assert_eq!(
            sched.take_due(t0 + Duration::from_secs(59), &s),
            Due::default()
        );
        assert_eq!(
            sched.take_due(t0 + Duration::from_secs(60), &s),
            Due {
                usage: true,
                ..Due::default()
            }
        );
        assert_eq!(
            sched.take_due(t0 + Duration::from_secs(300), &s),
            Due {
                scan: true,
                usage: true,
                ..Due::default()
            }
        );
        assert_eq!(
            sched.take_due(t0 + Duration::from_secs(599), &s),
            Due {
                usage: true,
                ..Due::default()
            }
        );
        assert_eq!(
            sched.take_due(t0 + Duration::from_secs(600), &s),
            Due {
                stats: true,
                scan: true,
                ..Due::default()
            }
        );
        assert!(
            !sched
                .take_due(t0 + PURGE_INTERVAL - Duration::from_secs(1), &s)
                .purge
        );
        assert!(sched.take_due(t0 + PURGE_INTERVAL, &s).purge);
        assert_eq!(
            PURGE_INTERVAL,
            Duration::from_secs(86_400),
            "削除は1日に1回"
        );
    }

    #[test]
    fn wait_events_collects_all_pending_paths() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(PathBuf::from("/a")).unwrap();
        tx.send(PathBuf::from("/b")).unwrap();
        let mut dirty = HashSet::new();
        wait_events(&rx, Duration::from_millis(10), &mut dirty);
        assert_eq!(
            dirty,
            HashSet::from([PathBuf::from("/a"), PathBuf::from("/b")])
        );
        drop(tx);
        wait_events(&rx, Duration::from_millis(1), &mut dirty);
        assert_eq!(dirty.len(), 2, "送信側が消えても止まらずに戻る");
    }

    #[test]
    fn wait_is_until_next_usage_or_scan_capped_at_one_second() {
        let t0 = Instant::now();
        let mut s = settings();
        s.usage_interval = Duration::from_millis(10);
        let mut sched = Schedule::starting(t0);
        sched.take_due(t0, &s);
        assert_eq!(sched.wait(t0), Duration::from_millis(10));
        assert_eq!(
            sched.wait(t0 + Duration::from_millis(4)),
            Duration::from_millis(6)
        );
        s.usage_interval = Duration::from_secs(60);
        let mut sched = Schedule::starting(t0);
        sched.take_due(t0, &s);
        assert_eq!(sched.wait(t0), Duration::from_secs(1));
        assert_eq!(sched.wait(t0 + Duration::from_secs(120)), Duration::ZERO);
    }

    #[test]
    fn shutdown_flag_stops_immediately() {
        let (_d, store) = temp_store();
        let store = Arc::new(store);
        let clock = Arc::new(FixedClock::at(Utc::now()));
        let process = Arc::new(crate::models::gateways::process::SysProcessInfo::new());
        let api = Arc::new(FakeApi {
            responses: HashMap::new(),
            calls: Mutex::new(vec![]),
        });
        let collector = UsageCollector::new(
            CollectorDeps {
                profiles: store.clone(),
                creds: Arc::new(FakeCreds(HashMap::new())),
                api: api.clone(),
                usage: store.clone(),
                log: store.clone(),
                clock: clock.clone(),
            },
            "/h".into(),
        );
        let ingestor = Ingestor::new(
            store.clone(),
            store.clone(),
            process.clone(),
            clock.clone(),
            chrono::Duration::days(90),
        );
        let mut daemon = Daemon::new(
            DaemonParts {
                collector,
                ingestor,
                profiles: store.clone(),
                settings: store.clone(),
                log: store.clone(),
                maintenance: store.clone(),
                process,
                clock,
                home: "/h".into(),
                alerter: None,
                paused: Arc::new(AtomicBool::new(false)),
            },
            DaemonSettings::default(),
        );
        let (_tx, rx) = sync_channel::<PathBuf>(1);
        daemon.run(&rx, &AtomicBool::new(true)).unwrap();
        assert!(api.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn profile_for_path_picks_owning_config_dir() {
        let home = std::path::Path::new("/h");
        let ps = vec![
            crate::models::domain::profile::Profile {
                id: 1,
                name: "default".into(),
                config_dir: None,
                is_active: true,
            },
            crate::models::domain::profile::Profile {
                id: 2,
                name: "sub".into(),
                config_dir: Some("/c".into()),
                is_active: false,
            },
        ];
        assert_eq!(
            profile_for_path(&ps, home, std::path::Path::new("/c/projects/x.jsonl"))
                .unwrap()
                .id,
            2
        );
        assert_eq!(
            profile_for_path(
                &ps,
                home,
                std::path::Path::new("/h/.claude/sessions/1.json")
            )
            .unwrap()
            .id,
            1
        );
        assert!(profile_for_path(&ps, home, std::path::Path::new("/tmp/x")).is_none());
    }

    fn one_tick(follow: bool) -> DaemonSettings {
        DaemonSettings {
            usage_interval: Duration::from_millis(10),
            max_usage_ticks: Some(1),
            follow_settings: follow,
            ..DaemonSettings::default()
        }
    }

    #[test]
    fn full_scan_records_ingest_counts_in_fetch_log() {
        let (_d, store) = temp_store();
        let store = Arc::new(store);
        store.ensure_default().unwrap();
        let home = tempfile::tempdir().unwrap();
        let f = home.path().join(".claude/projects/-w/s1.jsonl");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let line = format!(
            r#"{{"type":"user","uuid":"u1","sessionId":"s1","timestamp":"{}","message":{{"content":"hi"}}}}"#,
            crate::models::repositories::db::ts(Utc::now())
        );
        std::fs::write(&f, format!("{line}\n{{broken\n")).unwrap();
        let mut d = daemon_for(&store, home.path(), one_tick(false));
        let (_tx, rx) = sync_channel::<PathBuf>(1);
        d.run(&rx, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            store.recent("ingest", 1).unwrap()[0].message,
            "files=1 lines=2 malformed=1"
        );
    }

    #[test]
    fn follow_settings_reloads_interval_and_retention() {
        let (_d, store) = temp_store();
        let store = Arc::new(store);
        store.ensure_default().unwrap();
        store
            .save(&Settings {
                usage_interval_secs: 300,
                retention_days: 7,
                ..Settings::default()
            })
            .unwrap();
        let home = tempfile::tempdir().unwrap();
        let (_tx, rx) = sync_channel::<PathBuf>(1);
        let mut d = daemon_for(&store, home.path(), one_tick(true));
        d.run(&rx, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            (d.settings().usage_interval, d.settings().retention_days),
            (Duration::from_secs(300), 7)
        );
        let mut fixed = daemon_for(&store, home.path(), one_tick(false));
        fixed.run(&rx, &AtomicBool::new(false)).unwrap();
        assert_eq!(fixed.settings().usage_interval, Duration::from_millis(10));
    }
}
