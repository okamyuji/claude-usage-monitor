//! 描画。controllers/guiのViewModelを受け取ってGPUIで描き、操作を`Action`で返す。
//!
//! DBやtraitに触れないため、表示の変更がデータの取り方に波及しない。
pub mod a11y;
#[cfg(target_os = "macos")]
pub mod about_icon;
pub mod app;
pub mod app_menu;
pub mod dashboard;
pub mod fonts;
pub mod header;
pub mod icons;
pub mod input;
pub mod layout;
pub mod plot;
pub mod probe;
pub mod tabs;
pub mod theme;
pub mod tray;
pub mod widgets;
