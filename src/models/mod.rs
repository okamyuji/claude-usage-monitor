//! MVCのModel層。
//!
//! ドメインの型と純粋関数（domain）、外部依存の抽象（ports）、永続化（repositories）、
//! 外部システムとの接続（gateways）に分ける。controllersがI/Oの種類を知らずに済むようにするため。
pub mod domain;
pub mod gateways;
pub mod ports;
pub mod repositories;
