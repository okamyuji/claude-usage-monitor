//! セッション一覧（spec 7.2節の中段の左）。稼働中と履歴を切り替える。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::dashboard::memory::MemAction;
use crate::controllers::gui::dashboard::runs::RunItem;
use crate::controllers::gui::dashboard::sessions::{DetailTab, SessionItem};
use crate::controllers::gui::dashboard::{DashAction, DashboardVm, ListMode};
use crate::gpui_views::app::Acts;
use crate::gpui_views::input::FormInputs;
use crate::gpui_views::probe::Role;
use crate::gpui_views::theme::{BODY, GAP_X, GAP_Y, Palette, SMALL, color};
use crate::gpui_views::widgets::{
    badge, help, label, probed, section, selectable, small_button, text, truncate, weak,
};
use crate::models::domain::activity::RunKind;
use crate::models::domain::display::help as h;
use crate::models::domain::transcript::SessionKind;
use crate::views::layout::{GAP, MIN_FLEX};
use gpui::{
    AnyElement, Div, InteractiveElement as _, IntoElement, ParentElement as _, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::input::Input;
use gpui_component::scroll::{Scrollbar, ScrollbarMode};

/// 履歴の列（入力、開始、経過、コンテキスト、Token、コスト、ターン）。`None`の入力の列だけが伸びる。
const HISTORY_COLUMNS: [Option<f32>; 7] = [
    None,
    Some(72.0),
    Some(56.0),
    Some(64.0),
    Some(56.0),
    Some(64.0),
    Some(40.0),
];
const HISTORY_HEADS: [&str; 7] = [
    "入力",
    "開始",
    "経過",
    "コンテキスト",
    "Token",
    "コスト",
    "ターン",
];

/// 一覧を描く。
pub fn render(
    vm: &DashboardVm,
    inputs: &FormInputs,
    scroll: &ScrollHandle,
    p: &'static Palette,
    acts: &Acts,
) -> Div {
    let rows: Vec<AnyElement> = match vm.mode {
        ListMode::Active => active(vm, p, acts),
        ListMode::History => history(vm, p, acts),
    };
    // 浮動のスクロールバーは右端に寄ると帯だけが現れ、行のボタンと見分けにくい。eguiと同じく溝の見える常設の形にする。
    let list = div()
        .relative()
        .flex_1()
        .min_h_0()
        .child(
            div()
                .id("session_list")
                .size_full()
                .track_scroll(scroll)
                .overflow_y_scroll()
                // eguiの常設のスクロールバーが取る幅（bar_inner_margin 4 + bar_width 6）だけ中身を詰める。
                .pr(px(10.0))
                .flex()
                .flex_col()
                .gap(GAP_Y)
                .children(rows),
        )
        .child(Scrollbar::vertical(scroll).mode(ScrollbarMode::Always));
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(header(vm, p, acts))
        .children((vm.mode == ListMode::History).then(|| filters(vm, inputs, p, acts)))
        .child(separator(p))
        .child(list)
}

fn separator(p: &Palette) -> Div {
    div().h(px(1.0)).w_full().bg(color(p.border))
}

/// 見出しのⓘ。終了の操作がある稼働中の表示でだけ、サブエージェントを止められない理由も出す。
fn list_help(mode: ListMode) -> &'static str {
    match mode {
        ListMode::Active => h::ACTIVE_RUNS,
        ListMode::History => h::RUN_KIND,
    }
}

fn header(vm: &DashboardVm, p: &'static Palette, acts: &Acts) -> Div {
    let n = vm.active.iter().filter(|r| r.depth == 0).count();
    let modes = [
        (ListMode::Active, format!("稼働中 {n}")),
        (ListMode::History, "履歴".to_string()),
    ]
    .map(|(m, t)| {
        selectable(
            p,
            SharedString::from(format!("mode:{m:?}")),
            vm.mode == m,
            &t,
            acts.on(Action::Dash(DashAction::SetListMode(m))),
        )
    });
    div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(section(p, "セッション"))
        .children(modes)
        .child(help(p, list_help(vm.mode)))
}

/// 種別のバッジの色。バックグラウンドの実行は青、対話は灰（spec 7.3節）。
pub fn kind_colors(k: RunKind, p: &Palette) -> (u32, u32) {
    match k {
        RunKind::Interactive => (p.text, p.track),
        RunKind::Headless | RunKind::Job | RunKind::Subagent => (p.accent, p.accent_soft),
    }
}

/// 状態のバッジの色。応答生成中とツール実行中は緑、入力待ちは黄、それ以外は灰（spec 7.3節）。
pub fn state_colors(state: &str, p: &Palette) -> (u32, u32) {
    match state {
        "応答生成中" | "ツール実行中" => (p.ok, p.ok_soft),
        "入力待ち" => (p.warn, p.warn_soft),
        _ => (p.weak, p.track),
    }
}

fn active(vm: &DashboardVm, p: &'static Palette, acts: &Acts) -> Vec<AnyElement> {
    if vm.active.is_empty() {
        return vec![weak(p, "稼働中の実行はありません").into_any_element()];
    }
    vm.active
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let selected = vm.selected.as_deref() == Some(r.session_id.as_str());
            run_row(i, r, selected, p, acts).into_any_element()
        })
        .collect()
}

/// 選べる題名。押すと`a`を届ける。選んでいる行はアクセント色にする。
fn title(s: &str, selected: bool, p: &Palette, acts: &Acts, a: Action) -> impl IntoElement {
    let t = text(s, BODY, if selected { p.accent } else { p.text })
        .cursor_pointer()
        .on_click(acts.on(a));
    truncate(p, t, s)
}

fn run_row(i: usize, r: &RunItem, selected: bool, p: &'static Palette, acts: &Acts) -> Div {
    let indent = px(16.0 * r.depth as f32);
    let (kfg, kbg) = kind_colors(r.kind, p);
    let (sfg, sbg) = state_colors(&r.state, p);
    // ボタンを右端に置き、残りの幅で題名を省略する。
    let head = div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .pl(indent)
        .child(badge(r.kind.label(), kfg, kbg))
        .child(badge(&r.state, sfg, sbg))
        .children(
            r.memory
                .as_ref()
                .filter(|m| m.desktop)
                .map(|_| badge("Desktop", p.accent, p.accent_soft)),
        )
        .child(div().flex_1().min_w_0().flex().child(title(
            &r.title,
            selected,
            p,
            acts,
            Action::Dash(DashAction::Select(r.session_id.clone(), DetailTab::LiveLog)),
        )))
        .children(r.memory.as_ref().map(|m| {
            small_button(
                p,
                SharedString::from(format!("exit:{i}")),
                "プロセスを終了",
                acts.on(Action::Memory(MemAction::Ask(m.exit.clone()))),
            )
        }));
    let context = format!("コンテキスト {}", r.context);
    let meta: Vec<&str> = [
        r.project.as_str(),
        context.as_str(),
        &r.elapsed,
        &r.tokens,
        &r.cost,
        r.memory.as_ref().map_or("—", |m| m.text.as_str()),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect();
    let meta = meta.join(" · ");
    div()
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .pb(px(4.0))
        .child(head)
        .child(
            div()
                .pl(indent)
                .min_w_0()
                .child(truncate(p, text(&meta, SMALL, p.weak), &meta)),
        )
        .children(r.detail.as_deref().map(|d| {
            div()
                .pl(indent)
                .min_w_0()
                .child(truncate(p, text(d, SMALL, p.text), d))
        }))
}

fn filters(vm: &DashboardVm, inputs: &FormInputs, p: &'static Palette, acts: &Acts) -> Div {
    let search = div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(label(p, "検索"))
        .child(
            probed("session-query", Role::TextInput, "検索")
                .flex_1()
                .child(Input::new(&inputs.session_query).aria_label("検索")),
        );
    let kinds = [
        ("全種別", None),
        ("対話", Some(SessionKind::Interactive)),
        ("ヘッドレス", Some(SessionKind::Headless)),
        ("ジョブ", Some(SessionKind::BackgroundJob)),
    ]
    .map(|(t, k)| {
        selectable(
            p,
            SharedString::from(format!("kind:{t}")),
            vm.kind == k,
            t,
            acts.on(Action::Dash(DashAction::SetKind(k))),
        )
    });
    let profiles = vm.profiles.iter().map(|(id, name)| {
        selectable(
            p,
            SharedString::from(format!("profile:{id}")),
            vm.profile == Some(*id),
            name,
            acts.on(Action::Dash(DashAction::SetProfile(Some(*id)))),
        )
    });
    let chips = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(GAP_X)
        .children(kinds)
        .child(div().w(px(1.0)).h(px(16.0)).bg(color(p.border)))
        .child(selectable(
            p,
            "profile:all",
            vm.profile.is_none(),
            "全プロファイル",
            acts.on(Action::Dash(DashAction::SetProfile(None))),
        ))
        .children(profiles);
    div()
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(search)
        .child(chips)
}

/// 表の1行。最初の列だけが伸び、残りは固定幅（`views::layout::flex_columns`と同じ割り付け）。
fn table_row(cells: Vec<AnyElement>) -> Div {
    div().flex().items_center().gap(px(GAP / 2.0)).children(
        cells.into_iter().zip(HISTORY_COLUMNS).map(|(c, w)| {
            let cell = div().min_w_0().child(c);
            match w {
                Some(w) => cell.w(px(w)).flex_shrink_0(),
                None => cell.flex_1().min_w(px(MIN_FLEX)),
            }
        }),
    )
}

fn history(vm: &DashboardVm, p: &'static Palette, acts: &Acts) -> Vec<AnyElement> {
    if vm.history.is_empty() {
        return vec![weak(p, "条件に合う完了したセッションはありません").into_any_element()];
    }
    let mut out = Vec::with_capacity(vm.history.len() + 2);
    if vm.history_limited {
        out.push(text("新しい順に1,000件を表示しています", SMALL, p.weak).into_any_element());
    }
    out.push(
        table_row(
            HISTORY_HEADS
                .iter()
                // eguiの見出しは折り返さず、狭い列では隣へはみ出して描く。
                .map(|t| {
                    text(t, SMALL, p.weak)
                        .whitespace_nowrap()
                        .into_any_element()
                })
                .collect(),
        )
        .into_any_element(),
    );
    for i in &vm.history {
        let selected = vm.selected.as_deref() == Some(i.session_id.as_str());
        out.push(history_row(i, selected, p, acts).into_any_element());
    }
    out
}

fn history_row(i: &SessionItem, selected: bool, p: &'static Palette, acts: &Acts) -> Div {
    let mut cells = vec![
        title(
            &i.title,
            selected,
            p,
            acts,
            Action::Dash(DashAction::Select(i.session_id.clone(), DetailTab::Turns)),
        )
        .into_any_element(),
    ];
    for v in [
        &i.started, &i.elapsed, &i.context, &i.tokens, &i.cost, &i.turns,
    ] {
        cells.push(truncate(p, text(v, SMALL, p.text), v).into_any_element());
    }
    table_row(cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpui_views::theme::LIGHT;

    #[test]
    fn list_help_mentions_subagents_only_for_active_runs() {
        assert_eq!(list_help(ListMode::Active), h::ACTIVE_RUNS);
        assert_eq!(list_help(ListMode::History), h::RUN_KIND);
    }

    #[test]
    fn badge_colors_by_kind_and_state() {
        assert_eq!(
            kind_colors(RunKind::Interactive, &LIGHT),
            (LIGHT.text, LIGHT.track)
        );
        assert_eq!(
            kind_colors(RunKind::Job, &LIGHT),
            (LIGHT.accent, LIGHT.accent_soft)
        );
        assert_eq!(
            state_colors("応答生成中", &LIGHT),
            (LIGHT.ok, LIGHT.ok_soft)
        );
        assert_eq!(
            state_colors("ツール実行中", &LIGHT),
            (LIGHT.ok, LIGHT.ok_soft)
        );
        assert_eq!(
            state_colors("入力待ち", &LIGHT),
            (LIGHT.warn, LIGHT.warn_soft)
        );
        assert_eq!(state_colors("終了", &LIGHT), (LIGHT.weak, LIGHT.track));
    }
}
