//! 使用率カード（spec 7.2節の上段）。空き幅をプロファイル数で等分し、狭いときは次の行へ折り返す。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::dashboard::cards::{LimitView, ProfileCard};
use crate::controllers::gui::tabs::profiles::ProfilesAction;
use crate::gpui_views::app::Acts;
use crate::gpui_views::theme::{BODY, GAP_X, GAP_Y, Palette, SMALL};
use crate::gpui_views::widgets::{
    badge, card, help, label, section, severity_color, small_button, strong, text, truncate,
    usage_bar,
};
use crate::models::domain::display::help as h;
use crate::views::layout::{GAP, MIN_LIMIT, card_columns, columns};
use gpui::{Div, IntoElement, ParentElement as _, Styled as _, div, px};

/// カードの内側の余白（横）。`card`の`px`と同じ値。
const CARD_PAD_X: f32 = 10.0;

/// カードを並べる。`width`は本文の幅。
pub fn render(cards: &[ProfileCard], p: &'static Palette, acts: &Acts, width: f32) -> Div {
    let (cols, w) = card_columns(width, cards.len());
    let rows = cards.chunks(cols).map(|row| {
        div()
            .flex()
            .items_start()
            .gap(px(GAP))
            .children(row.iter().map(|c| card_view(c, p, acts, w)))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(GAP / 2.0))
        .child(section(p, "レート制限"))
        .children(rows)
}

fn card_view(c: &ProfileCard, p: &'static Palette, acts: &Acts, width: f32) -> Div {
    let border = if c.is_active { 1.5 } else { 1.0 };
    let inner = width - 2.0 * (CARD_PAD_X + border);
    let head = div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(strong(p, &c.name))
        .child(if c.is_active {
            badge("使用中", p.accent, p.accent_soft).into_any_element()
        } else {
            small_button(
                p,
                gpui::SharedString::from(format!("use:{}", c.name)),
                "使用中にする",
                acts.on(Action::Profiles(ProfilesAction::Use(c.name.clone()))),
            )
            .into_any_element()
        })
        .child(div().flex_1())
        .child(text(&c.fetched, SMALL, p.weak));
    let problem = c
        .problem
        .as_deref()
        .map(|s| truncate(p, text(s, BODY, p.err), s));
    // 幅の広いカードではバーを横に並べ、カードの高さを抑えて中段のセッション一覧に高さを回す。
    let (cols, lw) = columns(inner, c.limits.len(), MIN_LIMIT);
    let limit_rows = c.limits.chunks(cols).map(|row| {
        div()
            .flex()
            .items_start()
            .gap(px(GAP))
            .children(row.iter().map(|l| limit(l, p, lw)))
    });
    // 用途と追加課金は補足なので1行にまとめ、中段（セッションとライブログ）に高さを回す。
    let mut extra = div().flex().flex_wrap().items_center().gap(GAP_X);
    if !c.breakdown.is_empty() {
        extra = extra
            .child(text(
                &format!("用途 {}", c.breakdown.join(" / ")),
                SMALL,
                p.weak,
            ))
            .child(help(p, h::BREAKDOWN));
    }
    if let Some(s) = &c.spend {
        extra = extra
            .child(text(&format!("追加課金 {s}"), SMALL, p.weak))
            .child(help(p, h::SPEND));
    }
    card(p, c.is_active)
        .w(px(width))
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(head)
        .children(problem)
        .children(limit_rows)
        .child(extra)
}

fn limit(l: &LimitView, p: &'static Palette, width: f32) -> Div {
    let title = div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(label(p, &l.title))
        .child(help(p, l.help))
        .child(div().flex_1())
        .child(strong(p, &l.percent));
    let projection = l.projection.as_deref().map(|s| {
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .min_w_0()
            .child(truncate(p, text(s, SMALL, p.weak), s))
            .child(help(p, h::PROJECTION))
    });
    div()
        .w(px(width))
        .pt(px(4.0))
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(title)
        .child(usage_bar(l.ratio, severity_color(l.severity, p), p))
        .child(truncate(p, text(&l.reset, SMALL, p.weak), &l.reset))
        .children(projection)
}
