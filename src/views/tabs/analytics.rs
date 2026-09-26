//! 分析タブの描画。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::tabs::analytics::{AnalyticsVm, GroupRow, Period};
use crate::models::domain::display::help as h;
use crate::views::layout::{GAP, card_columns, flex_columns};
use crate::views::theme::card_frame;
use crate::views::widgets::{cell, help, pal, section, trunc, usage_bar};
use egui::{RichText, Ui};

/// 分析タブを描く。
pub fn show(ui: &mut Ui, vm: &AnalyticsVm, acts: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        section(ui, "分析");
        for p in [Period::Days7, Period::Days30] {
            if ui.selectable_label(vm.period == p, p.label()).clicked() {
                acts.push(Action::SetPeriod(p));
            }
        }
        ui.label(RichText::new(&vm.agent_share).strong());
        help(ui, h::RUN_KIND);
    });
    ui.add_space(GAP / 2.0);
    let groups = [
        ("プロジェクト別", &vm.by_project),
        ("モデル別", &vm.by_model),
        ("ブランチ別", &vm.by_branch),
    ];
    let (cols, width) = card_columns(ui.available_width(), groups.len());
    for row in groups.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (title, rows) in row {
                ui.vertical(|ui| {
                    ui.set_width(width);
                    group_card(ui, title, rows);
                });
            }
        });
        ui.add_space(GAP);
    }
    let (cols, width) = card_columns(ui.available_width(), 2);
    let halves: [&dyn Fn(&mut Ui); 2] = [&|ui| cache_card(ui, vm), &|ui| tools_card(ui, vm)];
    for row in halves.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for f in row {
                ui.vertical(|ui| {
                    ui.set_width(width);
                    f(ui);
                });
            }
        });
        ui.add_space(GAP);
    }
}

/// 列（軸の値、割合の棒、Token、コスト）。軸の値の列だけが伸びる。
const GROUP_COLUMNS: [Option<f32>; 4] = [None, Some(96.0), Some(56.0), Some(64.0)];

fn group_card(ui: &mut Ui, title: &str, rows: &[GroupRow]) {
    let p = pal(ui);
    card_frame(p, false).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section(ui, title);
        if rows.is_empty() {
            ui.label(RichText::new("この期間の記録はありません").color(p.weak));
        }
        let widths = flex_columns(ui.available_width(), &GROUP_COLUMNS);
        for r in rows {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP / 2.0;
                cell(ui, widths[0], |ui| trunc(ui, &r.key));
                cell(ui, widths[1], |ui| usage_bar(ui, r.share, p.accent));
                cell(ui, widths[2], |ui| {
                    ui.label(RichText::new(&r.tokens).small())
                });
                cell(ui, widths[3], |ui| ui.label(RichText::new(&r.cost).small()));
            });
        }
    });
}

fn cache_card(ui: &mut Ui, vm: &AnalyticsVm) {
    let p = pal(ui);
    card_frame(p, false).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            section(ui, "キャッシュヒット率（日ごと）");
            help(ui, h::CACHE_HIT);
        });
        ui.horizontal_wrapped(|ui| {
            for (day, rate) in &vm.daily_cache {
                ui.label(RichText::new(day).small().color(p.weak));
                ui.label(RichText::new(rate).strong());
                ui.add_space(8.0);
            }
        });
    });
}

/// 列（ツール、回数、エラー、エラー率）。ツールの列だけが伸びる。
const TOOL_COLUMNS: [Option<f32>; 4] = [None, Some(56.0), Some(56.0), Some(64.0)];

fn tools_card(ui: &mut Ui, vm: &AnalyticsVm) {
    let p = pal(ui);
    card_frame(p, false).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section(ui, "ツール");
        let widths = flex_columns(ui.available_width(), &TOOL_COLUMNS);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP / 2.0;
            for (w, t) in widths.iter().zip(["ツール", "回数", "エラー", "エラー率"]) {
                cell(ui, *w, |ui| {
                    ui.label(RichText::new(t).small().color(p.weak))
                });
            }
        });
        for t in &vm.tools {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP / 2.0;
                cell(ui, widths[0], |ui| trunc(ui, &t.name));
                cell(ui, widths[1], |ui| ui.label(&t.calls));
                cell(ui, widths[2], |ui| ui.label(&t.errors));
                cell(ui, widths[3], |ui| ui.label(&t.error_rate));
            });
        }
    });
}
