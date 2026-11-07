//! デザイントークン（spec 7.3節）。色、角丸、余白、文字サイズをここだけで決める。
use gpui::{Pixels, Rgba, px};

/// 役割ごとの色。値は`0xRRGGBB`で持ち、`const`で表を組めるようにする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// 画面の地。
    pub bg: u32,
    /// カードの地。
    pub card: u32,
    /// 枠線。
    pub border: u32,
    /// 本文。
    pub text: u32,
    /// 補足の文字。
    pub weak: u32,
    /// アクセント（選択、使用中、通常の使用率）。
    pub accent: u32,
    /// アクセントの淡い地。
    pub accent_soft: u32,
    /// 成功、応答生成中。
    pub ok: u32,
    /// 成功の淡い地。
    pub ok_soft: u32,
    /// 注意、入力待ち。
    pub warn: u32,
    /// 注意の淡い地。
    pub warn_soft: u32,
    /// エラー。
    pub err: u32,
    /// エラーの淡い地。
    pub err_soft: u32,
    /// 使用率バーの下地。
    pub track: u32,
}

/// ライト（GitHub Primerのライト配色）。
pub const LIGHT: Palette = Palette {
    bg: 0xf6f8fa,
    card: 0xffffff,
    border: 0xd0d7de,
    text: 0x1f2328,
    weak: 0x656d76,
    accent: 0x0969da,
    accent_soft: 0xddf4ff,
    ok: 0x1a7f37,
    ok_soft: 0xdafbe1,
    warn: 0x9a6700,
    warn_soft: 0xfff8c5,
    err: 0xcf222e,
    err_soft: 0xffebe9,
    track: 0xeaeef2,
};

/// ダーク（GitHub Primerのダーク配色）。
pub const DARK: Palette = Palette {
    bg: 0x0d1117,
    card: 0x161b22,
    border: 0x30363d,
    text: 0xe6edf3,
    weak: 0x8d96a0,
    accent: 0x4493f8,
    accent_soft: 0x122d4f,
    ok: 0x3fb950,
    ok_soft: 0x122d1d,
    warn: 0xd29922,
    warn_soft: 0x342a0f,
    err: 0xf85149,
    err_soft: 0x3b1719,
    track: 0x21262d,
};

/// カレンダーの帯の色（Tableau 10を元に、灰色を「その他」と見分けられる色へ替えたもの）。
/// 帯は薄めた塗りと濃い枠線で描くので、ライトとダークで同じ表を使う。
pub const SERIES: [u32; 10] = [
    0x4e79a7, 0xf28e2b, 0xe15759, 0x76b7b2, 0x59a14f, 0xedc948, 0xb07aa1, 0xff9da7, 0x9c755f,
    0x17becf,
];

/// 色の番号の色。`None`（その他）は弱い文字の色にする。
pub fn series(c: Option<usize>, p: &Palette) -> u32 {
    c.map_or(p.weak, |i| SERIES[i % SERIES.len()])
}

/// `hex`の色を不透明度`a`倍にする。
pub fn with_alpha(hex: u32, a: f32) -> gpui::Hsla {
    let mut c: gpui::Hsla = color(hex).into();
    c.a *= a;
    c
}

/// 表示中のテーマの色。
pub fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// `0xRRGGBB`をGPUIの色にする。
pub fn color(hex: u32) -> Rgba {
    gpui::rgb(hex)
}

/// 見出しの文字の大きさ。
pub const HEADING: Pixels = px(16.0);
/// 本文の文字の大きさ。
pub const BODY: Pixels = px(14.0);
/// ボタンの文字の大きさ。
pub const BUTTON: Pixels = px(14.0);
/// 補足の文字の大きさ。等倍の外部ディスプレイで画数の多い漢字がつぶれないよう、12pxを下限にする。
pub const SMALL: Pixels = px(12.0);
/// 等幅の文字の大きさ。
pub const MONO: Pixels = px(13.0);
/// 項目の横の間隔。
pub const GAP_X: Pixels = px(8.0);
/// 項目の縦の間隔。
pub const GAP_Y: Pixels = px(6.0);
/// カードの角丸。
pub const RADIUS: Pixels = px(8.0);
/// 部品の角丸。
pub const WIDGET_RADIUS: Pixels = px(6.0);

/// テーマの選択と窓の明暗から、ダークで描くかを決める。
pub fn is_dark(
    t: crate::models::domain::settings::Theme,
    appearance: gpui::WindowAppearance,
) -> bool {
    match t {
        crate::models::domain::settings::Theme::Light => false,
        crate::models::domain::settings::Theme::Dark => true,
        crate::models::domain::settings::Theme::System => matches!(
            appearance,
            gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_alpha_scales_only_the_alpha() {
        let base: gpui::Hsla = color(0x3366CC).into();
        let c = with_alpha(0x3366CC, 0.4);
        assert_eq!((c.h, c.s, c.l), (base.h, base.s, base.l));
        assert!((c.a - base.a * 0.4).abs() < 1e-6, "{}", c.a);
    }

    #[test]
    fn palette_and_series_pick_by_theme_and_index() {
        assert_eq!(palette(false), &LIGHT);
        assert_eq!(palette(true), &DARK);
        assert_eq!(series(None, &LIGHT), LIGHT.weak);
        assert_eq!(series(Some(1), &DARK), SERIES[1]);
        assert_eq!(series(Some(SERIES.len() + 2), &DARK), SERIES[2]);
    }

    #[test]
    fn text_sizes_are_fixed() {
        assert_eq!(
            [HEADING, BODY, BUTTON, SMALL, MONO],
            [16.0, 14.0, 14.0, 12.0, 13.0].map(gpui::px)
        );
    }

    #[test]
    fn color_converts_hex_exactly() {
        let c = color(0x0969da);
        assert_eq!((c.r * 255.0).round() as u8, 0x09);
        assert_eq!((c.g * 255.0).round() as u8, 0x69);
        assert_eq!((c.b * 255.0).round() as u8, 0xda);
        assert_eq!(c.a, 1.0);
    }

    #[test]
    fn dark_follows_choice_or_window_appearance() {
        use crate::models::domain::settings::Theme;
        use gpui::WindowAppearance::*;
        assert!(is_dark(Theme::Dark, Light));
        assert!(!is_dark(Theme::Light, Dark));
        assert!(is_dark(Theme::System, Dark));
        assert!(is_dark(Theme::System, VibrantDark));
        assert!(!is_dark(Theme::System, Light));
        assert!(!is_dark(Theme::System, VibrantLight));
    }
}
