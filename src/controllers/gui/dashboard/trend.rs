//! 推移グラフ。5時間枠と週間枠の使用率の折れ線、リセット時刻の縦線、通知閾値の横線。
use crate::controllers::gui::app::GuiDeps;
use crate::models::domain::display::limit_help;
use crate::models::ports::RepoError;
use chrono::Duration;

/// 表示範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrendRange {
    /// 5時間。
    #[default]
    Hours5,
    /// 24時間。
    Hours24,
    /// 7日。
    Days7,
}

impl TrendRange {
    /// 文言。
    pub fn label(self) -> &'static str {
        match self {
            TrendRange::Hours5 => "5時間",
            TrendRange::Hours24 => "24時間",
            TrendRange::Days7 => "7日",
        }
    }

    /// 長さ。
    pub fn duration(self) -> Duration {
        match self {
            TrendRange::Hours5 => Duration::hours(5),
            TrendRange::Hours24 => Duration::hours(24),
            TrendRange::Days7 => Duration::days(7),
        }
    }
}

/// 折れ線1本（プロファイルごと）。
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// プロファイル名。
    pub name: String,
    /// 点（UNIX秒、使用率）。
    pub points: Vec<[f64; 2]>,
}

/// グラフ1つ。
#[derive(Debug, Clone, PartialEq)]
pub struct TrendPanel {
    /// 見出し。
    pub title: String,
    /// 説明。
    pub help: &'static str,
    /// 折れ線。
    pub series: Vec<Series>,
    /// リセット時刻（UNIX秒）。
    pub resets: Vec<f64>,
}

/// 推移のViewModel。
#[derive(Debug, Clone, PartialEq)]
pub struct TrendsVm {
    /// 範囲。
    pub range: TrendRange,
    /// 5時間枠と週間枠のグラフ。
    pub panels: Vec<TrendPanel>,
    /// 通知閾値（%）。
    pub threshold: f64,
    /// 横軸の時刻を現地時刻で出すための時差（秒）。
    pub tz_offset_secs: i32,
    /// 横軸の左端（UNIX秒）。
    pub x_min: f64,
    /// 横軸の右端（UNIX秒）。
    pub x_max: f64,
}

/// 推移を作る。
pub fn build(deps: &GuiDeps, range: TrendRange) -> Result<TrendsVm, RepoError> {
    let now = deps.clock.now();
    let since = now - range.duration();
    // 表示範囲の長さより先のリセット時刻（5時間の表示での週間枠のリセットなど）は描かない。
    // 横軸がその時刻まで伸び、範囲内の折れ線が端に潰れるため。
    let horizon = now + range.duration();
    let profiles = deps.profiles.list()?;
    let mut panels = vec![];
    for (kind, title) in [("session", "5時間枠"), ("weekly_all", "週間枠")] {
        let mut series = vec![];
        let mut resets = vec![];
        for p in &profiles {
            let points = deps
                .usage
                .samples(p.id, kind, since)?
                .into_iter()
                .map(|s| [s.at.timestamp() as f64, s.percent])
                .collect();
            series.push(Series {
                name: p.name.clone(),
                points,
            });
            resets.extend(
                deps.analytics
                    .reset_times(p.id, kind, since)?
                    .into_iter()
                    .filter(|t| *t <= horizon)
                    .map(|t| t.timestamp() as f64),
            );
        }
        resets.sort_by(f64::total_cmp);
        resets.dedup();
        panels.push(TrendPanel {
            title: title.into(),
            help: limit_help(kind),
            series,
            resets,
        });
    }
    let x_max = panels
        .iter()
        .flat_map(|p| p.resets.iter().copied())
        .fold(now.timestamp() as f64, f64::max);
    Ok(TrendsVm {
        range,
        panels,
        threshold: deps.settings.load()?.notify_threshold_percent,
        tz_offset_secs: deps.tz.local_minus_utc(),
        x_min: since.timestamp() as f64,
        x_max,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::settings::Settings;
    use crate::models::domain::usage::parse_usage;
    use crate::models::ports::{ProfileRepo, SettingsRepo, UsageRepo};
    use crate::test_support::{FakeCreds, FakeDaemon, FixedClock, gui_deps, temp_store};
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn panels_have_series_resets_and_threshold() {
        let now = Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap();
        let (_d, s) = temp_store();
        let p = s.ensure_default().unwrap();
        s.save(&Settings {
            notify_threshold_percent: 90.0,
            ..Settings::default()
        })
        .unwrap();
        let mut snap =
            parse_usage(include_str!("../../../../tests/fixtures/usage_ok.json")).unwrap();
        snap.limits[0].resets_at = Some(now + Duration::hours(1));
        s.record_snapshot(p.id, now - Duration::hours(1), &snap)
            .unwrap();
        s.record_snapshot(p.id, now - Duration::hours(30), &snap)
            .unwrap();
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            Arc::new(s),
            Arc::new(FixedClock::at(now)),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        let vm = build(&deps, TrendRange::Hours24).unwrap();
        assert_eq!(
            vm.panels
                .iter()
                .map(|p| p.title.as_str())
                .collect::<Vec<_>>(),
            ["5時間枠", "週間枠"]
        );
        let five = &vm.panels[0];
        assert_eq!(
            (five.series.len(), five.series[0].name.as_str()),
            (1, "default")
        );
        assert_eq!(
            five.series[0].points,
            [[(now - Duration::hours(1)).timestamp() as f64, 13.0]]
        );
        assert_eq!(five.resets, [(now + Duration::hours(1)).timestamp() as f64]);
        assert_eq!((vm.threshold, vm.tz_offset_secs), (90.0, 9 * 3600));
        let latest_reset = vm
            .panels
            .iter()
            .flat_map(|p| p.resets.iter().copied())
            .fold(f64::MIN, f64::max);
        assert_eq!(vm.x_min, (now - Duration::hours(24)).timestamp() as f64);
        assert_eq!(
            vm.x_max, latest_reset,
            "横軸の右端は、どのグラフのリセット時刻も入る位置にする"
        );
        assert!(vm.x_max > (now + Duration::hours(1)).timestamp() as f64);
        let short = build(&deps, TrendRange::Hours5).unwrap();
        assert!(
            short
                .panels
                .iter()
                .flat_map(|p| p.resets.iter())
                .all(|r| *r <= (now + Duration::hours(5)).timestamp() as f64),
            "5時間の表示では5時間より先のリセット時刻を描かない"
        );
        assert_eq!(
            short.panels[0].resets,
            [(now + Duration::hours(1)).timestamp() as f64]
        );
        assert!(short.x_max <= (now + Duration::hours(5)).timestamp() as f64);
        assert_eq!(
            build(&deps, TrendRange::Days7).unwrap().panels[0].series[0]
                .points
                .len(),
            2
        );
        assert_eq!(
            [
                TrendRange::Hours5.label(),
                TrendRange::Hours24.label(),
                TrendRange::Days7.label()
            ],
            ["5時間", "24時間", "7日"]
        );
    }
}
