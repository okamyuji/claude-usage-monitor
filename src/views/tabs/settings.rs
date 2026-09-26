//! 設定タブの描画。
use crate::controllers::gui::app::{Action, Forms};
use crate::controllers::gui::tabs::settings::{SettingsAction, SettingsVm};
use crate::models::domain::settings::Theme;
use crate::views::layout::{GAP, flex_columns};
use crate::views::theme::card_frame;
use crate::views::widgets::{cell, pal, section, trunc};
use egui::{RichText, Ui};

fn field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        let l = ui.label(label);
        ui.add(egui::TextEdit::singleline(value).desired_width(96.0))
            .labelled_by(l.id);
    });
}

/// 列（モデルID、単価、コンテキスト長、取得元、編集）。単価の列だけが伸びる。
const COLUMNS: [Option<f32>; 5] = [Some(180.0), None, Some(96.0), Some(56.0), Some(56.0)];

/// 設定タブを描く。
pub fn show(ui: &mut Ui, vm: &SettingsVm, forms: &mut Forms, acts: &mut Vec<Action>) {
    let p = pal(ui);
    card_frame(p, false).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section(ui, "動作");
        let s = &mut forms.settings;
        field(ui, "取得間隔（秒）", &mut s.interval);
        field(ui, "通知閾値（%）", &mut s.threshold);
        field(ui, "保持日数", &mut s.retention);
        field(ui, "ヘッドレスの稼働判定（秒）", &mut s.headless);
        field(ui, "ジョブの稼働判定（分）", &mut s.job);
        ui.horizontal(|ui| {
            ui.label("テーマ");
            for (t, text) in [
                (Theme::System, "OSに合わせる"),
                (Theme::Light, "ライト"),
                (Theme::Dark, "ダーク"),
            ] {
                ui.selectable_value(&mut s.theme, t, text);
            }
        });
        if let Some(mut on) = vm.autostart {
            if ui
                .checkbox(&mut on, "ログイン時にデーモンを起動する")
                .changed()
            {
                acts.push(Action::Settings(SettingsAction::SetAutostart(on)));
            }
        } else {
            ui.label(RichText::new("ログイン時の起動の状態を読めません").color(p.weak));
        }
        if ui.button("保存").clicked() {
            acts.push(Action::Settings(SettingsAction::Save));
        }
        if let Some(m) = &vm.message {
            ui.label(m);
        }
    });
    ui.add_space(GAP);
    card_frame(p, false).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            section(ui, "モデル情報");
            ui.label(RichText::new(&vm.catalog).small().color(p.weak));
            let label = if vm.refreshing {
                "取得中…"
            } else {
                "今すぐ更新"
            };
            if ui
                .add_enabled(!vm.refreshing, egui::Button::new(label))
                .clicked()
            {
                acts.push(Action::Settings(SettingsAction::RefreshCatalog));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to("料金ページ", vm.pricing_url);
            ui.hyperlink_to("モデル一覧", vm.models_url);
            ui.label(
                RichText::new(
                    "単価はUSD/MTok（入力 / 出力 / キャッシュ読込 / 作成5分 / 作成1時間）",
                )
                .small()
                .color(p.weak),
            );
        });
        let widths = flex_columns(ui.available_width(), &COLUMNS);
        for m in &vm.models {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP / 2.0;
                cell(ui, widths[0], |ui| trunc(ui, &m.prefix));
                cell(ui, widths[1], |ui| {
                    trunc(ui, RichText::new(&m.prices).small())
                });
                cell(ui, widths[2], |ui| {
                    ui.label(RichText::new(&m.context).small())
                });
                cell(ui, widths[3], |ui| {
                    ui.label(RichText::new(&m.source).small())
                });
                cell(ui, widths[4], |ui| {
                    if ui.small_button("編集").clicked() {
                        acts.push(Action::Settings(SettingsAction::EditModel(
                            m.prefix.clone(),
                        )));
                    }
                });
            });
        }
        if let Some(d) = &mut forms.model {
            ui.separator();
            ui.label(RichText::new(format!("{} を編集", d.prefix)).strong());
            field(ui, "入力", &mut d.input);
            field(ui, "出力", &mut d.output);
            field(ui, "キャッシュ読込", &mut d.cache_read);
            field(ui, "キャッシュ作成5分", &mut d.cache_write_5m);
            field(ui, "キャッシュ作成1時間", &mut d.cache_write_1h);
            field(ui, "コンテキスト長", &mut d.context);
            ui.horizontal(|ui| {
                if ui.button("単価を保存").clicked() {
                    acts.push(Action::Settings(SettingsAction::SaveModel));
                }
                if ui.button("やめる").clicked() {
                    acts.push(Action::Settings(SettingsAction::CancelModel));
                }
            });
        }
    });
}
