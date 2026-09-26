//! 描画。controllers/guiのViewModelを受け取って描き、操作を`Action`で返す。
//!
//! DBやtraitに触れないため、表示の変更がデータの取り方に波及しない。
pub mod app;
pub mod dashboard;
pub mod header;
pub mod layout;
pub mod tabs;
pub mod theme;
pub mod widgets;
