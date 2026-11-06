//! テストが部品を役割と名前で探すための記録。
//!
//! GPUIのテスト窓は読み上げ用の木を作らないため、部品が描くたびに役割と名前をここへ積み、
//! 要素には同じ値の`debug_selector`を付ける。記録はデバッグビルドだけで行い、releaseでは何もしない。
use std::cell::RefCell;

/// 部品の役割。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// 文字。
    Label,
    /// ボタン。
    Button,
    /// 文字の入力欄。
    TextInput,
    /// チェック。
    CheckBox,
    /// スライダー。
    Slider,
    /// 選択。
    ComboBox,
    /// リンク。
    Link,
    /// タブ。
    Tab,
    /// 区画の境界のつまみ。
    Splitter,
}

impl Role {
    /// すべての役割。
    pub const ALL: [Role; 9] = [
        Role::Label,
        Role::Button,
        Role::TextInput,
        Role::CheckBox,
        Role::Slider,
        Role::ComboBox,
        Role::Link,
        Role::Tab,
        Role::Splitter,
    ];

    /// 目印の接頭辞。
    pub fn key(self) -> &'static str {
        match self {
            Role::Label => "label",
            Role::Button => "button",
            Role::TextInput => "textinput",
            Role::CheckBox => "checkbox",
            Role::Slider => "slider",
            Role::ComboBox => "combobox",
            Role::Link => "link",
            Role::Tab => "tab",
            Role::Splitter => "splitter",
        }
    }

    /// 読み上げ用の役割。タブは押せるボタンとして読ませる。
    pub fn a11y(self) -> gpui::Role {
        match self {
            Role::Label => gpui::Role::Label,
            Role::Button | Role::Tab => gpui::Role::Button,
            Role::TextInput => gpui::Role::TextInput,
            Role::CheckBox => gpui::Role::CheckBox,
            Role::Slider => gpui::Role::Slider,
            Role::ComboBox => gpui::Role::ComboBox,
            Role::Link => gpui::Role::Link,
            Role::Splitter => gpui::Role::Splitter,
        }
    }
}

thread_local! {
    static SEEN: RefCell<Vec<(Role, String)>> = const { RefCell::new(Vec::new()) };
}

/// 描いた部品を記録し、同じ役割と名前の部品のうち何番目か（0から）を返す。
/// releaseでは記録せず0を返す。
pub fn record(role: Role, label: &str) -> usize {
    #[cfg(debug_assertions)]
    return SEEN.with(|s| {
        let mut s = s.borrow_mut();
        let n = s.iter().filter(|(r, l)| *r == role && l == label).count();
        s.push((role, label.to_string()));
        n
    });
    #[cfg(not(debug_assertions))]
    {
        let _ = (role, label);
        0
    }
}

/// 記録を取り出して空にする。
pub fn take() -> Vec<(Role, String)> {
    SEEN.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

/// 要素に付ける目印。同じ役割と名前の2番目からは`#番号`を付けて区別する。
pub fn selector(role: Role, label: &str, nth: usize) -> String {
    if nth == 0 {
        format!("{}:{label}", role.key())
    } else {
        format!("{}:{label}#{nth}", role.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_take_round_trip() {
        take();
        record(Role::Button, "デーモンを起動");
        record(Role::Label, "今日");
        assert_eq!(
            take(),
            vec![
                (Role::Button, "デーモンを起動".to_string()),
                (Role::Label, "今日".to_string())
            ]
        );
    }

    #[test]
    fn take_clears() {
        take();
        record(Role::Tab, "診断");
        take();
        assert!(take().is_empty());
    }

    #[test]
    fn selector_joins_role_and_label() {
        assert_eq!(selector(Role::CheckBox, "通知", 0), "checkbox:通知");
        assert_eq!(selector(Role::TextInput, "検索", 0), "textinput:検索");
        assert_eq!(selector(Role::Label, "ⓘ", 2), "label:ⓘ#2");
    }

    #[test]
    fn record_numbers_repeated_parts_in_drawing_order() {
        take();
        assert_eq!(record(Role::Label, "ⓘ"), 0);
        assert_eq!(record(Role::Button, "ⓘ"), 0, "役割が違えば別に数える");
        assert_eq!(record(Role::Label, "ⓘ"), 1);
        assert_eq!(record(Role::Label, "ⓘ"), 2);
        take();
        assert_eq!(record(Role::Label, "ⓘ"), 0, "描き直すと0から数え直す");
    }

    #[test]
    fn every_role_has_a_distinct_key_and_a11y_role() {
        let all = Role::ALL;
        let keys: std::collections::HashSet<_> = all.iter().map(|r| r.key()).collect();
        assert_eq!(keys.len(), all.len());
        assert_eq!(Role::Button.a11y(), gpui::Role::Button);
        assert_eq!(Role::Label.a11y(), gpui::Role::Label);
        assert_eq!(Role::TextInput.a11y(), gpui::Role::TextInput);
        assert_eq!(Role::CheckBox.a11y(), gpui::Role::CheckBox);
        assert_eq!(Role::Slider.a11y(), gpui::Role::Slider);
        assert_eq!(Role::ComboBox.a11y(), gpui::Role::ComboBox);
        assert_eq!(Role::Link.a11y(), gpui::Role::Link);
        assert_eq!(Role::Tab.a11y(), gpui::Role::Button);
    }
}
