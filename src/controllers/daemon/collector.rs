//! 使用量の周期取得。プロファイルごとに独立して取得し、1つの失敗で他を止めない。
use crate::models::domain::backoff::Backoff;
use crate::models::domain::profile::Profile;
use crate::models::domain::records::{FetchLogEntry, FetchResult};
use crate::models::domain::usage::UsageSnapshot;
use crate::models::ports::{
    Clock, CredentialError, CredentialStore, FetchLogRepo, ProfileRepo, RepoError, UsageApi,
    UsageApiError, UsageRepo,
};
use chrono::Duration;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// 収集に必要な依存。コンストラクタ引数が長くなるのを避けるため1つにまとめる。
pub struct CollectorDeps {
    /// プロファイル一覧。
    pub profiles: Arc<dyn ProfileRepo>,
    /// 認証情報。
    pub creds: Arc<dyn CredentialStore>,
    /// 使用量API。
    pub api: Arc<dyn UsageApi>,
    /// 保存先。
    pub usage: Arc<dyn UsageRepo>,
    /// 取得ログ。
    pub log: Arc<dyn FetchLogRepo>,
    /// 時計。
    pub clock: Arc<dyn Clock>,
}

/// 1周期の結果。テストと診断で取得の成否を数えるため。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    /// 成功数。
    pub ok: usize,
    /// 失敗数。
    pub failed: usize,
    /// バックオフ中で飛ばした数。
    pub skipped: usize,
}

enum CollectError {
    Credential(CredentialError),
    Expired,
    Api(UsageApiError),
}

impl CollectError {
    fn status(&self) -> Option<u16> {
        match self {
            CollectError::Api(UsageApiError::Unauthorized) | CollectError::Expired => Some(401),
            CollectError::Api(UsageApiError::RateLimited) => Some(429),
            CollectError::Api(UsageApiError::Http(s)) => Some(*s),
            _ => None,
        }
    }

    fn should_back_off(&self) -> bool {
        matches!(
            self,
            CollectError::Api(
                UsageApiError::RateLimited | UsageApiError::Transport(_) | UsageApiError::Http(_)
            )
        )
    }

    fn message(&self) -> String {
        match self {
            CollectError::Credential(e) => e.to_string(),
            CollectError::Expired => {
                "トークンの期限が切れています。このプロファイルでclaudeを一度起動してください"
                    .into()
            }
            CollectError::Api(e) => e.to_string(),
        }
    }
}

/// 使用量の収集役。
pub struct UsageCollector {
    deps: CollectorDeps,
    home: PathBuf,
    backoffs: HashMap<i64, Backoff>,
}

impl UsageCollector {
    /// 依存とホームディレクトリを受け取って作る。ホームは既定プロファイルの`~/.claude`を解決するのに使う。
    pub fn new(deps: CollectorDeps, home: PathBuf) -> Self {
        Self {
            deps,
            home,
            backoffs: HashMap::new(),
        }
    }

    /// 全プロファイルを1回ずつ取得する。DBへの書き込み失敗だけをエラーとして返す。
    pub fn tick(&mut self) -> Result<TickReport, RepoError> {
        let now = self.deps.clock.now();
        let mut report = TickReport::default();
        for p in self.deps.profiles.list()? {
            let backoff = self
                .backoffs
                .entry(p.id)
                .or_insert_with(|| Backoff::new(Duration::seconds(60), Duration::seconds(600)));
            if !backoff.ready(now) {
                report.skipped += 1;
                continue;
            }
            let target = format!("usage:{}", p.id);
            let (result, status, message) = match fetch_one(&self.deps, &self.home, &p) {
                Ok(snap) => {
                    self.deps.usage.record_snapshot(p.id, now, &snap)?;
                    backoff.on_success();
                    report.ok += 1;
                    (FetchResult::Ok, Some(200), String::new())
                }
                Err(e) => {
                    if e.should_back_off() {
                        backoff.on_failure(now);
                    }
                    report.failed += 1;
                    (FetchResult::Failed, e.status(), e.message())
                }
            };
            self.deps.log.log(&FetchLogEntry {
                target,
                at: now,
                result,
                http_status: status,
                message,
            })?;
        }
        Ok(report)
    }
}

fn fetch_one(
    deps: &CollectorDeps,
    home: &std::path::Path,
    p: &Profile,
) -> Result<UsageSnapshot, CollectError> {
    let cred = deps
        .creds
        .load(p, &p.resolved_config_dir(home))
        .map_err(CollectError::Credential)?;
    if cred.expires_at.is_some_and(|t| t <= deps.clock.now()) {
        return Err(CollectError::Expired);
    }
    deps.api
        .fetch(&cred.access_token)
        .map_err(CollectError::Api)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{
        Credential, CredentialError, FetchLogRepo, ProfileRepo, UsageApiError, UsageRepo,
    };
    use crate::test_support::{FakeApi, FakeCreds, FixedClock, temp_store};
    use chrono::{Duration, TimeZone, Utc};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn cred(tok: &str, expires_in_min: i64) -> Credential {
        Credential {
            access_token: tok.into(),
            expires_at: Some(
                Utc.with_ymd_and_hms(2026, 9, 26, 0, 0, 0).unwrap()
                    + Duration::minutes(expires_in_min),
            ),
            subscription_type: None,
        }
    }

    struct Env {
        _d: tempfile::TempDir,
        store: Arc<crate::models::repositories::db::SqliteStore>,
        clock: Arc<FixedClock>,
        api: Arc<FakeApi>,
        collector: UsageCollector,
    }

    fn env(
        api: HashMap<String, Result<crate::models::domain::usage::UsageSnapshot, UsageApiError>>,
        creds: HashMap<String, Result<Credential, CredentialError>>,
    ) -> Env {
        let (d, store) = temp_store();
        let store = Arc::new(store);
        store.ensure_default().unwrap();
        store.add("sub", None).unwrap();
        let clock = Arc::new(FixedClock::at(
            Utc.with_ymd_and_hms(2026, 9, 26, 0, 0, 0).unwrap(),
        ));
        let api = Arc::new(FakeApi {
            responses: api,
            calls: Mutex::new(vec![]),
        });
        let collector = UsageCollector::new(
            CollectorDeps {
                profiles: store.clone(),
                creds: Arc::new(FakeCreds(creds)),
                api: api.clone(),
                usage: store.clone(),
                log: store.clone(),
                clock: clock.clone(),
            },
            "/home".into(),
        );
        Env {
            _d: d,
            store,
            clock,
            api,
            collector,
        }
    }

    fn ok() -> crate::models::domain::usage::UsageSnapshot {
        parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap()
    }

    #[test]
    fn one_profile_failure_does_not_stop_others() {
        let mut e = env(
            HashMap::from([
                ("a".into(), Ok(ok())),
                ("b".into(), Err(UsageApiError::Unauthorized)),
            ]),
            HashMap::from([
                ("default".into(), Ok(cred("a", 60))),
                ("sub".into(), Ok(cred("b", 60))),
            ]),
        );
        let r = e.collector.tick().unwrap();
        assert_eq!((r.ok, r.failed, r.skipped), (1, 1, 0));
        let profiles = e.store.list().unwrap();
        let default = profiles.iter().find(|p| p.name == "default").unwrap();
        let sub = profiles.iter().find(|p| p.name == "sub").unwrap();
        assert_eq!(
            e.store
                .samples(default.id, "session", e.clock.now() - Duration::hours(1))
                .unwrap()
                .len(),
            1
        );
        let log = e.store.recent(&format!("usage:{}", sub.id), 1).unwrap();
        assert_eq!(log[0].http_status, Some(401));
        assert!(!log[0].message.contains("Bearer"));
    }

    #[test]
    fn rate_limit_backs_off_but_unauthorized_does_not() {
        let mut e = env(
            HashMap::from([
                ("a".into(), Err(UsageApiError::RateLimited)),
                ("b".into(), Err(UsageApiError::Unauthorized)),
            ]),
            HashMap::from([
                ("default".into(), Ok(cred("a", 60))),
                ("sub".into(), Ok(cred("b", 60))),
            ]),
        );
        e.collector.tick().unwrap();
        let default_id = e
            .store
            .list()
            .unwrap()
            .into_iter()
            .find(|p| p.name == "default")
            .unwrap()
            .id;
        assert_eq!(
            e.store.recent(&format!("usage:{default_id}"), 1).unwrap()[0].http_status,
            Some(429)
        );
        e.clock.advance(Duration::seconds(30));
        let r = e.collector.tick().unwrap();
        assert_eq!((r.skipped, r.failed), (1, 1));
        e.clock.advance(Duration::seconds(30));
        assert_eq!(e.collector.tick().unwrap().skipped, 0);
    }

    #[test]
    fn expired_or_missing_credentials_skip_api_call() {
        let mut e = env(
            HashMap::new(),
            HashMap::from([("default".into(), Ok(cred("a", -1)))]),
        );
        let r = e.collector.tick().unwrap();
        assert_eq!(r.failed, 2);
        assert!(e.api.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn transport_errors_back_off() {
        let mut e = env(
            HashMap::from([("a".into(), Err(UsageApiError::Transport("x".into())))]),
            HashMap::from([("default".into(), Ok(cred("a", 60)))]),
        );
        e.collector.tick().unwrap();
        e.clock.advance(Duration::seconds(10));
        assert_eq!(e.collector.tick().unwrap().skipped, 1);
    }
}
