//! 画面で使うPhosphorのアイコン。符号位置はegui-phosphor 0.14.0の`regular`と同じ。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_sit_in_the_private_use_area_like_egui_phosphor() {
        for (name, egui) in [
            (SQUARES_FOUR, egui_phosphor::regular::SQUARES_FOUR),
            (CALENDAR_BLANK, egui_phosphor::regular::CALENDAR_BLANK),
            (CHART_BAR, egui_phosphor::regular::CHART_BAR),
            (USERS, egui_phosphor::regular::USERS),
            (GEAR, egui_phosphor::regular::GEAR),
            (STETHOSCOPE, egui_phosphor::regular::STETHOSCOPE),
            (SUN, egui_phosphor::regular::SUN),
            (MOON, egui_phosphor::regular::MOON),
            (CARET_DOWN, egui_phosphor::regular::CARET_DOWN),
            (CARET_LEFT, egui_phosphor::regular::CARET_LEFT),
            (CARET_RIGHT, egui_phosphor::regular::CARET_RIGHT),
            (LIST, egui_phosphor::regular::LIST),
            (NOTE, egui_phosphor::regular::NOTE),
            (PAUSE, egui_phosphor::regular::PAUSE),
            (PLAY, egui_phosphor::regular::PLAY),
            (SKIP_BACK, egui_phosphor::regular::SKIP_BACK),
            (SKIP_FORWARD, egui_phosphor::regular::SKIP_FORWARD),
            (TERMINAL, egui_phosphor::regular::TERMINAL),
        ] {
            assert_eq!(name, egui);
        }
    }
}
