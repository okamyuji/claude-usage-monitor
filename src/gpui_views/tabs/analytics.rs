//! 分析タブの描画。期間の切り替え、軸ごとの使用量、日ごとのキャッシュヒット率、ツール。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::tabs::analytics::{AnalyticsVm, GroupRow, Period};
use crate::gpui_views::app::Acts;
use crate::gpui_views::theme::{GAP_X, GAP_Y, Palette, SMALL};
use crate::gpui_views::widgets::{
    card, help, label, section, selectable, small, strong, table_row, text, trunc, usage_bar, weak,
};
use crate::models::domain::display::help as h;
use crate::views::layout::{GAP, card_columns};
use gpui::{AnyElement, Div, IntoElement, ParentElement as _, Styled as _, div, px};

/// 列（軸の値、割合の棒、Token、コスト）。軸の値の列だけが伸びる。
const GROUP_COLUMNS: [Option<f32>; 4] = [None, Some(96.0), Some(56.0), Some(64.0)];
/// 列（ツール、回数、エラー、エラー率）。ツールの列だけが伸びる。
const TOOL_COLUMNS: [Option<f32>; 4] = [None, Some(56.0), Some(56.0), Some(64.0)];

/// 分析タブを描く。`width`は本文の幅。
pub fn render(vm: &AnalyticsVm, p: &'static Palette, acts: &Acts, width: f32) -> Div {
    let periods = [Period::Days7, Period::Days30].map(|d| {
        selectable(
            p,
            format!("period:{}", d.label()),
            vm.period == d,
            d.label(),
            acts.on(Action::SetPeriod(d)),
        )
    });
    let head = div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(section(p, "分析"))
        .children(periods)
        .child(strong(p, &vm.agent_share))
        .child(help(p, h::RUN_KIND));
    let groups = [
        ("プロジェクト別", &vm.by_project),
        ("モデル別", &vm.by_model),
        ("ブランチ別", &vm.by_branch),
    ]
    .map(|(title, rows)| group_card(p, title, rows));
    let halves = [cache_card(p, vm), tools_card(p, vm)];
    div()
        .flex()
        .flex_col()
        .child(head)
        .child(div().h(px(GAP / 2.0)))
        .children(rows_of(groups.into(), width))
        .children(rows_of(halves.into(), width))
}

/// カードを幅に合わせて列に並べ、行ごとに返す（eguiの`card_columns`と`horizontal_top`）。
fn rows_of(cards: Vec<Div>, width: f32) -> Vec<Div> {
    let (cols, w) = card_columns(width, cards.len());
    let mut rows = Vec::new();
    let mut it = cards.into_iter().peekable();
    while it.peek().is_some() {
        let row = it.by_ref().take(cols).map(|c| c.w(px(w)).flex_shrink_0());
        rows.push(
            div()
                .flex()
                .items_start()
                .gap(px(GAP))
                .mb(px(GAP))
                .children(row),
        );
    }
    rows
}

fn group_card(p: &'static Palette, title: &str, rows: &[GroupRow]) -> Div {
    let body: Vec<AnyElement> = if rows.is_empty() {
        vec![weak(p, "この期間の記録はありません").into_any_element()]
    } else {
        rows.iter()
            .map(|r| {
                table_row(
                    &GROUP_COLUMNS,
                    vec![
                        trunc(p, &r.key).into_any_element(),
                        usage_bar(r.share, p.accent, p).into_any_element(),
                        small(p, &r.tokens).into_any_element(),
                        small(p, &r.cost).into_any_element(),
                    ],
                )
                .into_any_element()
            })
            .collect()
    };
    card(p, false)
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(section(p, title))
        .children(body)
}

fn cache_card(p: &'static Palette, vm: &AnalyticsVm) -> Div {
    let days = vm.daily_cache.iter().map(|(day, rate)| {
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .mr(px(8.0))
            .child(text(day, SMALL, p.weak))
            .child(strong(p, rate))
    });
    card(p, false)
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(
            div()
                .flex()
                .items_center()
                .gap(GAP_X)
                .child(section(p, "キャッシュヒット率（日ごと）"))
                .child(help(p, h::CACHE_HIT)),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(GAP_X)
                .children(days),
        )
}

fn tools_card(p: &'static Palette, vm: &AnalyticsVm) -> Div {
    let head = table_row(
        &TOOL_COLUMNS,
        ["ツール", "回数", "エラー", "エラー率"]
            .map(|t| text(t, SMALL, p.weak).into_any_element())
            .into(),
    );
    let rows = vm.tools.iter().map(|t| {
        table_row(
            &TOOL_COLUMNS,
            vec![
                trunc(p, &t.name).into_any_element(),
                label(p, &t.calls).into_any_element(),
                label(p, &t.errors).into_any_element(),
                label(p, &t.error_rate).into_any_element(),
            ],
        )
    });
    card(p, false)
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(section(p, "ツール"))
        .child(head)
        .children(rows)
}
