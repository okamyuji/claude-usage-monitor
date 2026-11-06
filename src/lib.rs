//! claude-usage-monitor のライブラリ部。
//!
//! バイナリ`cumon`と結合テストの両方から同じロジックを使うため、処理はすべてここに置く。
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(test)]
#[path = "../build/version.rs"]
mod build_version;
pub mod controllers;
pub mod models;
#[cfg(test)]
pub(crate) mod test_support;
pub mod views;
