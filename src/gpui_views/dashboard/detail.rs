//! 選んだセッションの詳細（spec 7.2節の中段の右）。見出し、合計、ターン、ライブログ、再生。
use crate::controllers::gui::app::{Action, Forms};
use crate::controllers::gui::dashboard::DashAction;
use crate::controllers::gui::dashboard::live_log::{LiveAction, LiveLogVm};
use crate::controllers::gui::dashboard::replay::{ReplayAction, ReplayVm};
use crate::controllers::gui::dashboard::sessions::{DetailTab, SessionDetail, SummaryVm};
use crate::gpui_views::app::{Acts, CumonRoot};
use crate::gpui_views::icons;
use crate::gpui_views::probe::Role;
use crate::gpui_views::theme::{BODY, GAP_X, GAP_Y, Palette, SMALL, color};
use crate::gpui_views::widgets::{
    button, checkbox, help, icon, label, mono, section, selectable, small_button, strong, text,
    timed, truncate, weak,
};
use crate::models::domain::display::help as h;
use crate::models::domain::live_log::LiveKind;
use crate::views::layout::MIN_FLEX;
use gpui::{
    AnyElement, AppContext as _, Context, Div, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, anchored, deferred, div, prelude::FluentBuilder as _, px,
};
use gpui_component::slider::{Slider, SliderEvent, SliderState};

/// 下端とみなす余り（eguiの「最新へ戻る」の判定と同じ4px）。
const BOTTOM_SLACK: f32 = 4.0;

/// 詳細の描画をまたいで持つ状態（スクロールの位置、再生位置のスライダー、発生元の一覧の開閉）。
pub struct DetailUi {
    summary: ScrollHandle,
    turns: ScrollHandle,
    pub(crate) live: ScrollHandle,
    replay: ScrollHandle,
    pub(crate) source_open: bool,
    slider: Option<(usize, Entity<SliderState>, Subscription)>,
    seen_position: usize,
}

impl Default for DetailUi {
    fn default() -> Self {
        Self {
            summary: ScrollHandle::new(),
            turns: ScrollHandle::new(),
            live: ScrollHandle::new(),
            replay: ScrollHandle::new(),
            source_open: false,
            slider: None,
            seen_position: 0,
        }
    }
}

/// 前の描画で下端にいたか。中身が窓より短いときも下端とみなす。
fn at_bottom(h: &ScrollHandle) -> bool {
    f32::from(h.offset().y) <= -f32::from(h.max_offset().y) + BOTTOM_SLACK
}

impl DetailUi {
    /// 再生位置のスライダーの値。スライダーがまだ無ければ`None`。
    pub fn slider_value(&self, cx: &gpui::App) -> Option<f32> {
        self.slider
            .as_ref()
            .map(|(_, s, _)| s.read(cx).value().start())
    }

    /// 描く前の準備。下端に付く一覧を下端へ寄せ、再生位置のスライダーを長さと位置に合わせる。
    /// `jump`はcontrollerが最下端への移動を求めたとき（`LiveLogVm::jump_to_bottom`）に真。
    pub fn prepare(
        &mut self,
        replay: Option<(usize, usize)>,
        jump: bool,
        window: &mut Window,
        cx: &mut Context<CumonRoot>,
    ) {
        // eguiの`stick_to_bottom`と同じく、前の描画で下端にいた一覧だけを、伸びた分だけ下へ送る。
        if jump || at_bottom(&self.live) {
            self.live.scroll_to_bottom();
        }
        if at_bottom(&self.replay) {
            self.replay.scroll_to_bottom();
        }
        let Some((len, pos)) = replay.filter(|(len, _)| *len > 0) else {
            return;
        };
        if self.slider.as_ref().is_none_or(|(l, _, _)| *l != len) {
            // 最大値は作るときにしか決められないので、ターンの数が変わったら作り直す。
            let state = cx.new(|_| {
                SliderState::new()
                    .min(0.0)
                    .max((len - 1) as f32)
                    .step(1.0)
                    .default_value(pos as f32)
            });
            let sub = cx.subscribe(&state, |root, _, ev: &SliderEvent, cx| {
                let SliderEvent::Change(v) = ev else { return };
                let p = v.start().round() as usize;
                root.detail_ui.seen_position = p;
                root.controller_mut().forms_mut().replay_position = p;
                root.act(Action::Dash(DashAction::Replay(ReplayAction::Seek(p))), cx);
            });
            self.slider = Some((len, state, sub));
            self.seen_position = pos;
        } else if pos != self.seen_position {
            self.seen_position = pos;
            if let Some((_, s, _)) = &self.slider {
                s.update(cx, |s, cx| s.set_value(pos as f32, window, cx));
            }
        }
    }
}

/// 詳細を描く。
pub fn render(
    d: Option<&SessionDetail>,
    forms: &Forms,
    ui: &DetailUi,
    p: &'static Palette,
    acts: &Acts,
) -> AnyElement {
    let Some(d) = d else {
        return weak(p, "左の一覧からセッションを選んでください").into_any_element();
    };
    let tabs = [
        (DetailTab::Summary, icons::NOTE, "概要"),
        (DetailTab::Turns, icons::LIST, "ターン"),
        (DetailTab::LiveLog, icons::TERMINAL, "ライブログ"),
        (DetailTab::Replay, icons::PLAY, "再生"),
    ]
    .map(|(tab, ic, t)| {
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .child(icon(ic, if d.tab == tab { p.accent } else { p.weak }))
            .child(selectable(
                p,
                SharedString::from(format!("detail-tab:{t}")),
                d.tab == tab,
                t,
                acts.on(Action::Dash(DashAction::SetDetailTab(tab))),
            ))
    });
    let body: AnyElement = match d.tab {
        DetailTab::Summary => match &d.summary {
            Some(s) => summary(s, &ui.summary, p).into_any_element(),
            None => div().into_any_element(),
        },
        DetailTab::Turns => turns(d, &ui.turns, p, acts).into_any_element(),
        DetailTab::LiveLog => match &d.live {
            Some(v) => live(v, forms, ui, p, acts).into_any_element(),
            None => text(
                "ライブログを開けません。セッションの記録が見つかりません",
                BODY,
                p.warn,
            )
            .into_any_element(),
        },
        DetailTab::Replay => match (&d.replay, &ui.slider) {
            (Some(v), Some((_, s, _))) if v.len > 0 => {
                replay(v, s, &ui.replay, p, acts).into_any_element()
            }
            _ => weak(p, "再生できるターンがありません").into_any_element(),
        },
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(section(p, &d.title))
        .child(truncate(p, text(&d.meta, SMALL, p.weak), &d.meta))
        .child(totals(d, p))
        .child(div().flex().items_center().gap(GAP_X).children(tabs))
        .child(separator(p))
        .child(div().flex_1().min_h_0().flex().flex_col().child(body))
        .into_any_element()
}

fn separator(p: &Palette) -> Div {
    div().h(px(1.0)).w_full().bg(color(p.border))
}

fn totals(d: &SessionDetail, p: &'static Palette) -> Div {
    let t = &d.totals;
    let items = [
        ("入力", &t.input, h::TOKENS_INPUT),
        ("出力", &t.output, h::TOKENS_OUTPUT),
        ("キャッシュ読込", &t.cache_read, h::TOKENS_CACHE_READ),
        ("キャッシュ作成", &t.cache_write, h::TOKENS_CACHE_WRITE),
        ("コスト", &t.cost, h::COST),
        ("キャッシュヒット率", &t.cache_hit, h::CACHE_HIT),
        ("サブエージェントの割合", &t.subagent_share, h::RUN_KIND),
    ]
    .map(|(name, value, tip)| {
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .mr(px(8.0))
            .child(text(name, SMALL, p.weak))
            .child(help(p, tip))
            .child(strong(p, value))
    });
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(GAP_X)
        .children(items)
}

fn scroll_area(id: &'static str, handle: &ScrollHandle) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_h_0()
        .track_scroll(handle)
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(GAP_Y)
}

fn summary(s: &SummaryVm, handle: &ScrollHandle, p: &'static Palette) -> impl IntoElement {
    let mut area = scroll_area("summary", handle).child(strong(p, "要約"));
    if s.recaps.is_empty() {
        area = area.child(weak(
            p,
            "離席中の要約はありません。Claude Codeは端末を3分以上離れたときだけ要約を書きます",
        ));
    }
    if let Some(t) = &s.title {
        area = area
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(GAP_X)
                    .child(text("題名", SMALL, p.weak))
                    .child(label(p, t)),
            )
            .child(text(
                "Claude Codeが最初の依頼から自動で付けた題名です",
                SMALL,
                p.weak,
            ));
    }
    area = area.children(s.recaps.iter().map(|(t, l)| timed(p, t, l)));
    let stats = s.stats.iter().map(|(name, value)| {
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .mr(px(8.0))
            .child(text(name, SMALL, p.weak))
            .child(strong(p, value))
    });
    area = area.child(
        div()
            .mt(px(8.0))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(GAP_X)
            .children(stats),
    );
    if s.truncated {
        area = area.child(text("直近10万件のターンから集計しています", SMALL, p.warn));
    }
    let models: Vec<[&str; 7]> = s
        .models
        .iter()
        .map(|m| {
            [
                &m.model,
                &m.requests,
                &m.input,
                &m.output,
                &m.cache_read,
                &m.cache_write,
                &m.cost,
            ]
            .map(String::as_str)
        })
        .collect();
    let tools: Vec<[&str; 3]> = s
        .tools
        .iter()
        .map(|(n, c, e)| [n.as_str(), c.as_str(), e.as_str()])
        .collect();
    area.child(div().mt(px(8.0)).child(table(
        "summary_models",
        [
            "モデル",
            "要求",
            "入力",
            "出力",
            "キャッシュ読込",
            "キャッシュ作成",
            "コスト",
        ],
        &models,
        p,
    )))
    .child(div().mt(px(8.0)).child(table(
        "summary_tools",
        ["ツール名", "呼び出し", "エラー"],
        &tools,
        p,
    )))
    .child(div().mt(px(8.0)).child(strong(p, "依頼の一覧")))
    .children(s.prompts.iter().map(|(t, l)| timed(p, t, l)))
}

/// 縞模様の表（eguiの`Grid::striped`）。列の幅は各列でいちばん広い中身に合わせ、読み上げはeguiと同じく行の順にする。
/// モデル表は7列あり詳細の区画より広くなるので、表だけを横に送る。
fn table<const N: usize>(
    id: &'static str,
    head: [&str; N],
    rows: &[[&str; N]],
    p: &'static Palette,
) -> impl IntoElement {
    let head = head.map(|h| text(h, SMALL, p.weak).into_any_element());
    let body = rows
        .iter()
        .map(|r| r.map(|v| label(p, v).into_any_element()));
    let cells = std::iter::once(head)
        .chain(body)
        .enumerate()
        .flat_map(|(i, row)| {
            // 縞は見出しの次の行から1行おき。
            let striped = i > 0 && i % 2 == 0;
            row.into_iter().map(move |cell| {
                div()
                    .h(px(20.0))
                    .px(px(4.0))
                    .flex()
                    .items_center()
                    .whitespace_nowrap()
                    .when(striped, |d| d.bg(color(p.track)))
                    .child(cell)
            })
        });
    div()
        .id(id)
        .overflow_x_scroll()
        .child(div().grid().grid_cols_max_content(N as u16).children(cells))
}

/// ターン表の列（時刻、種類、内容、コンテキスト、所要時間、内訳）。`None`の内容の列だけが伸びる。
const TURN_COLUMNS: [Option<f32>; 6] = [
    Some(64.0),
    Some(140.0),
    None,
    Some(56.0),
    Some(56.0),
    Some(44.0),
];

fn turn_row(cells: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .children(cells.into_iter().zip(TURN_COLUMNS).map(|(c, w)| {
            let cell = div().min_w_0().flex().items_center().child(c);
            match w {
                Some(w) => cell.w(px(w)).flex_shrink_0(),
                None => cell.flex_1().min_w(px(MIN_FLEX)),
            }
        }))
}

fn turns(
    d: &SessionDetail,
    handle: &ScrollHandle,
    p: &'static Palette,
    acts: &Acts,
) -> impl IntoElement {
    let lead: f32 = TURN_COLUMNS[0].unwrap_or(0.0) + TURN_COLUMNS[1].unwrap_or(0.0) + 2.0 * 6.0;
    scroll_area("turns", handle).children(d.turns.iter().enumerate().map(|(i, t)| {
        let kind = format!("{}（{}）", t.kind, t.source);
        let row = turn_row(vec![
            mono(&t.time, p.weak).into_any_element(),
            div()
                .pl(px(12.0 * t.depth as f32))
                .min_w_0()
                .child(truncate(p, label(p, &kind), &kind))
                .into_any_element(),
            truncate(p, label(p, &t.summary), &t.summary).into_any_element(),
            text(&t.context, SMALL, p.text).into_any_element(),
            text(&t.duration, SMALL, p.text).into_any_element(),
            small_button(
                p,
                SharedString::from(format!("turn-breakdown:{i}")),
                "内訳",
                acts.on(Action::Dash(DashAction::ToggleTurn(t.key.clone()))),
            )
            .into_any_element(),
        ]);
        div()
            .flex()
            .flex_col()
            .child(row)
            .children(t.breakdown.as_deref().map(|b| {
                div()
                    .pl(px(lead))
                    .min_w_0()
                    .child(truncate(p, text(b, SMALL, p.weak), b))
            }))
    }))
}

/// 発生元の選択（eguiの`ComboBox::from_label("発生元")`）。押すと下に候補の一覧を出す。
fn source_combo(
    vm: &LiveLogVm,
    forms: &Forms,
    open: bool,
    p: &'static Palette,
    acts: &Acts,
) -> impl IntoElement {
    let current = forms
        .live_filter
        .source
        .clone()
        .unwrap_or_else(|| "すべて".into());
    let toggle = acts.clone();
    let trigger = crate::gpui_views::widgets::combo(p, "live-source", "発生元", &current)
        .on_click(move |_, w, cx| toggle.set_source_open(!open, w, cx));
    let menu = open.then(|| {
        let close = acts.clone();
        let choice = |t: &str, v: Option<String>| {
            let selected = forms.live_filter.source == v;
            selectable(
                p,
                SharedString::from(format!("source:{t}")),
                selected,
                t,
                acts.edit(
                    move |f| f.live_filter.source = v.clone(),
                    Action::Live(LiveAction::FilterChanged),
                ),
            )
        };
        let items = std::iter::once(choice("すべて", None))
            .chain(vm.sources.iter().map(|s| choice(s, Some(s.clone()))));
        deferred(
            anchored().snap_to_window().child(
                div()
                    .id("live-source-menu")
                    .occlude()
                    .mt(px(22.0))
                    .p(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .bg(color(p.card))
                    .border_1()
                    .border_color(color(p.border))
                    .rounded(px(4.0))
                    .on_mouse_down_out(move |_, w, cx| close.set_source_open(false, w, cx))
                    .children(items),
            ),
        )
    });
    div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(div().child(trigger).children(menu))
        .child(label(p, "発生元"))
}

fn live(
    vm: &LiveLogVm,
    forms: &Forms,
    ui: &DetailUi,
    p: &'static Palette,
    acts: &Acts,
) -> impl IntoElement {
    let f = &forms.live_filter;
    let changed = || Action::Live(LiveAction::FilterChanged);
    let filters = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(GAP_X)
        .child(checkbox(
            p,
            "live-tools",
            f.tools,
            "ツール",
            acts.edit(|f| f.live_filter.tools = !f.live_filter.tools, changed()),
        ))
        .child(checkbox(
            p,
            "live-text",
            f.text,
            "本文",
            acts.edit(|f| f.live_filter.text = !f.live_filter.text, changed()),
        ))
        .child(checkbox(
            p,
            "live-errors",
            f.errors_only,
            "エラーのみ",
            acts.edit(
                |f| f.live_filter.errors_only = !f.live_filter.errors_only,
                changed(),
            ),
        ))
        .child(source_combo(vm, forms, ui.source_open, p, acts))
        .child(text(
            &format!("{}行を保持（最大2,000行）", vm.total),
            SMALL,
            p.weak,
        ));
    let lines = vm.lines.iter().map(|l| {
        let fg = if l.kind == LiveKind::ToolError {
            p.err
        } else {
            p.text
        };
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .child(mono(&l.time, p.weak))
            .child(div().w(px(16.0 * l.depth as f32)).flex_shrink_0())
            .child(strong(p, &l.source))
            .children(l.tool.as_deref().map(|t| mono(t, p.accent)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(truncate(p, text(&l.text, BODY, fg), &l.text)),
            )
    });
    // 手で上へスクロールしたときだけ「最新へ戻る」を出す（spec 7.4節）。
    let back = (!at_bottom(&ui.live)).then(|| {
        button(
            p,
            "live-latest",
            "最新へ戻る",
            acts.on(Action::Live(LiveAction::JumpToLatest)),
        )
    });
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(filters)
        .children(vm.note.as_deref().map(|n| text(n, BODY, p.warn)))
        .child(scroll_area("live_log", &ui.live).children(lines))
        .children(back)
}

fn replay(
    vm: &ReplayVm,
    slider: &Entity<SliderState>,
    handle: &ScrollHandle,
    p: &'static Palette,
    acts: &Acts,
) -> impl IntoElement {
    let act = |a| Action::Dash(DashAction::Replay(a));
    let (ic, play) = if vm.playing {
        (icons::PAUSE, "一時停止")
    } else {
        (icons::PLAY, "再生する")
    };
    let speeds = [1, 4, 16].map(|s| {
        selectable(
            p,
            SharedString::from(format!("speed:{s}")),
            vm.speed == s,
            &format!("{s}倍"),
            acts.on(act(ReplayAction::SetSpeed(s))),
        )
    });
    let controls = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(GAP_X)
        .child(button(
            p,
            "replay-prev",
            &format!("{} 前のターン", icons::SKIP_BACK),
            acts.on(act(ReplayAction::Step(-1))),
        ))
        .child(button(
            p,
            "replay-play",
            &format!("{ic} {play}"),
            acts.on(act(ReplayAction::TogglePlay)),
        ))
        .child(button(
            p,
            "replay-next",
            &format!("{} 次のターン", icons::SKIP_FORWARD),
            acts.on(act(ReplayAction::Step(1))),
        ))
        .children(speeds)
        .child(strong(p, &format!("{} / {}", vm.position + 1, vm.len)));
    let current = vm.current.as_ref().map(|c| {
        let now = format!("現在: {}", c.summary);
        let meta = format!(
            "{} · {}（{}） · コンテキスト {}",
            c.time, c.kind, c.source, c.context
        );
        div()
            .flex()
            .flex_col()
            .gap(GAP_Y)
            .child(truncate(p, strong(p, &now), &now))
            .child(truncate(p, text(&meta, SMALL, p.weak), &meta))
    });
    let recent = vm.recent.iter().map(|t| {
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .child(mono(&t.time, p.weak))
            .child(div().w(px(12.0 * t.depth as f32)).flex_shrink_0())
            .child(label(p, &format!("{}（{}）", t.kind, t.source)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(truncate(p, label(p, &t.summary), &t.summary)),
            )
    });
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(controls)
        .child(
            // eguiのSliderのレールの長さ（`spacing.slider_width`の既定値100）。
            crate::gpui_views::widgets::probed("replay-slider", Role::Slider, "再生位置")
                .h(px(20.0))
                .w(px(100.0))
                .child(Slider::new(slider).accessibility_label("再生位置")),
        )
        .children(current)
        .child(separator(p))
        .child(scroll_area("replay", handle).children(recent))
}
