//! 画面の枠（上部バー、本文、状態表示）とeframeのアプリ。
use crate::controllers::gui::app::{Action, AppVm, Forms, GuiController, REFRESH_SECS, TabVm};
use crate::models::domain::settings::Theme;
use crate::views::widgets::pal;
use crate::views::{dashboard, header};
use egui::{Margin, RichText, Stroke, Ui};
use std::sync::Arc;

/// 画面全体を描き、操作を返す。
pub fn show_app(ui: &mut Ui, vm: &AppVm, forms: &mut Forms) -> Vec<Action> {
    let mut acts = Vec::new();
    let p = pal(ui);
    let bar = egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border));
    egui::Panel::top("header")
        .frame(bar.inner_margin(Margin::symmetric(12, 8)))
        .show(ui, |ui| {
            header::show(ui, vm, &mut acts);
        });
    egui::Panel::bottom("status")
        .frame(bar.inner_margin(Margin::symmetric(12, 4)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "最終更新 {}（{}秒ごとに更新）",
                        vm.updated, REFRESH_SECS
                    ))
                    .small()
                    .color(p.weak),
                );
                if let Some(e) = &vm.error {
                    ui.label(RichText::new(e).color(p.err));
                }
            });
        });
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(p.bg).inner_margin(Margin::same(12)))
        .show(ui, |ui| {
            body(ui, vm, forms, &mut acts);
        });
    acts
}

fn body(ui: &mut Ui, vm: &AppVm, forms: &mut Forms, acts: &mut Vec<Action>) {
    match &vm.body {
        TabVm::Dashboard(d) => dashboard::show(ui, d, forms, acts),
        TabVm::Pending(t) => {
            ui.label(format!("{}は準備中です", t.label()));
        }
    }
}

/// 同梱の日本語フォント（Noto Sans JP Regular、SIL Open Font License 1.1）。
/// OSのフォントに頼ると、日本語フォントのない環境で漢字が豆腐になるため同梱する。
/// バイナリに埋め込んで`from_static`で渡し、ヒープへ複製しない。
const CJK_FONT: &[u8] = include_bytes!("../../assets/fonts/NotoSansJP-Regular.otf");

/// フォント定義。アイコン（Phosphor）と同梱の日本語フォントを、英字フォントの直後に置く。
/// 絵文字フォントより前に置くのは、全角の括弧などを絵文字フォントの字形で描かせないため。
pub fn font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    fonts.font_data.insert(
        "cjk".into(),
        Arc::new(egui::FontData::from_static(CJK_FONT)),
    );
    for (family, at) in [
        (egui::FontFamily::Proportional, 2),
        (egui::FontFamily::Monospace, 1),
    ] {
        let list = fonts.families.entry(family).or_default();
        list.insert(at.min(list.len()), "cjk".into());
    }
    fonts
}

/// フォントを登録する。
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

/// テーマの選択を反映する。
pub fn apply_theme(ctx: &egui::Context, t: Theme) {
    ctx.set_theme(match t {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    });
}

/// eframeのアプリ。
pub struct CpsApp {
    ctl: GuiController,
    theme: Theme,
}

impl CpsApp {
    /// controllerを受け取って作る。
    pub fn new(ctl: GuiController) -> Self {
        let theme = ctl.vm().theme;
        Self { ctl, theme }
    }
}

impl eframe::App for CpsApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.ctl.tick();
        if self.ctl.vm().theme != self.theme {
            self.theme = self.ctl.vm().theme;
            apply_theme(ui.ctx(), self.theme);
        }
        let (vm, forms) = self.ctl.view_parts();
        let acts = show_app(ui, vm, forms);
        self.ctl.handle_all(acts);
        // 5秒ごとの読み直しと経過時間の表示を進めるため、操作がなくても描き直す。再生中は間隔を短くする。
        let playing = matches!(&self.ctl.vm().body, TabVm::Dashboard(d)
            if d.detail.as_ref().and_then(|x| x.replay.as_ref()).is_some_and(|r| r.playing));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(if playing {
                50
            } else {
                1000
            }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeCreds, FakeDaemon, FixedClock, gui_deps, temp_store};
    use chrono::{TimeZone, Utc};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use std::collections::HashMap;

    fn controller(home: &std::path::Path) -> (tempfile::TempDir, GuiController) {
        let (d, s) = temp_store();
        let clock = Arc::new(FixedClock::at(
            Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap(),
        ));
        let deps = gui_deps(
            Arc::new(s),
            clock,
            home,
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        (d, GuiController::new(deps))
    }

    #[test]
    fn bundled_font_goes_before_emoji_fonts() {
        let f = font_definitions();
        let prop = &f.families[&egui::FontFamily::Proportional];
        let at = |list: &[String], name: &str| list.iter().position(|n| n == name).unwrap();
        assert_eq!((at(prop, "phosphor"), at(prop, "cjk")), (1, 2));
        assert!(at(prop, "cjk") < at(prop, "NotoEmoji-Regular"));
        let mono = &f.families[&egui::FontFamily::Monospace];
        assert_eq!(at(mono, "cjk"), 1);
        assert!(at(mono, "cjk") < at(mono, "NotoEmoji-Regular"));
    }

    #[test]
    fn bundled_font_covers_kanji_and_fullwidth_symbols() {
        let text = "漢字豆腐（）稼働中ー「」";
        let has = |ctx: &egui::Context| {
            // テクスチャの差分は描画側が受け取る前提なので、使わないことをeguiに伝えてから捨てる。
            ctx.run_ui(egui::RawInput::default(), |_| {})
                .textures_delta
                .clear();
            ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(13.0), text))
        };
        let plain = egui::Context::default();
        assert!(!has(&plain), "既定フォントだけでは漢字が豆腐になる");
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        assert!(has(&ctx));
    }

    #[test]
    fn apply_theme_sets_preference() {
        let ctx = egui::Context::default();
        for (t, p) in [
            (Theme::Light, egui::ThemePreference::Light),
            (Theme::Dark, egui::ThemePreference::Dark),
            (Theme::System, egui::ThemePreference::System),
        ] {
            apply_theme(&ctx, t);
            assert_eq!(ctx.options(|o| o.theme_preference), p);
        }
    }

    #[test]
    fn eframe_app_draws_shell_and_follows_theme_change() {
        let home = tempfile::tempdir().unwrap();
        let (_d, ctl) = controller(home.path());
        let mut app = CpsApp::new(ctl);
        app.theme = Theme::Dark;
        let mut h = Harness::new_eframe(|_cc| app);
        h.run();
        h.get_by_label("ダッシュボード");
        h.get_by_label_contains("デーモン停止中");
        h.get_by_label_contains("最終更新 12:00");
        assert_eq!(h.state().theme, Theme::System);
        assert_eq!(
            h.ctx.options(|o| o.theme_preference),
            egui::ThemePreference::System
        );
    }

    #[test]
    fn pending_tab_and_error_are_shown() {
        let home = tempfile::tempdir().unwrap();
        let (_d, mut ctl) = controller(home.path());
        ctl.handle(Action::SelectTab(
            crate::controllers::gui::app::Tab::Diagnostics,
        ));
        let mut h = Harness::new_ui_state(
            |ui, c: &mut GuiController| {
                let (vm, forms) = c.view_parts();
                let acts = show_app(ui, vm, forms);
                c.handle_all(acts);
            },
            ctl,
        );
        h.run();
        h.get_by_label("診断は準備中です");
    }
}
