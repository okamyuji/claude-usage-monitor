//! Desktopのカードと、終了の要求の確認ダイアログ。
use crate::controllers::gui::app::Action;
use crate::controllers::gui::dashboard::memory::{ConfirmExit, DesktopMemoryVm, MemAction};
use crate::models::domain::display::help as h;
use crate::views::app::Acts;
use crate::views::theme::{BODY, GAP_X, GAP_Y, Palette, color};
use crate::views::widgets::{button, card, help, label, small_button, strong, text};
use gpui::{
    Div, InteractiveElement as _, MouseButton, ParentElement as _, Stateful, Styled as _, div, hsla,
};

/// Desktopのカード。
pub fn desktop(d: &DesktopMemoryVm, p: &'static Palette, acts: &Acts) -> Div {
    card(p, false).child(
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .child(strong(p, "Claude Desktop"))
            .child(label(p, &d.text))
            .child(help(p, h::MEMORY))
            .child(small_button(
                p,
                "desktop-exit",
                "Desktopを終了",
                acts.on(Action::Memory(MemAction::Ask(d.exit.clone()))),
            )),
    )
}

/// 確認ダイアログ。窓全体を覆い、枠の外を押したときはキャンセルとして扱う。
pub fn confirm(c: &ConfirmExit, p: &'static Palette, acts: &Acts) -> Stateful<Div> {
    let dialog = card(p, false)
        .flex()
        .flex_col()
        .gap(GAP_Y)
        // ダイアログの中を押しても、外側の「閉じる」へ伝わらないようにする。
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(strong(
            p,
            &format!("{} を終了します（{}）", c.title, c.memory),
        ))
        .child(text("実行中の編集が失われることがあります", BODY, p.warn))
        .children(
            c.resume
                .as_deref()
                .map(|cmd| label(p, &format!("再開するには次を実行します: {cmd}"))),
        )
        .child(
            div()
                .flex()
                .gap(GAP_X)
                .child(button(
                    p,
                    "confirm-exit",
                    "終了する",
                    acts.on(Action::Memory(MemAction::Confirm)),
                ))
                .child(button(
                    p,
                    "cancel-exit",
                    "やめる",
                    acts.on(Action::Memory(MemAction::Cancel)),
                )),
        );
    let cancel = acts.clone();
    div()
        .id("confirm-backdrop")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        // 背景は黒で、アルファは100（255中）。
        .bg(hsla(0.0, 0.0, 0.0, 100.0 / 255.0))
        .on_mouse_down(MouseButton::Left, move |_, w, cx| {
            cancel.send(Action::Memory(MemAction::Cancel), w, cx)
        })
        .child(dialog)
        .text_color(color(p.text))
}
