//! セッション一覧（spec 7.2節の中段の左）。稼働中と履歴を切り替える。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::dashboard::runs::RunItem;
use crate::controllers::gui::dashboard::{DashAction, DashboardVm, ListMode};
use crate::models::domain::activity::RunKind;
use crate::models::domain::display::help as h;
use crate::views::theme::Palette;
use crate::views::widgets::{badge, help, pal, section, trunc};
use egui::{Color32, RichText, Ui};

/// 一覧を描く。
pub fn show(ui: &mut Ui, vm: &DashboardVm, acts: &mut Vec<Action>) {
    header(ui, vm, acts);
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("session_list")
        .auto_shrink([false, false])
        .show(ui, |ui| match vm.mode {
            ListMode::Active => active(ui, &vm.active),
            ListMode::History => {
                ui.label(RichText::new("完了したセッションはまだありません").color(pal(ui).weak));
            }
        });
}

fn header(ui: &mut Ui, vm: &DashboardVm, acts: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        section(ui, "セッション");
        let n = vm.active.iter().filter(|r| r.depth == 0).count();
        for (m, text) in [
            (ListMode::Active, format!("稼働中 {n}")),
            (ListMode::History, "履歴".to_string()),
        ] {
            if ui.selectable_label(vm.mode == m, text).clicked() {
                acts.push(Action::Dash(DashAction::SetListMode(m)));
            }
        }
        help(ui, h::RUN_KIND);
    });
}

/// 種別のバッジの色。バックグラウンドの実行は青、対話は灰（spec 7.3節）。
pub fn kind_colors(k: RunKind, p: &Palette) -> (Color32, Color32) {
    match k {
        RunKind::Interactive => (p.text, p.track),
        RunKind::Headless | RunKind::Job | RunKind::Subagent => (p.accent, p.accent_soft),
    }
}

/// 状態のバッジの色。応答生成中とツール実行中は緑、入力待ちは黄、それ以外は灰（spec 7.3節）。
pub fn state_colors(state: &str, p: &Palette) -> (Color32, Color32) {
    match state {
        "応答生成中" | "ツール実行中" => (p.ok, p.ok_soft),
        "入力待ち" => (p.warn, p.warn_soft),
        _ => (p.weak, p.track),
    }
}

fn active(ui: &mut Ui, runs: &[RunItem]) {
    if runs.is_empty() {
        ui.label(RichText::new("稼働中の実行はありません").color(pal(ui).weak));
    }
    for r in runs {
        run_row(ui, r);
    }
}

fn run_row(ui: &mut Ui, r: &RunItem) {
    let p = pal(ui);
    let indent = 16.0 * r.depth as f32;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        let (fg, bg) = kind_colors(r.kind, p);
        badge(ui, r.kind.label(), fg, bg);
        let (fg, bg) = state_colors(&r.state, p);
        badge(ui, &r.state, fg, bg);
        trunc(ui, RichText::new(&r.title).strong());
    });
    let context = format!("コンテキスト {}", r.context);
    let meta: Vec<&str> = [
        r.project.as_str(),
        context.as_str(),
        &r.elapsed,
        &r.tokens,
        &r.cost,
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect();
    ui.horizontal(|ui| {
        ui.add_space(indent);
        trunc(ui, RichText::new(meta.join(" · ")).small().color(p.weak));
    });
    if let Some(d) = &r.detail {
        ui.horizontal(|ui| {
            ui.add_space(indent);
            trunc(ui, RichText::new(d).small());
        });
    }
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::theme::LIGHT;

    #[test]
    fn badge_colors_by_kind_and_state() {
        assert_eq!(
            kind_colors(RunKind::Interactive, &LIGHT),
            (LIGHT.text, LIGHT.track)
        );
        assert_eq!(
            kind_colors(RunKind::Job, &LIGHT),
            (LIGHT.accent, LIGHT.accent_soft)
        );
        assert_eq!(
            state_colors("応答生成中", &LIGHT),
            (LIGHT.ok, LIGHT.ok_soft)
        );
        assert_eq!(
            state_colors("ツール実行中", &LIGHT),
            (LIGHT.ok, LIGHT.ok_soft)
        );
        assert_eq!(
            state_colors("入力待ち", &LIGHT),
            (LIGHT.warn, LIGHT.warn_soft)
        );
        assert_eq!(state_colors("終了", &LIGHT), (LIGHT.weak, LIGHT.track));
    }
}
