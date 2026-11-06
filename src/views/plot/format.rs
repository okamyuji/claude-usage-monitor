//! グラフの文字。x軸はUnix秒を持つ。
use chrono::{DateTime, FixedOffset};

/// x（Unix秒）を表示の時刻にする。表せない値は空にする。
pub fn plot_time(x: f64, tz: FixedOffset) -> String {
    DateTime::from_timestamp(x as i64, 0)
        .map(|t| t.with_timezone(&tz).format("%m/%d %H:%M").to_string())
        .unwrap_or_default()
}

/// マウスを乗せた点のツールチップ。
pub fn hover_label(name: &str, x: f64, y: f64, tz: FixedOffset, unit: &str) -> String {
    format!("{name}\n{}\n{y:.1}{unit}", plot_time(x, tz))
}

/// 軸の数字。刻みが0.01なら小数2桁のように、刻みの桁まで出す。
pub fn tick_label(value: f64, step: f64) -> String {
    let decimals = (-step.log10().round()).max(0.0) as usize;
    format!("{value:.decimals$}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone, Utc};

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
    fn hover_label_shows_series_time_and_value_rounded_to_one_decimal() {
        assert_eq!(
            hover_label("default", noon_jst(), 30.0, jst(), "%"),
            "default\n09/26 12:00\n30.0%"
        );
        assert_eq!(
            hover_label("RSS（MB）", noon_jst(), 12.34, jst(), "MB"),
            "RSS（MB）\n09/26 12:00\n12.3MB"
        );
    }

    #[test]
    fn tick_label_uses_as_many_decimals_as_the_step_needs() {
        assert_eq!(tick_label(20.0, 10.0), "20");
        assert_eq!(tick_label(2.9, 0.1), "2.9");
        assert_eq!(tick_label(2.86, 0.01), "2.86");
        assert_eq!(tick_label(300.0, 100.0), "300");
    }
}
