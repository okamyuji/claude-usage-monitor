//! 上部バー。タブ、今日と今週の集計、デーモンの状態、テーマの切り替え。
use crate::controllers::gui::app::{Action, AppVm, Tab};
use crate::gpui_views::app::Acts;
use crate::gpui_views::icons;
use crate::gpui_views::theme::{GAP_X, Palette};
use crate::gpui_views::widgets::{badge, button, icon, icon_button, selectable, strong, weak};
use crate::models::domain::settings::Theme;
use gpui::{IntoElement, ParentElement as _, Styled as _, div, px};

fn tab_icon(t: Tab) -> &'static str {
    match t {
        Tab::Dashboard => icons::SQUARES_FOUR,
        Tab::Calendar => icons::CALENDAR_BLANK,
        Tab::Analytics => icons::CHART_BAR,
        Tab::Profiles => icons::USERS,
        Tab::Settings => icons::GEAR,
        Tab::Diagnostics => icons::STETHOSCOPE,
    }
}

/// 上部バーを描く。
pub fn render(vm: &AppVm, p: &Palette, dark: bool, acts: &Acts) -> impl IntoElement {
    let tabs = Tab::ALL.into_iter().map(|t| {
        let selected = vm.tab == t;
        div()
            .flex()
            .items_center()
            .gap(GAP_X)
            .mr(px(6.0))
            .child(icon(tab_icon(t), if selected { p.accent } else { p.weak }))
            .child(selectable(
                p,
                t.label(),
                selected,
                t.label(),
                acts.on(Action::SelectTab(t)),
            ))
    });
    let (next, glyph, word) = if dark {
        (Theme::Light, icons::SUN, "ライト")
    } else {
        (Theme::Dark, icons::MOON, "ダーク")
    };
    div()
        .flex()
        .items_center()
        .gap(GAP_X)
        .children(tabs)
        .child(div().flex_1())
        .children(totals(vm, p))
        .child(daemon(vm, p, acts))
        .child(icon_button(
            p,
            "theme",
            glyph,
            word,
            acts.on(Action::SetTheme(next)),
        ))
}

fn daemon(vm: &AppVm, p: &Palette, acts: &Acts) -> impl IntoElement {
    let d = div().flex().items_center().gap(GAP_X);
    if vm.daemon_running {
        d.child(badge("● 監視中", p.ok, p.ok_soft))
    } else {
        d.child(badge("● デーモン停止中", p.err, p.err_soft))
            .child(button(
                p,
                "start-daemon",
                "デーモンを起動",
                acts.on(Action::StartDaemon),
            ))
    }
}

/// 今日と今週の集計。eguiの右寄せの並びと同じく、左から今日、今週の順に出す。
fn totals(vm: &AppVm, p: &Palette) -> Vec<gpui::AnyElement> {
    [("今日", &vm.header.today), ("今週", &vm.header.week)]
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .flat_map(|(name, value)| {
            [
                weak(p, name).into_any_element(),
                strong(p, value).into_any_element(),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::controllers::gui::app::Tab;
    use crate::gpui_views::app::tests::{controller, open};
    use crate::gpui_views::icons;
    use crate::gpui_views::probe::{self, Role};

    #[test]
    fn header_shows_tabs_daemon_and_theme() {
        let home = tempfile::tempdir().unwrap();
        let (_d, ctl) = controller(home.path());
        let mut cx = crate::test_support::gpui_cx();
        let _ = open(&mut cx, ctl);
        let seen = probe::take();
        for t in Tab::ALL {
            assert!(
                seen.contains(&(Role::Tab, t.label().to_string())),
                "{}",
                t.label()
            );
        }
        assert!(seen.contains(&(Role::Button, "デーモンを起動".into())));
        assert!(seen.contains(&(Role::Label, "● デーモン停止中".into())));
        assert!(seen.contains(&(Role::Button, format!("{} ダーク", icons::MOON))));
        assert!(seen.contains(&(Role::Label, "今日".into())));
        assert!(seen.contains(&(Role::Label, "0 $0.00".into())));
    }

    #[test]
    fn empty_totals_are_left_out() {
        let home = tempfile::tempdir().unwrap();
        let (_d, ctl) = controller(home.path());
        let mut vm = ctl.vm().clone();
        vm.header.week = String::new();
        probe::take();
        assert_eq!(
            super::totals(&vm, &crate::gpui_views::theme::LIGHT).len(),
            2
        );
        assert_eq!(
            probe::take(),
            vec![
                (Role::Label, "今日".into()),
                (Role::Label, vm.header.today.clone())
            ]
        );
    }
}
