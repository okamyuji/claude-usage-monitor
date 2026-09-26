//! 使用率が閾値を超えたときの通知。
//!
//! 同じ枠の同じリセット時刻では1回だけ通知する。取得のたびに同じ通知が出ると、利用者が通知そのものを無視するようになるため。

use crate::models::domain::display::{fmt_clock, fmt_percent, limit_title};
use crate::models::domain::read_models::LatestUsage;
use crate::models::domain::usage::LimitWindow;
use crate::models::ports::{Clock, DashboardRepo, Notifier, ProfileRepo, RepoError, SettingsRepo};
use chrono::{DateTime, FixedOffset, Utc};
use std::collections::HashSet;
use std::sync::Arc;

/// 通知の依存。
pub struct AlertDeps {
    /// プロファイル。
    pub profiles: Arc<dyn ProfileRepo>,
    /// 最新の使用率。
    pub dashboard: Arc<dyn DashboardRepo>,
    /// 閾値。
    pub settings: Arc<dyn SettingsRepo>,
    /// 通知の送り先。
    pub notifier: Arc<dyn Notifier>,
    /// 時計。
    pub clock: Arc<dyn Clock>,
    /// 通知文の時刻の時差。
    pub tz: FixedOffset,
}

/// 通知済みの枠の識別。プロファイル、枠の種類、モデル名、リセット時刻（分単位）の組。
type WindowKey = (i64, String, Option<String>, Option<String>);

/// 閾値を超えた枠を通知する。
pub struct Alerter {
    pub(crate) deps: AlertDeps,
    notified: HashSet<WindowKey>,
}

fn window_key(profile_id: i64, l: &LimitWindow) -> WindowKey {
    (
        profile_id,
        l.kind.clone(),
        l.scope_label.clone(),
        l.resets_at.map(|r| r.format("%Y-%m-%dT%H:%M").to_string()),
    )
}

impl Alerter {
    /// 依存から作る。
    pub fn new(deps: AlertDeps) -> Self {
        Self {
            deps,
            notified: HashSet::new(),
        }
    }

    /// 最新の使用率を見て、閾値を超えた未通知の枠を通知する。出した件数を返す。
    pub fn check(&mut self) -> Result<usize, RepoError> {
        let threshold = self.deps.settings.load()?.notify_threshold_percent;
        let now = self.deps.clock.now();
        let latest: Vec<(String, i64, LatestUsage)> = self
            .deps
            .profiles
            .list()?
            .into_iter()
            .filter_map(|p| Some((p.name, p.id, self.deps.dashboard.latest_usage(p.id).ok()??)))
            .collect();
        let mut sent = 0;
        for (name, id, usage) in &latest {
            for l in usage.limits.iter().filter(|l| l.percent >= threshold) {
                let key = window_key(*id, l);
                if self.notified.contains(&key) {
                    continue;
                }
                let (title, body) = message(name, l, &latest, now, self.deps.tz);
                if self.deps.notifier.notify(&title, &body).is_ok() {
                    self.notified.insert(key);
                    sent += 1;
                }
            }
        }
        // 過ぎたリセット時刻の記録を捨て、集合が常駐中に増え続けないようにする。
        let current: HashSet<WindowKey> = latest
            .iter()
            .flat_map(|(_, id, u)| u.limits.iter().map(|l| window_key(*id, l)))
            .collect();
        self.notified.retain(|k| current.contains(k));
        Ok(sent)
    }
}

/// 通知文。同じ種類の枠に余裕のある別プロファイルがあれば、切替の方法を添える。
fn message(
    name: &str,
    l: &LimitWindow,
    all: &[(String, i64, LatestUsage)],
    now: DateTime<Utc>,
    tz: FixedOffset,
) -> (String, String) {
    let title = format!(
        "{name}の{}が{}です",
        limit_title(&l.kind, l.scope_label.as_deref()),
        fmt_percent(l.percent)
    );
    let mut body = match l.resets_at {
        Some(r) => format!("リセットは{}です", fmt_clock(r, now, tz)),
        None => "リセット時刻は不明です".to_string(),
    };
    let other = all
        .iter()
        .filter(|(n, _, _)| n != name)
        .filter_map(|(n, _, u)| {
            u.limits
                .iter()
                .find(|x| x.kind == l.kind && x.scope_label == l.scope_label)
                .map(|x| (n, x.percent))
        })
        .filter(|(_, p)| *p < l.percent)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((n, p)) = other {
        body.push_str(&format!(
            "。{n}の{}は{}です。cumon profile use {n} で切り替えられます",
            limit_title(&l.kind, l.scope_label.as_deref()),
            fmt_percent(p)
        ));
    }
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::settings::Settings;
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{ProfileRepo, SettingsRepo, UsageRepo};
    use crate::test_support::{FixedClock, RecordingNotifier, temp_store};
    use chrono::{Duration, TimeZone, Utc};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    struct T {
        _d: tempfile::TempDir,
        store: Arc<crate::models::repositories::db::SqliteStore>,
        notes: Arc<RecordingNotifier>,
        clock: Arc<FixedClock>,
        alerter: Alerter,
    }

    fn setup() -> T {
        let (d, s) = temp_store();
        let store = Arc::new(s);
        store.ensure_default().unwrap();
        let notes = Arc::new(RecordingNotifier::default());
        let clock = Arc::new(FixedClock::at(now()));
        let alerter = Alerter::new(AlertDeps {
            profiles: store.clone(),
            dashboard: store.clone(),
            settings: store.clone(),
            notifier: notes.clone(),
            clock: clock.clone(),
            tz: FixedOffset::east_opt(9 * 3600).unwrap(),
        });
        T {
            _d: d,
            store,
            notes,
            clock,
            alerter,
        }
    }

    fn record(t: &T, profile: &str, session_pct: f64, reset: DateTime<Utc>) {
        let p = t
            .store
            .list()
            .unwrap()
            .into_iter()
            .find(|p| p.name == profile)
            .unwrap();
        let mut snap = parse_usage(include_str!("../../../tests/fixtures/usage_ok.json")).unwrap();
        snap.limits[0].percent = session_pct;
        snap.limits[0].resets_at = Some(reset);
        snap.limits.truncate(1);
        t.store.record_snapshot(p.id, t.clock.now(), &snap).unwrap();
    }

    #[test]
    fn below_threshold_is_silent() {
        let mut t = setup();
        record(&t, "default", 79.0, now() + Duration::hours(2));
        assert_eq!(t.alerter.check().unwrap(), 0);
        assert!(t.notes.0.lock().unwrap().is_empty());
    }

    #[test]
    fn notifies_once_per_window() {
        let mut t = setup();
        record(&t, "default", 85.0, now() + Duration::hours(2));
        assert_eq!(t.alerter.check().unwrap(), 1);
        t.clock.advance(Duration::minutes(1));
        record(
            &t,
            "default",
            90.0,
            now() + Duration::hours(2) + Duration::milliseconds(80),
        );
        assert_eq!(t.alerter.check().unwrap(), 0);
        let n = t.notes.0.lock().unwrap();
        assert_eq!(
            n[0],
            (
                "defaultの5時間枠が85%です".to_string(),
                "リセットは14:00です".to_string()
            )
        );
    }

    #[test]
    fn notifies_again_after_reset() {
        let mut t = setup();
        record(&t, "default", 85.0, now() + Duration::hours(1));
        t.alerter.check().unwrap();
        t.clock.advance(Duration::hours(2));
        record(&t, "default", 82.0, now() + Duration::hours(6));
        assert_eq!(t.alerter.check().unwrap(), 1);
    }

    #[test]
    fn suggests_profile_with_more_room() {
        let mut t = setup();
        t.store.add("sub", None).unwrap();
        record(&t, "sub", 20.0, now() + Duration::hours(3));
        record(&t, "default", 90.0, now() + Duration::hours(2));
        assert_eq!(t.alerter.check().unwrap(), 1);
        let body = t.notes.0.lock().unwrap()[0].1.clone();
        assert_eq!(
            body,
            "リセットは14:00です。subの5時間枠は20%です。cumon profile use sub で切り替えられます"
        );
    }

    fn window(kind: &str, scope: Option<&str>, percent: f64) -> LimitWindow {
        LimitWindow {
            kind: kind.into(),
            group: String::new(),
            percent,
            severity: "normal".into(),
            resets_at: Some(now() + Duration::hours(2)),
            scope_label: scope.map(Into::into),
        }
    }

    fn latest(limits: Vec<LimitWindow>) -> LatestUsage {
        LatestUsage {
            fetched_at: now(),
            limits,
            breakdown: vec![],
            spend: None,
        }
    }

    #[test]
    fn suggests_only_same_window_with_strictly_more_room() {
        let own = window("session", None, 90.0);
        let all = vec![
            ("default".to_string(), 1, latest(vec![own.clone()])),
            (
                "a".to_string(),
                2,
                latest(vec![window("weekly_all", None, 10.0)]),
            ),
            (
                "b".to_string(),
                3,
                latest(vec![window("session", Some("Fable"), 10.0)]),
            ),
            (
                "c".to_string(),
                4,
                latest(vec![window("session", None, 90.0)]),
            ),
        ];
        let tz = FixedOffset::east_opt(9 * 3600).unwrap();
        let (_, body) = message("default", &own, &all, now(), tz);
        assert_eq!(
            body, "リセットは14:00です",
            "種類かスコープが違う枠や、同じ使用率の相手は勧めない"
        );
    }

    #[test]
    fn threshold_follows_settings() {
        let mut t = setup();
        t.store
            .save(&Settings {
                notify_threshold_percent: 95.0,
                ..Settings::default()
            })
            .unwrap();
        record(&t, "default", 90.0, now() + Duration::hours(2));
        assert_eq!(t.alerter.check().unwrap(), 0);
        t.store
            .save(&Settings {
                notify_threshold_percent: 90.0,
                ..Settings::default()
            })
            .unwrap();
        assert_eq!(t.alerter.check().unwrap(), 1);
    }

    #[test]
    fn notifier_failure_is_not_fatal_and_retried() {
        struct Failing;
        impl Notifier for Failing {
            fn notify(&self, _t: &str, _b: &str) -> Result<(), String> {
                Err("通知を出せません".into())
            }
        }
        let mut t = setup();
        t.alerter.deps.notifier = Arc::new(Failing);
        record(&t, "default", 85.0, now() + Duration::hours(2));
        assert_eq!(t.alerter.check().unwrap(), 0);
        t.alerter.deps.notifier = t.notes.clone();
        assert_eq!(t.alerter.check().unwrap(), 1);
    }
}
