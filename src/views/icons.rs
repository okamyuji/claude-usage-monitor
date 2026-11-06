//! 画面で使うPhosphorのアイコン。符号位置は同梱の`Phosphor.ttf`（Phosphor Iconsのregular）の私用領域。
#![allow(missing_docs)]

pub const CALENDAR_BLANK: &str = "\u{E10A}";
pub const CARET_DOWN: &str = "\u{E136}";
pub const CARET_LEFT: &str = "\u{E138}";
pub const CARET_RIGHT: &str = "\u{E13A}";
pub const CHART_BAR: &str = "\u{E150}";
pub const GEAR: &str = "\u{E270}";
pub const LIST: &str = "\u{E2F0}";
pub const MOON: &str = "\u{E330}";
pub const NOTE: &str = "\u{E348}";
pub const PAUSE: &str = "\u{E39E}";
pub const PLAY: &str = "\u{E3D0}";
pub const SKIP_BACK: &str = "\u{E5A4}";
pub const SKIP_FORWARD: &str = "\u{E5A6}";
pub const SQUARES_FOUR: &str = "\u{E464}";
pub const STETHOSCOPE: &str = "\u{E7EA}";
pub const SUN: &str = "\u{E472}";
pub const TERMINAL: &str = "\u{E47E}";
pub const USERS: &str = "\u{E4D6}";

/// 文字列を、私用領域の字（Phosphorのアイコン）とそれ以外の連なりに分ける。真の連なりがアイコン。
pub fn split_icons(s: &str) -> Vec<(bool, &str)> {
    let mut out: Vec<(bool, &str)> = Vec::new();
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        let icon = ('\u{E000}'..='\u{F8FF}').contains(&ch);
        match out.last() {
            Some((was, _)) if *was == icon => {}
            Some(_) => {
                out.last_mut().expect("直前の連なり").1 = &s[start..i];
                start = i;
                out.push((icon, ""));
            }
            None => out.push((icon, "")),
        }
    }
    if let Some(last) = out.last_mut() {
        last.1 = &s[start..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_icons_separates_private_use_glyphs() {
        let l = CARET_LEFT;
        let r = CARET_RIGHT;
        assert_eq!(
            split_icons(&format!("{l} 前週")),
            vec![(true, l), (false, " 前週")]
        );
        assert_eq!(
            split_icons(&format!("次週 {r}")),
            vec![(false, "次週 "), (true, r)]
        );
        assert_eq!(split_icons("保存"), vec![(false, "保存")]);
        assert_eq!(split_icons(""), vec![]);
        assert_eq!(
            split_icons(&format!("{l}{r}a")),
            vec![(true, &*format!("{l}{r}")), (false, "a")]
        );
    }
}
