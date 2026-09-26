//! GUIの操作と状態遷移。
//!
//! eguiに依存しないため、画面なしでユニットテストできる。描画は`views`が担う。
pub mod app;
pub mod dashboard;
pub mod header;
pub mod tabs;
