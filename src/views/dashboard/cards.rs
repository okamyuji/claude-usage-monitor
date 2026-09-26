//! 使用率カード（spec 7.2節の上段）。空き幅をプロファイル数で等分し、狭いときは次の行へ折り返す。
use crate::controllers::gui::dashboard::cards::{LimitView, ProfileCard};
use crate::models::domain::display::help as h;
use crate::views::layout::{GAP, card_columns};
use crate::views::theme::card_frame;
use crate::views::widgets::{badge, help, pal, section, severity_color, trunc, usage_bar};
use egui::{Align, Layout, RichText, Ui};

/// カードを並べる。
pub fn show(ui: &mut Ui, cards: &[ProfileCard]) {
    section(ui, "レート制限");
    let (cols, width) = card_columns(ui.available_width(), cards.len());
    for row in cards.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for c in row {
                ui.vertical(|ui| {
                    ui.set_width(width);
                    card(ui, c);
                });
            }
        });
        ui.add_space(GAP / 2.0);
    }
}

fn card(ui: &mut Ui, c: &ProfileCard) {
    let p = pal(ui);
    card_frame(p, c.is_active).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(&c.name).strong());
            if c.is_active {
                badge(ui, "使用中", p.accent, p.accent_soft);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(&c.fetched).small().color(p.weak));
            });
        });
        if let Some(problem) = &c.problem {
            trunc(ui, RichText::new(problem).color(p.err));
        }
        for l in &c.limits {
            limit(ui, l);
        }
        if !c.breakdown.is_empty() {
            ui.horizontal(|ui| {
                trunc(
                    ui,
                    RichText::new(format!("用途 {}", c.breakdown.join(" / ")))
                        .small()
                        .color(p.weak),
                );
                help(ui, h::BREAKDOWN);
            });
        }
        if let Some(s) = &c.spend {
            ui.horizontal(|ui| {
                trunc(
                    ui,
                    RichText::new(format!("追加課金 {s}")).small().color(p.weak),
                );
                help(ui, h::SPEND);
            });
        }
    });
}

fn limit(ui: &mut Ui, l: &LimitView) {
    let p = pal(ui);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(&l.title);
        help(ui, l.help);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(&l.percent).strong());
        });
    });
    usage_bar(ui, l.ratio, severity_color(l.severity, p));
    trunc(ui, RichText::new(&l.reset).small().color(p.weak));
    if let Some(proj) = &l.projection {
        ui.horizontal(|ui| {
            trunc(ui, RichText::new(proj).small().color(p.weak));
            help(ui, h::PROJECTION);
        });
    }
}
