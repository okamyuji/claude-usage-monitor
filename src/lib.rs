//! claude-profile-switcher のライブラリ部。
//!
//! バイナリ`cps`と結合テストの両方から同じロジックを使うため、処理はすべてここに置く。
#![warn(missing_docs)]

pub mod controllers;
pub mod models;
#[cfg(test)]
pub(crate) mod test_support;
