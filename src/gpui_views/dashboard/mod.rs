//! ダッシュボードの配置（spec 7.2節の3段）。区画の中身は同じディレクトリの各ファイルが描く。
pub mod cards;
pub mod detail;
pub mod memory;
pub mod sessions;
pub mod trend;

use crate::controllers::gui::app::Forms;
use crate::controllers::gui::dashboard::DashboardVm;
use crate::gpui_views::app::{Acts, Plots};
use crate::gpui_views::input::FormInputs;
use crate::gpui_views::probe::Role;
use crate::gpui_views::theme::{GAP_X, GAP_Y, Palette, color};
use crate::gpui_views::widgets::{card, probed};
use crate::views::layout::{
    GAP, MIN_MIDDLE, SPLITTER, TREND_COLLAPSED, split_widths, trend_height,
};
use gpui::{
    CursorStyle, Div, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    ScrollHandle, Styled as _, div, px,
};

/// ダッシュボードを描く。`width`と`height`は本文の大きさ。
pub fn render(
    vm: &DashboardVm,
    forms: &Forms,
    p: &'static Palette,
    acts: &Acts,
    plots: &mut Plots,
    (inputs, scroll, detail_ui): (&FormInputs, &ScrollHandle, &detail::DetailUi),
    (width, height): (f32, f32),
) -> Div {
    let trend_h = if vm.trend_open {
        trend_height(height)
    } else {
        TREND_COLLAPSED
    };
    // 左、つまみ、右の間には部品の間隔が2つ入るので、その分を引いてから分ける（eguiの`item_spacing.x`）。
    let avail = width - 2.0 * f32::from(GAP_X);
    let (lw, rw) = split_widths(avail, forms.split_ratio);
    let middle = div()
        .flex_1()
        .min_h(px(MIN_MIDDLE))
        .flex_shrink_0()
        .flex()
        .gap(GAP_X)
        .child(
            card(p, false)
                .w(px(lw))
                .h_full()
                .child(sessions::render(vm, inputs, scroll, p, acts)),
        )
        .child(splitter(p, acts, avail, lw))
        .child(card(p, false).w(px(rw)).h_full().child(detail::render(
            vm.detail.as_ref(),
            forms,
            detail_ui,
            p,
            acts,
        )));
    let trend_area = card(p, false)
        .w_full()
        .h(px(trend_h))
        .min_h_0()
        .flex()
        .flex_col()
        .child(trend::render(vm, p, acts, plots));
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(cards::render(&vm.cards, p, acts, width))
        .children(vm.desktop.as_ref().map(|d| memory::desktop(d, p, acts)))
        .child(div().h(px(GAP - f32::from(GAP_Y))))
        .child(middle)
        .child(div().h(px(GAP - f32::from(GAP_Y))))
        .child(trend_area)
}

/// 中段の境界のつまみ。ドラッグすると比率が変わり、ウィンドウの幅に比例して伸縮する。
/// eguiのつまみは名前を持たないので、読み上げ用の名前は付けず、テスト用の記録だけを残す。
fn splitter(p: &Palette, acts: &Acts, avail: f32, left: f32) -> impl IntoElement {
    let (border, accent) = (p.border, p.accent);
    let down = acts.clone();
    let line = div()
        .w(px(1.0))
        .h_full()
        .bg(color(border))
        .group_hover("splitter", move |s| s.bg(color(accent)));
    probed("splitter", Role::Splitter, "左右の境界")
        .group("splitter")
        .w(px(SPLITTER))
        .h_full()
        .flex()
        .justify_center()
        .cursor(CursorStyle::ResizeLeftRight)
        .on_mouse_down(MouseButton::Left, move |ev, w, cx| {
            down.drag_split(ev.position.x, avail, left, w, cx)
        })
        .child(line)
}
