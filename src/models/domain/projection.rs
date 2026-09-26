//! 使用率の推移から、上限（100%）に届く時刻を予測する。
//!
//! 直近の傾きだけを使うのは、枠のリセットをまたいだ古い値で予測がずれるのを避けるため。
use chrono::{DateTime, Duration, Utc};

/// 予測に使う期間（分）。短すぎると揺れ、長すぎると直近の使い方の変化に追従しないため30分にする。
pub const WINDOW_MINUTES: i64 = 30;

/// ある時刻の使用率。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// 取得時刻。
    pub at: DateTime<Utc>,
    /// 使用率（0〜100）。
    pub percent: f64,
}

/// 予測結果。画面で理由ごとに文言を変えるため、失敗理由も区別して返す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// 期間内の点が3点未満、または時刻が1点に集まっている。
    NotEnoughData,
    /// 使用率が増えていない。
    NotIncreasing,
    /// この時刻に100%へ届く見込み。
    ReachesAt(DateTime<Utc>),
    /// リセットまでに100%へ届かない見込み。
    NotBeforeReset,
}

/// 増えているとみなす傾きの下限（1分あたりの%）。
pub const MIN_SLOPE_PER_MIN: f64 = 1e-6;

/// 最小二乗法で1分あたりの増加量を求め、最新の使用率から100%までの時間を出す。
pub fn project(
    samples: &[Sample],
    now: DateTime<Utc>,
    resets_at: Option<DateTime<Utc>>,
) -> Projection {
    let since = now - Duration::minutes(WINDOW_MINUTES);
    let pts: Vec<(f64, f64, DateTime<Utc>)> = samples
        .iter()
        .filter(|s| s.at >= since && s.at <= now)
        .map(|s| ((s.at - since).num_seconds() as f64 / 60.0, s.percent, s.at))
        .collect();
    if pts.len() < 3 {
        return Projection::NotEnoughData;
    }
    let n = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx == 0.0 {
        return Projection::NotEnoughData;
    }
    // Σ(x-x̄)(y-ȳ) は Σ(x-x̄)y と等しいため、yの平均は求めない。
    let slope = pts.iter().map(|p| (p.0 - mx) * p.1).sum::<f64>() / sxx;
    // 使用率が一定でも、浮動小数点の誤差で傾きがごく小さな正の値になる。1分あたり0.000001%未満は増えていないとみなす。
    if slope < MIN_SLOPE_PER_MIN {
        return Projection::NotIncreasing;
    }
    let latest = pts.iter().max_by_key(|p| p.2).expect("3点以上ある").1;
    if latest >= 100.0 {
        return Projection::ReachesAt(now);
    }
    // 傾きの下限があるので、到達までの秒数は最大でも100÷0.000001×60（約190年）に収まり、時刻の範囲を超えない。
    let at = now + Duration::seconds(((100.0 - latest) / slope * 60.0).round() as i64);
    match resets_at {
        Some(r) if at > r => Projection::NotBeforeReset,
        _ => Projection::ReachesAt(at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap()
    }

    fn linear(start_pct: f64, per_min: f64, minutes: &[i64]) -> Vec<Sample> {
        minutes
            .iter()
            .map(|m| Sample {
                at: now() - Duration::minutes(*m),
                percent: start_pct - per_min * *m as f64,
            })
            .collect()
    }

    #[test]
    fn fewer_than_three_samples_is_not_enough() {
        assert_eq!(
            project(&linear(50.0, 1.0, &[0, 5]), now(), None),
            Projection::NotEnoughData
        );
    }

    #[test]
    fn same_timestamp_samples_are_not_enough() {
        let s = vec![
            Sample {
                at: now(),
                percent: 1.0
            };
            3
        ];
        assert_eq!(project(&s, now(), None), Projection::NotEnoughData);
    }

    #[test]
    fn flat_or_falling_usage_is_not_increasing() {
        assert_eq!(
            project(&linear(50.0, 0.0, &[0, 5, 10]), now(), None),
            Projection::NotIncreasing
        );
        assert_eq!(
            project(&linear(50.0, -1.0, &[0, 5, 10]), now(), None),
            Projection::NotIncreasing
        );
    }

    #[test]
    fn linear_growth_projects_time_to_100() {
        let p = project(&linear(70.0, 1.0, &[0, 10, 20]), now(), None);
        assert_eq!(p, Projection::ReachesAt(now() + Duration::minutes(30)));
    }

    #[test]
    fn uneven_samples_use_least_squares_slope() {
        let s = vec![
            Sample {
                at: now() - Duration::minutes(20),
                percent: 50.0,
            },
            Sample {
                at: now() - Duration::minutes(15),
                percent: 60.0,
            },
            Sample {
                at: now(),
                percent: 90.0,
            },
        ];
        assert_eq!(
            project(&s, now(), None),
            Projection::ReachesAt(now() + Duration::minutes(5))
        );
    }

    #[test]
    fn projection_after_reset_is_reported() {
        let reset = now() + Duration::minutes(10);
        assert_eq!(
            project(&linear(70.0, 1.0, &[0, 10, 20]), now(), Some(reset)),
            Projection::NotBeforeReset
        );
    }

    #[test]
    fn projection_exactly_at_reset_counts_as_reaching() {
        let reset = now() + Duration::minutes(30);
        assert_eq!(
            project(&linear(70.0, 1.0, &[0, 10, 20]), now(), Some(reset)),
            Projection::ReachesAt(reset)
        );
    }

    #[test]
    fn already_full_reaches_now() {
        assert_eq!(
            project(&linear(100.0, 1.0, &[0, 10, 20]), now(), None),
            Projection::ReachesAt(now())
        );
    }

    #[test]
    fn samples_outside_window_are_ignored() {
        let mut s = linear(70.0, 1.0, &[0, 10]);
        s.push(Sample {
            at: now() - Duration::minutes(WINDOW_MINUTES + 1),
            percent: 0.0,
        });
        assert_eq!(project(&s, now(), None), Projection::NotEnoughData);
    }

    #[test]
    fn sample_exactly_at_window_start_is_used() {
        let s = linear(70.0, 1.0, &[0, 10, WINDOW_MINUTES]);
        assert_eq!(
            project(&s, now(), None),
            Projection::ReachesAt(now() + Duration::minutes(30))
        );
    }

    #[test]
    fn future_samples_are_ignored() {
        let mut s = linear(70.0, 1.0, &[0, 10]);
        s.push(Sample {
            at: now() + Duration::minutes(1),
            percent: 99.0,
        });
        assert_eq!(project(&s, now(), None), Projection::NotEnoughData);
    }

    #[test]
    fn unsorted_samples_use_latest_percent() {
        let mut s = linear(70.0, 1.0, &[20, 0, 10]);
        s.reverse();
        assert_eq!(
            project(&s, now(), None),
            Projection::ReachesAt(now() + Duration::minutes(30))
        );
    }

    #[test]
    fn near_zero_slope_from_float_noise_is_not_increasing() {
        // 同じ使用率でも取得時刻の間隔によっては、誤差で傾きがごく小さな正の値になる。
        assert_eq!(
            project(&linear(13.0, 1e-12, &[0, 5, 10]), now(), None),
            Projection::NotIncreasing
        );
    }

    #[test]
    fn slope_just_above_threshold_reaches_far_future_without_overflow() {
        assert!(matches!(
            project(&linear(13.0, 2e-6, &[0, 5, 10]), now(), None),
            Projection::ReachesAt(t) if t > now() + Duration::days(365)
        ));
    }

    #[test]
    fn tiny_but_real_slope_does_not_overflow() {
        assert_eq!(
            project(
                &linear(13.0, 1e-5, &[0, 5, 10]),
                now(),
                Some(now() + Duration::hours(5))
            ),
            Projection::NotBeforeReset
        );
    }
}
