//! 選んだセッションの詳細（spec 7.2節の中段の右）。見出し、合計、ターン、ライブログ、再生。
use crate::controllers::gui::app::{Action, Forms};
use crate::controllers::gui::dashboard::DashAction;
use crate::controllers::gui::dashboard::sessions::{DetailTab, SessionDetail};
use crate::models::domain::display::help as h;
use crate::views::layout::flex_columns;
use crate::views::widgets::{cell, help, pal, section, trunc};
use egui::{RichText, Ui};
use egui_phosphor::regular as icon;

/// 詳細を描く。
#[allow(unused_variables)]
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
        DetailTab::Turns => turns(ui, d, acts),
        DetailTab::LiveLog => {
            ui.label(RichText::new("ライブログを準備しています").color(p.weak));
        }
        DetailTab::Replay => {
            ui.label(RichText::new("再生を準備しています").color(p.weak));
        }
    }
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
