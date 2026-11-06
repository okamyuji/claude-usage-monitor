//! グラフ。座標と範囲の計算はGPUIに依存しない`math`に置く。
pub mod element;
pub mod format;
pub mod math;
pub mod state;

pub use element::{Plot, PlotState};
