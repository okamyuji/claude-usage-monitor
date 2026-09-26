//! claude-usage-monitor のライブラリ部。
//!
//! バイナリ`cumon`と結合テストの両方から同じロジックを使うため、処理はすべてここに置く。
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod controllers;
pub mod models;
#[cfg(test)]
pub(crate) mod test_support;
pub mod views;
