//! アプリのメニュー。egui版のwinitが作る既定のメニューと同じ項目とキーにする。
//! GPUIはメニューを作らないので、無いとAbout、隠す、終了（⌘Q）が使えなくなる。
use gpui::{App, KeyBinding, Menu, MenuItem, SystemMenuType};
use std::path::Path;

/// メニューの操作。名前はメニューの項目と同じ。
#[allow(missing_docs)]
mod menu_actions {
    gpui::actions!(cumon, [About, Hide, HideOthers, ShowAll, Quit]);
}
pub use menu_actions::{About, Hide, HideOthers, Quit, ShowAll};

/// メニューに出すアプリの名前。winitと同じく実行ファイルの名前にする。
pub fn process_name(exe: &Path) -> String {
    exe.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("cumon")
        .to_string()
}

/// アプリのメニュー。
pub fn app_menu(name: &str) -> Menu {
    Menu::new(name.to_string()).items([
        MenuItem::action(format!("About {name}"), About),
        MenuItem::separator(),
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action(format!("Hide {name}"), Hide),
        MenuItem::action("Hide Others", HideOthers),
        MenuItem::action("Show All", ShowAll),
        MenuItem::separator(),
        MenuItem::action(format!("Quit {name}"), Quit),
    ])
}

/// メニューのキー。
pub fn key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]
}

/// メニューとキーを登録する。
pub fn install(cx: &mut App) {
    cx.on_action(|_: &About, _| show_about());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys(key_bindings());
    let exe = std::env::current_exe().unwrap_or_default();
    cx.set_menus([app_menu(&process_name(&exe))]);
}

fn show_about() {
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2_foundation::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(mtm).orderFrontStandardAboutPanel(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::MenuItem;

    fn names(menu: &gpui::Menu) -> Vec<String> {
        menu.items
            .iter()
            .map(|i| match i {
                MenuItem::Separator => "-".to_string(),
                MenuItem::SystemMenu(m) => m.name.to_string(),
                MenuItem::Action { name, .. } => name.to_string(),
                MenuItem::Submenu(m) => m.name.to_string(),
            })
            .collect()
    }

    #[test]
    fn menu_matches_the_winit_default() {
        let m = app_menu("cumon");
        assert_eq!(m.name.as_ref(), "cumon");
        assert_eq!(
            names(&m),
            [
                "About cumon",
                "-",
                "Services",
                "-",
                "Hide cumon",
                "Hide Others",
                "Show All",
                "-",
                "Quit cumon"
            ]
        );
    }

    #[test]
    fn keys_match_the_winit_default() {
        let keys: Vec<(usize, &str)> = key_bindings()
            .iter()
            .map(|b| (b.keystrokes().len(), b.action().name()))
            .collect();
        assert_eq!(
            keys,
            [
                (1, "cumon::Hide"),
                (1, "cumon::HideOthers"),
                (1, "cumon::Quit")
            ]
        );
    }

    #[test]
    fn process_name_comes_from_the_executable() {
        assert_eq!(
            process_name(std::path::Path::new("/usr/local/bin/cumon")),
            "cumon"
        );
        assert_eq!(process_name(std::path::Path::new("")), "cumon");
    }
}
