//! 上部の今日と今週の集計（spec 7.2節）。
use crate::controllers::gui::app::{GuiDeps, HeaderVm};
use crate::models::domain::display::{fmt_tokens, fmt_usd};
use crate::models::domain::pricing::{TokenUsage, sum_cost, total_tokens};
use crate::models::domain::read_models::GroupBy;
use crate::models::ports::RepoError;
use chrono::{DateTime, Duration, Utc};

/// 今日（現地の0時から）と、直近7日（6日前の0時から）の合計を作る。
pub fn build(deps: &GuiDeps) -> Result<HeaderVm, RepoError> {
    let local = deps.clock.now().with_timezone(&deps.tz);
    let midnight = local
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("0時は常に有効")
        .and_local_timezone(deps.tz)
        .single()
        .expect("固定の時差では1つに決まる")
        .with_timezone(&Utc);
    Ok(HeaderVm {
        today: total(deps, midnight)?,
        week: total(deps, midnight - Duration::days(6))?,
    })
}

/// `since`以降の合計を「1.26M $4.12」の形にする。コストはモデルごとの単価で出すため、モデル別の集計を使う。
fn total(deps: &GuiDeps, since: DateTime<Utc>) -> Result<String, RepoError> {
    let models = deps.models.all()?;
    let rows = deps.analytics.usage_by(GroupBy::Model, since)?;
    let pairs: Vec<(Option<&str>, TokenUsage)> =
        rows.iter().map(|r| (r.model.as_deref(), r.usage)).collect();
    let sum = pairs
        .iter()
        .fold(TokenUsage::default(), |a, (_, u)| a.plus(u));
    Ok(format!(
        "{} {}",
        fmt_tokens(total_tokens(&sum)),
        fmt_usd(sum_cost(&models, &pairs))
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::pricing::seed_models;
    use crate::models::domain::transcript::SessionKind;
    use crate::models::ports::ModelRepo;
    use crate::test_support::{
        FakeCreds, FakeDaemon, FixedClock, gui_deps, seed_session, seed_turn, temp_store, tokens,
    };
    use chrono::TimeZone;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn today_starts_at_local_midnight_and_week_covers_seven_days() {
        let now = Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap();
        let (_d, s) = temp_store();
        s.seed_if_empty(&seed_models()).unwrap();
        seed_session(&s, "s1", SessionKind::Interactive, None, now);
        let opus = Some("claude-opus-5-5");
        seed_turn(
            &s,
            "s1",
            "",
            "today",
            now - Duration::hours(1),
            opus,
            "text",
            tokens(1_000_000, 0),
        );
        seed_turn(
            &s,
            "s1",
            "",
            "yesterday23",
            now - Duration::hours(13),
            opus,
            "text",
            tokens(250_000, 0),
        );
        seed_turn(
            &s,
            "s1",
            "",
            "d3",
            now - Duration::days(3),
            opus,
            "text",
            tokens(500_000, 0),
        );
        seed_turn(
            &s,
            "s1",
            "",
            "d10",
            now - Duration::days(10),
            opus,
            "text",
            tokens(9_000_000, 0),
        );
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            Arc::new(s),
            Arc::new(FixedClock::at(now)),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        let h = build(&deps).unwrap();
        assert_eq!(h.today, "1.00M $4.00");
        assert_eq!(h.week, "1.75M $7.00");
    }
}
