//! カレンダーの帯の組み立て。週の範囲、空白での分割、日ごとの切り分け、列の割り当て、10分ごとの集計。
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};
use std::collections::HashMap;

/// これより長い空白で帯を切る（分）。
pub const GAP_MINUTES: i64 = 15;
/// 帯の最小の長さ（分）。1ターンだけの帯は長さが0になり見えないため。
pub const MIN_LEN_MINUTES: i64 = 5;
/// 濃淡の1区画（分）。
pub const BUCKET_MINUTES: i64 = 10;
/// 1日の分。
pub const DAY_MINUTES: i64 = 1440;
/// 1日の濃淡の区画数。
pub const BUCKETS: usize = (DAY_MINUTES / BUCKET_MINUTES) as usize;
/// 色を振るラベルの数。
pub const COLORS: usize = 10;
const WEEK_MINUTES: i64 = 7 * DAY_MINUTES;

/// `today`を含む週の日曜から、`weeks_back`週前の日曜。
pub fn week_start(today: NaiveDate, weeks_back: u32) -> NaiveDate {
    let sunday = today - Duration::days(i64::from(today.weekday().num_days_from_sunday()));
    sunday - Duration::weeks(i64::from(weeks_back))
}

/// 現地日付の0時をUTCにする。
pub fn local_midnight(d: NaiveDate, tz: FixedOffset) -> DateTime<Utc> {
    // FixedOffsetには夏時間がないので、現地時刻はいつも1つに定まる。
    tz.from_local_datetime(&d.and_time(NaiveTime::MIN))
        .unwrap()
        .with_timezone(&Utc)
}

/// 空白で区切った1区間。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// 最初のターン。
    pub start: DateTime<Utc>,
    /// 最後のターン。
    pub end: DateTime<Utc>,
    /// ターン数。
    pub turns: usize,
    /// Token数の合計。
    pub tokens: u64,
}

/// 時刻順の`(時刻, Token数)`を、`GAP_MINUTES`を超える空白で区切る。
pub fn segments(points: &[(DateTime<Utc>, u64)]) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for &(t, tokens) in points {
        match out.last_mut() {
            Some(s) if t - s.end <= Duration::minutes(GAP_MINUTES) => {
                s.end = t;
                s.turns += 1;
                s.tokens += tokens;
            }
            _ => out.push(Segment {
                start: t,
                end: t,
                turns: 1,
                tokens,
            }),
        }
    }
    out
}

/// 1日の中の区間。分は日の0時から数える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaySpan {
    /// 週の何日目か（日曜が0）。
    pub day: usize,
    /// 始まり（分）。
    pub start: u32,
    /// 終わり（分）。
    pub end: u32,
}

/// 区間を日ごとに切る。`from`は週の初日の現地0時。
/// 終わりは`MIN_LEN_MINUTES`以上に延ばし、週の外は捨てる。
/// 延ばした分だけが翌日へはみ出すときは、始まりを前へずらして始まりの日の中で長さを保つ。
/// 実際の終わりが翌日にあるときは始まりをずらさず、翌日側を延ばして合計の長さを保つ。
/// はみ出すとターンのない日に帯が出て見間違え、切り詰めると日の終わりの帯が見えなくなるため。
pub fn split_by_day(start: DateTime<Utc>, end: DateTime<Utc>, from: DateTime<Utc>) -> Vec<DaySpan> {
    let s = (start - from).num_minutes();
    let real_end = (end - from).num_minutes();
    let day_end = (s.div_euclid(DAY_MINUTES) + 1) * DAY_MINUTES;
    let s = if real_end <= day_end {
        s.min(day_end - MIN_LEN_MINUTES)
    } else {
        s
    };
    let e = real_end.max(s + MIN_LEN_MINUTES).min(WEEK_MINUTES);
    let mut cur = s.max(0);
    let mut out = Vec::new();
    while cur < e {
        let day = cur / DAY_MINUTES;
        let base = day * DAY_MINUTES;
        let stop = e.min(base + DAY_MINUTES);
        out.push(DaySpan {
            day: day as usize,
            start: (cur - base) as u32,
            end: (stop - base) as u32,
        });
        cur = stop;
    }
    out
}

/// 1日の帯に`(列, 列数)`を振る。開始順に並べ、最初に空いた列へ入れる。
/// 列数は、推移的に重なる帯の塊ごとに、その塊で使った列の数にする。
pub fn assign_lanes(spans: &[(u32, u32)]) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|&i| spans[i]);
    let mut out = vec![(0, 0); spans.len()];
    let mut cluster: Vec<usize> = Vec::new();
    let mut ends: Vec<u32> = Vec::new();
    let mut cluster_end = 0;
    for i in order {
        let (s, e) = spans[i];
        if s >= cluster_end {
            close(&cluster, ends.len(), &mut out);
            cluster.clear();
            ends.clear();
        }
        let lane = ends.iter().position(|&end| end <= s).unwrap_or(ends.len());
        if lane == ends.len() {
            ends.push(e);
        } else {
            ends[lane] = e;
        }
        out[i].0 = lane;
        cluster.push(i);
        cluster_end = cluster_end.max(e);
    }
    close(&cluster, ends.len(), &mut out);
    out
}

fn close(cluster: &[usize], lanes: usize, out: &mut [(usize, usize)]) {
    for &i in cluster {
        out[i].1 = lanes;
    }
}

/// 日ごとの10分あたりのターン数。週の外のターンは数えない。
pub fn buckets(ts: &[DateTime<Utc>], from: DateTime<Utc>) -> Vec<[u32; BUCKETS]> {
    let mut out = vec![[0u32; BUCKETS]; 7];
    for &t in ts {
        // 秒で比べる。分へ切り捨ててから比べると、0時の直前のターンが0分に入るため。
        let secs = (t - from).num_seconds();
        if !(0..WEEK_MINUTES * 60).contains(&secs) {
            continue;
        }
        let m = secs / 60;
        out[(m / DAY_MINUTES) as usize][((m % DAY_MINUTES) / BUCKET_MINUTES) as usize] += 1;
    }
    out
}

/// 順位を付けたラベル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranked {
    /// ラベル。
    pub label: String,
    /// 色の番号。上位`COLORS`件だけに振る。
    pub color: Option<usize>,
    /// セッション数。
    pub count: usize,
}

/// ラベルをセッション数の多い順、同数なら名前順に並べ、上位に色を振る。
/// 名前のハッシュで色を決めると、プロジェクトが10を超えたとき色が重なり凡例で見分けられないため。
pub fn rank_labels(counts: &HashMap<String, usize>) -> Vec<Ranked> {
    let mut v: Vec<(&String, &usize)> = counts.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    v.into_iter()
        .enumerate()
        .map(|(i, (label, &count))| Ranked {
            label: label.clone(),
            color: (i < COLORS).then_some(i),
            count,
        })
        .collect()
}

/// 作業ディレクトリの末尾の名前。凡例の幅に収めるため、パス全体は使わない。
pub fn project_label(cwd: Option<&str>) -> String {
    match cwd.filter(|c| !c.is_empty()) {
        None => "（なし）".into(),
        Some(c) => std::path::Path::new(c)
            .file_name()
            .map_or_else(|| c.to_string(), |n| n.to_string_lossy().into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }
    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }
    fn at(h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, h, mi, s).unwrap()
    }

    #[test]
    fn week_start_is_sunday_of_the_week() {
        assert_eq!(week_start(d(2026, 9, 27), 0), d(2026, 9, 27));
        assert_eq!(week_start(d(2026, 10, 3), 0), d(2026, 9, 27));
        assert_eq!(week_start(d(2026, 10, 3), 1), d(2026, 9, 20));
        assert_eq!(week_start(d(2026, 10, 3), 2), d(2026, 9, 13));
    }

    #[test]
    fn local_midnight_uses_the_offset() {
        assert_eq!(
            local_midnight(d(2026, 9, 27), jst()),
            Utc.with_ymd_and_hms(2026, 9, 26, 15, 0, 0).unwrap()
        );
    }

    #[test]
    fn segments_split_only_on_gaps_longer_than_15_minutes() {
        assert!(segments(&[]).is_empty());
        let one = segments(&[(at(1, 0, 0), 7)]);
        assert_eq!(
            one,
            vec![Segment {
                start: at(1, 0, 0),
                end: at(1, 0, 0),
                turns: 1,
                tokens: 7
            }]
        );
        let exact = segments(&[(at(1, 0, 0), 1), (at(1, 15, 0), 2)]);
        assert_eq!(exact.len(), 1);
        assert_eq!(
            (exact[0].end, exact[0].turns, exact[0].tokens),
            (at(1, 15, 0), 2, 3)
        );
        let over = segments(&[(at(1, 0, 0), 1), (at(1, 15, 1), 2)]);
        assert_eq!(over.len(), 2);
        assert_eq!(
            (over[1].start, over[1].turns, over[1].tokens),
            (at(1, 15, 1), 1, 2)
        );
    }

    #[test]
    fn split_by_day_cuts_at_midnight_and_extends_short_spans() {
        let from = at(0, 0, 0);
        let s = |h: i64, m: i64| from + Duration::minutes(h * 60 + m);
        assert_eq!(
            split_by_day(s(10, 0), s(11, 30), from),
            vec![DaySpan {
                day: 0,
                start: 600,
                end: 690
            }]
        );
        assert_eq!(
            split_by_day(s(10, 0), s(10, 0), from),
            vec![DaySpan {
                day: 0,
                start: 600,
                end: 605
            }]
        );
        assert_eq!(
            split_by_day(s(10, 0), s(10, 7), from),
            vec![DaySpan {
                day: 0,
                start: 600,
                end: 607
            }]
        );
        assert_eq!(
            split_by_day(s(23, 50), s(24, 20), from),
            vec![
                DaySpan {
                    day: 0,
                    start: 1430,
                    end: 1440
                },
                DaySpan {
                    day: 1,
                    start: 0,
                    end: 20
                }
            ]
        );
        assert_eq!(
            split_by_day(s(23, 0), s(24, 0), from),
            vec![DaySpan {
                day: 0,
                start: 1380,
                end: 1440
            }]
        );
    }

    #[test]
    fn short_span_is_not_extended_into_a_day_without_turns() {
        let from = at(0, 0, 0);
        let s = |h: i64, m: i64| from + Duration::minutes(h * 60 + m);
        assert_eq!(
            split_by_day(s(23, 58), s(23, 58), from),
            vec![DaySpan {
                day: 0,
                start: 1435,
                end: 1440
            }]
        );
        // 翌日にターンがあるときは始まりをずらさず、翌日側を延ばして合計5分にする。
        assert_eq!(
            split_by_day(s(23, 58), s(24, 1), from),
            vec![
                DaySpan {
                    day: 0,
                    start: 1438,
                    end: 1440
                },
                DaySpan {
                    day: 1,
                    start: 0,
                    end: 3
                }
            ]
        );
        // 実際の終わりがちょうど0時でも、翌日へははみ出さない。
        assert_eq!(
            split_by_day(s(23, 58), s(24, 0), from),
            vec![DaySpan {
                day: 0,
                start: 1435,
                end: 1440
            }]
        );
        assert_eq!(
            split_by_day(s(23, 54), s(23, 54), from),
            vec![DaySpan {
                day: 0,
                start: 1434,
                end: 1439
            }]
        );
        assert_eq!(
            split_by_day(s(23, 50), s(24, 1), from),
            vec![
                DaySpan {
                    day: 0,
                    start: 1430,
                    end: 1440
                },
                DaySpan {
                    day: 1,
                    start: 0,
                    end: 1
                }
            ]
        );
    }

    #[test]
    fn split_by_day_never_draws_outside_the_week() {
        let from = at(0, 0, 0);
        let last = from + Duration::minutes(7 * 1440 - 2);
        assert_eq!(
            split_by_day(last, last, from),
            vec![DaySpan {
                day: 6,
                start: 1435,
                end: 1440
            }]
        );
        assert!(split_by_day(from + Duration::days(7), from + Duration::days(8), from).is_empty());
        assert_eq!(
            split_by_day(
                from - Duration::minutes(30),
                from + Duration::minutes(10),
                from
            ),
            vec![DaySpan {
                day: 0,
                start: 0,
                end: 10
            }]
        );
    }

    #[test]
    fn assign_lanes_packs_overlaps_side_by_side() {
        assert!(assign_lanes(&[]).is_empty());
        assert_eq!(assign_lanes(&[(0, 10), (20, 30)]), vec![(0, 1), (0, 1)]);
        assert_eq!(assign_lanes(&[(0, 10), (10, 20)]), vec![(0, 1), (0, 1)]);
        assert_eq!(assign_lanes(&[(0, 30), (10, 20)]), vec![(0, 2), (1, 2)]);
        // AとB、BとCが重なり、AとCは重ならない。Cは空いた列0へ入り、塊の列数は2。
        assert_eq!(
            assign_lanes(&[(0, 20), (10, 40), (25, 50)]),
            vec![(0, 2), (1, 2), (0, 2)]
        );
        assert_eq!(assign_lanes(&[(10, 20), (0, 30)]), vec![(1, 2), (0, 2)]);
        assert_eq!(
            assign_lanes(&[(0, 60), (0, 60), (0, 60), (100, 110)]),
            vec![(0, 3), (1, 3), (2, 3), (0, 1)]
        );
    }

    #[test]
    fn buckets_count_turns_per_10_minutes_inside_the_week() {
        let from = at(0, 0, 0);
        let b = buckets(
            &[
                from,
                from + Duration::minutes(9),
                from + Duration::minutes(10),
                from + Duration::minutes(1440 + 25),
                from - Duration::seconds(1),
                from + Duration::days(7),
            ],
            from,
        );
        assert_eq!((b.len(), b[0].len()), (7, 144));
        assert_eq!((b[0][0], b[0][1], b[1][2]), (2, 1, 1));
        assert_eq!(b.iter().flatten().sum::<u32>(), 4);
        let last = buckets(&[from + Duration::days(7) - Duration::seconds(1)], from);
        assert_eq!(last[6][143], 1);
    }

    #[test]
    fn rank_labels_orders_by_count_then_name_and_caps_colors() {
        let mut m = HashMap::new();
        for i in 0..11 {
            m.insert(format!("p{i:02}"), 1);
        }
        m.insert("big".into(), 5);
        let r = rank_labels(&m);
        assert_eq!(
            r[0],
            Ranked {
                label: "big".into(),
                color: Some(0),
                count: 5
            }
        );
        assert_eq!(r[1].label, "p00");
        assert_eq!(r[2].label, "p01");
        assert_eq!(r[9].color, Some(9));
        assert_eq!(r[10].color, None);
        assert_eq!(r.len(), 12);
    }

    #[test]
    fn project_label_takes_the_last_directory() {
        assert_eq!(project_label(Some("/Users/a/devs/app")), "app");
        assert_eq!(project_label(Some("/Users/a/devs/app/")), "app");
        assert_eq!(project_label(Some("/")), "/");
        assert_eq!(project_label(Some("")), "（なし）");
        assert_eq!(project_label(None), "（なし）");
    }
}
