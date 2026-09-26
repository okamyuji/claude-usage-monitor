//! 推移グラフ（spec 7.2節の下段）。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::dashboard::trend::{TrendPanel, TrendRange, TrendsVm};
use crate::controllers::gui::dashboard::{DashAction, DashboardVm};
use crate::views::layout::GAP;
use crate::views::widgets::{help, pal, section};
use chrono::{DateTime, FixedOffset};
use egui::{RichText, Ui};
use egui_phosphor::regular as icon;
use egui_plot::{HLine, Legend, Line, Plot, PlotPoints, VLine};

/// 推移の区画を描く。見出しの行は折りたたみ中も出す。
pub fn show(ui: &mut Ui, vm: &DashboardVm, acts: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let (caret, text) = if vm.trend_open {
            (icon::CARET_DOWN, "折りたたむ")
        } else {
            (icon::CARET_RIGHT, "開く")
        };
        ui.label(caret);
        section(ui, "推移");
        for r in [TrendRange::Hours5, TrendRange::Hours24, TrendRange::Days7] {
            if ui
                .selectable_label(vm.trend_range == r, r.label())
                .clicked()
            {
                acts.push(Action::Dash(DashAction::SetTrendRange(r)));
            }
        }
        if ui.small_button(text).clicked() {
            acts.push(Action::Dash(DashAction::ToggleTrend));
        }
    });
    let Some(t) = &vm.trend else {
        return;
    };
    let width = ((ui.available_width() - GAP) / t.panels.len().max(1) as f32).max(0.0);
    let height = ui.available_height();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for p in &t.panels {
            ui.vertical(|ui| {
                ui.set_width(width);
                panel(ui, t, p, height);
            });
        }
    });
}

/// グラフのⓘの説明。目印の線は凡例に出すと低いグラフで凡例がはみ出すため、名前を空にして凡例から外し、ここで意味を伝える。
fn graph_help(base: &str) -> String {
    format!("{base}。縦の灰色の線はリセット時刻、横の線は通知閾値です")
}

fn panel(ui: &mut Ui, t: &TrendsVm, p: &TrendPanel, height: f32) {
    let c = pal(ui);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{}のグラフ", p.title))
                .small()
                .color(c.weak),
        );
        help(ui, &graph_help(p.help));
    });
    let tz =
        FixedOffset::east_opt(t.tz_offset_secs).unwrap_or(FixedOffset::east_opt(0).expect("UTC"));
    let plot_h = (height - ui.spacing().interact_size.y).max(60.0);
    Plot::new(&p.title)
        .height(plot_h)
        // 凡例はプロファイルごとに1行増える。本文の大きさでは、既定のウィンドウの高さで数行でもグラフの下端からはみ出す。
        .legend(Legend::default().text_style(egui::TextStyle::Small))
        .include_y(0.0)
        .include_y(100.0)
        .include_x(t.x_min)
        .include_x(t.x_max)
        .x_axis_formatter(move |mark, _| {
            DateTime::from_timestamp(mark.value as i64, 0)
                .map(|x| x.with_timezone(&tz).format("%m/%d %H:%M").to_string())
                .unwrap_or_default()
        })
        .show(ui, |plot| {
            for s in &p.series {
                plot.line(Line::new(
                    s.name.clone(),
                    PlotPoints::from(s.points.clone()),
                ));
            }
            for r in &p.resets {
                plot.vline(VLine::new("", *r).color(c.weak));
            }
            plot.hline(HLine::new("", t.threshold).color(c.warn));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_help_explains_marker_lines() {
        let s = graph_help("直近5時間の割合です");
        assert!(s.starts_with("直近5時間の割合です。"));
        assert!(s.contains("縦の灰色の線はリセット時刻"));
        assert!(s.contains("横の線は通知閾値"));
    }
}
