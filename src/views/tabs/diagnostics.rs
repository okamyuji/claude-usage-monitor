//! 診断タブの描画。
use crate::controllers::gui::tabs::diagnostics::DiagnosticsVm;
use crate::models::domain::display::help as h;
use crate::views::layout::{GAP, flex_columns};
use crate::views::theme::card_frame;
use crate::views::widgets::{cell, help, pal, section, trunc};
use chrono::{DateTime, FixedOffset};
use egui::{RichText, Ui};
use egui_plot::{Line, Plot, PlotPoints};

/// 列（対象、時刻、結果、詳細）。詳細の列だけが伸びる。
const COLUMNS: [Option<f32>; 4] = [Some(180.0), Some(72.0), Some(56.0), None];

/// 診断タブを描く。
pub fn show(ui: &mut Ui, vm: &DiagnosticsVm) {
    let p = pal(ui);
    card_frame(p, false).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section(ui, "取得の状況");
        let widths = flex_columns(ui.available_width(), &COLUMNS);
        for f in &vm.fetches {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP / 2.0;
                cell(ui, widths[0], |ui| trunc(ui, &f.target));
                cell(ui, widths[1], |ui| ui.label(RichText::new(&f.at).small()));
                cell(ui, widths[2], |ui| {
                    ui.label(RichText::new(&f.result).color(if f.failed { p.err } else { p.ok }))
                });
                cell(ui, widths[3], |ui| {
                    trunc(ui, RichText::new(&f.detail).small())
                });
            });
        }
        ui.label(RichText::new(&vm.ingest).small().color(p.weak));
    });
    ui.add_space(GAP);
    card_frame(p, false).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            section(ui, &format!("デーモンのメモリ（最新 {}）", vm.rss_latest));
            help(ui, h::RSS);
        });
        let tz = FixedOffset::east_opt(vm.tz_offset_secs)
            .unwrap_or(FixedOffset::east_opt(0).expect("UTC"));
        Plot::new("rss")
            .height(180.0)
            .include_y(0.0)
            .x_axis_formatter(move |m, _| {
                DateTime::from_timestamp(m.value as i64, 0)
                    .map(|t| t.with_timezone(&tz).format("%m/%d %H:%M").to_string())
                    .unwrap_or_default()
            })
            .show(ui, |pl| {
                pl.line(Line::new(
                    "RSS（MB）",
                    PlotPoints::from(vm.rss_points.clone()),
                ))
            });
        ui.label(format!("DBサイズ {}", vm.db_size));
    });
}
