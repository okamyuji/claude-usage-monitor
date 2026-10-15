//! 時系列グラフの共通部品。x軸はUnix秒を持つ。
use chrono::{DateTime, FixedOffset};
use egui_plot::HoverPosition;

/// x（Unix秒）を表示の時刻にする。表せない値は空にする。
pub fn plot_time(x: f64, tz: FixedOffset) -> String {
    DateTime::from_timestamp(x as i64, 0)
        .map(|t| t.with_timezone(&tz).format("%m/%d %H:%M").to_string())
        .unwrap_or_default()
}

/// マウスを乗せた点のツールチップ。線の頂点から離れているときは出さない。
pub fn hover_label(pos: &HoverPosition<'_>, tz: FixedOffset, unit: &str) -> Option<String> {
    let HoverPosition::NearDataPoint {
        plot_name,
        position,
        ..
    } = pos
    else {
        return None;
    };
    Some(format!(
        "{plot_name}\n{}\n{:.1}{unit}",
        plot_time(position.x, tz),
        position.y
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use egui_plot::PlotPoint;

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn noon_jst() -> f64 {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0)
            .unwrap()
            .timestamp() as f64
    }

    #[test]
    fn plot_time_shows_month_day_and_minute_in_the_given_zone() {
        assert_eq!(plot_time(noon_jst(), jst()), "09/26 12:00");
        assert_eq!(
            plot_time(noon_jst() - 3.0 * 3600.0 - 1.0, jst()),
            "09/26 08:59",
            "日付の境目でも時差を足す"
        );
        assert_eq!(
            plot_time(noon_jst(), FixedOffset::east_opt(0).unwrap()),
            "09/26 03:00"
        );
    }

    #[test]
    fn plot_time_is_empty_for_unrepresentable_x() {
        assert_eq!(plot_time(f64::MAX, jst()), "");
        assert_eq!(plot_time(f64::MIN, jst()), "");
    }

    #[test]
    fn hover_label_near_a_point_shows_series_time_and_value() {
        let pos = HoverPosition::NearDataPoint {
            plot_name: "default",
            position: PlotPoint::new(noon_jst(), 30.0),
            index: 2,
        };
        assert_eq!(
            hover_label(&pos, jst(), "%").as_deref(),
            Some("default\n09/26 12:00\n30.0%")
        );
    }

    #[test]
    fn hover_label_rounds_the_value_to_one_decimal() {
        let pos = HoverPosition::NearDataPoint {
            plot_name: "RSS（MB）",
            position: PlotPoint::new(noon_jst(), 12.34),
            index: 0,
        };
        assert_eq!(
            hover_label(&pos, jst(), "MB").as_deref(),
            Some("RSS（MB）\n09/26 12:00\n12.3MB")
        );
    }

    #[test]
    fn hover_label_is_none_away_from_points() {
        let pos = HoverPosition::Elsewhere {
            position: PlotPoint::new(noon_jst(), 50.0),
        };
        assert_eq!(hover_label(&pos, jst(), "%"), None);
    }
}
