//! 使用率カード（spec 7.2節の上段）。空き幅をプロファイル数で等分し、狭いときは次の行へ折り返す。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::dashboard::cards::{LimitView, ProfileCard};
use crate::controllers::gui::tabs::profiles::ProfilesAction;
use crate::models::domain::display::help as h;
use crate::views::layout::{GAP, MIN_LIMIT, card_columns, columns};
use crate::views::theme::card_frame;
use crate::views::widgets::{badge, help, pal, section, severity_color, trunc, usage_bar};
use egui::{Align, Layout, RichText, Ui};

/// カードを並べる。
pub fn show(ui: &mut Ui, cards: &[ProfileCard], acts: &mut Vec<Action>) {
    section(ui, "レート制限");
    let (cols, width) = card_columns(ui.available_width(), cards.len());
    for row in cards.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for c in row {
                ui.vertical(|ui| {
                    ui.set_width(width);
                    card(ui, c, acts);
                });
            }
        });
        ui.add_space(GAP / 2.0);
    }
}

fn card(ui: &mut Ui, c: &ProfileCard, acts: &mut Vec<Action>) {
    let p = pal(ui);
    card_frame(p, c.is_active).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(&c.name).strong());
            if c.is_active {
                badge(ui, "使用中", p.accent, p.accent_soft);
            } else if ui.small_button("使用中にする").clicked() {
                acts.push(Action::Profiles(ProfilesAction::Use(c.name.clone())));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(&c.fetched).small().color(p.weak));
            });
        });
        if let Some(problem) = &c.problem {
            trunc(ui, RichText::new(problem).color(p.err));
        }
        // 幅の広いカードではバーを横に並べ、カードの高さを抑えて中段のセッション一覧に高さを回す。
        let (cols, width) = columns(ui.available_width(), c.limits.len(), MIN_LIMIT);
        for row in c.limits.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for l in row {
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        limit(ui, l);
                    });
                }
            });
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
