//! カレンダータブの描画。上に週の操作と凡例、左に週間カレンダー、右に選んだセッションの詳細。
use crate::controllers::gui::app::{Action, Forms};
use crate::controllers::gui::dashboard::DashAction;
use crate::controllers::gui::dashboard::sessions::DetailTab;
use crate::controllers::gui::tabs::calendar::{BandVm, CalendarAction, CalendarVm, ColorBy};
use crate::views::dashboard::{calendar_ratio, detail, split};
use crate::views::layout::GAP;
use crate::views::theme::series;
use crate::views::widgets::{pal, section};
use egui::{
    Align2, Button, FontId, Painter, Rect, RichText, Sense, Stroke, StrokeKind, Ui, WidgetInfo,
    WidgetType, pos2, vec2,
};
use egui_phosphor::regular as icon;

/// 1時間の高さ。
const HOUR_PX: f32 = 40.0;
/// 左端の時刻の目盛りの幅。
const AXIS_PX: f32 = 44.0;
/// 日の列の左端に置く濃淡の幅。
const DENSITY_PX: f32 = 6.0;
/// 曜日の見出しの高さ。
const HEAD_PX: f32 = 22.0;

/// カレンダータブを描く。
pub fn show(ui: &mut Ui, vm: &CalendarVm, forms: &mut Forms, acts: &mut Vec<Action>) {
    toolbar(ui, vm, acts);
    legend(ui, vm);
    ui.add_space(GAP / 2.0);
    let h = ui.available_height();
    split(
        ui,
        forms,
        h,
        calendar_ratio,
        acts,
        |ui, _, acts| grid(ui, vm, acts),
        |ui, forms, acts| match &vm.detail {
            Some(d) => detail::show(ui, Some(d), forms, acts),
            None => {
                ui.label(RichText::new("帯を押すと詳細を出します").color(pal(ui).weak));
            }
        },
    );
}

fn toolbar(ui: &mut Ui, vm: &CalendarVm, acts: &mut Vec<Action>) {
    let p = pal(ui);
    let mut push = |a| acts.push(Action::Calendar(a));
    ui.horizontal(|ui| {
        if ui.button("今週へ").clicked() {
            push(CalendarAction::ThisWeek);
        }
        if ui.button(format!("{} 前週", icon::CARET_LEFT)).clicked() {
            push(CalendarAction::PrevWeek);
        }
        let next = Button::new(format!("次週 {}", icon::CARET_RIGHT));
        if ui.add_enabled(vm.can_next, next).clicked() {
            push(CalendarAction::NextWeek);
        }
        section(ui, &vm.title);
        ui.label(RichText::new(format!("{} セッション", vm.session_count)).color(p.weak));
        // 隠したヘッドレスがある週は空ではないので、件数の合計で判定する。
        if vm.session_count + vm.headless_count == 0 {
            ui.label(RichText::new("この週の記録はありません").color(p.weak));
        }
        ui.separator();
        ui.label(RichText::new("色分け").color(p.weak));
        for c in [ColorBy::Project, ColorBy::Profile] {
            if ui.selectable_label(vm.color_by == c, c.label()).clicked() {
                push(CalendarAction::SetColorBy(c));
            }
        }
        ui.separator();
        let mut show = vm.show_headless;
        let text = format!("ヘッドレスも出す（{}件）", vm.headless_count);
        if ui.checkbox(&mut show, text).changed() {
            push(CalendarAction::ToggleHeadless);
        }
    });
}

fn legend(ui: &mut Ui, vm: &CalendarVm) {
    let p = pal(ui);
    ui.horizontal_wrapped(|ui| {
        for item in &vm.legend {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, series(item.color, p));
            ui.label(format!("{} {}", item.label, item.count));
            ui.add_space(6.0);
        }
    });
}

fn grid(ui: &mut Ui, vm: &CalendarVm, acts: &mut Vec<Action>) {
    let p = pal(ui);
    let w = ui.available_width();
    let col = ((w - AXIS_PX) / 7.0).max(1.0);
    let (head, _) = ui.allocate_exact_size(vec2(w, HEAD_PX), Sense::hover());
    for (i, d) in vm.days.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(head.left() + AXIS_PX + col * i as f32, head.top()),
            vec2(col, HEAD_PX),
        );
        let c = if d.today { p.accent } else { p.text };
        ui.put(
            r,
            egui::Label::new(RichText::new(&d.label).color(c).small()),
        );
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(w, HOUR_PX * 24.0), Sense::hover());
            let painter = ui.painter_at(rect);
            let x_of = |i: usize| rect.left() + AXIS_PX + col * i as f32;
            let y_of = |min: u32| rect.top() + min as f32 / 60.0 * HOUR_PX;
            for (i, d) in vm.days.iter().enumerate() {
                if d.today {
                    let r = Rect::from_x_y_ranges(x_of(i)..=x_of(i + 1), rect.y_range());
                    painter.rect_filled(r, 0.0, p.accent_soft);
                }
            }
            for h in 0..24u32 {
                let y = y_of(h * 60);
                painter.hline(
                    rect.left() + AXIS_PX..=rect.right(),
                    y,
                    Stroke::new(0.5, p.border),
                );
                if h > 0 {
                    let at = pos2(rect.left() + AXIS_PX - 4.0, y);
                    let font = FontId::proportional(11.0);
                    painter.text(at, Align2::RIGHT_CENTER, format!("{h}:00"), font, p.weak);
                }
            }
            for i in 0..=7 {
                painter.vline(x_of(i), rect.y_range(), Stroke::new(0.5, p.border));
            }
            for (i, d) in vm.days.iter().enumerate() {
                for (b, v) in d.density.iter().enumerate() {
                    if *v > 0.0 {
                        let r = Rect::from_min_size(
                            pos2(x_of(i), y_of(b as u32 * 10)),
                            vec2(DENSITY_PX, HOUR_PX / 6.0),
                        );
                        // 1ターンだけの区画も見えるよう、薄さに下限を置く。
                        painter.rect_filled(r, 0.0, p.ok.gamma_multiply(v.max(0.15)));
                    }
                }
                for b in &d.bands {
                    let r = band_rect(b, x_of(i), col, y_of);
                    let selected = vm.selected.as_deref() == Some(b.session_id.as_str());
                    band(ui, &painter, b, r, selected, i, acts);
                }
            }
            if let Some((day, min)) = vm.now {
                painter.hline(
                    x_of(day)..=x_of(day + 1),
                    y_of(min),
                    Stroke::new(1.5, p.err),
                );
            }
        });
}

/// 帯の矩形。列の幅から濃淡の幅を引き、残りを列数で割る。
fn band_rect(b: &BandVm, x: f32, col: f32, y_of: impl Fn(u32) -> f32) -> Rect {
    let inner = (col - DENSITY_PX - 2.0).max(1.0);
    let lane_w = inner / b.lanes.max(1) as f32;
    let left = x + DENSITY_PX + 1.0 + lane_w * b.lane as f32;
    Rect::from_min_max(
        pos2(left, y_of(b.start)),
        pos2(left + (lane_w - 1.0).max(1.0), y_of(b.end)),
    )
}

fn band(
    ui: &mut Ui,
    painter: &Painter,
    b: &BandVm,
    r: Rect,
    selected: bool,
    day: usize,
    acts: &mut Vec<Action>,
) {
    let color = series(b.color, pal(ui));
    let id = ui.id().with(("band", day, &b.session_id, b.start));
    let resp = ui.interact(r, id, Sense::click());
    // 帯は文字を持たないので、説明を部品名にして読み上げと検索ができるようにする。
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &b.tooltip));
    let fill = if resp.hovered() { 0.55 } else { 0.35 };
    painter.rect_filled(r, 3.0, color.gamma_multiply(fill));
    let width = if selected { 2.5 } else { 1.0 };
    painter.rect_stroke(r, 3.0, Stroke::new(width, color), StrokeKind::Inside);
    if resp.clicked() {
        acts.push(Action::Dash(DashAction::Select(
            b.session_id.clone(),
            DetailTab::Turns,
        )));
    }
    resp.on_hover_text(&b.tooltip);
}
