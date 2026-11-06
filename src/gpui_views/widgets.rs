//! 画面をまたいで使う小さな部品。
//!
//! 文字はeguiと同じく選んで写せるようにする。部品は描くたびに役割と名前を`probe`へ積み、
//! 同じ値を目印（`debug_selector`）と読み上げ用の名前に付ける。
use crate::gpui_views::fonts::ICON_FAMILY;
use crate::gpui_views::probe::{self, Role};
use crate::gpui_views::theme::{BODY, BUTTON, DARK, HEADING, Palette, SMALL, WIDGET_RADIUS, color};
use crate::models::domain::display::Severity;
use gpui::{
    App, AppContext as _, ClickEvent, Div, ElementId, InteractiveElement as _, ParentElement as _,
    Pixels, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Toggled, Window,
    div, prelude::FluentBuilder as _, px, relative,
};
use gpui_base::SelectableText;
use std::cell::Cell;

thread_local! {
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

/// 描画の始めに呼ぶ。前の描画の記録を捨て、文字の番号を振り直す。
/// 番号は描く順に振るので、同じ画面なら描き直しても同じ番号になり、文字の選択が残る。
pub fn frame_start() {
    probe::take();
    NEXT_ID.with(|n| n.set(0));
}

fn next_id() -> ElementId {
    ("txt", NEXT_ID.with(|n| n.replace(n.get() + 1))).into()
}

/// 強調の色。eguiの強調文字と同じく、ライトでは黒、ダークでは白にする。
fn strong_color(p: &Palette) -> u32 {
    if *p == DARK { 0xffffff } else { 0x000000 }
}

/// ボタンの地、乗ったときの地、文字の色。eguiの既定の見た目と同じ灰色にする。
fn button_colors(p: &Palette) -> (u32, u32, u32) {
    if *p == DARK {
        (0x3c3c3c, 0x464646, 0xb4b4b4)
    } else {
        (0xe6e6e6, 0xdcdcdc, 0x3c3c3c)
    }
}

/// 役割と名前を記録し、目印と読み上げ用の名前を付けた入れ物。
pub(crate) fn tagged(id: impl Into<ElementId>, role: Role, name: &str) -> Stateful<Div> {
    let nth = probe::record(role, name);
    let name = SharedString::from(name.to_string());
    // releaseのdebug_selectorは関数を呼ばないので、目印の文字列はこの中で作る。
    let key_name = name.clone();
    div()
        .id(id)
        .role(role.a11y())
        .aria_label(name)
        .debug_selector(move || probe::selector(role, &key_name, nth))
}

/// テスト用の記録と目印だけを付けた入れ物。読み上げ用の名前を持たない部品（eguiで名前の無い部品）に使う。
pub fn probed(id: impl Into<ElementId>, role: Role, name: &str) -> Stateful<Div> {
    let nth = probe::record(role, name);
    let key_name = name.to_string();
    div()
        .id(id)
        .debug_selector(move || probe::selector(role, &key_name, nth))
}

/// 大きさと色を指定した、選んで写せる文字。
pub fn text(s: &str, size: Pixels, hex: u32) -> Stateful<Div> {
    tagged(next_id(), Role::Label, s)
        .text_size(size)
        .text_color(color(hex))
        .child(SelectableText::new(next_id(), s.to_string()))
}

/// 本文の文字。
pub fn label(p: &Palette, s: &str) -> Stateful<Div> {
    text(s, BODY, p.text)
}

/// 補足の文字。
pub fn weak(p: &Palette, s: &str) -> Stateful<Div> {
    text(s, BODY, p.weak)
}

/// 強調の文字。
pub fn strong(p: &Palette, s: &str) -> Stateful<Div> {
    text(s, BODY, strong_color(p))
}

/// 小さい文字。
pub fn small(p: &Palette, s: &str) -> Stateful<Div> {
    text(s, SMALL, p.text)
}

/// 区画の見出し。
pub fn section(p: &Palette, s: &str) -> Stateful<Div> {
    text(s, HEADING, strong_color(p))
}

/// 長い文言を末尾で省略する。全文は乗ったときのツールチップに出す。
pub fn trunc(p: &Palette, s: &str) -> Stateful<Div> {
    truncate(p, label(p, s), s)
}

/// 文字の部品`el`を末尾で省略し、全文`full`をツールチップに出す。
// shortcut: ツールチップは省略していないときも出る。省略の有無は描いた幅でしか分からないため。
pub fn truncate(p: &Palette, el: Stateful<Div>, full: &str) -> Stateful<Div> {
    el.min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .tooltip(tip(p, full))
}

/// 文字のツールチップ。アプリの配色で描き、中身を部品の記録と読み上げ用の名前に残す。
struct Tip {
    text: SharedString,
    colors: (u32, u32, u32),
}

impl gpui::Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        let (fg, bg, border) = self.colors;
        text(&self.text, BODY, fg)
            .bg(color(bg))
            .border_1()
            .border_color(color(border))
            .rounded(WIDGET_RADIUS)
            .px(px(6.0))
            .py(px(4.0))
    }
}

fn tip(p: &Palette, s: &str) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let text = SharedString::from(s.to_string());
    let colors = (p.text, p.card, p.border);
    move |_, cx| {
        cx.new(|_| Tip {
            text: text.clone(),
            colors,
        })
        .into()
    }
}

/// 指標名の横に置く「ⓘ」。説明はツールチップに出す（spec 7.1節）。
pub fn help(p: &Palette, s: &str) -> Stateful<Div> {
    weak(p, "ⓘ").tooltip(tip(p, s))
}

/// 淡い地と濃い文字のバッジ（spec 7.3節）。
pub fn badge(s: &str, fg: u32, bg: u32) -> Stateful<Div> {
    text(s, SMALL, fg)
        .bg(color(bg))
        .rounded(px(9.0))
        .px(px(6.0))
        .py(px(1.0))
}

/// 使用率の段階の色（spec 7.3節）。
pub fn severity_color(s: Severity, p: &Palette) -> u32 {
    match s {
        Severity::Normal => p.accent,
        Severity::Warning => p.warn,
        Severity::Critical => p.err,
    }
}

/// バーの塗りの割合。0から1に収める。
pub fn bar_fill(ratio: f32) -> f32 {
    ratio.clamp(0.0, 1.0)
}

/// 空き幅いっぱいに伸びる使用率バー。
pub fn usage_bar(ratio: f32, hex: u32, p: &Palette) -> Div {
    div()
        .w_full()
        .h(px(6.0))
        .rounded(px(3.0))
        .bg(color(p.track))
        .child(
            div()
                .h_full()
                .w(relative(bar_fill(ratio)))
                .rounded(px(3.0))
                .bg(color(hex)),
        )
}

/// ボタン。
pub fn button(
    p: &Palette,
    id: impl Into<ElementId>,
    s: &str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (bg, hover, fg) = button_colors(p);
    tagged(id, Role::Button, s)
        .px(px(10.0))
        .py(px(4.0))
        .rounded(WIDGET_RADIUS)
        .bg(color(bg))
        .hover(move |st| st.bg(color(hover)))
        .text_size(BUTTON)
        .text_color(color(fg))
        .cursor_pointer()
        .child(s.to_string())
        .on_click(on_click)
}

/// Phosphorのアイコン。符号位置は私用領域にあり、代替の探し順に任せると別のフォントの豆腐になるため、フォントを直接指定する。
pub fn icon(glyph: &str, hex: u32) -> Stateful<Div> {
    text(glyph, BODY, hex).font_family(ICON_FAMILY)
}

/// アイコンと文字を並べたボタン。読み上げ用の名前はeguiと同じく「アイコン 文字」にする。
pub fn icon_button(
    p: &Palette,
    id: impl Into<ElementId>,
    glyph: &str,
    s: &str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (bg, hover, fg) = button_colors(p);
    tagged(id, Role::Button, &format!("{glyph} {s}"))
        .flex()
        .items_center()
        .gap(px(4.0))
        .px(px(10.0))
        .py(px(4.0))
        .rounded(WIDGET_RADIUS)
        .bg(color(bg))
        .hover(move |st| st.bg(color(hover)))
        .text_size(BUTTON)
        .text_color(color(fg))
        .cursor_pointer()
        .child(div().font_family(ICON_FAMILY).child(glyph.to_string()))
        .child(s.to_string())
        .on_click(on_click)
}

/// 小さいボタン（eguiの`small_button`）。文字は補足の大きさで、上下の余白を持たない。
pub fn small_button(
    p: &Palette,
    id: impl Into<ElementId>,
    s: &str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    button(p, id, s, on_click)
        .px(px(4.0))
        .py(px(0.0))
        .text_size(SMALL)
}

/// カードの枠（spec 7.3節）。使用中のカードだけアクセント色の枠にする。
pub fn card(p: &Palette, highlighted: bool) -> Div {
    div()
        .bg(color(p.card))
        .border(px(if highlighted { 1.5 } else { 1.0 }))
        .border_color(color(if highlighted { p.accent } else { p.border }))
        .rounded(crate::gpui_views::theme::RADIUS)
        .px(px(10.0))
        .py(px(8.0))
        .shadow(vec![gpui::BoxShadow {
            color: gpui::hsla(0.0, 0.0, 0.0, 18.0 / 255.0),
            offset: gpui::point(px(0.0), px(1.0)),
            blur_radius: px(3.0),
            spread_radius: px(0.0),
            inset: false,
        }])
}

/// 選ぶと強調される文字のボタン（eguiの`selectable_label`）。
pub fn selectable(
    p: &Palette,
    id: impl Into<ElementId>,
    selected: bool,
    s: &str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (_, hover, fg) = button_colors(p);
    let d = tagged(id, Role::Tab, s)
        .aria_toggled(if selected {
            Toggled::True
        } else {
            Toggled::False
        })
        .px(px(10.0))
        .py(px(4.0))
        .rounded(WIDGET_RADIUS)
        .text_size(BUTTON)
        .cursor_pointer()
        .child(s.to_string())
        .on_click(on_click);
    if selected {
        d.bg(color(p.accent_soft)).text_color(color(p.accent))
    } else {
        d.text_color(color(fg)).hover(move |st| st.bg(color(hover)))
    }
}

/// チェック。
pub fn checkbox(
    p: &Palette,
    id: impl Into<ElementId>,
    checked: bool,
    s: &str,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (bg, _, fg) = button_colors(p);
    let mark = div()
        .size(px(16.0))
        .rounded(px(3.0))
        .flex()
        .items_center()
        .justify_center()
        .text_size(SMALL)
        .bg(color(if checked { p.accent } else { bg }))
        .text_color(color(0xffffff))
        .when(checked, |d| d.child("✔"));
    tagged(id, Role::CheckBox, s)
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(BODY)
        .text_color(color(fg))
        .cursor_pointer()
        .child(mark)
        .child(s.to_string())
        .on_click(on_toggle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpui_views::probe::{self, Role};
    use crate::gpui_views::theme::LIGHT;
    use gpui::{Context, IntoElement, Modifiers, Render, Window, div};
    use std::cell::Cell;
    use std::rc::Rc;

    struct Parts {
        clicks: Rc<Cell<u32>>,
    }

    impl Render for Parts {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            frame_start();
            let c = self.clicks.clone();
            div()
                .child(section(&LIGHT, "レート制限"))
                .child(help(&LIGHT, "5時間ごとに戻る上限"))
                .child(badge("入力待ち", LIGHT.warn, LIGHT.warn_soft))
                .child(usage_bar(1.5, LIGHT.accent, &LIGHT))
                .child(trunc(&LIGHT, "とても長い入力の冒頭"))
                .child(label(&LIGHT, "本文"))
                .child(weak(&LIGHT, "補足"))
                .child(strong(&LIGHT, "強調"))
                .child(small(&LIGHT, "小さい"))
                .child(button(&LIGHT, "b", "押す", move |_, _, _| {
                    c.set(c.get() + 1)
                }))
                .child(selectable(&LIGHT, "s", true, "選択中", |_, _, _| {}))
                .child(checkbox(&LIGHT, "c", false, "通知", |_, _, _| {}))
                .child(icon(crate::gpui_views::icons::GEAR, LIGHT.weak))
                .child(icon_button(
                    &LIGHT,
                    "i",
                    crate::gpui_views::icons::SUN,
                    "ライト",
                    |_, _, _| {},
                ))
        }
    }

    #[test]
    fn parts_record_their_role_and_text() {
        let mut cx = crate::test_support::gpui_cx();
        cx.update(gpui_component::init);
        let clicks = Rc::new(Cell::new(0));
        let c = clicks.clone();
        let (_, vcx) = cx.add_window_view(move |_, _| Parts { clicks: c });
        vcx.run_until_parked();
        let seen = probe::take();
        for (role, text) in [
            (Role::Label, "レート制限"),
            (Role::Label, "ⓘ"),
            (Role::Label, "入力待ち"),
            (Role::Label, "とても長い入力の冒頭"),
            (Role::Label, "本文"),
            (Role::Label, "補足"),
            (Role::Label, "強調"),
            (Role::Label, "小さい"),
            (Role::Button, "押す"),
            (Role::Tab, "選択中"),
            (Role::CheckBox, "通知"),
            (Role::Label, crate::gpui_views::icons::GEAR),
            (Role::Button, "\u{E472} ライト"),
        ] {
            assert!(
                seen.contains(&(role, text.to_string())),
                "{role:?} {text}: {seen:?}"
            );
        }
        let b = vcx.debug_bounds("button:押す").expect("ボタン");
        vcx.simulate_click(b.center(), Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(clicks.get(), 1);
    }

    #[test]
    fn strong_and_button_colors_follow_theme() {
        use crate::gpui_views::theme::DARK;
        assert_eq!(strong_color(&LIGHT), 0x000000);
        assert_eq!(strong_color(&DARK), 0xffffff);
        assert_ne!(button_colors(&LIGHT), button_colors(&DARK));
    }

    #[test]
    fn severity_colors_follow_palette() {
        use crate::gpui_views::theme::DARK;
        assert_eq!(severity_color(Severity::Normal, &LIGHT), LIGHT.accent);
        assert_eq!(severity_color(Severity::Warning, &DARK), DARK.warn);
        assert_eq!(severity_color(Severity::Critical, &LIGHT), LIGHT.err);
    }

    #[test]
    fn bar_ratio_is_clamped() {
        assert_eq!(bar_fill(1.5), 1.0);
        assert_eq!(bar_fill(-0.2), 0.0);
        assert_eq!(bar_fill(0.25), 0.25);
    }
}
