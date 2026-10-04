//! 選んだセッションの詳細（spec 7.2節の中段の右）。見出し、合計、ターン、ライブログ、再生。
use crate::controllers::gui::app::{Action, Forms};
use crate::controllers::gui::dashboard::DashAction;
use crate::controllers::gui::dashboard::live_log::{LiveAction, LiveLogVm};
use crate::controllers::gui::dashboard::replay::{ReplayAction, ReplayVm};
use crate::controllers::gui::dashboard::sessions::{DetailTab, SessionDetail, SummaryVm};
use crate::models::domain::display::help as h;
use crate::models::domain::live_log::LiveKind;
use crate::views::layout::flex_columns;
use crate::views::widgets::{cell, help, pal, section, trunc};
use egui::{RichText, Ui};
use egui_phosphor::regular as icon;

/// 詳細を描く。
pub fn show(ui: &mut Ui, d: Option<&SessionDetail>, forms: &mut Forms, acts: &mut Vec<Action>) {
    let Some(d) = d else {
        ui.label(RichText::new("左の一覧からセッションを選んでください").color(pal(ui).weak));
        return;
    };
    let p = pal(ui);
    section(ui, &d.title);
    trunc(ui, RichText::new(&d.meta).small().color(p.weak));
    totals(ui, d);
    ui.horizontal(|ui| {
        for (tab, ic, text) in [
            (DetailTab::Summary, icon::NOTE, "概要"),
            (DetailTab::Turns, icon::LIST, "ターン"),
            (DetailTab::LiveLog, icon::TERMINAL, "ライブログ"),
            (DetailTab::Replay, icon::PLAY, "再生"),
        ] {
            ui.label(RichText::new(ic).color(if d.tab == tab { p.accent } else { p.weak }));
            if ui.selectable_label(d.tab == tab, text).clicked() {
                acts.push(Action::Dash(DashAction::SetDetailTab(tab)));
            }
        }
    });
    ui.separator();
    match d.tab {
        DetailTab::Summary => {
            if let Some(s) = &d.summary {
                summary(ui, s);
            }
        }
        DetailTab::Turns => turns(ui, d, acts),
        DetailTab::LiveLog => match &d.live {
            Some(v) => live(ui, v, forms, acts),
            None => {
                ui.label(
                    RichText::new("ライブログを開けません。セッションの記録が見つかりません")
                        .color(p.warn),
                );
            }
        },
        DetailTab::Replay => match &d.replay {
            Some(v) => replay(ui, v, forms, acts),
            None => {
                ui.label(RichText::new("再生できるターンがありません").color(p.weak));
            }
        },
    }
}

fn summary(ui: &mut Ui, s: &SummaryVm) {
    let p = pal(ui);
    egui::ScrollArea::vertical()
        .id_salt("summary")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.label(RichText::new("要約").strong());
            if s.recaps.is_empty() {
                ui.label(
                    RichText::new(
                        "Claude Codeの要約はありません。離席中に書かれた要約だけを出します",
                    )
                    .color(p.weak),
                );
            }
            timed_lines(ui, &s.recaps);
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for (name, value) in &s.stats {
                    ui.label(RichText::new(*name).small().color(p.weak));
                    ui.label(RichText::new(value).strong());
                    ui.add_space(8.0);
                }
            });
            if s.truncated {
                ui.label(
                    RichText::new("直近10万件のターンから集計しています")
                        .small()
                        .color(p.warn),
                );
            }
            ui.add_space(8.0);
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
            table(
                ui,
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
            );
            ui.add_space(8.0);
            let tools: Vec<[&str; 3]> = s
                .tools
                .iter()
                .map(|(n, c, e)| [n.as_str(), c.as_str(), e.as_str()])
                .collect();
            table(
                ui,
                "summary_tools",
                ["ツール名", "呼び出し", "エラー"],
                &tools,
            );
            ui.add_space(8.0);
            ui.label(RichText::new("依頼の一覧").strong());
            timed_lines(ui, &s.prompts);
        });
}

fn timed_lines(ui: &mut Ui, lines: &[(String, String)]) {
    let p = pal(ui);
    for (t, text) in lines {
        // 時刻と本文を横に並べると、折り返しの幅が時刻の分だけ大きく見積もられ、区画が窓からはみ出す。
        // 1つのラベルにまとめ、区画の幅で折り返す。
        let mut job = egui::text::LayoutJob::default();
        for part in [
            RichText::new(format!("{t}  ")).monospace().color(p.weak),
            RichText::new(text),
        ] {
            part.append_to(
                &mut job,
                ui.style(),
                egui::FontSelection::Default,
                egui::Align::Center,
            );
        }
        ui.add(egui::Label::new(job).wrap());
    }
}

fn table<const N: usize>(ui: &mut Ui, id: &str, head: [&str; N], rows: &[[&str; N]]) {
    let p = pal(ui);
    // モデル表は7列あり、詳細の区画より広くなる。表だけを横に送り、区画が窓からはみ出さないようにする。
    egui::ScrollArea::horizontal().id_salt(id).show(ui, |ui| {
        egui::Grid::new(id).striped(true).show(ui, |ui| {
            for h in head {
                ui.label(RichText::new(h).small().color(p.weak));
            }
            ui.end_row();
            for r in rows {
                for v in r {
                    ui.label(*v);
                }
                ui.end_row();
            }
        });
    });
}

fn totals(ui: &mut Ui, d: &SessionDetail) {
    let p = pal(ui);
    ui.horizontal_wrapped(|ui| {
        for (name, value, tip) in [
            ("入力", &d.totals.input, h::TOKENS_INPUT),
            ("出力", &d.totals.output, h::TOKENS_OUTPUT),
            ("キャッシュ読込", &d.totals.cache_read, h::TOKENS_CACHE_READ),
            (
                "キャッシュ作成",
                &d.totals.cache_write,
                h::TOKENS_CACHE_WRITE,
            ),
            ("コスト", &d.totals.cost, h::COST),
            ("キャッシュヒット率", &d.totals.cache_hit, h::CACHE_HIT),
            (
                "サブエージェントの割合",
                &d.totals.subagent_share,
                h::RUN_KIND,
            ),
        ] {
            ui.label(RichText::new(name).small().color(p.weak));
            help(ui, tip);
            ui.label(RichText::new(value).strong());
            ui.add_space(8.0);
        }
    });
}

/// ターン表の列（時刻、種類、内容、コンテキスト、所要時間、内訳）。内容の列だけが伸びる。
const TURN_COLUMNS: [Option<f32>; 6] = [
    Some(64.0),
    Some(140.0),
    None,
    Some(56.0),
    Some(56.0),
    Some(44.0),
];

fn turns(ui: &mut Ui, d: &SessionDetail, acts: &mut Vec<Action>) {
    let p = pal(ui);
    egui::ScrollArea::vertical()
        .id_salt("turns")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let widths = flex_columns(ui.available_width(), &TURN_COLUMNS);
            for t in &d.turns {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    cell(ui, widths[0], |ui| {
                        ui.label(RichText::new(&t.time).monospace().color(p.weak))
                    });
                    cell(ui, widths[1], |ui| {
                        ui.add_space(12.0 * t.depth as f32);
                        trunc(ui, format!("{}（{}）", t.kind, t.source))
                    });
                    cell(ui, widths[2], |ui| trunc(ui, &t.summary));
                    cell(ui, widths[3], |ui| {
                        ui.label(RichText::new(&t.context).small())
                    });
                    cell(ui, widths[4], |ui| {
                        ui.label(RichText::new(&t.duration).small())
                    });
                    cell(ui, widths[5], |ui| {
                        if ui.small_button("内訳").clicked() {
                            acts.push(Action::Dash(DashAction::ToggleTurn(t.key.clone())));
                        }
                    });
                });
                if let Some(b) = &t.breakdown {
                    ui.horizontal(|ui| {
                        ui.add_space(widths[0] + widths[1]);
                        trunc(ui, RichText::new(b).small().color(p.weak));
                    });
                }
            }
        });
}

fn live(ui: &mut Ui, vm: &LiveLogVm, forms: &mut Forms, acts: &mut Vec<Action>) {
    let p = pal(ui);
    let f = &mut forms.live_filter;
    ui.horizontal_wrapped(|ui| {
        let mut changed = ui.checkbox(&mut f.tools, "ツール").changed();
        changed |= ui.checkbox(&mut f.text, "本文").changed();
        changed |= ui.checkbox(&mut f.errors_only, "エラーのみ").changed();
        egui::ComboBox::from_label("発生元")
            .selected_text(f.source.clone().unwrap_or_else(|| "すべて".into()))
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut f.source, None, "すべて").changed();
                for s in &vm.sources {
                    changed |= ui
                        .selectable_value(&mut f.source, Some(s.clone()), s)
                        .changed();
                }
            });
        if changed {
            acts.push(Action::Live(LiveAction::FilterChanged));
        }
        ui.label(
            RichText::new(format!("{}行を保持（最大2,000行）", vm.total))
                .small()
                .color(p.weak),
        );
    });
    if let Some(n) = &vm.note {
        ui.label(RichText::new(n).color(p.warn));
    }
    let mut area = egui::ScrollArea::vertical()
        .id_salt("live_log")
        .stick_to_bottom(true)
        .auto_shrink([false, false]);
    if vm.jump_to_bottom {
        area = area.vertical_scroll_offset(f32::MAX);
        acts.push(Action::Live(LiveAction::JumpDone));
    }
    let out = area.show(ui, |ui| {
        for l in &vm.lines {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&l.time).monospace().color(p.weak));
                ui.add_space(16.0 * l.depth as f32);
                ui.label(RichText::new(&l.source).strong());
                if let Some(t) = &l.tool {
                    ui.label(RichText::new(t).monospace().color(p.accent));
                }
                let color = if l.kind == LiveKind::ToolError {
                    p.err
                } else {
                    p.text
                };
                trunc(ui, RichText::new(&l.text).color(color));
            });
        }
    });
    // 手で上へスクロールしたときだけ「最新へ戻る」を出す（spec 7.4節）。
    let at_bottom = out.state.offset.y + out.inner_rect.height() >= out.content_size.y - 4.0;
    if !at_bottom && ui.button("最新へ戻る").clicked() {
        acts.push(Action::Live(LiveAction::JumpToLatest));
    }
}

fn replay(ui: &mut Ui, vm: &ReplayVm, forms: &mut Forms, acts: &mut Vec<Action>) {
    let p = pal(ui);
    if vm.len == 0 {
        ui.label(RichText::new("再生できるターンがありません").color(p.weak));
        return;
    }
    let act = |a| Action::Dash(DashAction::Replay(a));
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(format!("{} 前のターン", icon::SKIP_BACK))
            .clicked()
        {
            acts.push(act(ReplayAction::Step(-1)));
        }
        let (ic, text) = if vm.playing {
            (icon::PAUSE, "一時停止")
        } else {
            (icon::PLAY, "再生する")
        };
        if ui.button(format!("{ic} {text}")).clicked() {
            acts.push(act(ReplayAction::TogglePlay));
        }
        if ui
            .button(format!("{} 次のターン", icon::SKIP_FORWARD))
            .clicked()
        {
            acts.push(act(ReplayAction::Step(1)));
        }
        for s in [1, 4, 16] {
            if ui
                .selectable_label(vm.speed == s, format!("{s}倍"))
                .clicked()
            {
                acts.push(act(ReplayAction::SetSpeed(s)));
            }
        }
        ui.label(RichText::new(format!("{} / {}", vm.position + 1, vm.len)).strong());
    });
    let slider = egui::Slider::new(&mut forms.replay_position, 0..=vm.len - 1).show_value(false);
    if ui.add_sized([ui.available_width(), 20.0], slider).changed() {
        acts.push(act(ReplayAction::Seek(forms.replay_position)));
    }
    if let Some(c) = &vm.current {
        trunc(ui, RichText::new(format!("現在: {}", c.summary)).strong());
        trunc(
            ui,
            RichText::new(format!(
                "{} · {}（{}） · コンテキスト {}",
                c.time, c.kind, c.source, c.context
            ))
            .small()
            .color(p.weak),
        );
    }
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("replay")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for t in &vm.recent {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&t.time).monospace().color(p.weak));
                    ui.add_space(12.0 * t.depth as f32);
                    ui.label(format!("{}（{}）", t.kind, t.source));
                    trunc(ui, &t.summary);
                });
            }
        });
}
