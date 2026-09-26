//! 使用率カード。プロファイルごとの枠、残り時間、予測、用途の内訳、追加課金。
use crate::controllers::gui::app::GuiDeps;
use crate::models::domain::display::{
    Severity, fmt_clock, fmt_duration, fmt_percent, limit_help, limit_title, projection_text,
    severity,
};
use crate::models::domain::profile::Profile;
use crate::models::domain::projection::{WINDOW_MINUTES, project};
use crate::models::domain::read_models::LatestUsage;
use crate::models::domain::records::FetchResult;
use crate::models::domain::settings::Settings;
use crate::models::domain::usage::{LimitWindow, Spend};
use crate::models::ports::RepoError;
use chrono::{DateTime, Duration, Utc};

/// 枠1つの表示。
#[derive(Debug, Clone, PartialEq)]
pub struct LimitView {
    /// 見出し。
    pub title: String,
    /// 説明。
    pub help: &'static str,
    /// 進捗バーの割合（0〜1）。
    pub ratio: f32,
    /// 使用率の文言。
    pub percent: String,
    /// 色の段階。
    pub severity: Severity,
    /// 「残り1時間48分（14:00）」の形。
    pub reset: String,
    /// 予測。モデル別の週間枠は複数のモデルが同じ`kind`を持ち系列を分けられないため出さない。
    pub projection: Option<String>,
}

/// プロファイルのカード。
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileCard {
    /// 名前。
    pub name: String,
    /// 使用中か。
    pub is_active: bool,
    /// 取得時刻、または「N分前の値」。
    pub fetched: String,
    /// 取得の問題（期限切れ、未取得、通信エラー）。
    pub problem: Option<String>,
    /// 枠。
    pub limits: Vec<LimitView>,
    /// 用途の内訳（「Claude Code 97%」の形）。
    pub breakdown: Vec<String>,
    /// 追加課金（「$18.50 / $100.00」の形）。
    pub spend: Option<String>,
}

/// すべてのプロファイルのカードを作る。
pub fn cards(deps: &GuiDeps) -> Result<Vec<ProfileCard>, RepoError> {
    let now = deps.clock.now();
    let settings = deps.settings.load()?;
    deps.profiles
        .list()?
        .iter()
        .map(|p| card(deps, p, &settings, now))
        .collect()
}

fn card(
    deps: &GuiDeps,
    p: &Profile,
    settings: &Settings,
    now: DateTime<Utc>,
) -> Result<ProfileCard, RepoError> {
    let latest = deps.dashboard.latest_usage(p.id)?;
    let last_log = deps
        .logs
        .recent(&format!("usage:{}", p.id), 1)?
        .into_iter()
        .next();
    let problem = match (&latest, last_log) {
        (_, Some(l)) if l.result == FetchResult::Failed && l.http_status == Some(401) => {
            Some("トークン期限切れ。このプロファイルでclaudeを一度起動してください".to_string())
        }
        (_, Some(l)) if l.result == FetchResult::Failed => Some(l.message),
        (None, _) => {
            Some("まだ取得していません。デーモンが起動すると60秒以内に表示されます".to_string())
        }
        _ => None,
    };
    let limits = match &latest {
        Some(u) => u
            .limits
            .iter()
            .map(|l| limit_view(deps, p.id, l, now))
            .collect::<Result<Vec<_>, _>>()?,
        None => vec![],
    };
    Ok(ProfileCard {
        name: p.name.clone(),
        is_active: p.is_active,
        fetched: latest
            .as_ref()
            .map(|u| fetched_text(u, settings, deps, now))
            .unwrap_or_default(),
        problem,
        limits,
        breakdown: latest
            .as_ref()
            .map(|u| {
                u.breakdown
                    .iter()
                    .map(|b| format!("{} {}", b.display_name, fmt_percent(b.percent)))
                    .collect()
            })
            .unwrap_or_default(),
        spend: latest.and_then(|u| u.spend).map(|s| spend_text(&s)),
    })
}

/// 取得間隔の2倍より古い値には「N分前の値」と付ける（spec 10章）。
fn fetched_text(
    u: &LatestUsage,
    settings: &Settings,
    deps: &GuiDeps,
    now: DateTime<Utc>,
) -> String {
    let age = now - u.fetched_at;
    if age > Duration::seconds(2 * settings.usage_interval_secs as i64) {
        format!("{}前の値", fmt_duration(age))
    } else {
        format!("{} 取得", fmt_clock(u.fetched_at, now, deps.tz))
    }
}

fn limit_view(
    deps: &GuiDeps,
    profile_id: i64,
    l: &LimitWindow,
    now: DateTime<Utc>,
) -> Result<LimitView, RepoError> {
    let projection = if l.kind == "weekly_scoped" {
        None
    } else {
        let samples =
            deps.usage
                .samples(profile_id, &l.kind, now - Duration::minutes(WINDOW_MINUTES))?;
        Some(projection_text(
            &project(&samples, now, l.resets_at),
            now,
            deps.tz,
        ))
    };
    Ok(LimitView {
        title: limit_title(&l.kind, l.scope_label.as_deref()),
        help: limit_help(&l.kind),
        ratio: (l.percent / 100.0).clamp(0.0, 1.0) as f32,
        percent: fmt_percent(l.percent),
        severity: severity(&l.severity),
        reset: l
            .resets_at
            .map(|r| {
                format!(
                    "残り{}（{}）",
                    fmt_duration(r - now),
                    fmt_clock(r, now, deps.tz)
                )
            })
            .unwrap_or_else(|| "リセット時刻なし".to_string()),
        projection,
    })
}

fn spend_text(s: &Spend) -> String {
    let unit = 10f64.powi(s.exponent as i32);
    let used = s.used_minor as f64 / unit;
    match s.limit_minor {
        Some(l) => format!("${used:.2} / ${:.2}", l as f64 / unit),
        None => format!("${used:.2}（上限なし）"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::pricing::seed_models;
    use crate::models::domain::records::FetchLogEntry;
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{FetchLogRepo, ModelRepo, ProfileRepo, UsageRepo};
    use crate::models::repositories::db::SqliteStore;
    use crate::test_support::{FakeCreds, FakeDaemon, FixedClock, gui_deps, temp_store};
    use chrono::TimeZone;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    fn setup() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Arc<SqliteStore>,
        Arc<FixedClock>,
        GuiDeps,
    ) {
        let (d, s) = temp_store();
        let s = Arc::new(s);
        s.seed_if_empty(&seed_models()).unwrap();
        let home = tempfile::tempdir().unwrap();
        let clock = Arc::new(FixedClock::at(now()));
        let deps = gui_deps(
            s.clone(),
            clock.clone(),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        (d, home, s, clock, deps)
    }

    fn rising_usage(s: &SqliteStore) {
        let p = s.ensure_default().unwrap();
        let mut snap =
            parse_usage(include_str!("../../../../tests/fixtures/usage_ok.json")).unwrap();
        for (i, pct) in [10.0, 20.0, 30.0].iter().enumerate() {
            snap.limits[0].percent = *pct;
            snap.limits[0].resets_at = Some(now() + Duration::hours(2));
            s.record_snapshot(p.id, now() - Duration::minutes(20 - 10 * i as i64), &snap)
                .unwrap();
        }
    }

    #[test]
    fn successful_last_fetch_shows_no_problem() {
        let (_d, _h, s, _c, deps) = setup();
        rising_usage(&s);
        let id = s.ensure_default().unwrap().id;
        s.log(&FetchLogEntry {
            target: format!("usage:{id}"),
            at: now(),
            result: FetchResult::Ok,
            http_status: Some(200),
            message: "ok".into(),
        })
        .unwrap();
        assert_eq!(cards(&deps).unwrap()[0].problem, None);
    }

    #[test]
    fn value_is_marked_stale_only_after_twice_the_interval() {
        let (_d, _h, s, c, deps) = setup();
        rising_usage(&s);
        // 最新の取得は now()。既定の取得間隔は60秒なので、境界は120秒。
        let mut elapsed = 0;
        for (secs, stale) in [(100, false), (120, false), (121, true)] {
            c.advance(Duration::seconds(secs - elapsed));
            elapsed = secs;
            let fetched = cards(&deps).unwrap()[0].fetched.clone();
            assert_eq!(fetched.ends_with("前の値"), stale, "{secs}秒後: {fetched}");
        }
    }

    #[test]
    fn empty_db_shows_not_fetched() {
        let (_d, _h, s, _c, deps) = setup();
        s.ensure_default().unwrap();
        let cs = cards(&deps).unwrap();
        assert_eq!(cs.len(), 1);
        assert_eq!(
            cs[0].problem.as_deref(),
            Some("まだ取得していません。デーモンが起動すると60秒以内に表示されます")
        );
        assert!(cs[0].limits.is_empty());
    }

    #[test]
    fn limits_show_percent_remaining_and_projection() {
        let (_d, _h, s, _c, deps) = setup();
        rising_usage(&s);
        let card = &cards(&deps).unwrap()[0];
        assert_eq!(card.fetched, "12:00 取得");
        assert!(card.is_active);
        let l = &card.limits[0];
        assert_eq!(
            (l.title.as_str(), l.percent.as_str(), l.ratio),
            ("5時間枠", "30%", 0.3)
        );
        assert_eq!(l.reset, "残り2時間（14:00）");
        assert_eq!(l.projection.as_deref(), Some("このペースだと 13:10 に上限"));
        assert_eq!(card.limits[2].title, "週間枠（Fable）");
        assert_eq!(card.limits[2].projection, None);
        assert_eq!(card.limits[2].severity, Severity::Warning);
        assert_eq!(card.breakdown, ["Claude Code 97%", "Chats 3%"]);
        assert_eq!(card.spend.as_deref(), Some("$18.50 / $100.00"));
    }

    #[test]
    fn stale_value_and_401_are_explained() {
        let (_d, _h, s, clock, deps) = setup();
        rising_usage(&s);
        clock.advance(Duration::minutes(10));
        let p = s.ensure_default().unwrap();
        s.log(&FetchLogEntry {
            target: format!("usage:{}", p.id),
            at: now() + Duration::minutes(9),
            result: FetchResult::Failed,
            http_status: Some(401),
            message: "トークンが無効です".into(),
        })
        .unwrap();
        let card = &cards(&deps).unwrap()[0];
        assert_eq!(card.fetched, "10分前の値");
        assert_eq!(
            card.problem.as_deref(),
            Some("トークン期限切れ。このプロファイルでclaudeを一度起動してください")
        );
        s.log(&FetchLogEntry {
            target: format!("usage:{}", p.id),
            at: now() + Duration::minutes(10),
            result: FetchResult::Failed,
            http_status: Some(503),
            message: "使用量APIがHTTP 503を返しました".into(),
        })
        .unwrap();
        assert_eq!(
            cards(&deps).unwrap()[0].problem.as_deref(),
            Some("使用量APIがHTTP 503を返しました")
        );
    }

    #[test]
    fn spend_without_limit() {
        let s = Spend {
            used_minor: 1850,
            limit_minor: None,
            exponent: 2,
            currency: "USD".into(),
        };
        assert_eq!(spend_text(&s), "$18.50（上限なし）");
    }
}
