//! 上部バー。タブ、今日と今週の集計、デーモンの状態、テーマの切り替え。
use crate::controllers::gui::app::{Action, AppVm, Tab};
use crate::models::domain::settings::Theme;
use crate::views::widgets::{badge, pal};
use egui::{Align, Layout, RichText, Ui};
use egui_phosphor::regular as icon;

fn tab_icon(t: Tab) -> &'static str {
    match t {
        Tab::Dashboard => icon::SQUARES_FOUR,
        Tab::Analytics => icon::CHART_BAR,
        Tab::Profiles => icon::USERS,
        Tab::Settings => icon::GEAR,
        Tab::Diagnostics => icon::STETHOSCOPE,
    }
}

/// 上部バーを描く。
pub fn show(ui: &mut Ui, vm: &AppVm, acts: &mut Vec<Action>) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        for t in Tab::ALL {
            let selected = vm.tab == t;
            ui.label(RichText::new(tab_icon(t)).color(if selected { p.accent } else { p.weak }));
            if ui.selectable_label(selected, t.label()).clicked() {
                acts.push(Action::SelectTab(t));
            }
            ui.add_space(6.0);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (next, text) = if ui.visuals().dark_mode {
                (Theme::Light, format!("{} ライト", icon::SUN))
            } else {
                (Theme::Dark, format!("{} ダーク", icon::MOON))
            };
            if ui.button(text).clicked() {
                acts.push(Action::SetTheme(next));
            }
            daemon(ui, vm, acts);
            totals(ui, vm);
        });
    });
}

fn daemon(ui: &mut Ui, vm: &AppVm, acts: &mut Vec<Action>) {
    let p = pal(ui);
    if vm.daemon_running {
        badge(ui, "● 監視中", p.ok, p.ok_soft);
    } else {
        if ui.button("デーモンを起動").clicked() {
            acts.push(Action::StartDaemon);
        }
        badge(ui, "● デーモン停止中", p.err, p.err_soft);
    }
}

fn totals(ui: &mut Ui, vm: &AppVm) {
    let p = pal(ui);
    for (label, value) in [("今週", &vm.header.week), ("今日", &vm.header.today)] {
        if value.is_empty() {
            continue;
        }
        ui.label(RichText::new(value).strong());
        ui.label(RichText::new(label).color(p.weak));
    }
}
