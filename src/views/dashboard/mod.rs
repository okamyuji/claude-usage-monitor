//! ダッシュボードの配置（spec 7.2節の3段）。区画の中身は同じディレクトリの各ファイルが描く。
pub mod cards;
pub mod detail;
pub mod sessions;
pub mod trend;

use crate::controllers::gui::app::{Action, Forms};
use crate::controllers::gui::dashboard::DashboardVm;
use crate::views::layout::{
    GAP, SPLITTER, TREND_COLLAPSED, drag_ratio, middle_height, split_widths, trend_height,
};
use crate::views::theme::card_frame;
use crate::views::widgets::pal;
use egui::{Align, Layout, Sense, Ui, vec2};

/// ダッシュボードを描く。
pub fn show(ui: &mut Ui, vm: &DashboardVm, forms: &mut Forms, acts: &mut Vec<Action>) {
    let total = ui.available_height();
    cards::show(ui, &vm.cards, acts);
    ui.add_space(GAP);
    let trend_h = if vm.trend_open {
        trend_height(total)
    } else {
        TREND_COLLAPSED
    };
    let mid_h = middle_height(ui.available_height(), trend_h);
    split(
        ui,
        forms,
        mid_h,
        acts,
        |ui, forms, acts| sessions::show(ui, vm, forms, acts),
        |ui, forms, acts| detail::show(ui, vm.detail.as_ref(), forms, acts),
    );
    ui.add_space(GAP);
    trend_area(ui, vm, trend_h, acts);
}

/// 大きさを決めた領域にカードの枠を描き、中身で領域を埋める。
fn filled_card(ui: &mut Ui, size: egui::Vec2, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
        card_frame(pal(ui), false).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            add(ui);
        });
    });
}

/// 中段を左右に分けて描く。境界のつまみをドラッグすると比率が変わり、ウィンドウの幅に比例して伸縮する。
/// 左右の描画にも入力欄の状態を渡すため、クロージャは`forms`を引数で受け取る。
pub fn split(
    ui: &mut Ui,
    forms: &mut Forms,
    height: f32,
    acts: &mut Vec<Action>,
    left: impl FnOnce(&mut Ui, &mut Forms, &mut Vec<Action>),
    right: impl FnOnce(&mut Ui, &mut Forms, &mut Vec<Action>),
) {
    let avail = ui.available_width();
    let (lw, rw) = split_widths(avail, forms.split_ratio);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        filled_card(ui, vec2(lw, height), |ui| left(ui, forms, acts));
        let (rect, resp) = ui.allocate_exact_size(vec2(SPLITTER, height), Sense::drag());
        let resp = resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        let line = if resp.hovered() || resp.dragged() {
            pal(ui).accent
        } else {
            pal(ui).border
        };
        ui.painter().vline(
            rect.center().x,
            rect.y_range(),
            egui::Stroke::new(1.0, line),
        );
        if resp.dragged() {
            forms.split_ratio = drag_ratio(avail, lw, resp.drag_delta().x);
        }
        filled_card(ui, vec2(rw, height), |ui| right(ui, forms, acts));
    });
}

fn trend_area(ui: &mut Ui, vm: &DashboardVm, height: f32, acts: &mut Vec<Action>) {
    let w = ui.available_width();
    filled_card(ui, vec2(w, height), |ui| trend::show(ui, vm, acts));
}
