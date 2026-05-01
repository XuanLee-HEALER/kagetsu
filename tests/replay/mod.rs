//! 真实牌谱回归测试: 解析 mjai NDJSON → ReplayLog (中性 IR) → 用 GameState replay → 对比断言.
//!
//! 这是 B 层测试: 用第三方真实对局 (mjai 格式公开 sample) 当 fixture,
//! 不存在自证循环.
//!
//! ## 模块布局
//!
//! - [`mjai_pai`]   — 牌名 (`1m`/`5pr`/`E` ...) ↔ [`Tile`] 双向映射
//! - [`mjai_parser`] — NDJSON → `Vec<MjaiEvent>`
//! - [`replay_log`]  — 中性 IR (`ReplayLog` / `KyokuLog` / `KyokuEvent`)
//! - [`driver`]      — 用 `GameState` replay 一局 + 对比 expected
//!
//! [`Tile`]: tui_majo::tile::Tile

#![allow(dead_code)]

pub mod driver;
pub mod mjai_pai;
pub mod mjai_parser;
pub mod replay_log;
