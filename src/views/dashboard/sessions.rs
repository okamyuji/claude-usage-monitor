//! セッション一覧（spec 7.2節の中段の左）。稼働中と履歴を切り替える。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::app::Forms;
use crate::controllers::gui::dashboard::memory::MemAction;
use crate::controllers::gui::dashboard::runs::RunItem;
use crate::controllers::gui::dashboard::sessions::{DetailTab, SessionItem};
use crate::controllers::gui::dashboard::{DashAction, DashboardVm, ListMode};
use crate::models::domain::activity::RunKind;
use crate::models::domain::display::help as h;
use crate::models::domain::transcript::SessionKind;
use crate::views::layout::{GAP, flex_columns};
use crate::views::theme::Palette;
use crate::views::widgets::cell;
use crate::views::widgets::{badge, help, pal, section, trunc};
use egui::Sense;
use egui::{Color32, RichText, Ui};

/// 一覧を描く。
pub fn show(ui: &mut Ui, vm: &DashboardVm, forms: &mut Forms, acts: &mut Vec<Action>) {
    header(ui, vm, acts);
    if vm.mode == ListMode::History {
        filters(ui, vm, forms, acts);
    }
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("session_list")
        .auto_shrink([false, false])
        .show(ui, |ui| match vm.mode {
            ListMode::Active => active(ui, vm, acts),
            ListMode::History => history(ui, vm, acts),
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

fn active(ui: &mut Ui, vm: &DashboardVm, acts: &mut Vec<Action>) {
    if vm.active.is_empty() {
        ui.label(RichText::new("稼働中の実行はありません").color(pal(ui).weak));
    }
    for r in &vm.active {
        run_row(
            ui,
            r,
            vm.selected.as_deref() == Some(r.session_id.as_str()),
            acts,
        );
    }
}

fn run_row(ui: &mut Ui, r: &RunItem, selected: bool, acts: &mut Vec<Action>) {
    let p = pal(ui);
    let indent = 16.0 * r.depth as f32;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        let (fg, bg) = kind_colors(r.kind, p);
        badge(ui, r.kind.label(), fg, bg);
        let (fg, bg) = state_colors(&r.state, p);
        badge(ui, &r.state, fg, bg);
        if r.memory.as_ref().is_some_and(|m| m.desktop) {
            badge(ui, "Desktop", p.accent, p.accent_soft);
        }
        let title =
            RichText::new(&r.title)
                .strong()
                .color(if selected { p.accent } else { p.text });
        let resp = ui.add(egui::Label::new(title).truncate().sense(Sense::click()));
        if resp.clicked() {
            acts.push(Action::Dash(DashAction::Select(
                r.session_id.clone(),
                DetailTab::LiveLog,
            )));
        }
        if let Some(m) = &r.memory {
            resp.context_menu(|ui| {
                if ui.button("終了してメモリを解放").clicked() {
                    acts.push(Action::Memory(MemAction::Ask(m.exit.clone())));
                    ui.close();
                }
            });
        }
    });
    let context = format!("コンテキスト {}", r.context);
    let meta: Vec<&str> = [
        r.project.as_str(),
        context.as_str(),
        &r.elapsed,
        &r.tokens,
        &r.cost,
        r.memory.as_ref().map_or("", |m| m.text.as_str()),
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

fn filters(ui: &mut Ui, vm: &DashboardVm, forms: &mut Forms, acts: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let l = ui.label("検索");
        let w = ui.available_width();
        if ui
            .add(egui::TextEdit::singleline(&mut forms.session_query).desired_width(w))
            .labelled_by(l.id)
            .changed()
        {
            acts.push(Action::Dash(DashAction::Search));
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (text, k) in [
            ("全種別", None),
            ("対話", Some(SessionKind::Interactive)),
            ("ヘッドレス", Some(SessionKind::Headless)),
            ("ジョブ", Some(SessionKind::BackgroundJob)),
        ] {
            if ui.selectable_label(vm.kind == k, text).clicked() {
                acts.push(Action::Dash(DashAction::SetKind(k)));
            }
        }
        ui.separator();
        if ui
            .selectable_label(vm.profile.is_none(), "全プロファイル")
            .clicked()
        {
            acts.push(Action::Dash(DashAction::SetProfile(None)));
        }
        for (id, name) in &vm.profiles {
            if ui.selectable_label(vm.profile == Some(*id), name).clicked() {
                acts.push(Action::Dash(DashAction::SetProfile(Some(*id))));
            }
        }
    });
}

/// 履歴の列（入力、開始、経過、コンテキスト、Token、コスト、ターン）。入力の列だけが伸びる。
const HISTORY_COLUMNS: [Option<f32>; 7] = [
    None,
    Some(72.0),
    Some(56.0),
    Some(64.0),
    Some(56.0),
    Some(64.0),
    Some(40.0),
];

fn history(ui: &mut Ui, vm: &DashboardVm, acts: &mut Vec<Action>) {
    let p = pal(ui);
    if vm.history.is_empty() {
        ui.label(RichText::new("条件に合う完了したセッションはありません").color(p.weak));
        return;
    }
    if vm.history_limited {
        ui.label(
            RichText::new("新しい順に1,000件を表示しています")
                .small()
                .color(p.weak),
        );
    }
    let widths = flex_columns(ui.available_width(), &HISTORY_COLUMNS);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP / 2.0;
        for (w, t) in widths.iter().zip([
            "入力",
            "開始",
            "経過",
            "コンテキスト",
            "Token",
            "コスト",
            "ターン",
        ]) {
            cell(ui, *w, |ui| {
                ui.label(RichText::new(t).small().color(p.weak))
            });
        }
    });
    for i in &vm.history {
        history_row(
            ui,
            i,
            &widths,
            vm.selected.as_deref() == Some(i.session_id.as_str()),
            acts,
        );
    }
}

fn history_row(
    ui: &mut Ui,
    i: &SessionItem,
    widths: &[f32],
    selected: bool,
    acts: &mut Vec<Action>,
) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP / 2.0;
        let title = RichText::new(&i.title).color(if selected { p.accent } else { p.text });
        let clicked = cell(ui, widths[0], |ui| {
            ui.add(egui::Label::new(title).truncate().sense(Sense::click()))
                .clicked()
        });
        if clicked {
            acts.push(Action::Dash(DashAction::Select(
                i.session_id.clone(),
                DetailTab::Turns,
            )));
        }
        for (w, v) in widths[1..].iter().zip([
            &i.started, &i.elapsed, &i.context, &i.tokens, &i.cost, &i.turns,
        ]) {
            cell(ui, *w, |ui| trunc(ui, RichText::new(v).small()));
        }
    });
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
