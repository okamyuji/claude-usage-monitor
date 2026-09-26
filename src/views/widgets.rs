//! 画面をまたいで使う小さな部品。
use crate::models::domain::display::Severity;
use crate::views::theme::{Palette, palette};
use egui::{Color32, CornerRadius, RichText, Ui};

/// 表示中のテーマの色。
pub fn pal(ui: &Ui) -> &'static Palette {
    palette(ui.visuals().dark_mode)
}

/// 指標名の横に置く「ⓘ」。説明はツールチップに出す（spec 7.1節）。
pub fn help(ui: &mut Ui, text: &str) {
    let c = pal(ui).weak;
    ui.label(RichText::new("ⓘ").color(c)).on_hover_text(text);
}

/// 淡い地と濃い文字のバッジ（spec 7.3節）。
pub fn badge(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(fg).small().strong());
        });
}

/// 使用率の段階の色（spec 7.3節）。
pub fn severity_color(s: Severity, p: &Palette) -> Color32 {
    match s {
        Severity::Normal => p.accent,
        Severity::Warning => p.warn,
        Severity::Critical => p.err,
    }
}

/// 空き幅いっぱいに伸びる使用率バー。
pub fn usage_bar(ui: &mut Ui, ratio: f32, color: Color32) {
    let track = pal(ui).track;
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
    let mut fill = rect;
    fill.set_width(rect.width() * ratio.clamp(0.0, 1.0));
    ui.painter().rect_filled(rect, CornerRadius::same(3), track);
    ui.painter().rect_filled(fill, CornerRadius::same(3), color);
}

/// 長い文言を末尾で省略する。省略したときはeguiの標準の動作でホバー時に全文が出る。
pub fn trunc(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Label::new(text).truncate())
}

/// 区画の見出し。
pub fn section(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).heading().strong());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::theme::{DARK, LIGHT};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn parts_render_their_text() {
        let h = Harness::new_ui(|ui| {
            section(ui, "レート制限");
            help(ui, "5時間ごとに戻る上限");
            badge(ui, "入力待ち", LIGHT.warn, LIGHT.warn_soft);
            usage_bar(ui, 1.5, LIGHT.accent);
            trunc(ui, "とても長い入力の冒頭");
        });
        h.get_by_label("レート制限");
        h.get_by_label("ⓘ");
        h.get_by_label("入力待ち");
        h.get_by_label("とても長い入力の冒頭");
    }

    #[test]
    fn severity_colors_follow_palette() {
        assert_eq!(severity_color(Severity::Normal, &LIGHT), LIGHT.accent);
        assert_eq!(severity_color(Severity::Warning, &DARK), DARK.warn);
        assert_eq!(severity_color(Severity::Critical, &LIGHT), LIGHT.err);
    }
}
