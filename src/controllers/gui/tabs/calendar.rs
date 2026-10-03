//! カレンダータブ。週のセッションを帯にし、日ごとに列を割り当てる。選択と詳細はダッシュボードと別に持つ。
use crate::controllers::gui::app::GuiDeps;
use crate::controllers::gui::dashboard::sessions::SessionDetail;
use crate::controllers::gui::dashboard::{DashboardState, selected_detail};
use crate::models::domain::calendar::{
    BUCKETS, DAY_MINUTES, Ranked, Segment, assign_lanes, buckets, local_midnight, project_label,
    rank_labels, segments, split_by_day, week_start,
};
use crate::models::domain::display::{fmt_duration, fmt_tokens};
use crate::models::domain::read_models::CalendarTurn;
use crate::models::domain::transcript::{SessionKind, one_line};
use crate::models::ports::RepoError;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Utc};
use std::collections::HashMap;

const WEEKDAYS: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];

/// 色分けの軸。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorBy {
    /// 作業ディレクトリの末尾の名前。
    #[default]
    Project,
    /// プロファイル。
    Profile,
}

impl ColorBy {
    /// 切り替えの文言。
    pub fn label(self) -> &'static str {
        match self {
            ColorBy::Project => "プロジェクト別",
            ColorBy::Profile => "プロファイル別",
        }
    }
}

/// カレンダーの状態。タブを移っても保つ。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalendarState {
    /// 今週から何週前か。
    pub weeks_back: u32,
    /// 色分けの軸。
    pub color_by: ColorBy,
    /// ヘッドレス実行も出すか。既定は隠す。大量に並行すると列が細くなり読めないため。
    pub show_headless: bool,
    /// 直前に表示した週の初日。時計で週が変わったことを知るために持つ。
    pub shown_week: Option<NaiveDate>,
    /// 選択、詳細の表示、再生の状態。ダッシュボードの選択と混ざらないよう別に持つ。
    pub sel: DashboardState,
}

/// カレンダーの操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarAction {
    /// 前の週。
    PrevWeek,
    /// 次の週。今週より先へは進まない。
    NextWeek,
    /// 今週。
    ThisWeek,
    /// 色分けの軸を変える。
    SetColorBy(ColorBy),
    /// ヘッドレス実行の表示を切り替える。
    ToggleHeadless,
}

/// 操作を状態に反映する。
pub fn handle(st: &mut CalendarState, a: CalendarAction) {
    match a {
        CalendarAction::PrevWeek => move_to(st, st.weeks_back + 1),
        CalendarAction::NextWeek => move_to(st, st.weeks_back.saturating_sub(1)),
        CalendarAction::ThisWeek => move_to(st, 0),
        CalendarAction::SetColorBy(c) => st.color_by = c,
        CalendarAction::ToggleHeadless => {
            st.show_headless = !st.show_headless;
            // 選んだヘッドレス実行の詳細が、帯の消えた後も右に残らないようにする。
            if !st.show_headless {
                clear_selection(st);
            }
        }
    }
}

/// 表示する週の初日を記録する。日曜0時を過ぎて表示する週が操作なしに変わったら、選択を外す。
pub fn sync_week(st: &mut CalendarState, today: NaiveDate) {
    let week = week_start(today, st.weeks_back);
    if st.shown_week.is_some_and(|w| w != week) {
        clear_selection(st);
    }
    st.shown_week = Some(week);
}

/// 選択を外し、再生も止める。詳細が消えた後も再生が進み、読み直しが続くのを防ぐため。
fn clear_selection(st: &mut CalendarState) {
    st.sel.selected = None;
    st.sel.replay.playing = false;
}

/// 週を移したら選択を外す。表示中の週に無いセッションの詳細が右に残ると、その週のものと見間違えるため。
fn move_to(st: &mut CalendarState, weeks_back: u32) {
    if st.weeks_back != weeks_back {
        st.weeks_back = weeks_back;
        clear_selection(st);
    }
}

/// 帯1本。
#[derive(Debug, Clone, PartialEq)]
pub struct BandVm {
    /// セッションID。
    pub session_id: String,
    /// 始まり（日の0時からの分）。
    pub start: u32,
    /// 終わり（日の0時からの分）。
    pub end: u32,
    /// 列。
    pub lane: usize,
    /// 列数。
    pub lanes: usize,
    /// 色の番号。`None`は「その他」。
    pub color: Option<usize>,
    /// マウスを載せたときの説明。1行目はセッションの見出し。
    pub tooltip: String,
}

/// 1日の列。
#[derive(Debug, Clone, PartialEq)]
pub struct DayVm {
    /// 見出し（「日 9/27」）。
    pub label: String,
    /// 今日か。
    pub today: bool,
    /// 10分ごとの濃さ（0.0〜1.0、144件）。
    pub density: Vec<f32>,
    /// 帯。
    pub bands: Vec<BandVm>,
}

/// 凡例1件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegendItem {
    /// ラベル。
    pub label: String,
    /// 色の番号。
    pub color: Option<usize>,
    /// セッション数。
    pub count: usize,
}

/// カレンダーのViewModel。
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarVm {
    /// 週の見出し。
    pub title: String,
    /// 週のセッション数。
    pub session_count: usize,
    /// 次の週へ進めるか。
    pub can_next: bool,
    /// 色分けの軸。
    pub color_by: ColorBy,
    /// ヘッドレス実行も出しているか。
    pub show_headless: bool,
    /// 週のヘッドレス実行のセッション数。隠していても数える。
    pub headless_count: usize,
    /// 7日分。
    pub days: Vec<DayVm>,
    /// 凡例。
    pub legend: Vec<LegendItem>,
    /// 今週なら、今の日と分。
    pub now: Option<(usize, u32)>,
    /// 選んだセッション。
    pub selected: Option<String>,
    /// 選んだセッションの詳細。
    pub detail: Option<SessionDetail>,
}

/// カレンダーのViewModelを作る。
pub fn build(
    deps: &GuiDeps,
    st: &CalendarState,
    now: DateTime<Utc>,
) -> Result<CalendarVm, RepoError> {
    let today = now.with_timezone(&deps.tz).date_naive();
    let start = week_start(today, st.weeks_back);
    let from = local_midnight(start, deps.tz);
    let all = deps
        .sessions
        .calendar_turns(from, local_midnight(start + Duration::days(7), deps.tz))?;
    let sessions: Vec<&[CalendarTurn]> =
        all.chunk_by(|a, b| a.session_id == b.session_id).collect();
    let headless_count = sessions
        .iter()
        .filter(|g| g[0].kind == SessionKind::Headless)
        .count();
    let groups: Vec<&[CalendarTurn]> = sessions
        .into_iter()
        .filter(|g| st.show_headless || g[0].kind != SessionKind::Headless)
        .collect();
    let label_of = |t: &CalendarTurn| match st.color_by {
        ColorBy::Project => project_label(t.cwd.as_deref()),
        ColorBy::Profile => t.profile_name.clone(),
    };
    let mut counts: HashMap<String, usize> = HashMap::new();
    for g in &groups {
        *counts.entry(label_of(&g[0])).or_default() += 1;
    }
    let ranked = rank_labels(&counts);
    let colors: HashMap<&str, Option<usize>> =
        ranked.iter().map(|r| (r.label.as_str(), r.color)).collect();
    let bands = week_bands(&groups, from, deps.tz, |t| colors[label_of(t).as_str()]);
    let ts: Vec<DateTime<Utc>> = groups.iter().flat_map(|g| g.iter().map(|t| t.ts)).collect();
    let this_week = st.weeks_back == 0;
    let end = start + Duration::days(6);
    Ok(CalendarVm {
        title: format!(
            "{}年{}/{}〜{}/{}",
            start.year(),
            start.month(),
            start.day(),
            end.month(),
            end.day()
        ),
        session_count: groups.len(),
        can_next: st.weeks_back > 0,
        color_by: st.color_by,
        show_headless: st.show_headless,
        headless_count,
        days: days(
            bands,
            &buckets(&ts, from),
            start,
            this_week.then_some(today),
        ),
        legend: legend(ranked),
        now: this_week.then(|| {
            let m = (now - from).num_minutes();
            ((m / DAY_MINUTES) as usize, (m % DAY_MINUTES) as u32)
        }),
        selected: st.sel.selected.clone(),
        detail: selected_detail(deps, &st.sel)?,
    })
}

/// 凡例。色のないラベルは「その他」の1行にまとめる。灰色の帯がどれか、凡例だけで決まるようにするため。
fn legend(ranked: Vec<Ranked>) -> Vec<LegendItem> {
    let (colored, rest): (Vec<Ranked>, Vec<Ranked>) =
        ranked.into_iter().partition(|r| r.color.is_some());
    let mut out: Vec<LegendItem> = colored
        .into_iter()
        .map(|r| LegendItem {
            label: r.label,
            color: r.color,
            count: r.count,
        })
        .collect();
    if !rest.is_empty() {
        out.push(LegendItem {
            label: "その他".into(),
            color: None,
            count: rest.iter().map(|r| r.count).sum(),
        });
    }
    out
}

/// セッションごとに帯を作り、日ごとに列を振る。
fn week_bands(
    groups: &[&[CalendarTurn]],
    from: DateTime<Utc>,
    tz: FixedOffset,
    color_of: impl Fn(&CalendarTurn) -> Option<usize>,
) -> Vec<Vec<BandVm>> {
    let mut bands: Vec<Vec<BandVm>> = vec![Vec::new(); 7];
    for g in groups {
        let pts: Vec<(DateTime<Utc>, u64)> = g.iter().map(|t| (t.ts, t.tokens)).collect();
        for seg in segments(&pts) {
            let tip = tooltip(&g[0], &seg, tz);
            for d in split_by_day(seg.start, seg.end, from) {
                bands[d.day].push(BandVm {
                    session_id: g[0].session_id.clone(),
                    start: d.start,
                    end: d.end,
                    lane: 0,
                    lanes: 1,
                    color: color_of(&g[0]),
                    tooltip: tip.clone(),
                });
            }
        }
    }
    for day in &mut bands {
        let spans: Vec<(u32, u32)> = day.iter().map(|b| (b.start, b.end)).collect();
        for (b, (lane, lanes)) in day.iter_mut().zip(assign_lanes(&spans)) {
            (b.lane, b.lanes) = (lane, lanes);
        }
    }
    bands
}

/// 日の列を作る。濃さは週の10分あたりの最大のターン数で割る。
fn days(
    bands: Vec<Vec<BandVm>>,
    counts: &[[u32; BUCKETS]],
    start: NaiveDate,
    today: Option<NaiveDate>,
) -> Vec<DayVm> {
    let max = counts.iter().flatten().copied().max().unwrap_or(0).max(1) as f32;
    bands
        .into_iter()
        .zip(counts)
        .enumerate()
        .map(|(i, (bands, c))| {
            let date = start + Duration::days(i as i64);
            DayVm {
                label: format!("{} {}/{}", WEEKDAYS[i], date.month(), date.day()),
                today: today == Some(date),
                density: c.iter().map(|&n| n as f32 / max).collect(),
                bands,
            }
        })
        .collect()
}

/// 帯の説明。1行目に見出しを置くのは、画面の部品名（アクセシビリティ）にも使うため。
fn tooltip(t: &CalendarTurn, seg: &Segment, tz: FixedOffset) -> String {
    let title = t
        .title
        .as_deref()
        .map(|s| one_line(s, 40))
        .unwrap_or_else(|| t.session_id.chars().take(8).collect());
    let (s, e) = (seg.start.with_timezone(&tz), seg.end.with_timezone(&tz));
    // 日をまたぐ帯は2つの列に分かれるため、終わりにも日付を付けて列の日付と食い違わないようにする。
    let end = if s.date_naive() == e.date_naive() {
        e.format("%H:%M").to_string()
    } else {
        e.format("%-m/%-d %H:%M").to_string()
    };
    let len = seg.end - seg.start;
    let len = if len.num_seconds() == 0 {
        String::new()
    } else {
        format!("（{}）", fmt_duration(len))
    };
    format!(
        "{title}\n{} · {}\n{}〜{end}{len}\nターン {} · Token {}",
        project_label(t.cwd.as_deref()),
        t.profile_name,
        s.format("%-m/%-d %H:%M"),
        seg.turns,
        fmt_tokens(seg.tokens)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::gui::dashboard::sessions::DetailTab;
    use crate::controllers::gui::dashboard::{DashAction, handle as dash_handle};
    use crate::models::domain::transcript::SessionKind;
    use crate::models::ports::ProfileRepo;
    use crate::models::repositories::db::SqliteStore;
    use crate::test_support::{
        FakeCreds, FakeDaemon, FixedClock, gui_deps, seed_session, seed_turn, temp_store, tokens,
    };
    use chrono::TimeZone;
    use std::sync::Arc;

    // 2026-09-26 03:00 UTC は土曜 12:00 JST。週は 9/20（日）から始まる。
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    fn setup() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Arc<SqliteStore>,
        GuiDeps,
    ) {
        let (d, s) = temp_store();
        let s = Arc::new(s);
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            s.clone(),
            Arc::new(FixedClock::at(now())),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        (d, home, s, deps)
    }

    #[test]
    fn empty_week_has_seven_days_and_no_bands() {
        let (_d, _h, _s, deps) = setup();
        let vm = build(&deps, &CalendarState::default(), deps.clock.now()).unwrap();
        assert_eq!(vm.days.len(), 7);
        assert_eq!(vm.days[0].label, "日 9/20");
        assert_eq!(vm.days[6].label, "土 9/26");
        assert!(vm.days[6].today && !vm.days[5].today);
        assert_eq!(vm.title, "2026年9/20〜9/26");
        assert_eq!((vm.session_count, vm.can_next), (0, false));
        assert_eq!(vm.now, Some((6, 720)));
        assert!(vm.days.iter().all(|d| d.bands.is_empty()
            && d.density.len() == 144
            && d.density.iter().all(|v| *v == 0.0)));
        assert!(vm.legend.is_empty() && vm.detail.is_none() && vm.selected.is_none());
        assert_eq!(vm.color_by, ColorBy::Project);
    }

    #[test]
    fn overlapping_sessions_of_two_projects_are_side_by_side() {
        let (_d, _h, s, deps) = setup();
        seed_session(&s, "a", SessionKind::Interactive, None, now());
        seed_session(&s, "b", SessionKind::Interactive, None, now());
        let t = now() - Duration::hours(1); // 11:00 JST
        seed_turn(&s, "a", "", "a1", t, None, "prompt", tokens(1, 1));
        seed_turn(
            &s,
            "a",
            "x",
            "a2",
            t + Duration::minutes(10),
            None,
            "text",
            tokens(1, 1),
        );
        seed_turn(
            &s,
            "a",
            "",
            "a3",
            t + Duration::minutes(40),
            None,
            "text",
            tokens(1, 1),
        );
        seed_turn(
            &s,
            "b",
            "",
            "b1",
            t + Duration::minutes(5),
            None,
            "prompt",
            tokens(2, 2),
        );
        let vm = build(&deps, &CalendarState::default(), deps.clock.now()).unwrap();
        let day = &vm.days[6];
        let a: Vec<_> = day.bands.iter().filter(|b| b.session_id == "a").collect();
        let b: Vec<_> = day.bands.iter().filter(|b| b.session_id == "b").collect();
        // aは10分の後に30分の空白があるので2本に分かれる。
        assert_eq!(a.len(), 2);
        assert_eq!((a[0].start, a[0].end), (660, 670));
        assert_eq!((a[1].start, a[1].end), (700, 705));
        assert_eq!((b[0].start, b[0].end), (665, 670));
        assert_eq!((a[0].lanes, b[0].lanes, a[1].lanes), (2, 2, 1));
        assert_ne!(a[0].lane, b[0].lane);
        assert_eq!((vm.session_count, vm.headless_count), (2, 0));
        assert_eq!(
            vm.legend,
            vec![
                LegendItem {
                    label: "a".into(),
                    color: Some(0),
                    count: 1
                },
                LegendItem {
                    label: "b".into(),
                    color: Some(1),
                    count: 1
                },
            ]
        );
        assert_eq!((a[0].color, b[0].color), (Some(0), Some(1)));
        assert_eq!(
            a[0].tooltip,
            "name-a\na · default\n9/26 11:00〜11:10（10分）\nターン 2 · Token 4"
        );
        // 11:00〜11:10の区画にaとbで2ターンあり、それが週の最大になる。
        assert_eq!(day.density[66], 1.0);
        assert!(day.density[70] > 0.0 && day.density[70] < 1.0);
    }

    #[test]
    fn color_by_profile_uses_profile_names() {
        let (_d, _h, s, deps) = setup();
        seed_session(&s, "a", SessionKind::Interactive, None, now());
        seed_turn(
            &s,
            "a",
            "",
            "a1",
            now() - Duration::hours(1),
            None,
            "prompt",
            tokens(1, 1),
        );
        let st = CalendarState {
            color_by: ColorBy::Profile,
            ..CalendarState::default()
        };
        let vm = build(&deps, &st, deps.clock.now()).unwrap();
        let profile = s.ensure_default().unwrap().name;
        assert_eq!(vm.legend[0].label, profile);
        assert_eq!(vm.color_by, ColorBy::Profile);
        let proj = build(&deps, &CalendarState::default(), deps.clock.now()).unwrap();
        assert_eq!(proj.legend[0].label, "a");
    }

    #[test]
    fn past_week_has_no_now_line_and_can_go_forward() {
        let (_d, _h, _s, deps) = setup();
        let st = CalendarState {
            weeks_back: 1,
            ..CalendarState::default()
        };
        let vm = build(&deps, &st, deps.clock.now()).unwrap();
        assert_eq!(vm.title, "2026年9/13〜9/19");
        assert!(vm.can_next && vm.now.is_none() && vm.days.iter().all(|d| !d.today));
    }

    #[test]
    fn untitled_session_uses_id_prefix_in_tooltip() {
        let (_d, _h, s, deps) = setup();
        seed_session(&s, "abcdefghijk", SessionKind::Interactive, None, now());
        s.with(|c| c.execute("UPDATE sessions SET name = NULL, first_prompt = NULL", []))
            .unwrap();
        seed_turn(
            &s,
            "abcdefghijk",
            "",
            "m",
            now() - Duration::hours(1),
            None,
            "prompt",
            tokens(1, 1),
        );
        let vm = build(&deps, &CalendarState::default(), deps.clock.now()).unwrap();
        assert!(vm.days[6].bands[0].tooltip.starts_with("abcdefgh\n"));
    }

    #[test]
    fn headless_sessions_are_hidden_until_toggled() {
        let (_d, _h, s, deps) = setup();
        seed_session(&s, "i", SessionKind::Interactive, None, now());
        seed_session(&s, "h", SessionKind::Headless, None, now());
        let t = now() - Duration::hours(1);
        seed_turn(&s, "i", "", "i1", t, None, "prompt", tokens(1, 1));
        seed_turn(
            &s,
            "h",
            "",
            "h1",
            t + Duration::minutes(30),
            None,
            "prompt",
            tokens(5, 5),
        );
        let hidden = build(&deps, &CalendarState::default(), deps.clock.now()).unwrap();
        assert_eq!((hidden.session_count, hidden.headless_count), (1, 1));
        assert!(!hidden.show_headless);
        assert!(hidden.days[6].bands.iter().all(|b| b.session_id == "i"));
        assert_eq!(hidden.legend.len(), 1);
        // 隠したセッションのターンは濃淡にも数えない。
        assert_eq!(hidden.days[6].density[69], 0.0);
        let st = CalendarState {
            show_headless: true,
            ..CalendarState::default()
        };
        let shown = build(&deps, &st, deps.clock.now()).unwrap();
        assert_eq!((shown.session_count, shown.headless_count), (2, 1));
        assert!(shown.show_headless);
        assert!(shown.days[6].bands.iter().any(|b| b.session_id == "h"));
        assert_eq!(shown.days[6].density[69], 1.0);
    }

    #[test]
    fn hiding_headless_clears_selection_but_showing_keeps_it() {
        let mut st = CalendarState::default();
        st.sel.selected = Some("a".into());
        handle(&mut st, CalendarAction::ToggleHeadless);
        assert_eq!(
            (st.show_headless, st.sel.selected.as_deref()),
            (true, Some("a"))
        );
        handle(&mut st, CalendarAction::ToggleHeadless);
        assert_eq!((st.show_headless, st.sel.selected.clone()), (false, None));
    }

    #[test]
    fn week_rollover_clears_selection_once() {
        let mut st = CalendarState::default();
        let sat = NaiveDate::from_ymd_opt(2026, 9, 26).unwrap();
        let sun = NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();
        sync_week(&mut st, sat);
        st.sel.selected = Some("a".into());
        sync_week(&mut st, sat);
        assert_eq!(st.sel.selected.as_deref(), Some("a"), "同じ週では外さない");
        sync_week(&mut st, sun);
        assert_eq!(st.sel.selected, None, "日曜0時に週が繰り上がったら外す");
        st.sel.selected = Some("b".into());
        st.weeks_back = 1;
        sync_week(&mut st, sun);
        assert_eq!(st.sel.selected, None, "表示する週が変わったら外す");
    }

    #[test]
    fn clearing_selection_stops_replay() {
        for a in [CalendarAction::PrevWeek, CalendarAction::ToggleHeadless] {
            let mut st = CalendarState {
                show_headless: true,
                ..CalendarState::default()
            };
            st.sel.selected = Some("a".into());
            st.sel.replay.playing = true;
            handle(&mut st, a);
            assert_eq!(
                (st.sel.selected.clone(), st.sel.replay.playing),
                (None, false)
            );
        }
    }

    #[test]
    fn labels_after_the_tenth_are_one_other_legend_item() {
        let (_d, _h, s, deps) = setup();
        let t = now() - Duration::hours(1);
        for i in 0..12 {
            let id = format!("p{i:02}");
            seed_session(&s, &id, SessionKind::Interactive, None, now());
            seed_turn(&s, &id, "", "m", t, None, "prompt", tokens(1, 1));
        }
        let vm = build(&deps, &CalendarState::default(), deps.clock.now()).unwrap();
        assert_eq!(vm.legend.len(), 11);
        assert_eq!(vm.legend[9].color, Some(9));
        assert_eq!(
            vm.legend[10],
            LegendItem {
                label: "その他".into(),
                color: None,
                count: 2
            }
        );
    }

    #[test]
    fn tooltip_of_a_span_over_midnight_shows_both_dates() {
        let (_d, _h, s, deps) = setup();
        seed_session(&s, "a", SessionKind::Interactive, None, now());
        // 金 9/25 23:50 JST と 土 9/26 0:10 JST。
        let t = Utc.with_ymd_and_hms(2026, 9, 25, 14, 50, 0).unwrap();
        seed_turn(&s, "a", "", "m1", t, None, "prompt", tokens(1, 1));
        seed_turn(
            &s,
            "a",
            "",
            "m2",
            t + Duration::minutes(10),
            None,
            "text",
            tokens(1, 1),
        );
        seed_turn(
            &s,
            "a",
            "",
            "m3",
            t + Duration::minutes(20),
            None,
            "text",
            tokens(1, 1),
        );
        let vm = build(&deps, &CalendarState::default(), deps.clock.now()).unwrap();
        let tip = &vm.days[6].bands[0].tooltip;
        assert_eq!(tip, &vm.days[5].bands[0].tooltip);
        assert!(tip.contains("9/25 23:50〜9/26 00:10（20分）"), "{tip}");
    }

    #[test]
    fn single_turn_tooltip_has_no_zero_duration() {
        let (_d, _h, s, deps) = setup();
        seed_session(&s, "a", SessionKind::Interactive, None, now());
        seed_turn(
            &s,
            "a",
            "",
            "m1",
            now() - Duration::hours(1),
            None,
            "prompt",
            tokens(1, 1),
        );
        let vm = build(&deps, &CalendarState::default(), deps.clock.now()).unwrap();
        assert!(
            vm.days[6].bands[0]
                .tooltip
                .contains("\n9/26 11:00〜11:00\n")
        );
    }

    #[test]
    fn local_date_decides_the_week_when_utc_is_still_the_day_before() {
        let (_d, _h, _s, deps) = setup();
        // 2026-09-26T15:30Z は 日 9/27 0:30 JST。
        let clock = Arc::new(FixedClock::at(
            Utc.with_ymd_and_hms(2026, 9, 26, 15, 30, 0).unwrap(),
        ));
        let deps = GuiDeps { clock, ..deps };
        let vm = build(&deps, &CalendarState::default(), deps.clock.now()).unwrap();
        assert_eq!(vm.title, "2026年9/27〜10/3");
        assert_eq!(vm.now, Some((0, 30)));
        assert!(vm.days[0].today);
    }

    #[test]
    fn build_uses_the_given_time_not_a_second_clock_read() {
        let (_d, _h, _s, deps) = setup();
        // 時計は土 9/26 12:00 JST のまま、渡す時刻だけを日 9/27 0:30 JST にする。
        let later = Utc.with_ymd_and_hms(2026, 9, 26, 15, 30, 0).unwrap();
        let vm = build(&deps, &CalendarState::default(), later).unwrap();
        assert_eq!(vm.title, "2026年9/27〜10/3");
        assert_eq!(vm.now, Some((0, 30)));
    }

    #[test]
    fn sub_second_span_tooltip_has_no_zero_duration() {
        let (_d, _h, s, deps) = setup();
        seed_session(&s, "a", SessionKind::Interactive, None, now());
        let t = now() - Duration::hours(1);
        seed_turn(&s, "a", "", "m1", t, None, "prompt", tokens(1, 1));
        seed_turn(
            &s,
            "a",
            "",
            "m2",
            t + Duration::milliseconds(500),
            None,
            "text",
            tokens(1, 1),
        );
        let vm = build(&deps, &CalendarState::default(), deps.clock.now()).unwrap();
        let tip = &vm.days[6].bands[0].tooltip;
        let end = (t + Duration::milliseconds(500)).with_timezone(&deps.tz);
        assert!(
            tip.contains(&format!("〜{}\nターン 2", end.format("%H:%M"))),
            "{tip}"
        );
    }

    #[test]
    fn selection_builds_detail() {
        let (_d, _h, s, deps) = setup();
        seed_session(&s, "a", SessionKind::Interactive, None, now());
        seed_turn(
            &s,
            "a",
            "",
            "a1",
            now() - Duration::hours(1),
            None,
            "prompt",
            tokens(1, 1),
        );
        let mut st = CalendarState::default();
        dash_handle(
            &mut st.sel,
            DashAction::Select("a".into(), DetailTab::Turns),
        );
        let vm = build(&deps, &st, deps.clock.now()).unwrap();
        assert_eq!(vm.selected.as_deref(), Some("a"));
        assert_eq!(vm.detail.unwrap().session_id, "a");
    }

    #[test]
    fn moving_weeks_clears_selection_and_never_goes_past_this_week() {
        let mut st = CalendarState::default();
        st.sel.selected = Some("a".into());
        handle(&mut st, CalendarAction::NextWeek);
        assert_eq!((st.weeks_back, st.sel.selected.as_deref()), (0, Some("a")));
        handle(&mut st, CalendarAction::ThisWeek);
        assert_eq!(st.sel.selected.as_deref(), Some("a"));
        handle(&mut st, CalendarAction::PrevWeek);
        assert_eq!((st.weeks_back, st.sel.selected.clone()), (1, None));
        st.sel.selected = Some("b".into());
        handle(&mut st, CalendarAction::PrevWeek);
        handle(&mut st, CalendarAction::NextWeek);
        assert_eq!((st.weeks_back, st.sel.selected.clone()), (1, None));
        st.sel.selected = Some("c".into());
        handle(&mut st, CalendarAction::ThisWeek);
        assert_eq!((st.weeks_back, st.sel.selected.clone()), (0, None));
        handle(&mut st, CalendarAction::SetColorBy(ColorBy::Profile));
        assert_eq!(st.color_by, ColorBy::Profile);
        assert_eq!(ColorBy::Project.label(), "プロジェクト別");
        assert_eq!(ColorBy::Profile.label(), "プロファイル別");
    }
}
