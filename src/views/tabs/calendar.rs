//! カレンダータブの描画。上に週の操作と凡例、左に週間カレンダー、右に選んだセッションの詳細。
use crate::controllers::gui::app::{Action, Forms};
use crate::controllers::gui::tabs::calendar::{BandVm, CalendarAction, CalendarVm, ColorBy};
use crate::views::app::Acts;
use crate::views::dashboard::detail::{self, DetailUi};
use crate::views::dashboard::{calendar_ratio, split};
use crate::views::icons;
use crate::views::layout::GAP;
use crate::views::probe::Role;
use crate::views::theme::{BODY, GAP_X, GAP_Y, Palette, SMALL, color, series, with_alpha};
use crate::views::widgets::{
    button, checkbox, disabled_button, section, selectable, tagged, text, tip, weak,
};
use gpui::{
    Bounds, Div, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, TextAlign, TextRun, canvas, div,
    fill, font, point, px, size,
};

/// 1時間の高さ。
const HOUR_PX: f32 = 40.0;
/// 左端の時刻の目盛りの幅。
const AXIS_PX: f32 = 44.0;
/// 日の列の左端に置く濃淡の幅。
const DENSITY_PX: f32 = 6.0;
/// 曜日の見出しの高さ。
const HEAD_PX: f32 = 22.0;
/// 時刻の目盛りの文字の大きさ（11）。
const AXIS_FONT: f32 = 11.0;
/// 帯の塗りの濃さ。乗ったときは濃くする。
const BAND_FILL: f32 = 0.35;
const BAND_FILL_HOVER: f32 = 0.55;

/// カレンダータブを描く。`width`は本文の幅。
pub fn render(
    vm: &CalendarVm,
    forms: &Forms,
    (scroll, detail_ui): (&ScrollHandle, &DetailUi),
    p: &'static Palette,
    acts: &Acts,
    width: f32,
) -> Div {
    let right = match &vm.detail {
        Some(d) => div()
            .size_full()
            .flex()
            .flex_col()
            .gap(GAP_Y)
            .children(vm.band_label.as_deref().map(|l| text(l, BODY, p.accent)))
            .child(detail::render(Some(d), forms, detail_ui, p, acts))
            .into_any_element(),
        None => weak(p, "帯を押すと詳細を出します").into_any_element(),
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(toolbar(vm, p, acts))
        .child(legend(vm, p))
        .child(div().h(px(GAP / 2.0 - f32::from(GAP_Y)).max(px(0.0))))
        .child(
            split(
                p,
                acts,
                (forms.calendar_split, calendar_ratio),
                width,
                grid(vm, scroll, p, acts),
                right,
            )
            .min_h_0(),
        )
}

fn vline(p: &Palette) -> Div {
    div().w(px(1.0)).h(px(16.0)).bg(color(p.border))
}

fn toolbar(vm: &CalendarVm, p: &'static Palette, acts: &Acts) -> Div {
    let cal = |a| Action::Calendar(a);
    let next_label = format!("次週 {}", icons::CARET_RIGHT);
    // 今週では次週を押せない。
    let next = if vm.can_next {
        button(
            p,
            "cal-next",
            &next_label,
            acts.on(cal(CalendarAction::NextWeek)),
        )
        .into_any_element()
    } else {
        disabled_button(p, "cal-next", &next_label).into_any_element()
    };
    let colors = [ColorBy::Project, ColorBy::Profile].map(|c| {
        selectable(
            p,
            SharedString::from(format!("color-by:{}", c.label())),
            vm.color_by == c,
            c.label(),
            acts.on(cal(CalendarAction::SetColorBy(c))),
        )
    });
    div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(button(
            p,
            "cal-this",
            "今週へ",
            acts.on(cal(CalendarAction::ThisWeek)),
        ))
        .child(button(
            p,
            "cal-prev",
            &format!("{} 前週", icons::CARET_LEFT),
            acts.on(cal(CalendarAction::PrevWeek)),
        ))
        .child(next)
        .child(section(p, &vm.title))
        .child(weak(p, &format!("{} セッション", vm.session_count)))
        // 隠したヘッドレスがある週は空ではないので、件数の合計で判定する。
        .children(
            (vm.session_count + vm.headless_count == 0)
                .then(|| weak(p, "この週の記録はありません")),
        )
        .child(vline(p))
        .child(weak(p, "色分け"))
        .children(colors)
        .child(vline(p))
        .child(checkbox(
            p,
            "cal-headless",
            vm.show_headless,
            &format!("ヘッドレスも出す（{}件）", vm.headless_count),
            acts.on(cal(CalendarAction::ToggleHeadless)),
        ))
}

fn legend(vm: &CalendarVm, p: &'static Palette) -> Div {
    let items = vm.legend.iter().map(|item| {
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .mr(px(6.0))
            .child(
                div()
                    .size(px(10.0))
                    .rounded(px(2.0))
                    .bg(color(series(item.color, p))),
            )
            .child(text(
                &format!("{} {}", item.label, item.count),
                BODY,
                p.text,
            ))
    });
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(GAP_X)
        .children(items)
}

fn grid(vm: &CalendarVm, scroll: &ScrollHandle, p: &'static Palette, acts: &Acts) -> Div {
    // 列の幅は描画のときの枠の幅で決まる。見出しと帯は割合で置き、canvasは枠から求める。
    let head =
        div()
            .relative()
            .h(px(HEAD_PX))
            .ml(px(AXIS_PX))
            .children(vm.days.iter().enumerate().map(|(i, d)| {
                div()
                    .absolute()
                    .top_0()
                    .left(gpui::relative(i as f32 / 7.0))
                    .w(gpui::relative(1.0 / 7.0))
                    .h(px(HEAD_PX))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(text(
                        &d.label,
                        SMALL,
                        if d.today { p.accent } else { p.text },
                    ))
            }));
    let paint = {
        let days = vm.days.clone();
        let now = vm.now;
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| paint_grid(bounds, &days, now, p, window, cx),
        )
        .absolute()
        .size_full()
    };
    let bands = vm.days.iter().enumerate().flat_map(|(i, d)| {
        d.bands
            .iter()
            .map(move |b| band(b, i, p, acts).into_any_element())
    });
    let body = div()
        .relative()
        .h(px(HOUR_PX * 24.0))
        .w_full()
        .child(paint)
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(AXIS_PX))
                .right_0()
                .children(bands),
        );
    div().size_full().flex().flex_col().child(head).child(
        div()
            .id("calendar-grid")
            .flex_1()
            .min_h_0()
            .track_scroll(scroll)
            .overflow_y_scroll()
            .child(body),
    )
}

/// 時刻の目盛り、時と日の線、今日の塗り、濃淡、今の時刻の線を描く。
fn paint_grid(
    bounds: Bounds<Pixels>,
    days: &[crate::controllers::gui::tabs::calendar::DayVm],
    now: Option<(usize, u32)>,
    p: &Palette,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    let left = f32::from(bounds.left()) + AXIS_PX;
    let top = f32::from(bounds.top());
    let col = ((f32::from(bounds.size.width) - AXIS_PX) / 7.0).max(1.0);
    let x_of = |i: usize| left + col * i as f32;
    let y_of = |min: u32| top + min as f32 / 60.0 * HOUR_PX;
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
        Bounds::new(point(px(x0), px(y0)), size(px(x1 - x0), px(y1 - y0)))
    };
    let bottom = top + HOUR_PX * 24.0;
    for (i, d) in days.iter().enumerate() {
        if d.today {
            window.paint_quad(fill(
                rect(x_of(i), top, x_of(i + 1), bottom),
                color(p.accent_soft),
            ));
        }
    }
    let line = color(p.border);
    for h in 0..24u32 {
        let y = y_of(h * 60);
        // 0.5pxの線は、画面の1pxに丸めて描かれる。
        window.paint_quad(fill(
            rect(left, y, f32::from(bounds.right()), y + 0.5),
            line,
        ));
        if h > 0 {
            let t = format!("{h}:00");
            let run = TextRun {
                len: t.len(),
                font: font(crate::views::fonts::BODY_FAMILY),
                color: color(p.weak).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window
                .text_system()
                .shape_line(t.into(), px(AXIS_FONT), &[run], None);
            let x = left - 4.0 - f32::from(shaped.width);
            let y = y - AXIS_FONT * 0.65;
            let _ = shaped.paint(
                point(px(x), px(y)),
                px(AXIS_FONT * 1.3),
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }
    for i in 0..=7 {
        window.paint_quad(fill(rect(x_of(i), top, x_of(i) + 0.5, bottom), line));
    }
    for (i, d) in days.iter().enumerate() {
        for (b, v) in d.density.iter().enumerate() {
            if *v > 0.0 {
                let y = y_of(b as u32 * 10);
                // 1ターンだけの区画も見えるよう、薄さに下限を置く。
                window.paint_quad(fill(
                    rect(x_of(i), y, x_of(i) + DENSITY_PX, y + HOUR_PX / 6.0),
                    with_alpha(p.ok, v.max(0.15)),
                ));
            }
        }
    }
    if let Some((day, min)) = now {
        let y = y_of(min);
        window.paint_quad(fill(
            rect(x_of(day), y - 0.75, x_of(day + 1), y + 0.75),
            color(p.err),
        ));
    }
}

/// 帯の外側（割合）と内側（ピクセル）の置き方。
/// 式は`left = x + DENSITY + 1 + lane_w * lane`、`width = lane_w - 1`、`lane_w = (col - DENSITY - 2) / lanes`で、
/// 列の幅に比例する部分（外側の割合）と、ピクセルの部分（内側の左右の寄せ）に分ける。
/// 戻り値は（外側の左端と幅の割合、内側の左と右の寄せ）。
pub fn band_place(day: usize, lane: usize, lanes: usize) -> ((f32, f32), (f32, f32)) {
    let lanes = lanes.max(1) as f32;
    let lane = lane as f32;
    let col = 1.0 / 7.0;
    let outer = (col * day as f32 + col * lane / lanes, col / lanes);
    let shrink = (DENSITY_PX + 2.0) / lanes;
    let left = DENSITY_PX + 1.0 - lane * shrink;
    // 内側の幅は 外側の幅 - shrink - 1 になる。右の寄せは残り。
    let right = shrink + 1.0 - left;
    (outer, (left, right))
}

/// 帯。説明を部品名にして読み上げと検索ができるようにする。
fn band(b: &BandVm, day: usize, p: &'static Palette, acts: &Acts) -> impl IntoElement {
    let c = series(b.color, p);
    let top = b.start as f32 / 60.0 * HOUR_PX;
    let height = (b.end.saturating_sub(b.start)) as f32 / 60.0 * HOUR_PX;
    let ((x, w), (l, r)) = band_place(day, b.lane, b.lanes);
    let id = SharedString::from(format!("band:{day}:{}:{}", b.session_id, b.start));
    div()
        .absolute()
        .top(px(top))
        .h(px(height.max(1.0)))
        .left(gpui::relative(x))
        .w(gpui::relative(w))
        .child(
            tagged(id, Role::Button, &b.tooltip)
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(l))
                .right(px(r))
                .rounded(px(3.0))
                .bg(with_alpha(c, BAND_FILL))
                .border(px(if b.selected { 2.5 } else { 1.0 }))
                .border_color(color(c))
                .hover(move |s| s.bg(with_alpha(c, BAND_FILL_HOVER)))
                .cursor_pointer()
                .tooltip(tip(p, &b.tooltip))
                .on_click(acts.on(Action::Calendar(CalendarAction::SelectBand(
                    b.session_id.clone(),
                    b.seg_start,
                )))),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実装とは独立に書いた式で、列の幅`col`での帯の左端と幅を出して実装を確かめる。
    fn reference_band(day: usize, lane: usize, lanes: usize, col: f32) -> (f32, f32) {
        let inner = (col - DENSITY_PX - 2.0).max(1.0);
        let lane_w = inner / lanes.max(1) as f32;
        let left = col * day as f32 + DENSITY_PX + 1.0 + lane_w * lane as f32;
        (left, (lane_w - 1.0).max(1.0))
    }

    proptest::proptest! {
        #[test]
        fn prop_band_place_matches_reference(
            day in 0usize..7,
            lanes in 1usize..5,
            lane_seed in 0usize..5,
            col in 40.0f32..400.0,
        ) {
            let lane = lane_seed % lanes;
            let ((x, w), (l, r)) = band_place(day, lane, lanes);
            let width = col * 7.0;
            let left = x * width + l;
            let right = (x + w) * width - r;
            let (el, ew) = reference_band(day, lane, lanes, col);
            proptest::prop_assert!((left - el).abs() < 1e-3, "{left} {el}");
            proptest::prop_assert!((right - left - ew).abs() < 1e-3, "{} {ew}", right - left);
        }
    }
}
