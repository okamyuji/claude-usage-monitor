//! プロファイルタブの描画。
use crate::controllers::gui::app::{Action, Forms};
use crate::controllers::gui::tabs::profiles::{ProfileRow, ProfilesAction, ProfilesVm};
use crate::views::layout::{GAP, flex_columns};
use crate::views::theme::card_frame;
use crate::views::widgets::{badge, cell, pal, section, trunc};
use egui::{RichText, Ui};

/// 列（名前、設定ディレクトリ、トークン、契約、操作）。設定ディレクトリの列だけが伸びる。
const COLUMNS: [Option<f32>; 5] = [Some(140.0), None, Some(200.0), Some(64.0), Some(180.0)];

/// プロファイルタブを描く。
pub fn show(ui: &mut Ui, vm: &ProfilesVm, forms: &mut Forms, acts: &mut Vec<Action>) {
    let p = pal(ui);
    card_frame(p, false).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section(ui, "プロファイル");
        let widths = flex_columns(ui.available_width(), &COLUMNS);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP / 2.0;
            for (w, t) in widths.iter().zip(["名前", "設定ディレクトリ", "トークン", "契約", ""]) {
                cell(ui, *w, |ui| ui.label(RichText::new(t).small().color(p.weak)));
            }
        });
        for r in &vm.rows {
            row(ui, r, &widths, acts);
        }
        if let Some(name) = &vm.pending_remove {
            ui.add_space(GAP / 2.0);
            ui.label(RichText::new(format!("{name} を削除すると、このプロファイルの使用量とセッションの記録も消えます。設定ディレクトリは残ります")).color(p.warn));
            ui.horizontal(|ui| {
                if ui.button("本当に削除する").clicked() {
                    acts.push(Action::Profiles(ProfilesAction::ConfirmRemove));
                }
                if ui.button("やめる").clicked() {
                    acts.push(Action::Profiles(ProfilesAction::CancelRemove));
                }
            });
        }
    });
    ui.add_space(GAP);
    card_frame(p, false).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section(ui, "プロファイルを追加");
        let w = (ui.available_width() - 160.0).max(120.0);
        ui.horizontal(|ui| {
            let l = ui.label("プロファイル名");
            ui.add(egui::TextEdit::singleline(&mut forms.new_profile_name).desired_width(w))
                .labelled_by(l.id);
        });
        ui.horizontal(|ui| {
            let l = ui.label("設定ディレクトリ");
            ui.add(
                egui::TextEdit::singleline(&mut forms.new_profile_dir)
                    .desired_width(w)
                    .hint_text("空なら ~/.claude-<名前>"),
            )
            .labelled_by(l.id);
        });
        if ui.button("追加").clicked() {
            acts.push(Action::Profiles(ProfilesAction::Add));
        }
        if let Some(m) = &vm.message {
            ui.label(m);
        }
        trunc(ui, RichText::new(vm.login_help).small().color(p.weak));
    });
}

fn row(ui: &mut Ui, r: &ProfileRow, widths: &[f32], acts: &mut Vec<Action>) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP / 2.0;
        cell(ui, widths[0], |ui| {
            ui.label(RichText::new(&r.name).strong());
            if r.active {
                badge(ui, "使用中", p.accent, p.accent_soft);
            }
        });
        cell(ui, widths[1], |ui| trunc(ui, RichText::new(&r.dir).small()));
        cell(ui, widths[2], |ui| {
            trunc(ui, RichText::new(&r.token).small())
        });
        cell(ui, widths[3], |ui| ui.label(RichText::new(&r.plan).small()));
        cell(ui, widths[4], |ui| {
            if !r.active && ui.button("使用中にする").clicked() {
                acts.push(Action::Profiles(ProfilesAction::Use(r.name.clone())));
            }
            if !r.active && ui.button("削除").clicked() {
                acts.push(Action::Profiles(ProfilesAction::Remove(r.name.clone())));
            }
        });
    });
}
