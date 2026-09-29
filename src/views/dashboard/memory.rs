//! Desktopのカードと、終了の要求の確認ダイアログ。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::dashboard::memory::{ConfirmExit, DesktopMemoryVm, MemAction};
use crate::models::domain::display::help as h;
use crate::views::theme::card_frame;
use crate::views::widgets::{help, pal};
use egui::{RichText, Ui};

/// Desktopのカード。
pub fn desktop(ui: &mut Ui, d: &DesktopMemoryVm, acts: &mut Vec<Action>) {
    card_frame(pal(ui), false).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Claude Desktop").strong());
            ui.label(&d.text);
            help(ui, h::MEMORY);
            if ui.small_button("Desktopを終了").clicked() {
                acts.push(Action::Memory(MemAction::Ask(d.exit.clone())));
            }
        });
    });
}

/// 確認ダイアログ。枠の外を押すなどして閉じたときはキャンセルとして扱う。
pub fn confirm(ui: &mut Ui, c: &ConfirmExit, acts: &mut Vec<Action>) {
    let p = pal(ui);
    let before = acts.len();
    let m = egui::Modal::new(egui::Id::new("confirm_exit")).show(ui.ctx(), |ui| {
        ui.label(RichText::new(format!("{} を終了します（{}）", c.title, c.memory)).strong());
        ui.label(RichText::new("実行中の編集が失われることがあります").color(p.warn));
        if let Some(cmd) = &c.resume {
            ui.label(format!("再開するには次を実行します: {cmd}"));
        }
        ui.horizontal(|ui| {
            if ui.button("終了する").clicked() {
                acts.push(Action::Memory(MemAction::Confirm));
            }
            if ui.button("やめる").clicked() {
                acts.push(Action::Memory(MemAction::Cancel));
            }
        });
    });
    if m.should_close() && acts.len() == before {
        acts.push(Action::Memory(MemAction::Cancel));
    }
}
