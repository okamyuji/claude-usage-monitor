//! プロファイルタブの描画。一覧、削除の確認、追加の入力欄。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::tabs::profiles::{ProfileRow, ProfilesAction, ProfilesVm};
use crate::views::app::Acts;
use crate::views::input::{Field, FormInputs};
use crate::views::layout::GAP;
use crate::views::probe::Role;
use crate::views::theme::{GAP_X, GAP_Y, Palette, SMALL};
use crate::views::widgets::{
    badge, button, card, label, probed, section, small, strong, table_row, text, truncate,
};
use gpui::{AnyElement, Div, IntoElement, ParentElement as _, Styled as _, div, px};
use gpui_component::input::Input;

/// 列（名前、設定ディレクトリ、トークン、契約、操作）。設定ディレクトリの列だけが伸びる。
const COLUMNS: [Option<f32>; 5] = [Some(140.0), None, Some(200.0), Some(64.0), Some(180.0)];
/// 入力欄の左に置く見出しの分の幅。入力欄は残りの幅から160を引いた幅にする。
const LABEL_PX: f32 = 160.0;
/// 入力欄の最小の幅。
const MIN_INPUT_PX: f32 = 120.0;
/// カードの左右の余白と枠の幅（`card`の`px(10)`と枠の1px）。
const CARD_INSET_PX: f32 = 22.0;

/// プロファイルタブを描く。`width`は本文の幅。
pub fn render(
    vm: &ProfilesVm,
    inputs: &FormInputs,
    p: &'static Palette,
    acts: &Acts,
    width: f32,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .child(list(vm, p, acts))
        .child(add(vm, inputs, p, acts, width))
}

fn list(vm: &ProfilesVm, p: &'static Palette, acts: &Acts) -> Div {
    let head = table_row(
        &COLUMNS,
        ["名前", "設定ディレクトリ", "トークン", "契約", ""]
            .map(|t| text(t, SMALL, p.weak).into_any_element())
            .into(),
    );
    let confirm = vm.pending_remove.as_ref().map(|name| {
        let pa = |a| Action::Profiles(a);
        div()
            .flex()
            .flex_col()
            .gap(GAP_Y)
            .mt(px(GAP / 2.0))
            .child(text(
                &format!("{name} を削除すると、このプロファイルの使用量とセッションの記録も消えます。設定ディレクトリは残ります"),
                crate::views::theme::BODY,
                p.warn,
            ))
            .child(
                div()
                    .flex()
                    .gap(GAP_X)
                    .child(button(
                        p,
                        "profile-confirm-remove",
                        "本当に削除する",
                        acts.on(pa(ProfilesAction::ConfirmRemove)),
                    ))
                    .child(button(
                        p,
                        "profile-cancel-remove",
                        "やめる",
                        acts.on(pa(ProfilesAction::CancelRemove)),
                    )),
            )
    });
    card(p, false)
        .flex()
        .flex_col()
        .gap(GAP_Y)
        .child(section(p, "プロファイル"))
        .child(head)
        .children(vm.rows.iter().map(|r| row(r, p, acts)))
        .children(confirm)
}

fn row(r: &ProfileRow, p: &'static Palette, acts: &Acts) -> Div {
    let name = div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .child(strong(p, &r.name))
        .children(r.active.then(|| badge("使用中", p.accent, p.accent_soft)));
    let ops: Vec<AnyElement> = if r.active {
        Vec::new()
    } else {
        let pa = |a| Action::Profiles(a);
        vec![
            button(
                p,
                format!("profile-use:{}", r.name),
                "使用中にする",
                acts.on(pa(ProfilesAction::Use(r.name.clone()))),
            )
            .into_any_element(),
            button(
                p,
                format!("profile-remove:{}", r.name),
                "削除",
                acts.on(pa(ProfilesAction::Remove(r.name.clone()))),
            )
            .into_any_element(),
        ]
    };
    table_row(
        &COLUMNS,
        vec![
            name.into_any_element(),
            truncate(p, small(p, &r.dir), &r.dir).into_any_element(),
            truncate(p, small(p, &r.token), &r.token).into_any_element(),
            small(p, &r.plan).into_any_element(),
            div().flex().gap(GAP_X).children(ops).into_any_element(),
        ],
    )
}

fn add(vm: &ProfilesVm, inputs: &FormInputs, p: &'static Palette, acts: &Acts, width: f32) -> Div {
    let w = (width - CARD_INSET_PX - LABEL_PX).max(MIN_INPUT_PX);
    let field = |f: Field, name: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .child(label(p, name))
            .child(
                probed(format!("input:{name}"), Role::TextInput, name)
                    .w(px(w))
                    .child(Input::new(inputs.get(f)).aria_label(name)),
            )
    };
    card(p, false)
        .flex()
        .flex_col()
        .items_start()
        .gap(GAP_Y)
        .child(section(p, "プロファイルを追加"))
        .child(field(Field::ProfileName, "プロファイル名"))
        .child(field(Field::ProfileDir, "設定ディレクトリ"))
        .child(button(
            p,
            "profile-add",
            "追加",
            acts.on(Action::Profiles(ProfilesAction::Add)),
        ))
        .children(vm.message.as_deref().map(|m| label(p, m)))
        .child(truncate(p, text(vm.login_help, SMALL, p.weak), vm.login_help).max_w_full())
}
