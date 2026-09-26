//! デザイントークン（spec 7.3節）。色、角丸、余白、文字サイズをここだけで決める。
use egui::{
    Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme, Visuals,
};

/// 役割ごとの色。ライトとダークで同じ役割に別の色を割り当てる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// 画面の地。
    pub bg: Color32,
    /// カードの地。
    pub card: Color32,
    /// 枠線。
    pub border: Color32,
    /// 本文。
    pub text: Color32,
    /// 補足の文字。
    pub weak: Color32,
    /// アクセント（選択、使用中、通常の使用率）。
    pub accent: Color32,
    /// アクセントの淡い地。
    pub accent_soft: Color32,
    /// 成功、応答生成中。
    pub ok: Color32,
    /// 成功の淡い地。
    pub ok_soft: Color32,
    /// 注意、入力待ち。
    pub warn: Color32,
    /// 注意の淡い地。
    pub warn_soft: Color32,
    /// エラー。
    pub err: Color32,
    /// エラーの淡い地。
    pub err_soft: Color32,
    /// 使用率バーの下地。
    pub track: Color32,
}

/// ライト（GitHub Primerのライト配色）。
pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xf6, 0xf8, 0xfa),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    border: Color32::from_rgb(0xd0, 0xd7, 0xde),
    text: Color32::from_rgb(0x1f, 0x23, 0x28),
    weak: Color32::from_rgb(0x65, 0x6d, 0x76),
    accent: Color32::from_rgb(0x09, 0x69, 0xda),
    accent_soft: Color32::from_rgb(0xdd, 0xf4, 0xff),
    ok: Color32::from_rgb(0x1a, 0x7f, 0x37),
    ok_soft: Color32::from_rgb(0xda, 0xfb, 0xe1),
    warn: Color32::from_rgb(0x9a, 0x67, 0x00),
    warn_soft: Color32::from_rgb(0xff, 0xf8, 0xc5),
    err: Color32::from_rgb(0xcf, 0x22, 0x2e),
    err_soft: Color32::from_rgb(0xff, 0xeb, 0xe9),
    track: Color32::from_rgb(0xea, 0xee, 0xf2),
};

/// ダーク（GitHub Primerのダーク配色）。
pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x0d, 0x11, 0x17),
    card: Color32::from_rgb(0x16, 0x1b, 0x22),
    border: Color32::from_rgb(0x30, 0x36, 0x3d),
    text: Color32::from_rgb(0xe6, 0xed, 0xf3),
    weak: Color32::from_rgb(0x8d, 0x96, 0xa0),
    accent: Color32::from_rgb(0x44, 0x93, 0xf8),
    accent_soft: Color32::from_rgb(0x12, 0x2d, 0x4f),
    ok: Color32::from_rgb(0x3f, 0xb9, 0x50),
    ok_soft: Color32::from_rgb(0x12, 0x2d, 0x1d),
    warn: Color32::from_rgb(0xd2, 0x99, 0x22),
    warn_soft: Color32::from_rgb(0x34, 0x2a, 0x0f),
    err: Color32::from_rgb(0xf8, 0x51, 0x49),
    err_soft: Color32::from_rgb(0x3b, 0x17, 0x19),
    track: Color32::from_rgb(0x21, 0x26, 0x2d),
};

/// 角丸。
const RADIUS: u8 = 8;

/// 表示中のテーマの色。
pub fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

fn visuals(p: &Palette, dark: bool) -> Visuals {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.extreme_bg_color = p.card;
    v.faint_bg_color = p.bg;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.weak_text_color = Some(p.weak);
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.err;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
    ] {
        w.corner_radius = CornerRadius::same(6);
    }
    v
}

/// ライトとダークの両方の見た目を登録する。OSのテーマが変わっても同じ規則で描くため。
pub fn apply(ctx: &egui::Context) {
    for (theme, dark) in [(Theme::Light, false), (Theme::Dark, true)] {
        ctx.set_visuals_of(theme, visuals(palette(dark), dark));
        ctx.style_mut_of(theme, |s| {
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.button_padding = egui::vec2(10.0, 4.0);
            // 等倍（1倍）の外部ディスプレイでは、11px以下の画数の多い漢字（間、週）は線がつぶれて読めない。補足でも12pxを下限にする。
            for (style, size, family) in [
                (TextStyle::Heading, 16.0, FontFamily::Proportional),
                (TextStyle::Body, 14.0, FontFamily::Proportional),
                (TextStyle::Button, 14.0, FontFamily::Proportional),
                (TextStyle::Small, 12.0, FontFamily::Proportional),
                (TextStyle::Monospace, 13.0, FontFamily::Monospace),
            ] {
                s.text_styles.insert(style, FontId::new(size, family));
            }
        });
    }
}

/// カードの枠。使用中のカードだけアクセント色の枠にする。
pub fn card_frame(p: &Palette, highlighted: bool) -> egui::Frame {
    let stroke = if highlighted {
        Stroke::new(1.5, p.accent)
    } else {
        Stroke::new(1.0, p.border)
    };
    egui::Frame::new()
        .fill(p.card)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::symmetric(10, 8))
        .shadow(Shadow {
            offset: [0, 1],
            blur: 3,
            spread: 0,
            color: Color32::from_black_alpha(18),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_sets_both_themes() {
        let ctx = egui::Context::default();
        apply(&ctx);
        assert_eq!(ctx.style_of(Theme::Light).visuals.panel_fill, LIGHT.bg);
        assert_eq!(ctx.style_of(Theme::Dark).visuals.panel_fill, DARK.bg);
        assert_eq!(
            ctx.style_of(Theme::Dark).visuals.selection.bg_fill,
            DARK.accent_soft
        );
        assert_eq!(
            ctx.style_of(Theme::Light).text_styles[&TextStyle::Body].size,
            14.0
        );
        assert_eq!(
            ctx.style_of(Theme::Dark).text_styles[&TextStyle::Small].size,
            12.0
        );
        assert!(ctx.style_of(Theme::Dark).visuals.dark_mode);
    }

    #[test]
    fn palette_and_card_frame() {
        assert_eq!(palette(false), &LIGHT);
        assert_eq!(palette(true), &DARK);
        assert_eq!(
            card_frame(&LIGHT, true).stroke,
            Stroke::new(1.5, LIGHT.accent)
        );
        assert_eq!(
            card_frame(&DARK, false).stroke,
            Stroke::new(1.0, DARK.border)
        );
        assert_eq!(card_frame(&DARK, false).fill, DARK.card);
    }
}
