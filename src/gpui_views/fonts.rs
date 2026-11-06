//! 同梱のフォントの登録と、本文と等幅の字形の探し順。
//!
//! OSのフォントに頼ると日本語フォントの無い環境で漢字が豆腐になるため、すべて同梱して埋め込む。
//! 探し順はegui版と同じにする。本文は英数字も日本語と同じフォントで描き、数字だけ細く低くなるのを避ける。
//! 等幅は桁を揃えるためHackを先に置く。絵文字のフォントは最後に置き、全角の括弧を絵文字の字形で描かせない。
use gpui::{App, FontFallbacks};
use std::borrow::Cow;

/// 本文のフォント。
pub const BODY_FAMILY: &str = "Noto Sans JP";
/// 等幅のフォント。
pub const MONO_FAMILY: &str = "Hack";
/// アイコンのフォント。
pub const ICON_FAMILY: &str = "Phosphor";

const FONTS: [&[u8]; 5] = [
    include_bytes!("../../assets/fonts/NotoSansJP-Regular.otf"),
    include_bytes!("../../assets/fonts/Phosphor.ttf"),
    include_bytes!("../../assets/fonts/Ubuntu-Light.ttf"),
    include_bytes!("../../assets/fonts/NotoEmoji-Regular.ttf"),
    include_bytes!("../../assets/fonts/Hack-Regular.ttf"),
];

/// 同梱のフォントを登録する。埋め込んだバイト列をそのまま渡し、ヒープへ複製しない。
pub fn install(cx: &mut App) -> gpui::Result<()> {
    cx.text_system()
        .add_fonts(FONTS.iter().map(|b| Cow::Borrowed(*b)).collect())
}

/// 本文で字形が無いときに次に探すフォント。
pub fn body_fallbacks() -> FontFallbacks {
    FontFallbacks::from_fonts(
        [ICON_FAMILY, "Ubuntu", "Noto Emoji"]
            .map(String::from)
            .to_vec(),
    )
}

/// 等幅で字形が無いときに次に探すフォント。
pub fn mono_fallbacks() -> FontFallbacks {
    FontFallbacks::from_fonts([BODY_FAMILY, "Noto Emoji"].map(String::from).to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    // テスト窓の文字の仕組みはフォントを持たないため、本物の文字の仕組みを持つテスト用アプリで確かめる。
    #[test]
    fn install_registers_bundled_families() {
        let cx = gpui::TestAppContext::build_with_text_system(
            gpui::TestDispatcher::new(0),
            None,
            crate::test_support::gpui_text_system(),
        );
        cx.update(|cx| {
            install(cx).unwrap();
            let names = cx.text_system().all_font_names();
            for family in [
                BODY_FAMILY,
                MONO_FAMILY,
                ICON_FAMILY,
                "Noto Emoji",
                "Ubuntu",
            ] {
                assert!(
                    names.iter().any(|n| n == family),
                    "{family} が無い: {names:?}"
                );
            }
        });
    }

    #[test]
    fn body_tries_japanese_then_icons_then_latin_then_emoji() {
        assert_eq!(BODY_FAMILY, "Noto Sans JP");
        assert_eq!(
            body_fallbacks().0.as_slice(),
            ["Phosphor", "Ubuntu", "Noto Emoji"]
        );
    }

    #[test]
    fn mono_keeps_hack_first_for_aligned_digits() {
        assert_eq!(MONO_FAMILY, "Hack");
        assert_eq!(
            mono_fallbacks().0.as_slice(),
            ["Noto Sans JP", "Noto Emoji"]
        );
    }
}
