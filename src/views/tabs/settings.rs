//! 設定タブの描画。動作の設定とモデル情報（単価）の表示と編集。
use crate::controllers::gui::app::{Action, Forms};
use crate::controllers::gui::tabs::settings::{SettingsAction, SettingsVm};
use crate::models::domain::settings::Theme;
use crate::views::app::Acts;
use crate::views::input::{Field, FormInputs};
use crate::views::layout::GAP;
use crate::views::probe::Role;
use crate::views::theme::{GAP_X, GAP_Y, Palette, SMALL, color};
use crate::views::widgets::{
    button, card, checkbox, disabled_button, label, link, probed, section, selectable,
    small_button, strong, table_row, text, trunc, truncate, weak,
};
use gpui::{AnyElement, Div, IntoElement, ParentElement as _, Styled as _, div, px};
use gpui_component::input::Input;

/// 列（モデルID、単価、コンテキスト長、取得元、編集）。単価の列だけが伸びる。
const COLUMNS: [Option<f32>; 5] = [Some(180.0), None, Some(96.0), Some(56.0), Some(56.0)];
/// 入力欄の幅（96）。
const INPUT_PX: f32 = 96.0;

/// 設定タブを描く。
pub fn render(
    vm: &SettingsVm,
    forms: &Forms,
    inputs: &FormInputs,
    p: &'static Palette,
    acts: &Acts,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .child(behavior(vm, forms, inputs, p, acts))
        .child(models(vm, forms, inputs, p, acts))
}

/// 見出しと入力欄の1行。見出しを読み上げ用の名前にする。
fn field(inputs: &FormInputs, p: &Palette, f: Field, name: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(label(p, name))
        .child(
            probed(format!("input:{name}"), Role::TextInput, name)
                .w(px(INPUT_PX))
                .child(Input::new(inputs.get(f)).aria_label(name)),
        )
}

fn behavior(
    vm: &SettingsVm,
    forms: &Forms,
    inputs: &FormInputs,
    p: &'static Palette,
    acts: &Acts,
) -> Div {
    let themes = [
        (Theme::System, "OSに合わせる"),
        (Theme::Light, "ライト"),
        (Theme::Dark, "ダーク"),
    ]
    .map(|(t, s)| {
        // 選ぶのは下書きだけで、保存するまで画面のテーマは変えない。
        selectable(
            p,
            format!("theme:{s}"),
            forms.settings.theme == t,
            s,
            acts.set(move |f| f.settings.theme = t),
        )
    });
    // 自動起動の状態を読めないOSや権限では、切り替えを出さずに理由を示す。
    let autostart = match vm.autostart {
        None => weak(p, "ログイン時の起動の状態を読めません"),
        Some(on) => checkbox(
            p,
            "autostart",
            on,
            "ログイン時にデーモンを起動する",
            acts.on(Action::Settings(SettingsAction::SetAutostart(!on))),
        ),
    };
    card(p, false)
        .flex()
        .flex_col()
        .items_start()
        .gap(GAP_Y)
        .child(section(p, "動作"))
        .child(field(inputs, p, Field::Interval, "取得間隔（秒）"))
        .child(field(inputs, p, Field::Threshold, "通知閾値（%）"))
        .child(field(inputs, p, Field::Retention, "保持日数"))
        .child(field(
            inputs,
            p,
            Field::Headless,
            "ヘッドレスの稼働判定（秒）",
        ))
        .child(field(inputs, p, Field::Job, "ジョブの稼働判定（分）"))
        .child(
            div()
                .flex()
                .items_center()
                .gap(GAP_X)
                .child(label(p, "テーマ"))
                .children(themes),
        )
        .child(autostart)
        .child(button(
            p,
            "settings-save",
            "保存",
            acts.on(Action::Settings(SettingsAction::Save)),
        ))
        .children(vm.message.as_deref().map(|m| label(p, m)))
}

fn models(
    vm: &SettingsVm,
    forms: &Forms,
    inputs: &FormInputs,
    p: &'static Palette,
    acts: &Acts,
) -> Div {
    let sa = Action::Settings;
    let refresh = if vm.refreshing {
        disabled_button(p, "catalog-refresh", "取得中…").into_any_element()
    } else {
        button(
            p,
            "catalog-refresh",
            "今すぐ更新",
            acts.on(sa(SettingsAction::RefreshCatalog)),
        )
        .into_any_element()
    };
    let head = div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(section(p, "モデル情報"))
        .child(text(&vm.catalog, SMALL, p.weak))
        .child(refresh);
    let links = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(GAP_X)
        .child(link(p, "link-pricing", "料金ページ", vm.pricing_url))
        .child(link(p, "link-models", "モデル一覧", vm.models_url))
        .child(text(
            "単価はUSD/MTok（入力 / 出力 / キャッシュ読込 / 作成5分 / 作成1時間）",
            SMALL,
            p.weak,
        ));
    let rows = vm.models.iter().map(|m| {
        table_row(
            &COLUMNS,
            vec![
                trunc(p, &m.prefix).into_any_element(),
                truncate(p, text(&m.prices, SMALL, p.text), &m.prices).into_any_element(),
                text(&m.context, SMALL, p.text).into_any_element(),
                text(&m.source, SMALL, p.text).into_any_element(),
                small_button(
                    p,
                    format!("model-edit:{}", m.prefix),
                    "編集",
                    acts.on(sa(SettingsAction::EditModel(m.prefix.clone()))),
                )
                .into_any_element(),
            ],
        )
    });
    card(p, false)
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(head)
        .child(links)
        .children(rows)
        .children(
            forms
                .model
                .as_ref()
                .map(|d| editor(&d.prefix, inputs, p, acts)),
        )
}

fn editor(prefix: &str, inputs: &FormInputs, p: &'static Palette, acts: &Acts) -> AnyElement {
    let sa = Action::Settings;
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(GAP_Y)
        .child(div().w_full().h(px(1.0)).bg(color(p.border)))
        .child(strong(p, &format!("{prefix} を編集")))
        .child(field(inputs, p, Field::ModelInput, "入力"))
        .child(field(inputs, p, Field::ModelOutput, "出力"))
        .child(field(inputs, p, Field::ModelCacheRead, "キャッシュ読込"))
        .child(field(
            inputs,
            p,
            Field::ModelCacheWrite5m,
            "キャッシュ作成5分",
        ))
        .child(field(
            inputs,
            p,
            Field::ModelCacheWrite1h,
            "キャッシュ作成1時間",
        ))
        .child(field(inputs, p, Field::ModelContext, "コンテキスト長"))
        .child(
            div()
                .flex()
                .gap(GAP_X)
                .child(button(
                    p,
                    "model-save",
                    "単価を保存",
                    acts.on(sa(SettingsAction::SaveModel)),
                ))
                .child(button(
                    p,
                    "model-cancel",
                    "やめる",
                    acts.on(sa(SettingsAction::CancelModel)),
                )),
        )
        .into_any_element()
}
